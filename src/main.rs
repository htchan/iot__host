//! IoT Host — serial <-> MQTT bridge (docs/v1).
//!
//! Wiring: config -> sqlite -> mqtt -> serial -> bridge tasks -> axum API.
//! Setup messages to the hub are sent by the bridge when the serial port
//! reports PortOpened (startup and every reconnect).

mod api;
mod bridge;
mod config;
mod db;
mod mqtt;
mod model;
mod protocol;
mod serial;

use std::sync::Arc;

use log::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .init();

    let cfg = config::AppConfig::from_env();
    info!("iot__host starting");

    match serial::list_ports() {
        Ok(ports) if ports.is_empty() => log::warn!("no serial ports found"),
        Ok(ports) => {
            for p in &ports {
                info!("found serial port: {}", p.port_name);
            }
        }
        Err(e) => log::error!("failed to list serial ports: {}", e),
    }

    info!("serial: {} @ {} baud", cfg.serial_port, cfg.baud_rate);
    info!("mqtt: {}:{}", cfg.mqtt_host, cfg.mqtt_port);
    info!("api: 0.0.0.0:{}", cfg.api_port);
    info!("db: {}", cfg.db_path);

    let store = db::DeviceStore::new(&cfg.db_path)
        .await
        .map_err(|e| format!("db init failed: {}", e))?;

    let (mqtt, mqtt_rx) = mqtt::MqttClient::connect(mqtt::MqttConfig {
        host: cfg.mqtt_host.clone(),
        port: cfg.mqtt_port,
        client_id: format!("iot_host__{}", &uuid::Uuid::new_v4().to_string()[..8]),
        username: cfg.mqtt_username.clone(),
        password: cfg.mqtt_password.clone(),
    })
    .await
    .map_err(|e| format!("mqtt connect failed: {}", e))?;

    let channel = serial::spawn(&cfg.serial_port, cfg.baud_rate)
        .map_err(|e| format!("serial init failed: {}", e))?;

    let bridge = Arc::new(bridge::Bridge::new(
        store.clone(),
        mqtt.clone(),
        channel.outgoing.clone(),
    ));

    // hub -> host: serial lines into the bridge
    tokio::spawn({
        let bridge = bridge.clone();
        async move { bridge.handle_serial_stream(channel.incoming).await }
    });

    // broker -> host: mqtt messages into the bridge
    tokio::spawn({
        let bridge = bridge.clone();
        async move { bridge.handle_mqtt_stream(mqtt_rx).await }
    });

    let state = api::AppState {
        store: store.clone(),
        mqtt: mqtt.clone(),
    };
    let app = api::create_router(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", cfg.api_port)).await?;
    info!("API server listening on 0.0.0.0:{}", cfg.api_port);
    axum::serve(listener, app).await?;

    Ok(())
}
