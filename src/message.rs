use serde::{Deserialize, Serialize};

/// Maximum number of key-value pairs in a message
pub const MAX_KEY_VALUES: usize = 8;

/// Maximum length of MAC address string (xx-xx-xx-xx-xx-xx)
pub const MAC_STR_LEN: usize = 17;

/// Maximum length of event name
pub const EVENT_MAX_LEN: usize = 16;

/// Maximum length of key
pub const KEY_MAX_LEN: usize = 10;

/// Maximum length of value
pub const VALUE_MAX_LEN: usize = 6;

/// Event types supported by the protocol
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum EventType {
    Awake,
    Cmd,
    Sleep,
}

impl Default for EventType {
    fn default() -> Self {
        EventType::Awake
    }
}

impl From<&str> for EventType {
    fn from(s: &str) -> Self {
        match s {
            "awake" => EventType::Awake,
            "cmd" => EventType::Cmd,
            "sleep" => EventType::Sleep,
            _ => EventType::Awake,
        }
    }
}

impl ToString for EventType {
    fn to_string(&self) -> String {
        match self {
            EventType::Awake => "awake".to_string(),
            EventType::Cmd => "cmd".to_string(),
            EventType::Sleep => "sleep".to_string(),
        }
    }
}

/// Key-value pair data
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DataPair {
    pub key: String,
    pub value: String,
}

impl DataPair {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

/// Message structure representing an ESP-NOW event
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub mac: String,
    pub event: EventType,
    pub data: Vec<DataPair>,
}

impl Message {
    pub fn new(mac: impl Into<String>, event: EventType) -> Self {
        Self {
            mac: mac.into(),
            event,
            data: Vec::new(),
        }
    }

    pub fn with_data(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.data.push(DataPair::new(key, value));
        self
    }
}

/// MQTT topic for device events
pub fn device_topic(mac: &str) -> String {
    format!("iot/devices/{}/events", mac)
}

/// MQTT topic prefix for all devices
pub fn devices_topic_prefix() -> String {
    "iot/devices".to_string()
}