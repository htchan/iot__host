use crate::message::{Message, EventType, DataPair};
use log::info;
use rumqttc::{AsyncClient, Event, Incoming, QoS, ConnectionError, MqttOptions, ClientError};
use std::time::Duration;

/// MQTT configuration
pub struct MqttConfig {
    pub host: String,
    pub port: u16,
    pub client_id: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub keep_alive: Duration,
}

impl Default for MqttConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 1883,
            client_id: "iot_host".to_string(),
            username: None,
            password: None,
            keep_alive: Duration::from_secs(30),
        }
    }
}

/// MQTT handler - provides basic publish/subscribe functionality
/// Caller is responsible for topic selection and message formatting
#[derive(Clone)]
pub struct MqttHandler {
    client: AsyncClient,
}

impl MqttHandler {
    /// Create a new MqttHandler with the given configuration
    pub async fn new(config: MqttConfig) -> Result<Self, ConnectionError> {
        info!(
            "Connecting to MQTT broker at {}:{}",
            config.host, config.port
        );

        let mut mqtt_options = MqttOptions::new(config.client_id, config.host, config.port);
        mqtt_options.set_keep_alive(config.keep_alive);

        let (client, mut eventloop) = AsyncClient::new(mqtt_options, 10);

        // Spawn event loop in background
        tokio::spawn(async move {
            loop {
                match eventloop.poll().await {
                    Ok(Event::Incoming(Incoming::Publish(publish))) => {
                        info!("Received MQTT message on topic: {}", publish.topic);
                    }
                    Ok(Event::Incoming(_)) => {}
                    Ok(Event::Outgoing(_)) => {}
                    Err(e) => {
                        log::error!("MQTT error: {}", e);
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        });

        Ok(Self { client })
    }

    /// Subscribe to a topic with QoS 1 (AtLeastOnce)
    pub async fn subscribe(&self, topic: &str) -> Result<(), ClientError> {
        info!("Subscribing to topic: {}", topic);
        self.client.subscribe(topic, QoS::AtLeastOnce).await?;
        Ok(())
    }

    /// Subscribe to a topic with specified QoS
    pub async fn subscribe_with_qos(&self, topic: &str, qos: QoS) -> Result<(), ClientError> {
        info!("Subscribing to topic: {} with QoS {}", topic, qos as u8);
        self.client.subscribe(topic, qos).await?;
        Ok(())
    }

    /// Publish a message to a topic with QoS 1 and retain=true
    pub async fn publish(&self, topic: &str, payload: &[u8]) -> Result<(), ClientError> {
        info!("Publishing to {}: {}", topic, String::from_utf8_lossy(payload));
        self.client.publish(topic, QoS::AtLeastOnce, true, payload).await?;
        Ok(())
    }

    /// Format MAC address with underscores for MQTT topics
    pub fn format_mac_for_topic(mac: &str) -> String {
        mac.replace('-', "_").to_uppercase()
    }
}
