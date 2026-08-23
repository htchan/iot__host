use crate::message::{Message, EventType, DataPair};
use std::collections::HashMap;

/// In-memory device state tracking
/// Stores both MQTT payloads (for compose) and command messages (for serial forwarding)
#[derive(Clone, Default)]
pub struct DeviceState {
    /// Stores all MQTT payloads: mac -> (topic -> payload)
    payloads: std::sync::Arc<std::sync::RwLock<HashMap<String, HashMap<String, String>>>>,
    /// Stores command messages: mac -> Message with EventType::Cmd
    commands: std::sync::Arc<std::sync::RwLock<HashMap<String, Message>>>,
}

impl DeviceState {
    /// Create a new DeviceState
    pub fn new() -> Self {
        Self {
            payloads: std::sync::Arc::new(std::sync::RwLock::new(HashMap::new())),
            commands: std::sync::Arc::new(std::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Extract MAC address from MQTT topic
    /// Topics should contain MAC in format: homeassistant/{device_class}/{MAC}/{field}/...
    /// MAC can be with dashes (xx-xx-xx-xx-xx-xx) or underscores (xx_xx_xx_xx_xx_xx)
    pub fn extract_mac_from_topic(topic: &str) -> Option<String> {
        let parts: Vec<&str> = topic.split('/').collect();
        
        for part in parts.iter() {
            if part.len() == 17 {
                let hex_chars: String = part.chars()
                    .filter(|c| *c != '-' && *c != '_')
                    .collect();
                if hex_chars.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Some(part.replace('-', "_").to_uppercase());
                }
            }
        }
        
        None
    }

    /// Store an MQTT payload with MAC extracted from topic
    pub fn store_payload(&self, topic: &str, payload: &str) {
        if let Some(mac) = Self::extract_mac_from_topic(topic) {
            let mut payloads = self.payloads.write().unwrap();
            let entry = payloads.entry(mac).or_insert_with(HashMap::new);
            entry.insert(topic.to_string(), payload.to_string());
        }
    }

    /// Get the payload for a specific topic
    pub fn get_payload(&self, mac: &str, topic: &str) -> Option<String> {
        let payloads = self.payloads.read().ok()?;
        payloads.get(mac).and_then(|topics| topics.get(topic)).cloned()
    }

    /// Get all payloads for a device
    pub fn get_all_payloads(&self, mac: &str) -> Option<HashMap<String, String>> {
        let payloads = self.payloads.read().ok()?;
        payloads.get(mac).cloned()
    }

    /// Update a command message for a device (merges data pairs into existing command message)
    pub fn update_command(&self, mac: &str, key: &str, value: &str) -> Message {
        let mut commands = self.commands.write().unwrap();
        let entry = commands.entry(mac.to_string()).or_insert_with(|| {
            Message::new(mac, EventType::Cmd)
        });
        
        let mut found = false;
        for pair in &mut entry.data {
            if pair.key == key {
                pair.value = value.to_string();
                found = true;
                break;
            }
        }
        if !found {
            entry.data.push(DataPair::new(key, value));
        }
        
        entry.clone()
    }

    /// Get the command message for a device
    pub fn get_command(&self, mac: &str) -> Option<Message> {
        let commands = self.commands.read().ok()?;
        commands.get(mac).cloned()
    }

    /// Delete the command message for a device
    pub fn delete_command(&self, mac: &str) {
        let mut commands = self.commands.write().unwrap();
        commands.remove(mac);
    }

    /// Delete all payloads for a device
    pub fn delete_payloads(&self, mac: &str) {
        let mut payloads = self.payloads.write().unwrap();
        payloads.remove(mac);
    }

    /// Delete all state for a device
    pub fn delete_all(&self, mac: &str) {
        self.delete_payloads(mac);
        self.delete_command(mac);
    }

    /// Clear all device states
    pub fn clear(&self) {
        let mut payloads = self.payloads.write().unwrap();
        payloads.clear();
        let mut commands = self.commands.write().unwrap();
        commands.clear();
    }
}
