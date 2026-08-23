# IoT Host - Serial to MQTT Bridge

A Rust application that bridges serial communication with MQTT for IoT device management.

## Overview

This application connects to an ESP-NOW hub via serial port and forwards device events to MQTT topics. It also subscribes to MQTT command topics and sends commands back to devices through the hub.

### Architecture

```mermaid
flowchart LR
    subgraph IoT_Device
        ESP_NOW[ESP-NOW Devices]
    end

    subgraph IoT_Hub
        HUB[ESP32/ESP8266 Hub]
    end

    subgraph IoT_Host
        HOST[Rust IoT Host]
    end

    subgraph MQTT_Broker
        BROKER[MQTT Broker]
    end

    ESP_NOW -- ESP-NOW --> HUB
    HUB -- Serial --> HOST
    HOST -- MQTT --> BROKER
    BROKER -- MQTT --> HOST
    HOST -- Serial --> HUB
    HUB -- ESP-NOW --> ESP_NOW

    style HUB fill:#f9f,stroke:#333
    style HOST fill:#bbf,stroke:#333
    style BROKER fill:#f96,stroke:#333
```

## Communication Protocol

### Serial Format

Messages are transmitted as colon-separated strings:

```
mac:event:key1:value1:key2:value2:...
```

**Example:**
```
aa-bb-cc-dd-ee-ff:awake:temp:25.5:humi:60
```

- **MAC Address**: 6 bytes in `xx-xx-xx-xx-xx-xx` format
- **Event**: `awake`, `cmd`, or `sleep`
- **Key-Value Pairs**: Up to 8 pairs of sensor data

### MQTT Topics

- **Device Events**: `homeassistant/{device_class}/{mac}/{field}/state`
- **Device Commands**: `homeassistant/{device_class}/{mac}/{field}/command`
- **HA Discovery**: `homeassistant/{device_class}/{mac}/{field}/config`

## Installation

### Prerequisites

- Rust 1.70+ (edition 2021)
- Cargo (comes with Rust)
- Docker (optional, for containerized deployment)

### Build from Source

```bash
cd iot__host
cargo build --release
```

### Run

```bash
# Set environment variables
export SERIAL_PORT=/dev/ttyUSB0
export BAUD_RATE=115200
export MQTT_HOST=localhost
export MQTT_PORT=1883

# Run
cargo run --release
```

## Docker Deployment

### Build Image

```bash
docker build -t iot-host .
```

### Run Container

```bash
docker run -it --rm \
  --name iot-host \
  -e SERIAL_PORT=/dev/ttyUSB0 \
  -e BAUD_RATE=115200 \
  -e MQTT_HOST=mqtt-broker \
  -e MQTT_PORT=1883 \
  -v /dev/ttyUSB0:/dev/ttyUSB0 \
  iot-host
```

## Configuration

| Environment Variable | Default | Description |
|---------------------|---------|-------------|
| `SERIAL_PORT` | `/dev/ttyUSB0` | Serial port device path |
| `BAUD_RATE` | `115200` | Serial port baud rate |
| `MQTT_HOST` | `localhost` | MQTT broker hostname |
| `MQTT_PORT` | `1883` | MQTT broker port |
| `API_PORT` | `8080` | HTTP API server port |
| `DB_PATH` | `/var/lib/iot_host/devices.db` | SQLite database path |

### Home Assistant Device Configuration API

The host provides a REST API for device configuration:

#### POST /api/device - Upsert Device

Create or update device configuration with MQTT topic setup.

**Request Body:**
```json
{
  "mac": "AA_BB_CC_DD_EE_00",
  "device_name": "Living Room Sensor",
  "mqtt_setup": [
    {
      "topic_prefix": "homeassistant/sensor/AA_BB_CC_DD_EE_00/temperature",
      "config": {
        "device_class": "temperature",
        "unit_of_measurement": "°C"
      }
    }
  ],
  "msg_setup": [
    {
      "format": "direct_bind",
      "key": "temperature",
      "awake_event_topic": "homeassistant/sensor/AA_BB_CC_DD_EE_00/temperature/state"
    }
  ]
}
```

**Response:**
```json
{
  "success": true,
  "message": "Device config saved for AA_BB_CC_DD_EE_00",
  "device_id": "AA_BB_CC_DD_EE_00"
}
```

#### DELETE /api/device/{mac} - Delete Device

Delete a device by MAC address. The device is soft-deleted by setting a `deleted_at` timestamp.

**Request:**
```
DELETE /api/device/AA_BB_CC_DD_EE_00
```

**Response:**
```json
{
  "success": true,
  "message": "Device AA_BB_CC_DD_EE_00 deleted",
  "device_id": "AA_BB_CC_DD_EE_00"
}
```

### Message Setup Formats

#### Direct Bind

Simple key-value binding for sensor data:

```json
{
  "format": "direct_bind",
  "key": "temperature",
  "awake_event_topic": "homeassistant/sensor/AA_BB_CC_DD_EE_00/temperature/state",
  "sleep_event_topic": "homeassistant/sensor/AA_BB_CC_DD_EE_00/temperature/state",
  "command_event_topic": "homeassistant/sensor/AA_BB_CC_DD_EE_00/temperature/command"
}
```

#### Servo Compose

For devices requiring composed values (e.g., servo position with direction):

```json
{
  "format": "servo_compose",
  "key": "servo",
  "command_event": {
    "direction_topic": "homeassistant/cover/AA_BB_CC_DD_EE_00/direction/state",
    "action_topic": "homeassistant/cover/AA_BB_CC_DD_EE_00/action/state",
    "on_value_topic": "homeassistant/cover/AA_BB_CC_DD_EE_00/on_value/state",
    "off_value_topic": "homeassistant/cover/AA_BB_CC_DD_EE_00/off_value/state"
  },
  "sleep_event": {
    "direction_topic": "homeassistant/cover/AA_BB_CC_DD_EE_00/direction/state",
    "action_topic": "homeassistant/cover/AA_BB_CC_DD_EE_00/action/state",
    "on_value_topic": "homeassistant/cover/AA_BB_CC_DD_EE_00/on_value/state",
    "off_value_topic": "homeassistant/cover/AA_BB_CC_DD_EE_00/off_value/state"
  }
}
```

**Compose Examples:**
- `direction=clock, action=on, on_value=20` → `+20`
- `direction=anticlock, action=on, on_value=20` → `-20`
- `action=null` → `n/a`

## Development

### Run Tests

```bash
cargo test
```

### Project Structure

```
src/
├── lib.rs        # Module exports
├── main.rs       # Application entry point
├── message.rs    # Message types and MQTT topics
├── converter.rs  # Serial/MQTT message conversion
├── serial.rs     # Serial port handler
├── mqtt.rs       # MQTT handler
├── api.rs        # HTTP API server
├── device_store.rs  # SQLite device configuration storage
├── device_state.rs  # In-memory device state tracking
```

## Related Projects

- [iot__hub](../iot__hub): ESP32/ESP8266 hub firmware
- [iot__helper](../iot__helper): ESP-NOW helper library

## License

MIT
