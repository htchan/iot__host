# V2 Implementation Plan — iot__host (Rust)

Source of truth: [`docs/v2/api-design.md`](api-design.md), [`docs/v2/db-design.md`](db-design.md),
[`docs/v2/serial-communication-design.md`](serial-communication-design.md),
[`docs/v2/esp-now-design.md`](esp-now-design.md).

Everything in `src/` is legacy (v1 colon-format protocol) and will be **dropped**. The v2
protocol is `[<mac>]"<field>/<topic>:<value>","<field>/<topic>:<value>"` on serial, and
`homeassistant/<type>/<mac>/<field>/<topic>` on MQTT.

---

## 1. Data model

### 1.1 DB tables (SQLite, exactly per db-design.md)

```sql
create table mqtt_topic_values (
  mac varchar(32) not null,
  type varchar(64) not null,
  field varchar(64) not null,
  topic varchar (16) not null,
  value varchar(1024) not null,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

create table iot_device_configs (
  mac varchar(32) not null,
  device_name varchar(128) not null,
  mqtt_configs text,
  replay_topics text,
  topic_mappings text,
  wake_msg varchar(128) not null,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT
);
```

Documented augmentations (constraints only, no semantic change; needed for upserts):

- `iot_device_configs`: add `PRIMARY KEY (mac)` (api-design says "unique key by `mac`").
- `mqtt_topic_values`: add `UNIQUE (mac, type, field, topic)` for `ON CONFLICT` upsert.
- Column names follow the v2 doc literally, including its `replay_topics` spelling.
  API field `reply_topics` ↔ DB column `replay_topics`; API field
  `esp_now_key_mappings` ↔ DB column `topic_mappings`.
- `mqtt_topic_values` has **no** `deleted_at` in v2 (unlike v1 README) — we follow v2.

### 1.2 Rust types (`model.rs`)

```rust
struct DeviceConfig {
    mac: String,                       // normalized: uppercase, '_' separators
    device_name: String,
    mqtt_configs: Option<Vec<MqttConfigItem>>,
    reply_topics: Option<Vec<ReplyTopic>>,
    esp_now_key_mappings: Option<HashMap<String, KeyMapping>>,
    wake_msg: String,
    created_at: String, updated_at: String, deleted_at: Option<String>,
}

struct MqttConfigItem {
    r#type: String,                    // e.g. "sensor"
    name: String,                      // e.g. "temperature" (the MQTT "field")
    config_payload: serde_json::Value,
    default_state: Option<String>,
    default_command: Option<String>,
}

struct ReplyTopic { r#type: String, name: String, key: String } // name = "<field>/<topic>"
struct KeyMapping { r#type: String, name: String }              // name = "<field>/<topic>"
```

MQTT topic helper: `homeassistant/<type>/<mac>/<name>` where `name = "<field>/<topic>"`,
e.g. `homeassistant/sensor/AA_BB_CC_DD_EE_00/temperature/state`.

### 1.3 Serial / ESP-NOW wire format (`protocol.rs`, pure functions)

- ESP-Now payload: `"<field>/<topic>:<value>","<field>/<topic>:<value>"` (JSON string array
  without `[` `]`). Serial line = `[` + mac + `]` + that payload.
- Parse: strip `[mac]`, split remaining quoted array on `","`, each element split at **first**
  `:` → (key, value). Compose is the inverse.
- Assumptions (documented): `field` and `topic` must not contain `/` or `:`; values are free
  text. Host does **not** enforce the 224-byte ESP-Now limit — the hub splits (serial-design
  note). No crypto at host (encryption is hub↔device only).

---

## 2. Resolved ambiguities (decisions)

