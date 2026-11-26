//! MQTT client wrapper (rumqttc).
//!
//! Subscribes to `homeassistant/#` and forwards every publish to the bridge
//! as `(topic, payload)`. Publishes are QoS1; discovery configs are retained.

use std::time::Duration;

use log::{info, warn};
use rumqttc::{AsyncClient, Event, Incoming, MqttOptions, QoS};
use tokio::sync::mpsc;

/// Configuration from environment (see `config.rs`).
#[derive(Debug, Clone)]
pub struct MqttConfig {
    pub host: String,
    pub port: u16,
    pub client_id: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

pub const MQTT_TOPIC_FILTER: &str = "homeassistant/#";

/// A received MQTT message.
#[derive(Debug, Clone)]
pub struct MqttMessage {
    pub topic: String,
    pub payload: String,
}

/// Async handle to the MQTT client.
#[derive(Clone)]
pub struct MqttClient {
    client: AsyncClient,
}

impl MqttClient {
    /// Connect to the broker, spawn the eventloop task and return the client
    /// plus a receiver delivering every incoming publish.
    pub async fn connect(
        config: MqttConfig,
    ) -> Result<(Self, mpsc::UnboundedReceiver<MqttMessage>), rumqttc::ClientError> {
        let mut options = MqttOptions::new(config.client_id, config.host, config.port);
        options.set_keep_alive(Duration::from_secs(30));
        if let (Some(user), Some(pass)) = (&config.username, &config.password) {
            options.set_credentials(user, pass);
        }

        let (client, mut eventloop) = AsyncClient::new(options, 10);
        let (event_tx, event_rx) = mpsc::unbounded_channel::<MqttMessage>();

        client
            .subscribe(MQTT_TOPIC_FILTER, QoS::AtLeastOnce)
            .await?;
        info!("Subscribed to {}", MQTT_TOPIC_FILTER);

        tokio::spawn(async move {
            loop {
                match eventloop.poll().await {
                    Ok(Event::Incoming(Incoming::Publish(publish))) => {
                        let payload = String::from_utf8_lossy(&publish.payload).into_owned();
                        info!("Received MQTT on {}: {}", publish.topic, payload);
                        let msg = MqttMessage {
                            topic: publish.topic.clone(),
                            payload,
                        };
                        if event_tx.send(msg).is_err() {
                            warn!("mqtt event receiver dropped; stopping eventloop");
                            return;
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        warn!("mqtt eventloop error (will reconnect): {}", e);
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        });

        Ok((Self { client }, event_rx))
    }

    /// Construct a client without connecting (used by tests; publish calls
    /// simply queue into the client buffer).
    #[cfg(test)]
    pub fn test_client() -> Self {
        let options = MqttOptions::new("iot_host_test", "127.0.0.1", 1883);
        let (client, _) = AsyncClient::new(options, 10);
        Self { client }
    }

    /// Publish a message. All publishes are retained (HA discovery configs and
    /// last-known state/command values must survive late subscribers), so
    /// `retain` is not a caller-controlled parameter.
    pub async fn publish(&self, topic: &str, payload: &str) -> Result<(), rumqttc::ClientError> {
        info!("Publishing to {} (retain=true): {}", topic, payload);
        self.client
            .publish(topic, QoS::AtLeastOnce, true, payload)
            .await
    }
}
