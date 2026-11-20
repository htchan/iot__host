# V2 Implementation Plan — iot__host (Rust)

Status: **implemented** (all items below are in `src/`; 35 tests green).
Note: the repo owner moved `docs/v2` → `docs/v1` (replacing the old v1 docs),
so relative links below point at the sibling files in this folder.

Source of truth: [`docs/v1/api-design.md`](api-design.md), [`docs/v1/db-design.md`](db-design.md),
[`docs/v1/serial-communication-design.md`](serial-communication-design.md),
[`docs/v1/esp-now-design.md`](esp-now-design.md).

The legacy v1 colon-format code was dropped; the v2
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
  encryption_key varchar(128) not null default '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT
);
```

Documented augmentations (constraints only, no semantic change; needed for upserts):

- `iot_device_configs`: add `PRIMARY KEY (mac)` (api-design says "unique key by `mac`").
- `mqtt_topic_values`: add `UNIQUE (mac, type, field, topic)` for `ON CONFLICT` upsert.
- `encryption_key` column (default `''`) mirrors the new API field (D12).
- Column names follow the design doc literally, including its `replay_topics` spelling.
  API field `reply_topics` ↔ DB column `replay_topics`; API field
  `esp_now_key_mappings` ↔ DB column `topic_mappings`.
- `mqtt_topic_values` has **no** `deleted_at` (unlike v1 README) — we follow the design doc.

### 1.2 Rust types (`model.rs`)

```rust
struct DeviceConfig {
    mac: String,                       // normalized: uppercase, '_' separators
    device_name: String,
    mqtt_configs: Option<Vec<MqttConfigItem>>,
    reply_topics: Option<Vec<ReplyTopic>>,
    esp_now_key_mappings: Option<HashMap<String, KeyMapping>>,
    encryption_key: String,            // D12: synced to hub in the setup line
    wake_msg: String,
    created_at: String, updated_at: String, deleted_at: Option<String>,
}

struct MqttConfigItem {
    #[serde(rename = "type")]
    field_type: String,                // e.g. "sensor" (wire key stays "type", D11)
    field: String,                     // e.g. "temperature" (the MQTT "field")
    config_payload: serde_json::Value,
    default_state: Option<String>,
    default_command: Option<String>,
}

