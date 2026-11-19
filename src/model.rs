//! Core data types shared across the API, DB, MQTT and serial layers.
//!
//! Field names follow docs/v2/api-design.md and docs/v2/db-design.md.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// MQTT topic prefix used for every device topic.
pub const TOPIC_PREFIX: &str = "homeassistant";

/// One entry of the device request `mqtt_configs` array.
///
/// `name` doubles as the MQTT "field": the host composes
/// `homeassistant/<type>/<mac>/<name>/config` for HA discovery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MqttConfigItem {
    // TODO: rename to field_type / sensor_type. avoid the r#type.
    pub r#type: String,
    pub name: String,
    pub config_payload: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_command: Option<String>,
}

/// One entry of the device request `reply_topics` array.
///
/// Maps an MQTT `<field>/<topic>` pair (in `name`) to a short ESP-Now key,
/// so the host can shrink MQTT topics when composing serial messages.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplyTopic {
    // TODO: rename to field_type / sensor_type. avoid the r#type.
    pub r#type: String,
    /// `<field>/<topic>`, e.g. `temperature/state`.
    pub name: String,
    /// Short key used on the wire, e.g. `temp/ste`.
    pub key: String,
}

/// Value of one entry of the device request `esp_now_key_mappings` object.
///
/// Maps a short ESP-Now key back to the MQTT `<field>/<topic>` pair (in `name`)
/// it should be translated to.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KeyMapping {
    // TODO: rename to field_type / sensor_type. avoid the r#type.
    pub r#type: String,
    /// `<field>/<topic>`, e.g. `temperature/state`.
    pub name: String,
}

/// Device configuration stored in `iot_device_configs`.
///
/// JSON columns: `mqtt_configs`, `replay_topics` (API: `reply_topics`),
/// `topic_mappings` (API: `esp_now_key_mappings`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    pub mac: String,
    pub device_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mqtt_configs: Option<Vec<MqttConfigItem>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_topics: Option<Vec<ReplyTopic>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub esp_now_key_mappings: Option<HashMap<String, KeyMapping>>,
    /// Software-encryption key synced to the hub in the setup message
    /// (`[<mac>]setup/<encryption_key>/<wake_msg>`); used by the ESP-Now
    /// software encryption (docs/v1/esp-now-design.md).
    #[serde(default)]
    pub encryption_key: String,
    pub wake_msg: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<String>,
}

impl DeviceConfig {
    /// Look up the short ESP-Now key for a stored MQTT value, using `reply_topics`.
    /// `name` is `<field>/<topic>`.
    // TODO: seems it understand it wrongly. in the serial msg compose, we will have input
    // - reply topics
    // - hash map containg all mqtt value for that mac keyed by mqtt topic
    // the process should be
    // for each items in reply topics
    //   compose the mqtt topic
    //   query the hash map to get the value
    //   compose the string item by `"<key>:<value>"`
    // I cant see what is the usage of key_for. We may need a func to help convert the mac + type + name into mqtt topic
    pub fn key_for(&self, r#type: &str, name: &str) -> Option<&str> {
        self.reply_topics.as_ref()?.iter().find_map(|rt| {
            (rt.r#type == r#type && rt.name == name).then(|| rt.key.as_str())
        })
    }

    /// Look up the MQTT mapping for an ESP-Now key, using `esp_now_key_mappings`.
    pub fn mapping_for(&self, key: &str) -> Option<&KeyMapping> {
        self.esp_now_key_mappings.as_ref()?.get(key)
    }
}

/// One row of the `mqtt_topic_values` table.
// TODO: should we name this class as MqttRecord?
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MqttValue {
    pub mac: String,
    pub r#type: String,
    pub field: String,
    pub topic: String,
    pub value: String,
    pub created_at: String,
    pub updated_at: String,
}

/// A parsed MQTT topic `homeassistant/<type>/<mac>/<field>/<topic>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MqttTopic {
    pub r#type: String,
    pub mac: String,
    pub field: String,
    pub topic: String,
}

impl MqttTopic {
    /// `<field>/<topic>`, the form used in `reply_topics` / `esp_now_key_mappings`.
    pub fn name(&self) -> String {
        format!("{}/{}", self.field, self.topic)
    }
}