| # | Ambiguity | Decision |
|---|-----------|----------|
| D1 | API path: heading says `DELETE /api/device/{mac}`, body example says `/api/devices/{mac}`; POST is `/api/devices` | Implement `POST /api/devices`, `DELETE /api/devices/{mac}`, **plus alias** `DELETE /api/device/{mac}` |
| D2 | "send the iot command to hub" format is unspecified | Compose it with the standard format: load the mac's stored `command`-topic values and map them through `reply_topics` → one line `[<mac>]"k:v",...`. This keeps a single compose path and matches "iot device hardcodes the expected key" |
| D3 | Purpose of `default_state` / `default_command` | On first create: publish `config_payload` to `.../config` (retained, per api-design); seed `mqtt_topic_values` with `default_state`→topic `state`, `default_command`→topic `command` (only when the row doesn't exist). Defaults then flow to the hub through the normal sync path / startup command line |
| D4 | When is a device "first created"? | Brand-new row: republish configs AND seed defaults. Revived (was soft-deleted): republish configs but keep stored values. Plain update of an active row: neither |
| D5 | MAC format | Normalize to uppercase + `_` (`aa-bb-...` accepted at input). Lookups normalize before matching |
| D6 | Hub (re)connect signal | Hub has no hello message in v2; host sends setup msgs at startup and again after any serial-port reopen |
| D7 | MQTT QoS/retain | Subscribe `homeassistant/#` QoS1; publish config retained/QoS1, state & command QoS1 non-retained |
| D8 | MQTT msg for unknown mac | Still stored in `mqtt_topic_values` (no FK in design); no serial output (no device → no reply_topics) |
| D9 | Serial line for deleted/unknown mac or unmapped key | Log a warning, skip that pair/line |
| D10 | Empty compose result | Send nothing (no bare `[<mac>]` line) |

---

## 3. Components & data flow

```
┌────────────┐   serial lines    ┌──────────────┐  publish+store  ┌────────────┐
│   HUB      │ ────────────────▶ │ serial reader│ ───────────────▶ │   MQTT     │
│ (ESP-NOW)  │                   │  (thread)    │                  │  broker    │
│            │ ◀──────────────── │              │ ◀─────────────── │            │
└────────────┘  compose via      └──────────────┘  mqtt eventloop  └────────────┘
                 reply_topics           │    ▲                          ▲
                                       ▼    │                          │
                                  ┌──────────────┐   POST/DELETE   ┌────────────┐
                                  │  SQLite DB   │ ◀────────────── │ axum API   │
                                  │  (2 tables)  │   first-create  └────────────┘
                                  └──────────────┘  publish configs
```

- **serial reader thread** (blocking `serialport`): read lines, log each, parse, map keys via
  `topic_mappings`, publish each MQTT msg one-by-one, upsert `mqtt_topic_values`.
- **MQTT eventloop**: on message → parse topic → upsert `mqtt_topic_values` → if device exists
  and not deleted: load **all** values for mac → compose via `reply_topics` → one serial line
  (no length limit; hub splits).
- **API**: upsert (with first-create side effects per D3/D4) and soft delete
  (`deleted_at = now`).
- **Startup setup** (serial-design §"Host sending Special Setup Msg"): for every non-deleted
  device send `[<mac>]setup/<wake_msg>`, then the command line (D2).

## 4. Module structure (fresh layout)

```
src/
├── lib.rs       # module decls
├── main.rs      # bootstrap: config → db init → serial + mqtt + api wiring → startup setup
├── config.rs    # AppConfig from env (SERIAL_PORT, BAUD_RATE, MQTT_HOST, MQTT_PORT, API_PORT, DB_PATH)
├── model.rs     # types + MQTT topic compose/parse + normalization
├── protocol.rs  # serial line parse/compose (pure, unit-tested)
├── db.rs        # DDL + DeviceStore: device upsert/soft-delete/get/list, value upsert/load
├── serial.rs    # reader thread (reconnect w/ hourly-rate-limited error log — old TODO.md item), writer
├── mqtt.rs      # rumqttc wrapper (connect, subscribe, publish, event channel)
├── bridge.rs    # orchestration: serial→mqtt, mqtt→serial, startup setup msgs
└── api.rs       # axum routes + AppState + error responses
```

No new dependencies; reuse the existing `Cargo.toml` set (tokio, serialport, rumqttc, serde,
serde_json, thiserror, log, env_logger, axum, tokio-util, sqlx, chrono, uuid).

## 5. Implementation order

1. `model.rs` + `protocol.rs` (pure logic) with unit tests.
2. `db.rs` (schema + repos) with tests on a temp SQLite file.
3. `serial.rs`, `mqtt.rs` wrappers.
4. `bridge.rs` orchestration.
5. `api.rs` routes.
6. `main.rs` wiring + startup setup msgs.
7. Update `README.md` to v2 API/protocol; leave `docs/v1` untouched as history.

## 6. Verification

- `cargo fmt --check`, `cargo clippy --all-targets`, `cargo test`.
- PTY smoke test (`socat`): run binary against a fake serial port; assert startup lines
  `[<mac>]setup/<wake_msg>` + command line appear; inject a serial line `[<mac>]"k:v"` and
  check MQTT publish + DB row; publish an MQTT state msg and check the composed serial line.
- curl the API: POST device (assert 200 + config retained on `.../config`), POST again
  (assert no re-publish), DELETE (assert soft delete + no further serial syncs).