struct ReplyTopic { #[serde(rename = "type")] field_type: String, name: String, key: String } // name = "<field>/<topic>"
struct KeyMapping { #[serde(rename = "type")] field_type: String, name: String }              // name = "<field>/<topic>"
```

MQTT topic helper: `homeassistant/<type>/<mac>/<name>` where `name = "<field>/<topic>"`,
e.g. `homeassistant/sensor/AA_BB_CC_DD_EE_00/temperature/state`.

### 1.3 Serial / ESP-NOW wire format (`protocol.rs`, pure functions)

- ESP-Now payload: `"<field>/<topic>:<value>","<field>/<topic>:<value>"` (JSON string array
  without `[` `]`). Serial line = `[` + mac + `]` + that payload.
- Parse: strip `[mac]`, parse the payload as a JSON string array, each element split at **first**
  `:` → (key, value). Compose is the inverse.
- Compose validates each pair item against `MAX_PAIR_LEN = 20` characters (D13): longer items
  are invalid per the hub firmware contract, are skipped, and a warning is logged.
- Setup line (host → hub only, so the host never parses one):
  `[<mac>]setup/<encryption_key>/<wake_msg>`.
- Assumptions (documented): `field` and `topic` must not contain `/` or `:`; values are free
  text. Host does **not** enforce the 224-byte ESP-Now limit — the hub splits (serial-design
  note). No crypto at host (encryption is hub↔device only).

---

## 2. Resolved ambiguities (decisions)

| # | Ambiguity | Decision |
|---|-----------|----------|
| D1 | API path: heading says `DELETE /api/device/{mac}`, body example uses `/api/devices/{mac}`; POST is `/api/devices` | Implement only `POST /api/devices` and `DELETE /api/devices/{mac}`; the `/api/device/{mac}` alias is removed |
| D2 | "send the iot command to hub" format is unspecified | Compose it with the standard format: map the device's stored values through `reply_topics` → one line `[<mac>]"k:v",...` in reply_topics order. Setup sends ALL mapped values, not just `command` (TODO #12) |
| D3 | Purpose of `default_state` / `default_command` | On first create: publish `config_payload` to `.../config` (retained), then publish `default_state` → `.../state` and `default_command` → `.../command` BEFORE seeding them into `mqtt_topic_values` (`insert_value_if_absent`), so HA subscribers see the initial values immediately |
| D4 | When is a device "first created"? | Brand-new row: republish configs AND seed defaults. Revived (was soft-deleted): republish configs but keep stored values. Plain update of an active row: neither |
| D5 | MAC format | Normalize to uppercase + `_` (`aa-bb-...` accepted at input). Lookups normalize before matching |
| D6 | Hub (re)connect signal | Hub has no hello message in v2; host sends setup msgs at startup and again after any serial-port reopen (PortOpened event) |
| D7 | MQTT QoS/retain | Subscribe `homeassistant/#` QoS1; **all** publishes QoS1 + retained (`MqttClient::publish` hardcodes retain=true; the retain parameter was removed) |
| D8 | MQTT msg for unknown mac | Still stored in `mqtt_topic_values` (no FK in design); no serial output (no device → no reply_topics) |
| D9 | Serial line for deleted/unknown mac or unmapped key | Log a warning, skip that pair/line |
| D10 | Empty compose result | Send nothing (no bare `[<mac>]` line) |
| D11 | Rust field names vs wire keys | `MqttConfigItem.name` → `field`; all three mapping structs use Rust field `field_type` with `#[serde(rename = "type")]` so the wire JSON key stays `"type"` |
| D12 | Encryption key transport | `encryption_key` API field + DB column; sent to the hub in the setup line `[<mac>]setup/<encryption_key>/<wake_msg>` |
| D13 | Pair length | Each composed pair item `"<key>:<value>"` must be ≤ 20 chars; invalid items are skipped with a warning (hub firmware contract) |
| D14 | Echo loops | Serial inbound: value stored BEFORE publish so the own MQTT echo is detected unchanged. MQTT inbound: `upsert_value` returns changed=false for echoes/retained redeliveries → no serial send |

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
  `topic_mappings`, store the value first (echo guard, D14), publish each MQTT msg; after all
  pairs are applied, compose the device's latest state and send it back to the hub.
- **MQTT eventloop**: on message → parse topic → upsert `mqtt_topic_values` → if the value
  changed and the device exists: load all values for mac → compose via `reply_topics` **in order**
  (values without a reply topic, e.g. config, stay on MQTT only) → one serial line (no length
  limit; the hub splits).
- **API**: upsert (with first-create side effects per D3/D4) and soft delete
  (`deleted_at = now`).
- **Startup setup** (serial-design §"Host sending Special Setup Msg"): for every non-deleted
  device send `[<mac>]setup/<encryption_key>/<wake_msg>`, then one line with all
  reply_topics-mapped values (D2/D6).

## 4. Module structure (fresh layout)

```
src/
├── lib.rs       # module decls
├── main.rs      # bootstrap: config → db init → serial + mqtt + api wiring → startup setup
├── config.rs    # AppConfig from env (SERIAL_PORT, BAUD_RATE, MQTT_HOST, MQTT_PORT, MQTT_USERNAME, MQTT_PASSWORD, API_PORT, DB_PATH)
├── model.rs     # types + MQTT topic compose/parse + normalization
├── protocol.rs  # serial line parse/compose, setup line, MAX_PAIR_LEN guard (pure, unit-tested)
├── db.rs        # DDL + DeviceStore: device upsert/soft-delete/get/list, value upsert/load
├── serial.rs    # reader thread (reconnect w/ hourly-rate-limited error log — old TODO.md item), writer
├── mqtt.rs      # rumqttc wrapper (connect w/ optional creds, subscribe, publish retained, event channel)
├── bridge.rs    # orchestration: serial→mqtt, mqtt→serial, setup msgs, echo guard
└── api.rs       # axum routes + AppState + error responses
```

No new dependencies; reuse the existing `Cargo.toml` set (tokio, serialport, rumqttc, serde,
serde_json, thiserror, log, env_logger, axum, tokio-util, sqlx, chrono, uuid).

## 5. Implementation order (done)

1. `model.rs` + `protocol.rs` (pure logic) with unit tests.
2. `db.rs` (schema + repos) with tests on a temp SQLite file.
3. `serial.rs`, `mqtt.rs` wrappers.
4. `bridge.rs` orchestration.
5. `api.rs` routes.
6. `main.rs` wiring + startup setup msgs.
7. `README.md` updated to the v2 API/protocol; `docs/v1` kept as the (renamed) design docs.

## 6. Verification

- `cargo build` + `cargo test`: 35 tests green (model / protocol / db / bridge).
  Build env notes (no root in sandbox): `CARGO_HOME=/tmp/cargo-home`,
  `PKG_CONFIG_PATH=/tmp/libudev LIBRARY_PATH=/tmp/libudev` (libudev shim for
  serialport's libudev-sys).
- PTY/`socat` serial smoke test: **skipped** — the sandbox denies `openpty` and escalation was
  declined ("we dont need to test the actual"); the serial logic is covered by the protocol and
  bridge unit tests instead.
- curl API flow (manual): POST /api/devices (assert config retained on `.../config` + default
  state/command published to `.../state` / `.../command` before DB seed), POST again (no
  re-publish on plain update), DELETE (soft delete, no further serial syncs).
