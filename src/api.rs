use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{post, delete},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::device_store::{DeviceStore, DeviceConfig};
use crate::device_state::DeviceState;

/// Request payload for upsert device config
#[derive(Debug, Deserialize)]
pub struct UpsertDeviceRequest {
    pub mac: String,
    pub device_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mqtt_setup: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msg_setup: Option<serde_json::Value>,
}

/// Request payload for publish all devices
#[derive(Debug, Deserialize)]
pub struct PublishAllRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mac: Option<String>,
}

/// Response payload
#[derive(Debug, Serialize)]
pub struct ApiResponse {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
}

/// Shared state for the API
#[derive(Clone)]
pub struct AppState {
    pub serial_tx: mpsc::UnboundedSender<String>,
    pub device_store: Option<Arc<DeviceStore>>,
    pub mqtt_handler: Option<Arc<crate::mqtt::MqttHandler>>,
    pub device_state: Option<Arc<DeviceState>>,
}

/// Create the API router
pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/api/device", post(upsert_device))
        .route("/api/device/publish-all", post(publish_all_devices))
        .route("/api/device/{mac}", delete(delete_device))
        .with_state(state)
}

/// Upsert device config - POST /api/device
async fn upsert_device(
    State(state): State<AppState>,
    Json(payload): Json<UpsertDeviceRequest>,
) -> Result<Json<ApiResponse>, (StatusCode, Json<String>)> {
    let mac = payload.mac.clone();

    if let Some(store) = &state.device_store {
        let device_config = DeviceConfig {
            mac: payload.mac.clone(),
            device_name: payload.device_name.clone(),
            mqtt_setup: payload.mqtt_setup.clone().and_then(|v| serde_json::from_value(v).ok()),
            msg_setup: payload.msg_setup.clone().and_then(|v| serde_json::from_value(v).ok()),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            deleted_at: None,
        };

        if let Err(e) = store.upsert_device(&device_config).await {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(format!("Failed to save device config: {}", e)),
            ));
        }

        // Publish HA discovery for each mqtt_setup item
        if let Some(mqtt_setup) = &payload.mqtt_setup {
            if let Some(mqtt) = &state.mqtt_handler {
                // Parse the mqtt_setup array and publish discovery for each item
                if let Ok(setups) = serde_json::from_value::<Vec<serde_json::Value>>(mqtt_setup.clone()) {
                    for item in setups {
                        if let Some(topic_prefix) = item.get("topic_prefix").and_then(|v| v.as_str()) {
                            // Build config topic: topic_prefix + /config
                            let config_topic = format!("{}/config", topic_prefix);
                            
                            // Get the config payload from mqtt_setup item
                            let payload = item.get("config").cloned().unwrap_or_default();
                            let payload_bytes = serde_json::to_vec(&payload).unwrap_or_default();
                            
                            let _ = mqtt.publish(&config_topic, &payload_bytes).await;
                        }
                    }
                }
            }
        }

        Ok(Json(ApiResponse {
            success: true,
            message: format!("Device config saved for {}", mac),
            device_id: Some(mac),
            count: None,
        }))
    } else {
        Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json("DeviceStore not configured".to_string()),
        ))
    }
}

/// Publish all device configs - POST /api/device/publish-all
async fn publish_all_devices(
    State(state): State<AppState>,
    Json(payload): Json<PublishAllRequest>,
) -> Result<Json<ApiResponse>, (StatusCode, Json<String>)> {
    Err((
        StatusCode::NOT_IMPLEMENTED,
        Json("Publish-all endpoint not implemented".to_string()),
    ))
}

/// Delete device config - DELETE /api/device/{mac}
async fn delete_device(
    State(state): State<AppState>,
    Path(mac): Path<String>,
) -> Result<Json<ApiResponse>, (StatusCode, Json<String>)> {
    if let Some(store) = &state.device_store {
        let now = chrono::Utc::now().to_rfc3339();
        
        // Get existing device config
        if let Some(mut device) = store.get_device(&mac).await.map_err(|e| (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(format!("Failed to get device: {}", e)),
        ))? {
            device.deleted_at = Some(now.clone());
            device.updated_at = now;
            
            if let Err(e) = store.upsert_device(&device).await {
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(format!("Failed to delete device: {}", e)),
                ));
            }
            
            Ok(Json(ApiResponse {
                success: true,
                message: format!("Device {} deleted", mac),
                device_id: Some(mac),
                count: None,
            }))
        } else {
            Err((
                StatusCode::NOT_FOUND,
                Json(format!("Device {} not found", mac)),
            ))
        }
    } else {
        Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json("DeviceStore not configured".to_string()),
        ))
    }
}
