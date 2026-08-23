use crate::message::{DataPair, Message};

/// Parse a serial message string into a Message struct
/// Format: xx-xx-xx-xx-xx-xx:event:key1:value1:key2:value2:...
pub fn parse_serial_message(input: &str) -> Result<Message, String> {
    let input = input.trim();
    
    if input.len() < 18 {
        return Err("Input too short for valid MAC format".to_string());
    }

    // Extract MAC (first 17 characters)
    let mac = &input[0..17];
    if !is_valid_mac(mac) {
        return Err("Invalid MAC address format".to_string());
    }

    // Parse rest after MAC
    let rest = input.get(18..).ok_or("No event data after MAC")?;
    
    let parts: Vec<&str> = rest.split(':').collect();
    if parts.is_empty() {
        return Err("No event type found".to_string());
    }

    let event_type: crate::message::EventType = parts[0].into();

    let mut message = Message::new(mac, event_type);

    // Parse key-value pairs
    let mut i = 1;
    while i < parts.len() {
        if i + 1 < parts.len() {
            let key = parts[i].trim();
            let value = parts[i + 1].trim();
            
            if !key.is_empty() && !value.is_empty() {
                message.data.push(DataPair::new(key, value));
            }
            i += 2;
        } else {
            break;
        }
    }

    Ok(message)
}

/// Convert a Message struct to serial format string
/// Format: xx-xx-xx-xx-xx-xx:event:key1:value1:key2:value2:...
pub fn to_serial_message(msg: &Message) -> String {
    let mut result = msg.mac.clone();
    
    result.push(':');
    result.push_str(&msg.event.to_string());

    for pair in &msg.data {
        result.push(':');
        result.push_str(&pair.key);
        result.push(':');
        result.push_str(&pair.value);
    }

    result
}

/// Check if MAC address is valid (xx-xx-xx-xx-xx-xx format)
fn is_valid_mac(mac: &str) -> bool {
    if mac.len() != 17 {
        return false;
    }

    for (i, c) in mac.chars().enumerate() {
        if i % 3 == 2 {
            // Should be a dash
            if c != '-' {
                return false;
            }
        } else {
            // Should be hex digit
            if !c.is_ascii_hexdigit() {
                return false;
            }
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_message() {
        let input = "aa-bb-cc-dd-ee-ff:awake:temp:25.5:humi:60";
        let msg = parse_serial_message(input).unwrap();
        
        assert_eq!(msg.mac, "aa-bb-cc-dd-ee-ff");
        assert_eq!(msg.event.to_string(), "awake");
        assert_eq!(msg.data.len(), 2);
        assert_eq!(msg.data[0].key, "temp");
        assert_eq!(msg.data[0].value, "25.5");
        assert_eq!(msg.data[1].key, "humi");
        assert_eq!(msg.data[1].value, "60");
    }

    #[test]
    fn test_to_serial_message() {
        let msg = Message::new("aa-bb-cc-dd-ee-ff", crate::message::EventType::Cmd)
            .with_data("cmd", "on")
            .with_data("val", "100");
        
        let serial = to_serial_message(&msg);
        assert_eq!(serial, "aa-bb-cc-dd-ee-ff:cmd:cmd:on:val:100");
    }

    #[test]
    fn test_invalid_mac() {
        let input = "aa-bb-cc:awake";
        assert!(parse_serial_message(input).is_err());
    }
}