/// Compose `homeassistant/<type>/<mac>/<name>` where `name` is `<field>/<topic>`.
pub fn compose_mqtt_topic(r#type: &str, mac: &str, name: &str) -> String {
    format!("{}/{}/{}/{}", TOPIC_PREFIX, r#type, mac, name)
}

/// Parse `homeassistant/<type>/<mac>/<field>/<topic>`.
///
/// `field` may itself contain `/` (the last segment is always the topic).
pub fn parse_mqtt_topic(topic: &str) -> Option<MqttTopic> {
    let parts: Vec<&str> = topic.split('/').collect();
    if parts.len() < 5 || parts[0] != TOPIC_PREFIX {
        return None;
    }
    let last = parts.len() - 1;
    Some(MqttTopic {
        r#type: parts[1].to_string(),
        mac: parts[2].to_string(),
        field: parts[3..last].join("/"),
        topic: parts[last].to_string(),
    })
}

/// Normalize a MAC to the canonical form `AA_BB_CC_DD_EE_00`.
///
/// Accepts `aa-bb-cc-dd-ee-ff` and `AA:BB:CC:DD:EE:00` style input.
pub fn normalize_mac(mac: &str) -> String {
    mac.trim()
        .to_uppercase()
        .replace(['-', ':'], "_")
}

/// Check the canonical MAC shape: 6 groups of 2 hex digits separated by `_`.
pub fn is_valid_mac(mac: &str) -> bool {
    let bytes = mac.as_bytes();
    if bytes.len() != 17 {
        return false;
    }
    for (i, b) in bytes.iter().enumerate() {
        if i % 3 == 2 {
            if *b != b'_' {
                return false;
            }
        } else if !b.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

/// Current UTC timestamp in RFC3339 (used for created_at/updated_at/deleted_at).
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_mac() {
        assert_eq!(normalize_mac("aa-bb-cc-dd-ee-ff"), "AA_BB_CC_DD_EE_FF");
        assert_eq!(normalize_mac("AA:BB:CC:DD:EE:00"), "AA_BB_CC_DD_EE_00");
        assert_eq!(normalize_mac(" AA_BB_CC_DD_EE_00 "), "AA_BB_CC_DD_EE_00");
    }

    #[test]
    fn test_is_valid_mac() {
        assert!(is_valid_mac("AA_BB_CC_DD_EE_00"));
        assert!(!is_valid_mac("AA-BB-CC-DD-EE-00"));
        assert!(!is_valid_mac("AA_BB_CC_DD_EE"));
        assert!(!is_valid_mac("GG_BB_CC_DD_EE_00"));
        assert!(!is_valid_mac(""));
    }

    #[test]
    fn test_mqtt_topic_roundtrip() {
        let topic = "homeassistant/sensor/AA_BB_CC_DD_EE_00/temperature/state";
        let parsed = parse_mqtt_topic(topic).expect("parse");
        assert_eq!(parsed.r#type, "sensor");
        assert_eq!(parsed.mac, "AA_BB_CC_DD_EE_00");
        assert_eq!(parsed.field, "temperature");
        assert_eq!(parsed.topic, "state");
        assert_eq!(parsed.name(), "temperature/state");
    }

    #[test]
    fn test_mqtt_topic_field_with_slash() {
        let parsed = parse_mqtt_topic("homeassistant/sensor/MAC/a/b/state").expect("parse");
        assert_eq!(parsed.field, "a/b");
        assert_eq!(parsed.topic, "state");
        assert_eq!(parsed.name(), "a/b/state");
    }

    #[test]
    fn test_mqtt_topic_invalid() {
        assert!(parse_mqtt_topic("homeassistant/sensor/MAC/state").is_none());
        assert!(parse_mqtt_topic("other/sensor/MAC/field/state").is_none());
        assert!(parse_mqtt_topic("").is_none());
    }

    #[test]
    fn test_compose_mqtt_topic() {
        assert_eq!(
            compose_mqtt_topic("sensor", "AA_BB_CC_DD_EE_00", "temperature/state"),
            "homeassistant/sensor/AA_BB_CC_DD_EE_00/temperature/state"
        );
    }

    #[test]
    fn test_reply_topic_lookup() {
        let cfg: DeviceConfig = serde_json::from_str(
            r#"{
                "mac": "AA_BB_CC_DD_EE_00",
                "device_name": "x",
                "wake_msg": "wake",
                "created_at": "t",
                "updated_at": "t",
                "reply_topics": [
                    {"type": "sensor", "name": "temperature/state", "key": "temp/ste"},
                    {"type": "switch", "name": "light/command", "key": "light/cmd"}
                ]
            }"#,
        )
        .unwrap();
        assert_eq!(cfg.key_for("sensor", "temperature/state"), Some("temp/ste"));
        assert_eq!(cfg.key_for("switch", "light/command"), Some("light/cmd"));
        assert_eq!(cfg.key_for("sensor", "light/command"), None);
        assert_eq!(cfg.key_for("switch", "temperature/state"), None);
        assert_eq!(cfg.key_for("sensor", "missing/state"), None);
    }

    #[test]
    fn test_key_mapping_lookup() {
        let cfg: DeviceConfig = serde_json::from_str(
            r#"{
                "mac": "AA_BB_CC_DD_EE_00",
                "device_name": "x",
                "wake_msg": "wake",
                "created_at": "t",
                "updated_at": "t",
                "esp_now_key_mappings": {
                    "act/ste": {"type": "sensor", "name": "temperature/state"},
                    "humi/st": {"type": "sensor", "name": "humidity/state"}
                }
            }"#,
        )
        .unwrap();
        let m = cfg.mapping_for("act/ste").expect("mapping");
        assert_eq!(m.r#type, "sensor");
        assert_eq!(m.name, "temperature/state");
        assert!(cfg.mapping_for("nope").is_none());
    }

    #[test]
    fn test_mqtt_value_serde() {
        let v = MqttValue {
            mac: "M".into(),
            r#type: "sensor".into(),
            field: "f".into(),
            topic: "state".into(),
            value: "1".into(),
            created_at: "c".into(),
            updated_at: "u".into(),
        };
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(serde_json::from_str::<MqttValue>(&json).unwrap(), v);
    }

    #[test]
    fn test_device_config_roundtrip_keeps_api_field_names() {
        let cfg: DeviceConfig = serde_json::from_str(
            r#"{
                "mac": "AA_BB_CC_DD_EE_00",
                "device_name": "Living Room Light",
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-01T00:00:00Z",
                "mqtt_configs": [
                    {
                        "type": "sensor",
                        "name": "temperature",
                        "config_payload": {"device_class": "temperature"},
                        "default_state": "abc",
                        "default_command": "def"
                    }
                ],
                "reply_topics": [
                    {"type": "sensor", "name": "temperature/state", "key": "temp/ste"}
                ],
                "esp_now_key_mappings": {
                    "act/ste": {"type": "sensor", "name": "temperature/state"}
                },
                "wake_msg": "xxx"
            }"#,
        )
        .unwrap();
        assert_eq!(cfg.mac, "AA_BB_CC_DD_EE_00");
        assert_eq!(cfg.wake_msg, "xxx");
        let item = cfg.mqtt_configs.as_ref().unwrap()[0].clone();
        assert_eq!(item.default_state.as_deref(), Some("abc"));
        assert_eq!(item.default_command.as_deref(), Some("def"));
        assert_eq!(cfg.key_for("sensor", "temperature/state"), Some("temp/ste"));
        assert_eq!(cfg.mapping_for("act/ste").unwrap().name, "temperature/state");
        // JSON column content uses API field names; DB column names differ.
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains("\"reply_topics\""));
        assert!(json.contains("\"esp_now_key_mappings\""));
    }

    #[test]
    fn test_optional_fields_default() {
        let cfg: DeviceConfig = serde_json::from_str(
            r#"{"mac": "AA_BB_CC_DD_EE_00", "device_name": "x", "wake_msg": "w", "created_at": "t", "updated_at": "t"}"#,
        )
        .unwrap();
        assert!(cfg.mqtt_configs.is_none());
        assert!(cfg.reply_topics.is_none());
        assert!(cfg.esp_now_key_mappings.is_none());
    }
}
