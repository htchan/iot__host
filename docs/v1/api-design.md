# Home Assistant Device Configuration API

The host provides a REST API for device configuration:

## POST /api/devices - Upsert Device

Create or update device configuration with MQTT topic setup. It should publish the `mqtt_configs` content to mqtt when decvice was first created

- unique key by `mac`
- `mqtt_configs` is the mqtt msg to be published when the device is first create to setup mqtt for further processing. it include the config payload, default state and command value
- `reply_topics` is used to help parsing mqtt topic to a shorter key to save ESP Now msg space
- `esp_now_key_mappings` is used to covert the ESP Now msg key into correct mqtt topic

**Request Body**

```json
{
  "mac": "AA_BB_CC_DD_EE_00",
  "device_name": "Living Room Light",
  "mqtt_configs": [
    {
      // internal service will compose mac, type, field into topic eg. `homeassistant/<type>/<mac>/<field>/config
      "type": "sensor",
      "name": "temperature",
      "config_payload": {
        "device_class": "temperature",
        "unit_of_measurement": "°C"
      },
      "default_state": "abc", //optional field
      "default_command": "def" // optional field
    }
  ],
  "reply_topics": [
    {
      "type": "sensor",
      "name": "temperature/state", // <field>/<topic>
      "key": "temp/ste"
    }
  ],
  "esp_now_key_mappings": {
    "act/ste": {
      "type": "sensor",
      "name": "temperature/state" // <field>/<topic>
    }
  },
  "wake_msg": "xxx" // sync wake msg to hub so hub will send the command when a esp now containing wake msg was received
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

## DELETE /api/device/{mac} - Delete Device

Delete a device by MAC address. The device is soft-deleted by setting a `deleted_at` timestamp.

**Request:**

```
DELETE /api/devices/AA_BB_CC_DD_EE_00
```

**Response:**

```json
{
  "success": true,
  "message": "Device AA_BB_CC_DD_EE_00 deleted",
  "device_id": "AA_BB_CC_DD_EE_00"
}
```
