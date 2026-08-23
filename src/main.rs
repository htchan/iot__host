mod message;
mod serial;
mod mqtt;
mod converter;
mod api;
mod device_store;
mod device_state;

use message::Message;
use serial::{SerialConfig, SerialHandler, list_ports};
use mqtt::{MqttHandler, MqttConfig};
use api::{AppState, create_router};
use device_store::DeviceStore;
use device_state::DeviceState;

use log::{info, warn, error};
use std::env;
use std::io::BufRead;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    info!("IoT Host starting...");

    // Parse configuration from environment
    let serial_port = env::var("SERIAL_PORT").unwrap_or("/dev/ttyUSB0".to_string());
    let baud_rate: u32 = env::var("BAUD_RATE")
        .unwrap_or("115200".to_string())
        .parse()
        .unwrap_or(115200);

    let mqtt_host = env::var("MQTT_HOST").unwrap_or("localhost".to_string());
    let mqtt_port: u16 = env::var("MQTT_PORT")
        .unwrap_or("1883".to_string())
        .parse()
        .unwrap_or(1883);

    let api_port: u16 = env::var("API_PORT")
        .unwrap_or("8080".to_string())
        .parse()
        .unwrap_or(8080);

    let db_path = env::var("DB_PATH").unwrap_or("/var/lib/iot_host/devices.db".to_string());

    info!("Serial port: {} at {} baud", serial_port, baud_rate);
    info!("MQTT broker: {}:{}", mqtt_host, mqtt_port);
    info!("API server: 0.0.0.0:{}", api_port);
    info!("Database: {}", db_path);

    // Initialize device store
    let device_store = Arc::new(DeviceStore::new(&db_path).await
        .map_err(|e| format!("Failed to initialize device store: {}", e))?);

    // Initialize device state tracker (only for command events)
    let device_state = DeviceState::new();

    // List available serial ports
    match list_ports() {
        Ok(ports) => {
            if ports.is_empty() {
                warn!("No serial ports found!");
            } else {
                for port in ports {
                    info!("Found serial port: {}", port.port_name);
                }
            }
        }
        Err(e) => {
            error!("Failed to list serial ports: {}", e);
        }
    }

    // Configure serial port
    let serial_config = SerialConfig {
        port: serial_port,
        baud_rate,
        ..Default::default()
    };

    // Configure MQTT with unique client ID
    let mqtt_config = MqttConfig {
        host: mqtt_host,
        port: mqtt_port,
        client_id: format!("iot_host__{}", uuid::Uuid::new_v4().to_string()[..8].to_string()),
        ..Default::default()
    };

    // Connect to serial
    let mut serial = SerialHandler::new(serial_config)
        .map_err(|e| format!("Failed to connect to serial: {}", e))?;

    // Connect to MQTT
    let mqtt = MqttHandler::new(mqtt_config).await
        .map_err(|e| format!("Failed to connect to MQTT: {}", e))?;

    // Subscribe to all homeassistant/# topics with any QoS
    mqtt.subscribe_with_qos("homeassistant/#", rumqttc::QoS::AtMostOnce).await?;
    mqtt.subscribe_with_qos("homeassistant/#", rumqttc::QoS::AtLeastOnce).await?;
    mqtt.subscribe_with_qos("homeassistant/#", rumqttc::QoS::ExactlyOnce).await?;

    // Create channel for serial -> MQTT forwarding
    let (tx_serial, rx_serial) = std::sync::mpsc::channel::<Message>();

    // Create channel for API -> serial forwarding
    let (api_tx, mut api_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // Spawn serial reader (takes ownership of serial)
    std::thread::spawn(move || {
        // Merge API and serial read loops
        loop {
            // Check for API messages first
            if let Ok(msg) = api_rx.try_recv() {
                if let Err(e) = serial.write_message(&Message::new("placeholder", crate::message::EventType::Cmd).with_data("api_cmd", &msg)) {
                    error!("Failed to write API command to serial: {}", e);
                }
                continue;
            }

            // Check for serial messages
            let mut buffer = String::new();
            let mut reader = serial.reader();
            
            match reader.read_line(&mut buffer) {
                Ok(0) => {
                    // No data, sleep a bit
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                }
                Ok(_) => {
                    let line = buffer.trim();
                    if line.is_empty() {
                        continue;
                    }

                    info!("Received from hub: {}", line);

                    match converter::parse_serial_message(line) {
                        Ok(msg) => {
                            tx_serial.send(msg).ok();
                        }
                        Err(e) => {
                            log::warn!("Failed to parse message '{}': {}", line, e);
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to read from serial: {}", e);
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            }
        }
    });

    // Create API state
    let api_state = AppState { 
        serial_tx: api_tx,
        device_store: Some(device_store),
        mqtt_handler: Some(Arc::new(mqtt)),
        device_state: Some(Arc::new(device_state)),
    };

    // Create API router
    let app = create_router(api_state);

    // Start API server
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", api_port)).await?;
    info!("Starting API server on port {}", api_port);
    axum::serve(listener, app).await?;

    // Main bridge loop
    info!("Starting message bridge...");
    loop {
        match rx_serial.recv() {
            Ok(msg) => {
                // Forward device events to MQTT
                // Note: Caller should select appropriate topic and publish
            }
            Err(e) => {
                warn!("Serial receiver error: {}", e);
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
    }
}
