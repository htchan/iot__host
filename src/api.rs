//! REST API (docs/v1/api-design.md).
//!
//! - `POST /api/devices` — upsert device config; on first create (or revival of a
//!   soft-deleted device) publishes the `mqtt_configs` payloads to
//!   `homeassistant/<type>/<mac>/<field>/config` and publishes `default_state` /
//!   `default_command` to `.../state` / `.../command` before seeding them into
//!   `mqtt_topic_values`.
//! - `DELETE /api/devices/{mac}` — soft delete.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{delete, post},
    Router,
};
use serde::Serialize;

use crate::db::{DeviceStore, UpsertOutcome};
use crate::model::{
    is_valid_mac, normalize_mac, compose_mqtt_topic, DeviceConfig, KeyMapping, MqttConfigItem,
    ReplyTopic,
};
use crate::mqtt::MqttClient;

/// Shared API state.
#[derive(Clone)]
pub struct AppState {
    pub store: DeviceStore,
    pub mqtt: MqttClient,
}

/// Standard response body per api-design.md.
#[derive(Serialize)]
pub struct ApiResponse {
    pub success: bool,
    pub message: String,
    pub device_id: String,
}

/// Error body (shape kept consistent with ApiResponse, minus device_id).
#[derive(Serialize)]
pub struct ApiError {
    pub success: bool,
    pub message: String,
}

/// Request body for POST /api/devices.
#[derive(serde::Deserialize)]
pub struct UpsertDeviceRequest {
    pub mac: String,
    pub device_name: String,
    #[serde(default)]
    pub mqtt_configs: Option<Vec<MqttConfigItem>>,
    #[serde(default)]
    pub reply_topics: Option<Vec<ReplyTopic>>,
    #[serde(default)]
    pub esp_now_key_mappings: Option<std::collections::HashMap<String, KeyMapping>>,
    pub wake_msg: String,
    #[serde(default)]
    pub encryption_key: String,
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/api/devices", post(upsert_device))
        .route("/api/devices/{mac}", delete(delete_device))
        .with_state(state)
}

async fn upsert_device(
    State(state): State<AppState>,
    Json(payload): Json<UpsertDeviceRequest>,
) -> Result<Json<ApiResponse>, (StatusCode, Json<ApiError>)> {
    let mac = normalize_mac(&payload.mac);
    if !is_valid_mac(&mac) {
        return Err(bad_request("invalid mac; expected AA_BB_CC_DD_EE_00"));
    }
    if payload.device_name.trim().is_empty() {
        return Err(bad_request("device_name must not be empty"));
    }
    if payload.wake_msg.is_empty() {
        return Err(bad_request("wake_msg must not be empty"));
    }

    let now = crate::model::now_rfc3339();
    let config = DeviceConfig {
        mac: mac.clone(),
        device_name: payload.device_name.clone(),
        mqtt_configs: payload.mqtt_configs.clone(),
        reply_topics: payload.reply_topics.clone(),
        esp_now_key_mappings: payload.esp_now_key_mappings.clone(),
        wake_msg: payload.wake_msg.clone(),
        encryption_key: payload.encryption_key.clone(),
        created_at: now.clone(),
        updated_at: now,
        deleted_at: None,
    };

    let outcome: UpsertOutcome = state
        .store
        .upsert_device(&config)
        .await
        .map_err(|e| internal(format!("failed to save device config: {}", e)))?;

    if outcome.republish_configs {
        if let Some(items) = &payload.mqtt_configs {
            for item in items {
                let topic = format!(
                    "{}/config",
                    compose_mqtt_topic(&item.field_type, &mac, &item.field)
                );
                let payload = serde_json::to_vec(&item.config_payload).unwrap_or_default();
                if let Err(e) = state
                    .mqtt
                    .publish(&topic, &String::from_utf8_lossy(&payload))
                    .await
                {
                    return Err(internal(format!(
                        "failed to publish config to {}: {}",
                        topic, e
                    )));
                }
            }
        }
    }

    if outcome.seed_defaults {
        if let Some(items) = &payload.mqtt_configs {
            for item in items {
                // Publish the defaults onto MQTT first so HA subscribers observe
                // the initial values before they are persisted to the DB.
                if let Some(v) = &item.default_state {
                    let topic = format!(
                        "{}/state",
                        compose_mqtt_topic(&item.field_type, &mac, &item.field)
                    );
                    if let Err(e) = state.mqtt.publish(&topic, v).await {
                        return Err(internal(format!(
                            "failed to publish default state to {}: {}",
                            topic, e
                        )));
                    }
                    let _ = state
                        .store
                        .insert_value_if_absent(&mac, &item.field_type, &item.field, "state", v)
                        .await;
                }
                if let Some(v) = &item.default_command {
                    let topic = format!(
                        "{}/command",
                        compose_mqtt_topic(&item.field_type, &mac, &item.field)
                    );
                    if let Err(e) = state.mqtt.publish(&topic, v).await {
                        return Err(internal(format!(
                            "failed to publish default command to {}: {}",
                            topic, e
                        )));
                    }
                    let _ = state
                        .store
                        .insert_value_if_absent(&mac, &item.field_type, &item.field, "command", v)
                        .await;
                }
            }
        }
    }

    Ok(Json(ApiResponse {
        success: true,
        message: format!("Device config saved for {}", mac),
        device_id: mac,
    }))
}

async fn delete_device(
    State(state): State<AppState>,
    Path(mac): Path<String>,
) -> Result<Json<ApiResponse>, (StatusCode, Json<ApiError>)> {
    let mac = normalize_mac(&mac);
    if !is_valid_mac(&mac) {
        return Err(bad_request("invalid mac; expected AA_BB_CC_DD_EE_00"));
    }

    let deleted = state
        .store
        .soft_delete_device(&mac)
        .await
        .map_err(|e| internal(format!("failed to delete device: {}", e)))?;
    if !deleted {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ApiError {
                success: false,
                message: format!("Device {} not found", mac),
            }),
        ));
    }

    Ok(Json(ApiResponse {
        success: true,
        message: format!("Device {} deleted", mac),
        device_id: mac,
    }))
}

fn bad_request(msg: &str) -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ApiError {
            success: false,
            message: msg.to_string(),
        }),
    )
}

fn internal(msg: String) -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiError {
            success: false,
            message: msg,
        }),
    )
}
