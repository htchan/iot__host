use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool, Row};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::message::EventType;

/// MQTT setup item for device discovery topics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MqttSetupItem {
    pub topic_prefix: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
}

/// Direct bind message setup item
/// For simple key-value binding where the MQTT topic is directly derived from the key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectBindMsgSetup {
    pub key: String,
    pub awake_event_topic: Option<String>,
    pub sleep_event_topic: Option<String>,
    pub command_event_topic: Option<String>,
}

/// Servo compose message setup item
/// For servo devices where multiple fields need to be composed into a single value
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServoComposeMsgSetup {
    pub key: String,
    pub command_event: Option<ServoComposeMsgSetupEvent>,
    pub sleep_event: Option<ServoComposeMsgSetupEvent>,
}

/// Command event topic configuration for msg_setup
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServoComposeMsgSetupEvent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction_topic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_topic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_value_topic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub off_value_topic: Option<String>,
}

/// Message setup item - can be either direct_bind or servo_compose
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "format")]
pub enum MsgSetupItem {
    #[serde(rename = "direct_bind")]
    DirectBind(DirectBindMsgSetup),
    #[serde(rename = "servo_compose")]
    ServoCompose(ServoComposeMsgSetup),
}

/// Result of decompose_value - maps topics to payloads for MQTT publishing
pub type DecomposeResult = HashMap<String, String>;

/// Device configuration stored in SQLite
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    pub mac: String,
    pub device_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mqtt_setup: Option<Vec<MqttSetupItem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msg_setup: Option<Vec<MsgSetupItem>>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

impl DeviceConfig {
    pub fn new(mac: impl Into<String>) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            mac: mac.into(),
            device_name: "Unknown Device".to_string(),
            mqtt_setup: None,
            msg_setup: None,
            created_at: now.clone(),
            updated_at: now,
            deleted_at: None,
        }
    }
}

/// Device store with SQLite backend
pub struct DeviceStore {
    pool: Arc<RwLock<SqlitePool>>,
}

impl DeviceStore {
    /// Create a new DeviceStore with the given database path
    pub async fn new(db_path: &str) -> Result<Self, sqlx::Error> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(db_path)
            .await?;

