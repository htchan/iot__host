//! Serial / ESP-Now message protocol (docs/v2/serial-communication-design.md,
//! docs/v2/esp-now-design.md).
//!
//! ESP-Now payload: `"<field>/<topic>:<value>","<field>/<topic>:<value>"`
//! (a JSON string array without `[` `]`).
//!
//! Serial line: `[<mac>]<esp-now payload>`, e.g.
//! `[AA_BB_CC_DD_EE_00]"temp/ste:25.5","humi/ste:60"`.
//!
//! Special host→hub lines (serial-communication-design.md):
//! - setup: `[<mac>]setup/<wake_msg>`
//! - command: the standard format composed from the device's `command` values.
//!
//! The host never enforces the 224-byte ESP-Now limit; the hub splits long
//! messages. No crypto at this layer (encryption is hub↔device only).

// TODO: should this file either put into model / serial?

use crate::model::normalize_mac;

/// A parsed serial line from the hub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialMessage {
    pub mac: String,
    /// `(key, value)` pairs, key in the short `<field>/<topic>` wire form.
    pub pairs: Vec<(String, String)>,
}

/// Parse a serial line: `[<mac>]"k/v:v1","k/v:v2"`.
///
/// The payload part is parsed as a JSON array of strings (brackets added back),
/// so quoted strings with escapes are handled correctly.
pub fn parse_serial_line(line: &str) -> Result<SerialMessage, String> {
    let line = line.trim();
    if !line.starts_with('[') {
        return Err("line does not start with '['".to_string());
    }
    let close = line
        .find(']')
        .ok_or_else(|| "missing ']' after mac".to_string())?;
    let raw_mac = &line[1..close];
    let mac = normalize_mac(raw_mac);
    if !super::model::is_valid_mac(&mac) {
        return Err(format!("invalid mac in line: {}", line));
    }

    let payload = &line[close + 1..];
    if payload.is_empty() {
        return Ok(SerialMessage { mac, pairs: vec![] });
    }
    // TODO: add comment to tell that setup msg will only be sent from host to hub via serial so host dont need logic to parse setup msg
    if !payload.starts_with('"') {
        return Err(format!("payload must start with a quoted pair: {}", line));
    }

    let json = format!("[{}]", payload);
    let elements: Vec<String> = serde_json::from_str(&json)
        .map_err(|e| format!("payload is not a valid string array: {} ({})", e, line))?;

    let mut pairs = Vec::with_capacity(elements.len());
    for el in elements {
        let (key, value) = el
            .split_once(':')
            .ok_or_else(|| format!("pair without ':' in line: {}", line))?;
        pairs.push((key.to_string(), value.to_string()));
    }

    Ok(SerialMessage { mac, pairs })
}

/// Compose the ESP-Now payload (no mac prefix) from key/value pairs.
// TODO: add extra requirement, if length of pair items longer than 20 character. it will be invalid and we should log for it
pub fn compose_esp_now_payload(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| {
            let element = format!("{}:{}", k, v);
            serde_json::to_string(&element).expect("string serialization cannot fail")
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Compose a full serial line `[<mac>]"k/v:v1","k/v:v2"`.
///
/// Returns `[<mac>]` (no payload) when `pairs` is empty; callers are expected
/// to skip sending such lines.
pub fn compose_serial_line(mac: &str, pairs: &[(String, String)]) -> String {
    format!("[{}]{}", mac, compose_esp_now_payload(pairs))
}

/// Compose the special setup line `[<mac>]setup/<encryption_key>/<wake_msg>`.
pub fn compose_setup_line(mac: &str, encryption_key: &str, wake_msg: &str) -> String {
    format!("[{}]setup/{}/{}", mac, encryption_key, wake_msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic_line() {
        let msg = parse_serial_line(r#"[AA_BB_CC_DD_EE_00]"temp/ste:25.5","humi/ste:60""#)
            .expect("parse");
        assert_eq!(msg.mac, "AA_BB_CC_DD_EE_00");
        assert_eq!(
            msg.pairs,
            vec![
                ("temp/ste".to_string(), "25.5".to_string()),
                ("humi/ste".to_string(), "60".to_string())
            ]
        );
    }

    #[test]
    fn test_parse_normalizes_mac() {
        let msg = parse_serial_line(r#"[aa-bb-cc-dd-ee-ff]"k/v:1""#).expect("parse");
        assert_eq!(msg.mac, "AA_BB_CC_DD_EE_FF");
    }

    #[test]
    fn test_parse_value_with_colon_and_slash() {
        let msg = parse_serial_line(r#"[AA_BB_CC_DD_EE_00]"act/ste:on:extra/x""#).expect("parse");
        assert_eq!(msg.pairs[0].0, "act/ste");
        assert_eq!(msg.pairs[0].1, "on:extra/x");
    }

    #[test]
    fn test_parse_empty_payload() {
        let msg = parse_serial_line("[AA_BB_CC_DD_EE_00]").expect("parse");
        assert!(msg.pairs.is_empty());
    }

    #[test]
    fn test_parse_rejects_garbage() {
        assert!(parse_serial_line(r#"[AA_BB_CC_DD_EE_00]temp/ste:1"#).is_err());
        assert!(parse_serial_line(r#"[not_a_mac]"k/v:1""#).is_err());
        assert!(parse_serial_line(r#"[AA_BB_CC_DD_EE_00]"novalue""#).is_err());
        assert!(parse_serial_line("AA_BB_CC_DD_EE_00 \"k/v:1\"").is_err());
        assert!(parse_serial_line("").is_err());
    }

    #[test]
    fn test_compose_roundtrip() {
        let pairs = vec![
            ("temp/ste".to_string(), "25.5".to_string()),
            ("humi/ste".to_string(), "60".to_string()),
        ];
        let line = compose_serial_line("AA_BB_CC_DD_EE_00", &pairs);
        assert_eq!(
            line,
            r#"[AA_BB_CC_DD_EE_00]"temp/ste:25.5","humi/ste:60""#
        );
        assert_eq!(parse_serial_line(&line).expect("parse").pairs, pairs);
    }

    #[test]
    fn test_compose_escapes_special_chars() {
        let pairs = vec![("k/v".to_string(), r#"a"b,c"#.to_string())];
        let line = compose_serial_line("AA_BB_CC_DD_EE_00", &pairs);
        assert_eq!(parse_serial_line(&line).expect("parse").pairs, pairs);
    }

    #[test]
    fn test_compose_empty_pairs() {
        assert_eq!(compose_serial_line("AA_BB_CC_DD_EE_00", &[]), "[AA_BB_CC_DD_EE_00]");
        assert_eq!(compose_esp_now_payload(&[]), "");
    }

    #[test]
    fn test_compose_setup_line() {
        assert_eq!(
            compose_setup_line("AA_BB_CC_DD_EE_00", "key123", "wake123"),
            "[AA_BB_CC_DD_EE_00]setup/key123/wake123"
        );
        assert_eq!(
            compose_setup_line("AA_BB_CC_DD_EE_00", "", "wake123"),
            "[AA_BB_CC_DD_EE_00]setup//wake123"
        );
    }
}
