//! SQLite storage (docs/v2/db-design.md).
//!
//! Tables: `iot_device_configs` and `mqtt_topic_values`.
//!
//! Documented augmentations over the design doc (constraints only):
//! - `iot_device_configs.mac` is the PRIMARY KEY ("unique key by mac" per api-design).
//! - `mqtt_topic_values` has UNIQUE(mac, type, field, topic) to allow upserts.

use std::path::Path;

use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use sqlx::Row;

use crate::model::{now_rfc3339, DeviceConfig, MqttValue};

const CREATE_MQTT_TOPIC_VALUES: &str = r#"
create table if not exists mqtt_topic_values (
  mac varchar(32) not null,
  type varchar(64) not null,
  field varchar(64) not null,
  topic varchar (16) not null,
  value varchar(1024) not null,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  unique (mac, type, field, topic)
);
"#;

const CREATE_IOT_DEVICE_CONFIGS: &str = r#"
create table if not exists iot_device_configs (
  mac varchar(32) not null primary key,
  device_name varchar(128) not null,
  mqtt_configs text,
  replay_topics text,
  topic_mappings text,
  wake_msg varchar(128) not null,
  encryption_key varchar(128) not null default '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT
);
"#;

/// Outcome of a device upsert, telling the caller which side effects to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpsertOutcome {
    /// True when (re)created: republish HA discovery configs.
    /// False for a plain update of an already-active device.
    pub republish_configs: bool,
    /// True only for a brand-new device: seed `default_state` / `default_command`.
    pub seed_defaults: bool,
}

#[derive(Clone)]
pub struct DeviceStore {
    pool: SqlitePool,
}

