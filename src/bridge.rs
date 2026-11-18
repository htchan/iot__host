//! Orchestration between serial, MQTT and the DB.
//!
//! Implements the flows from docs/v2/serial-communication-design.md:
//!
//! - Receiving serial msg: log -> parse pairs -> map keys via
//!   `iot_device_configs.topic_mappings` -> publish MQTT one by one,
//!   updating `mqtt_topic_values` (value is stored before publishing so the
//!   loop-back echo of our own publish is detected as unchanged and does not
//!   echo back to the hub).
//! - Sending serial msg (on MQTT update): update `mqtt_topic_values` ->
//!   load all related values by mac -> compose one line via
//!   `iot_device_configs.reply_topics` (no length limit; the hub splits) ->
//!   send to hub.
//! - Special setup msgs on (re)start: `[<mac>]setup/<wake_msg>` followed by
//!   the stored `command` values composed through `reply_topics`.

use std::sync::{Arc, mpsc::Sender as StdSender};

use log::{info, warn};
use tokio::sync::mpsc as tokio_mpsc;

use crate::db::DeviceStore;
use crate::mqtt::{MqttClient, MqttMessage};
use crate::model::{
    normalize_mac, parse_mqtt_topic, compose_mqtt_topic, DeviceConfig,
};
use crate::protocol;
use crate::serial::SerialEvent;

pub struct Bridge {
    store: DeviceStore,
    mqtt: MqttClient,
    serial_tx: StdSender<String>,
}

impl Bridge {
    pub fn new(store: DeviceStore, mqtt: MqttClient, serial_tx: StdSender<String>) -> Self {
        Self { store, mqtt, serial_tx }
    }

    pub async fn handle_serial_event(&self, event: SerialEvent) {
        match event {
            SerialEvent::Line(line) => self.handle_serial_line(line).await,
            SerialEvent::PortOpened => {
                info!("serial port (re)opened; resending setup messages");
                if let Err(e) = self.send_setup_messages().await {
                    warn!("failed to send setup messages: {}", e);
                }
            }
        }
    }

