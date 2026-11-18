//! Runtime configuration from environment variables.

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub serial_port: String,
    pub baud_rate: u32,
    // TODO: mqtt need user and password too
    pub mqtt_host: String,
    pub mqtt_port: u16,
    pub api_port: u16,
    pub db_path: String,
}

impl AppConfig {
    pub fn from_env() -> Self {
        Self {
            serial_port: env("SERIAL_PORT", "/dev/ttyUSB0"),
            baud_rate: env_parse("BAUD_RATE", 115200),
            mqtt_host: env("MQTT_HOST", "localhost"),
            mqtt_port: env_parse("MQTT_PORT", 1883),
            api_port: env_parse("API_PORT", 8080),
            db_path: env("DB_PATH", "/var/lib/iot_host/devices.db"),
        }
    }
}

fn env(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

fn env_parse<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