        // Create tables if they don't exist
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS devices (
                mac TEXT PRIMARY KEY,
                device_name TEXT NOT NULL,
                mqtt_setup TEXT,
                msg_setup TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                deleted_at TEXT
            )
            "#,
        )
        .execute(&pool)
        .await?;

        Ok(Self {
            pool: Arc::new(RwLock::new(pool)),
        })
    }

    /// Get device config by MAC address
    pub async fn get_device(&self, mac: &str) -> Result<Option<DeviceConfig>, sqlx::Error> {
        let pool = self.pool.read().await;
        let row = sqlx::query(
            "SELECT mac, device_name, mqtt_setup, msg_setup, created_at, updated_at, deleted_at FROM devices WHERE mac = ?",
        )
        .bind(mac)
        .fetch_optional(&*pool)
        .await?;

        Ok(row.map(|r| {
            let mqtt_setup_str: Option<String> = r.get(2);
            let msg_setup_str: Option<String> = r.get(3);
            let deleted_at: Option<String> = r.get(6);

            DeviceConfig {
                mac: r.get(0),
                device_name: r.get(1),
                mqtt_setup: mqtt_setup_str.and_then(|s| serde_json::from_str(&s).ok()),
                msg_setup: msg_setup_str.and_then(|s| serde_json::from_str(&s).ok()),
                created_at: r.get(4),
                updated_at: r.get(5),
                deleted_at,
            }
        }))
    }

    /// Insert or update device config
    pub async fn upsert_device(&self, config: &DeviceConfig) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().to_rfc3339();
        let pool = self.pool.write().await;

        let mqtt_setup_str = config.mqtt_setup.as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default())
            .unwrap_or_default();
        let msg_setup_str = config.msg_setup.as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default())
            .unwrap_or_default();
        let deleted_at = config.deleted_at.clone();

        sqlx::query(
            r#"
            INSERT INTO devices (mac, device_name, mqtt_setup, msg_setup, created_at, updated_at, deleted_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(mac) DO UPDATE SET
                device_name = excluded.device_name,
                mqtt_setup = excluded.mqtt_setup,
                msg_setup = excluded.msg_setup,
                updated_at = excluded.updated_at,
                deleted_at = excluded.deleted_at
            "#,
        )
        .bind(&config.mac)
        .bind(&config.device_name)
        .bind(&mqtt_setup_str)
        .bind(&msg_setup_str)
        .bind(&config.created_at)
        .bind(now)
        .bind(deleted_at)
        .execute(&*pool)
        .await?;

        Ok(())
    }

    /// List all devices
    pub async fn list_devices(&self) -> Result<Vec<DeviceConfig>, sqlx::Error> {
        let pool = self.pool.read().await;
        let rows = sqlx::query(
            "SELECT mac, device_name, mqtt_setup, msg_setup, created_at, updated_at, deleted_at FROM devices ORDER BY created_at DESC",
        )
        .fetch_all(&*pool)
        .await?;

        let mut devices = Vec::new();
        for row in rows {
            let mqtt_setup_str: Option<String> = row.get(2);
            let msg_setup_str: Option<String> = row.get(3);
            let deleted_at: Option<String> = row.get(6);

            devices.push(DeviceConfig {
                mac: row.get(0),
                device_name: row.get(1),
                mqtt_setup: mqtt_setup_str.and_then(|s| serde_json::from_str(&s).ok()),
                msg_setup: msg_setup_str.and_then(|s| serde_json::from_str(&s).ok()),
                created_at: row.get(4),
                updated_at: row.get(5),
                deleted_at,
            });
        }

        Ok(devices)
    }

    /// Get MQTT setup items for a device
    pub async fn get_mqtt_setup(&self, mac: &str) -> Result<Vec<MqttSetupItem>, sqlx::Error> {
        let pool = self.pool.read().await;
        let result: Option<String> = sqlx::query_scalar(
            "SELECT mqtt_setup FROM devices WHERE mac = ?",
        )
        .bind(mac)
        .fetch_optional(&*pool)
        .await?;

        Ok(result
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default())
    }

    /// Get specific MQTT setup item by topic prefix
    pub async fn get_mqtt_setup_by_topic(&self, mac: &str, topic_prefix: &str) -> Result<Option<MqttSetupItem>, sqlx::Error> {
        let all_setup = self.get_mqtt_setup(mac).await?;
        Ok(all_setup.into_iter().find(|item| item.topic_prefix == topic_prefix))
    }

    /// Get message setup items for a device
    pub async fn get_msg_setup(&self, mac: &str) -> Result<Vec<MsgSetupItem>, sqlx::Error> {
        let pool = self.pool.read().await;
        let result: Option<String> = sqlx::query_scalar(
            "SELECT msg_setup FROM devices WHERE mac = ?",
        )
        .bind(mac)
        .fetch_optional(&*pool)
        .await?;

        Ok(result
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default())
    }

    /// Get specific message setup item by key
    pub async fn get_msg_setup_by_key(
        &self,
        mac: &str,
        key: &str,
    ) -> Result<Option<MsgSetupItem>, sqlx::Error> {
        let all_setup = self.get_msg_setup(mac).await?;
        Ok(all_setup.into_iter().find(|item| {
            match item {
                MsgSetupItem::DirectBind(d) => d.key == key,
                MsgSetupItem::ServoCompose(s) => s.key == key,
            }
        }))
    }

    /// Format MAC address with underscores for MQTT topics
    pub fn format_mac_for_topic(mac: &str) -> String {
        mac.replace('-', "_").to_uppercase()
    }

    /// Build state topic from topic prefix
    pub fn build_state_topic(topic_prefix: &str) -> String {
        format!("{}/state", topic_prefix)
    }

    /// Build config topic from topic prefix
    pub fn build_config_topic(topic_prefix: &str) -> String {
        format!("{}/config", topic_prefix)
    }

    /// Build command topic from topic prefix
    pub fn build_command_topic(topic_prefix: &str) -> String {
        format!("{}/command", topic_prefix)
    }

    /// Error type for compose_value when required event config is missing
    #[derive(Debug)]
    pub enum ComposeError {
        MissingEventConfig(String),
    }

    /// Compose value from msg_setup for serial message
    /// Takes the setup item, event type, and all payloads for the device, extracts required fields
    /// and composes the serial message value
    /// 
    /// Returns Ok(value) or Err(ComposeError) if event config is missing
    pub fn compose_value(
        setup: &MsgSetupItem,
        event: &EventType,
        payloads: &HashMap<String, String>,
    ) -> Result<String, ComposeError> {
        match setup {
            MsgSetupItem::ServoCompose(servo) => {
                // Get the correct event config based on event type
                let event_config = match event {
                    EventType::Awake => {
                        return Err(ComposeError::MissingEventConfig("awake event not supported for servo_compose".to_string()))
                    }
                    EventType::Sleep => {
                        servo.sleep_event.as_ref()
                            .ok_or_else(|| ComposeError::MissingEventConfig("sleep_event config missing".to_string()))?
                    }
                    EventType::Cmd => {
                        servo.command_event.as_ref()
                            .ok_or_else(|| ComposeError::MissingEventConfig("command_event config missing".to_string()))?
                    }
                };

                // Get all required values from payloads
                let direction = payloads.get(&event_config.direction_topic.clone().unwrap_or_default())
                    .cloned()
                    .unwrap_or_default();
                let action = payloads.get(&event_config.action_topic.clone().unwrap_or_default())
                    .cloned()
                    .unwrap_or_default();
                let on_value = payloads.get(&event_config.on_value_topic.clone().unwrap_or_default())
                    .cloned()
                    .or(None);
                let off_value = payloads.get(&event_config.off_value_topic.clone().unwrap_or_default())
                    .cloned()
                    .or(None);

                // Servo compose logic:
                // Only "on" and "off" actions produce valid values
                if action == "on" {
                    let val = on_value.unwrap_or_else(|| "0".to_string());
                    if direction == "clock" {
                        Ok(format!("+{}", val))
                    } else {
                        Ok(format!("-{}", val))
                    }
                } else if action == "off" {
                    let val = off_value.unwrap_or_else(|| "0".to_string());
                    if direction == "clock" {
                        Ok(format!("-{}", val))
                    } else {
                        Ok(format!("+{}", val))
                    }
                } else {
                    Ok("n/a".to_string())
                }
            }
            MsgSetupItem::DirectBind(direct) => {
                // For direct_bind, use awake_event_topic or sleep_event_topic based on event type
                let topic = match event {
                    EventType::Awake => direct.awake_event_topic.clone(),
                    EventType::Sleep => direct.sleep_event_topic.clone(),
                    EventType::Cmd => direct.command_event_topic.clone(),
                };
                
                if let Some(t) = topic {
                    if let Some(payload) = payloads.get(&t) {
                        return Ok(payload.clone());
                    }
                }
                
                Ok("".to_string())
            }
        }
    }

    /// Decompose value for MQTT message from serial data
    /// Returns a map of (topic -> payload) for MQTT publishing
    pub fn decompose_value(
        setup: &MsgSetupItem,
        event: &EventType,
        value: &str,
    ) -> DecomposeResult {
        let mut result = HashMap::new();

        match setup {
            MsgSetupItem::ServoCompose(servo) => {
                // Get the correct event config based on event type
                let event_config = match event {
                    EventType::Sleep => {
                        servo.sleep_event.as_ref()
                    }
                    EventType::Cmd => {
                        servo.command_event.as_ref()
                    }
                    EventType::Awake => {
                        // No action for awake event
                        return result;
                    }
                };
                
                if let Some(config) = event_config {
                    // For decompose, action is always "null" because the action was already performed
                    if value == "n/a" {
                        if let Some(topic) = &config.direction_topic {
                            result.insert(topic.clone(), "".to_string());
                        }
                        if let Some(topic) = &config.action_topic {
                            result.insert(topic.clone(), "null".to_string());
                        }
                        if let Some(topic) = &config.on_value_topic {
                            result.insert(topic.clone(), "n/a".to_string());
                        }
                        if let Some(topic) = &config.off_value_topic {
                            result.insert(topic.clone(), "n/a".to_string());
                        }
                    } else if value.starts_with('+') {
                        // e.g., "+20" -> direction=clock, action=null, on_value=20
                        let val = value[1..].to_string();
                        if let Some(topic) = &config.direction_topic {
                            result.insert(topic.clone(), "clock".to_string());
                        }
                        if let Some(topic) = &config.action_topic {
                            result.insert(topic.clone(), "null".to_string());
                        }
                        if let Some(topic) = &config.on_value_topic {
                            result.insert(topic.clone(), val);
                        }
                    } else if value.starts_with('-') {
                        // e.g., "-20" -> direction=anticlock, action=null, on_value=20
                        let val = value[1..].to_string();
                        if let Some(topic) = &config.direction_topic {
                            result.insert(topic.clone(), "anticlock".to_string());
                        }
                        if let Some(topic) = &config.action_topic {
                            result.insert(topic.clone(), "null".to_string());
                        }
                        if let Some(topic) = &config.on_value_topic {
                            result.insert(topic.clone(), val);
                        }
                    }
                }
            }
            MsgSetupItem::DirectBind(direct) => {
                // Use the correct topic based on event type
                let topic = match event {
                    EventType::Awake => direct.awake_event_topic.clone(),
                    EventType::Sleep => direct.sleep_event_topic.clone(),
                    EventType::Cmd => direct.command_event_topic.clone(),
                };
                
                if let Some(t) = topic {
                    result.insert(t, value.to_string());
                }
            }
        }

        result
    }
}