impl DeviceStore {
    pub async fn new(db_path: &str) -> Result<Self, sqlx::Error> {
        if let Some(parent) = Path::new(db_path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    sqlx::Error::Configuration(format!(
                        "failed to create db parent dir {}: {}",
                        parent.display(),
                        e
                    )
                    .into())
                })?;
            }
        }

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&format!("sqlite:{}?mode=rwc", db_path))
            .await?;

        sqlx::query(CREATE_MQTT_TOPIC_VALUES).execute(&pool).await?;
        sqlx::query(CREATE_IOT_DEVICE_CONFIGS).execute(&pool).await?;

        Ok(Self { pool })
    }

    /// Insert or update an active device config.
    pub async fn upsert_device(&self, config: &DeviceConfig) -> Result<UpsertOutcome, sqlx::Error> {
        let existing: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT deleted_at FROM iot_device_configs WHERE mac = ?",
        )
        .bind(&config.mac)
        .fetch_optional(&self.pool)
        .await?;

        let now = now_rfc3339();
        let (mqtt_configs, reply_topics, topic_mappings) = config.json_columns();

        // TODO: add comment to explain each case, eg. None means record does not exist
        let outcome = match existing {
            None => UpsertOutcome {
                republish_configs: true,
                seed_defaults: true,
            },
            Some((None,)) => UpsertOutcome {
                republish_configs: false,
                seed_defaults: false,
            },
            Some((Some(_),)) => UpsertOutcome {
                republish_configs: true,
                seed_defaults: false,
            },
        };

        match existing {
            None => {
                sqlx::query(
                    r#"
                    insert into iot_device_configs
                        (mac, device_name, mqtt_configs, replay_topics, topic_mappings,
                         wake_msg, encryption_key, created_at, updated_at, deleted_at)
                    values (?, ?, ?, ?, ?, ?, ?, ?, ?, NULL)
                    "#,
                )
                .bind(&config.mac)
                .bind(&config.device_name)
                .bind(mqtt_configs)
                .bind(reply_topics)
                .bind(topic_mappings)
                .bind(&config.wake_msg)
                .bind(&config.encryption_key)
                .bind(&config.created_at)
                .bind(&now)
                .execute(&self.pool)
                .await?;
            }
            _ => {
                sqlx::query(
                    r#"
                    update iot_device_configs
                    set device_name = ?, mqtt_configs = ?, replay_topics = ?,
                        topic_mappings = ?, wake_msg = ?, encryption_key = ?,
                        updated_at = ?, deleted_at = NULL
                    where mac = ?
                    "#,
                )
                .bind(&config.device_name)
                .bind(mqtt_configs)
                .bind(reply_topics)
                .bind(topic_mappings)
                .bind(&config.wake_msg)
                .bind(&config.encryption_key)
                .bind(&now)
                .bind(&config.mac)
                .execute(&self.pool)
                .await?;
            }
        }

        Ok(outcome)
    }

    /// Get an active (not soft-deleted) device config by mac.
    pub async fn get_device(&self, mac: &str) -> Result<Option<DeviceConfig>, sqlx::Error> {
        let row = sqlx::query(
            r#"
            select mac, device_name, mqtt_configs, replay_topics, topic_mappings,
                   wake_msg, encryption_key, created_at, updated_at, deleted_at
            from iot_device_configs
            where mac = ? and deleted_at is null
            "#,
        )
        .bind(mac)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| DeviceConfig {
            mac: r.get(0),
            device_name: r.get(1),
            mqtt_configs: parse_json_column(r.get(2)),
            reply_topics: parse_json_column(r.get(3)),
            esp_now_key_mappings: parse_json_column(r.get(4)),
            wake_msg: r.get(5),
            encryption_key: r.get(6),
            created_at: r.get(7),
            updated_at: r.get(8),
            deleted_at: r.get(9),
        }))
    }

    /// List all active device configs.
    pub async fn list_devices(&self) -> Result<Vec<DeviceConfig>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            select mac, device_name, mqtt_configs, replay_topics, topic_mappings,
                   wake_msg, encryption_key, created_at, updated_at, deleted_at
            from iot_device_configs
            where deleted_at is null
            order by created_at asc
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| DeviceConfig {
            mac: r.get(0),
            device_name: r.get(1),
            mqtt_configs: parse_json_column(r.get(2)),
            reply_topics: parse_json_column(r.get(3)),
            esp_now_key_mappings: parse_json_column(r.get(4)),
            wake_msg: r.get(5),
            encryption_key: r.get(6),
            created_at: r.get(7),
            updated_at: r.get(8),
            deleted_at: r.get(9),
        })
        .collect())
    }

    /// Soft-delete a device by setting `deleted_at`.
    /// Returns false when the device does not exist or is already deleted.
    pub async fn soft_delete_device(&self, mac: &str) -> Result<bool, sqlx::Error> {
        let now = now_rfc3339();
        let res = sqlx::query(
            "update iot_device_configs set deleted_at = ?, updated_at = ?
             where mac = ? and deleted_at is null",
        )
        .bind(&now)
        .bind(&now)
        .bind(mac)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    /// Insert or update an MQTT topic value.
    /// Returns true when the stored value changed (or the row is new).
    pub async fn upsert_value(
        &self,
        mac: &str,
        r#type: &str,
        field: &str,
        topic: &str,
        value: &str,
    ) -> Result<bool, sqlx::Error> {
        let now = now_rfc3339();
        let existing: Option<(String,)> = sqlx::query_as(
            "select value from mqtt_topic_values where mac = ? and type = ? and field = ? and topic = ?",
        )
        .bind(mac)
        .bind(r#type)
        .bind(field)
        .bind(topic)
        .fetch_optional(&self.pool)
        .await?;

        let changed = match existing {
            None => true,
            Some((old,)) => old != value,
        };

        if !changed {
            // Still refresh updated_at so the row reflects recent activity.
            sqlx::query(
                "update mqtt_topic_values set updated_at = ?
                 where mac = ? and type = ? and field = ? and topic = ?",
            )
            .bind(&now)
            .bind(mac)
            .bind(r#type)
            .bind(field)
            .bind(topic)
            .execute(&self.pool)
            .await?;
            return Ok(false);
        }

        sqlx::query(
            r#"
            insert into mqtt_topic_values (mac, type, field, topic, value, created_at, updated_at)
            values (?, ?, ?, ?, ?, ?, ?)
            on conflict (mac, type, field, topic) do update set
                value = excluded.value,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(mac)
        .bind(r#type)
        .bind(field)
        .bind(topic)
        .bind(value)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;

        Ok(true)
    }

    /// Insert a value only when no row exists yet (used for default seeding).
    /// Returns true when a row was inserted.
    pub async fn insert_value_if_absent(
        &self,
        mac: &str,
        r#type: &str,
        field: &str,
        topic: &str,
        value: &str,
    ) -> Result<bool, sqlx::Error> {
        let now = now_rfc3339();
        let res = sqlx::query(
            r#"
            insert into mqtt_topic_values (mac, type, field, topic, value, created_at, updated_at)
            select ?, ?, ?, ?, ?, ?, ?
            where not exists (
                select 1 from mqtt_topic_values
                where mac = ? and type = ? and field = ? and topic = ?
            )
            "#,
        )
        .bind(mac)
        .bind(r#type)
        .bind(field)
        .bind(topic)
        .bind(value)
        .bind(&now)
        .bind(&now)
        .bind(mac)
        .bind(r#type)
        .bind(field)
        .bind(topic)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    /// Load all stored MQTT values for a mac.
    pub async fn load_values(&self, mac: &str) -> Result<Vec<MqttValue>, sqlx::Error> {
        let rows = sqlx::query(
            r#"
            select mac, type, field, topic, value, created_at, updated_at
            from mqtt_topic_values
            where mac = ?
            order by type asc, field asc, topic asc
            "#,
        )
        .bind(mac)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| MqttValue {
                mac: r.get(0),
                r#type: r.get(1),
                field: r.get(2),
                topic: r.get(3),
                value: r.get(4),
                created_at: r.get(5),
                updated_at: r.get(6),
            })
            .collect())
    }
}

impl DeviceConfig {
    /// JSON text for the three JSON columns (API field names are kept inside the JSON).
    fn json_columns(&self) -> (Option<String>, Option<String>, Option<String>) {
        let mqtt_configs = self
            .mqtt_configs
            .as_ref()
            .map(|v| serde_json::to_string(v).expect("config serialization cannot fail"));
        let reply_topics = self
            .reply_topics
            .as_ref()
            .map(|v| serde_json::to_string(v).expect("reply_topics serialization cannot fail"));
        let topic_mappings = self
            .esp_now_key_mappings
            .as_ref()
            .map(|v| serde_json::to_string(v).expect("topic_mappings serialization cannot fail"));
        (mqtt_configs, reply_topics, topic_mappings)
    }
}

fn parse_json_column<T: serde::de::DeserializeOwned>(raw: Option<String>) -> Option<T> {
    let raw = raw?;
    match serde_json::from_str(&raw) {
        Ok(v) => Some(v),
        Err(e) => {
            log::warn!("skipping malformed json column value: {} ({})", e, raw);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{KeyMapping, MqttConfigItem, ReplyTopic};
    use std::collections::HashMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    async fn temp_store(tag: &str) -> DeviceStore {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("iot_host_test_{}_{}.db", tag, nanos));
        DeviceStore::new(&path.to_string_lossy()).await.expect("store")
    }

    fn sample_config(mac: &str) -> DeviceConfig {
        DeviceConfig {
            mac: mac.to_string(),
            device_name: "Living Room Light".to_string(),
            mqtt_configs: Some(vec![MqttConfigItem {
                r#type: "sensor".to_string(),
                name: "temperature".to_string(),
                config_payload: serde_json::json!({"device_class": "temperature"}),
                default_state: Some("abc".to_string()),
                default_command: Some("def".to_string()),
            }]),
            reply_topics: Some(vec![ReplyTopic {
                r#type: "sensor".to_string(),
                name: "temperature/state".to_string(),
                key: "temp/ste".to_string(),
            }]),
            esp_now_key_mappings: Some(HashMap::from([(
                "act/ste".to_string(),
                KeyMapping {
                    r#type: "sensor".to_string(),
                    name: "temperature/state".to_string(),
                },
            )])),
            wake_msg: "xxx".to_string(),
            encryption_key: "testkey".to_string(),
            created_at: now_rfc3339(),
            updated_at: now_rfc3339(),
            deleted_at: None,
        }
    }

    #[tokio::test]
    async fn test_upsert_then_get() {
        let store = temp_store("upsert_get").await;
        let cfg = sample_config("AA_BB_CC_DD_EE_00");
        let outcome = store.upsert_device(&cfg).await.unwrap();
        assert!(outcome.republish_configs && outcome.seed_defaults);

        let fetched = store.get_device("AA_BB_CC_DD_EE_00").await.unwrap().expect("device");
        assert_eq!(fetched.device_name, "Living Room Light");
        assert_eq!(fetched.wake_msg, "xxx");
        assert_eq!(fetched.key_for("sensor", "temperature/state"), Some("temp/ste"));
        assert_eq!(fetched.mapping_for("act/ste").unwrap().name, "temperature/state");
        assert_eq!(
            fetched.mqtt_configs.as_ref().unwrap()[0].default_command,
            Some("def".to_string())
        );

        // update: no republish, no reseed
        let mut cfg2 = cfg.clone();
        cfg2.device_name = "Renamed".to_string();
        let outcome = store.upsert_device(&cfg2).await.unwrap();
        assert!(!outcome.republish_configs && !outcome.seed_defaults);
        let fetched = store.get_device("AA_BB_CC_DD_EE_00").await.unwrap().expect("device");
        assert_eq!(fetched.device_name, "Renamed");
        assert!(fetched.deleted_at.is_none());
    }

    #[tokio::test]
    async fn test_soft_delete_and_revive() {
        let store = temp_store("soft_delete").await;
        store.upsert_device(&sample_config("AA_BB_CC_DD_EE_00")).await.unwrap();

        assert!(store.soft_delete_device("AA_BB_CC_DD_EE_00").await.unwrap());
        assert!(store.get_device("AA_BB_CC_DD_EE_00").await.unwrap().is_none());
        assert!(store.list_devices().await.unwrap().is_empty());
        // second delete is a no-op
        assert!(!store.soft_delete_device("AA_BB_CC_DD_EE_00").await.unwrap());

        // revive: configs republished, defaults not reseeded
        let outcome = store.upsert_device(&sample_config("AA_BB_CC_DD_EE_00")).await.unwrap();
        assert!(outcome.republish_configs && !outcome.seed_defaults);
        assert!(store.get_device("AA_BB_CC_DD_EE_00").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_value_upsert_and_load() {
        let store = temp_store("values").await;
        let changed = store
            .upsert_value("AA_BB_CC_DD_EE_00", "sensor", "temperature", "state", "25.5")
            .await
            .unwrap();
        assert!(changed);

        let changed = store
            .upsert_value("AA_BB_CC_DD_EE_00", "sensor", "temperature", "state", "25.5")
            .await
            .unwrap();
        assert!(!changed);

        let changed = store
            .upsert_value("AA_BB_CC_DD_EE_00", "sensor", "temperature", "state", "26.0")
            .await
            .unwrap();
        assert!(changed);

        let values = store.load_values("AA_BB_CC_DD_EE_00").await.unwrap();
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].value, "26.0");
        assert_eq!(values[0].topic, "state");

        let other = store.load_values("FF_FF_FF_FF_FF_FF").await.unwrap();
        assert!(other.is_empty());
    }

    #[tokio::test]
    async fn test_insert_value_if_absent() {
        let store = temp_store("if_absent").await;
        assert!(store
            .insert_value_if_absent("M", "sensor", "temperature", "state", "first")
            .await
            .unwrap());
        assert!(!store
            .insert_value_if_absent("M", "sensor", "temperature", "state", "second")
            .await
            .unwrap());
        let values = store.load_values("M").await.unwrap();
        assert_eq!(values[0].value, "first");
    }

    #[tokio::test]
    async fn test_list_devices_order() {
        let store = temp_store("list").await;
        store.upsert_device(&sample_config("AA_BB_CC_DD_EE_00")).await.unwrap();
        store.upsert_device(&sample_config("BB_BB_BB_BB_BB_BB")).await.unwrap();
        let list = store.list_devices().await.unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].mac, "AA_BB_CC_DD_EE_00");
    }

    #[tokio::test]
    async fn test_malformed_json_column_is_tolerated() {
        let store = temp_store("malformed").await;
        sqlx::query(
            "insert into iot_device_configs
             (mac, device_name, mqtt_configs, replay_topics, topic_mappings, wake_msg, created_at, updated_at)
             values ('M', 'n', 'not json', 'also not', null, 'w', 't', 't')",
        )
        .execute(&store.pool)
        .await
        .unwrap();
        let cfg = store.get_device("M").await.unwrap().expect("device");
        assert!(cfg.mqtt_configs.is_none());
        assert!(cfg.reply_topics.is_none());
        assert!(cfg.esp_now_key_mappings.is_none());
        assert_eq!(cfg.wake_msg, "w");
    }
}