    /// Receiving serial msg (hub -> device direction).
    async fn handle_serial_line(&self, line: String) {
        info!("Received from hub: {}", line);

        let msg = match protocol::parse_serial_line(&line) {
            Ok(m) => m,
            Err(e) => {
                warn!("skipping unparseable serial line: {} ({})", e, line);
                return;
            }
        };

        let device = match self.store.get_device(&msg.mac).await {
            Ok(Some(d)) => d,
            Ok(None) => {
                warn!("serial msg for unknown or deleted device {}", msg.mac);
                return;
            }
            Err(e) => {
                warn!("db error while looking up {}: {}", msg.mac, e);
                return;
            }
        };

        for (key, value) in &msg.pairs {
            let mapping = match device.mapping_for(key) {
                Some(m) => m,
                None => {
                    warn!(
                        "no topic mapping for key '{}' on device {} (line: {})",
                        key, device.mac, line
                    );
                    continue;
                }
            };

            // name is "<field>/<topic>" in the mapping; split at the last '/'.
            let (field, mq_topic) = match mapping.name.rsplit_once('/') {
                Some((f, t)) => (f.to_string(), t.to_string()),
                None => {
                    warn!("mapping name '{}' for key '{}' has no '/'", mapping.name, key);
                    continue;
                }
            };

            let mqtt_topic = compose_mqtt_topic(&mapping.r#type, &device.mac, &mapping.name);

            // Store first: our own subscription will echo the publish back and
            // must be recognized as unchanged (echo-loop guard).
            // TODO: publish mqtt first, then update the db reocrd
            let _ = self
                .store
                .upsert_value(&device.mac, &mapping.r#type, &field, &mq_topic, value)
                .await;

            if let Err(e) = self.mqtt.publish(&mqtt_topic, value, false).await {
                warn!("failed to publish {}: {}", mqtt_topic, e);
            }
            // TODO: after all updates. we should try to compose the latest serial msg and send it to hub
        }
    }

    /// Sending serial msg (MQTT update -> hub direction).
    async fn handle_mqtt_message(&self, message: MqttMessage) {
        let parsed = match parse_mqtt_topic(&message.topic) {
            Some(p) => p,
            None => {
                warn!("mqtt topic does not match homeassistant/<type>/<mac>/<field>/<topic>: {}", message.topic);
                return;
            }
        };
        let mac = normalize_mac(&parsed.mac);

        let changed = match self
            .store
            .upsert_value(&mac, &parsed.r#type, &parsed.field, &parsed.topic, &message.payload)
            .await
        {
            Ok(c) => c,
            Err(e) => {
                warn!("db error storing {}: {}", message.topic, e);
                return;
            }
        };
        if !changed {
            // Unchanged value (e.g. our own echo or a retained re-delivery):
            // nothing to sync to the hub.
            return;
        }

        let device = match self.store.get_device(&mac).await {
            Ok(Some(d)) => d,
            Ok(None) => return, // unknown mac: value kept in DB only (D8)
            Err(e) => {
                warn!("db error while looking up {}: {}", mac, e);
                return;
            }
        };

        let pairs = self.compose_pairs(&device).await;
        if pairs.is_empty() {
            return;
        }
        let line = protocol::compose_serial_line(&mac, &pairs);
        if let Err(e) = self.serial_tx.send(line) {
            warn!("failed to send serial line: {}", e);
        }
    }

    /// Drain the device's stored values into `(key, value)` pairs using
    /// `reply_topics` (no length limit; the hub splits long lines).
    async fn compose_pairs(&self, device: &DeviceConfig) -> Vec<(String, String)> {
        let values = match self.store.load_values(&device.mac).await {
            Ok(v) => v,
            Err(e) => {
                warn!("db error loading values for {}: {}", device.mac, e);
                return Vec::new();
            }
        };

        let mut pairs = Vec::new();
        // TODO: You are not going to iterate on values. You should iterate on the reply_topics of devices. 
        // TODO: It is possible that some values under the device do not requires reply to hub and send to iot devices, eg. config...
        // TODO: also, reply_topics define the order of the pairs
        for v in values {
            let name = format!("{}/{}", v.field, v.topic);
            match device.key_for(&v.r#type, &name) {
                Some(key) => pairs.push((key.to_string(), v.value)),
                None => {
                    // Values without a reply_topics mapping stay on MQTT only.
                }
            }
        }
        pairs
    }

    /// Special setup messages sent on host start / serial reconnect:
    /// `[<mac>]setup/<wake_msg>` followed by the stored `command` values.
    pub async fn send_setup_messages(&self) -> Result<(), String> {
        let devices = self.store.list_devices().await.map_err(|e| e.to_string())?;
        for device in &devices {
            let setup_line = protocol::compose_setup_line(&device.mac, &device.wake_msg);
            self.send_line(&setup_line)?;

            let command_pairs = self.load_command_pairs(device).await?;
            if command_pairs.is_empty() {
                continue;
            }
            let command_line = protocol::compose_serial_line(&device.mac, &command_pairs);
            self.send_line(&command_line)?;
        }
        Ok(())
    }

    /// Stored `command`-topic values mapped through `reply_topics` (D2).
    // TODO: in setup, it will not only send the command pairs, other topic, eg. state should also be included. We should not specifically load the command topic only.
    async fn load_command_pairs(
        &self,
        device: &DeviceConfig,
    ) -> Result<Vec<(String, String)>, String> {
        let values = self
            .store
            .load_values(&device.mac)
            .await
            .map_err(|e| e.to_string())?;
        Ok(values
            .into_iter()
            .filter(|v| v.topic == "command")
            .filter_map(|v| {
                let name = format!("{}/{}", v.field, v.topic);
                device.key_for(&v.r#type, &name).map(|k| (k.to_string(), v.value))
            })
            .collect())
    }

    fn send_line(&self, line: &str) -> Result<(), String> {
        info!("Sending setup to hub: {}", line);
        self.serial_tx
            .send(line.to_string())
            .map_err(|e| format!("serial send failed: {}", e))
    }

    /// Drive the bridge from an MQTT event stream.
    pub async fn handle_mqtt_stream(
        self: Arc<Self>,
        mut rx: tokio_mpsc::UnboundedReceiver<MqttMessage>,
    ) {
        while let Some(msg) = rx.recv().await {
            self.handle_mqtt_message(msg).await;
        }
    }

    /// Drive the bridge from the serial event stream.
    pub async fn handle_serial_stream(
        self: Arc<Self>,
        mut rx: tokio_mpsc::UnboundedReceiver<SerialEvent>,
    ) {
        while let Some(event) = rx.recv().await {
            self.handle_serial_event(event).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{now_rfc3339, KeyMapping, MqttConfigItem, ReplyTopic};
    use crate::mqtt::MqttClient;
    use std::collections::HashMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    const MAC: &str = "AA_BB_CC_DD_EE_00";

    async fn temp_store(tag: &str) -> DeviceStore {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("iot_host_bridge_test_{}_{}.db", tag, nanos));
        DeviceStore::new(&path.to_string_lossy()).await.expect("store")
    }

    fn device() -> DeviceConfig {
        DeviceConfig {
            mac: MAC.to_string(),
            device_name: "Test".to_string(),
            mqtt_configs: Some(vec![MqttConfigItem {
                r#type: "sensor".to_string(),
                name: "temperature".to_string(),
                config_payload: serde_json::json!({"device_class": "temperature"}),
                default_state: Some("abc".to_string()),
                default_command: Some("def".to_string()),
            }]),
            reply_topics: Some(vec![
                ReplyTopic {
                    r#type: "sensor".to_string(),
                    name: "temperature/state".to_string(),
                    key: "temp/ste".to_string(),
                },
                ReplyTopic {
                    r#type: "sensor".to_string(),
                    name: "temperature/command".to_string(),
                    key: "temp/cmd".to_string(),
                },
            ]),
            esp_now_key_mappings: Some(HashMap::from([(
                "act/ste".to_string(),
                KeyMapping {
                    r#type: "sensor".to_string(),
                    name: "temperature/state".to_string(),
                },
            )])),
            wake_msg: "wake123".to_string(),
            created_at: now_rfc3339(),
            updated_at: now_rfc3339(),
            deleted_at: None,
        }
    }

    async fn harness() -> (Arc<Bridge>, DeviceStore, std::sync::mpsc::Receiver<String>) {
        let store = temp_store("harness").await;
        let (serial_tx, serial_rx) = std::sync::mpsc::channel::<String>();
        let bridge = Arc::new(Bridge::new(store.clone(), MqttClient::test_client(), serial_tx));
        (bridge, store, serial_rx)
    }

    #[tokio::test]
    async fn test_serial_line_maps_to_db_value() {
        let (bridge, store, _rx) = harness().await;
        store.upsert_device(&device()).await.unwrap();

        bridge
            .handle_serial_event(SerialEvent::Line(format!(
                r#"[{MAC}]"act/ste:on""#
            )))
            .await;

        let values = store.load_values(MAC).await.unwrap();
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].r#type, "sensor");
        assert_eq!(values[0].field, "temperature");
        assert_eq!(values[0].topic, "state");
        assert_eq!(values[0].value, "on");
    }

    #[tokio::test]
    async fn test_serial_line_unknown_mac_is_skipped() {
        let (bridge, _store, _rx) = harness().await;
        // must not panic
        bridge
            .handle_serial_event(SerialEvent::Line(format!(
                r#"[FF_FF_FF_FF_FF_FF]"act/ste:on""#
            )))
            .await;
    }

    #[tokio::test]
    async fn test_mqtt_update_composes_full_state_line() {
        let (bridge, store, rx) = harness().await;
        store.upsert_device(&device()).await.unwrap();
        store
            .insert_value_if_absent(MAC, "sensor", "temperature", "state", "abc")
            .await
            .unwrap();
        store
            .insert_value_if_absent(MAC, "sensor", "temperature", "command", "def")
            .await
            .unwrap();

        bridge
            .handle_mqtt_message(MqttMessage {
                topic: format!("homeassistant/sensor/{MAC}/temperature/state"),
                payload: "26.0".to_string(),
            })
            .await;

        let line = rx.try_recv().expect("serial line");
        assert!(line.starts_with(&format!("[{MAC}]")), "{}", line);
        assert!(line.contains(r#""temp/cmd:def""#), "{}", line);
        assert!(line.contains(r#""temp/ste:26.0""#), "{}", line);
    }

    #[tokio::test]
    async fn test_mqtt_unchanged_value_does_not_echo() {
        let (bridge, store, rx) = harness().await;
        store.upsert_device(&device()).await.unwrap();
        store
            .upsert_value(MAC, "sensor", "temperature", "state", "25.5")
            .await
            .unwrap();

        bridge
            .handle_mqtt_message(MqttMessage {
                topic: format!("homeassistant/sensor/{MAC}/temperature/state"),
                payload: "25.5".to_string(),
            })
            .await;

        assert!(rx.try_recv().is_err(), "expected no serial echo");
    }

    #[tokio::test]
    async fn test_mqtt_unknown_mac_is_ignored() {
        let (bridge, _store, rx) = harness().await;
        bridge
            .handle_mqtt_message(MqttMessage {
                topic: "homeassistant/sensor/FF_FF_FF_FF_FF_FF/temperature/state".to_string(),
                payload: "1".to_string(),
            })
            .await;
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_setup_messages_include_command_line() {
        let (bridge, store, rx) = harness().await;
        store.upsert_device(&device()).await.unwrap();
        store
            .insert_value_if_absent(MAC, "sensor", "temperature", "command", "def")
            .await
            .unwrap();

        bridge.send_setup_messages().await.unwrap();

        let setup = rx.try_recv().expect("setup line");
        assert_eq!(setup, format!("[{MAC}]setup/wake123"));
        let command = rx.try_recv().expect("command line");
        assert_eq!(command, format!(r#"[{MAC}]"temp/cmd:def""#));
    }

    #[tokio::test]
    async fn test_setup_messages_without_command_values_send_setup_only() {
        let (bridge, store, rx) = harness().await;
        let cfg = DeviceConfig {
            wake_msg: "only-wake".to_string(),
            ..device()
        };
        store.upsert_device(&cfg).await.unwrap();

        bridge.send_setup_messages().await.unwrap();

        let setup = rx.try_recv().expect("setup line");
        assert_eq!(setup, format!("[{MAC}]setup/only-wake"));
        assert!(rx.try_recv().is_err(), "no command line expected");
    }
}
