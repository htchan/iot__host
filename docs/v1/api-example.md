# API Example

This docs provide a API example to register a iot device which provide temperature and humidity.

## requestBody

```json
{
  "mac": "AA_BB_CC_DD_EE_00",
  "device_name": "Environment Sensor",
  "mqtt_configs": [
    {
      "type": "sensor",
      "field": "temperature",
      "config_payload": {
        "device": {
          "identifiers": ["AA_BB_CC_DD_EE_00"],
          "model": "D1 Mini",
          "manufacturer": "Espressif",
          "name": "Environment Sensor"
        },
        "unique_id": "AA_BB_CC_DD_EE_00__temperature",
        "unit_of_measurement": "°C",
        "state_topic": "homeassistant/sensor/AA_BB_CC_DD_EE_00/temperature/state",
        "availability_topic": "homeassistant/binary_sensor/AA_BB_CC_DD_EE_00/sensor_status/state",
        "payload_not_available": "",
        "state_class": "measurement",
        "device_class": "temperature",
        "name": "Temperature"
      }
    },
    {
      "type": "sensor",
      "field": "humidity",
      "config_payload": {
        "device": {
          "identifiers": ["AA_BB_CC_DD_EE_00"],
          "model": "D1 Mini",
          "manufacturer": "Espressif",
          "name": "Environment Sensor"
        },
        "unique_id": "AA_BB_CC_DD_EE_00__humidity",
        "unit_of_measurement": "%",
        "state_topic": "homeassistant/sensor/AA_BB_CC_DD_EE_00/humidity/state",
        "availability_topic": "homeassistant/binary_sensor/AA_BB_CC_DD_EE_00/sensor_status/state",
        "payload_not_available": "",
        "state_class": "measurement",
        "device_class": "humidity",
        "name": "Humidity"
      }
    },
    {
      "type": "binary_sensor",
      "field": "sensor_status",
      "config_payload": {
        "device": {
          "identifiers": ["AA_BB_CC_DD_EE_00"],
          "model": "D1 Mini",
          "manufacturer": "Espressif",
          "name": "Environment Sensor"
        },
        "unique_id": "AA_BB_CC_DD_EE_00__sensor_status",
        "state_topic": "homeassistant/binary_sensor/AA_BB_CC_DD_EE_00/sensor_status/state",
        "payload_on": "on",
        "payload_off": "off",
        "availability_topic": "homeassistant/binary_sensor/AA_BB_CC_DD_EE_00/sensor_status/state",
        "entity_category": "diagnostic",
        "device_class": "connectivity",
        "name": "Sensor Status"
      }
    },
    {
      "type": "sensor",
      "field": "vcc_voltage",
      "config_payload": {
        "device": {
          "identifiers": ["AA_BB_CC_DD_EE_00"],
          "model": "D1 Mini",
          "manufacturer": "Espressif",
          "name": "Environment Sensor"
        },
        "unique_id": "AA_BB_CC_DD_EE_00__vcc_voltage",
        "unit_of_measurement": "V",
        "state_topic": "homeassistant/sensor/AA_BB_CC_DD_EE_00/vcc_voltage/state",
        "payload_not_available": "",
        "entity_category": "diagnostic",
        "state_class": "measurement",
        "device_class": "connectivity",
        "name": "Sensor Status"
      }
    },
    {
      "type": "number",
      "field": "vcc_voltage_low_threshold",
      "config_payload": {
        "device": {
          "identifiers": ["AA_BB_CC_DD_EE_00"],
          "model": "D1 Mini",
          "manufacturer": "Espressif",
          "name": "Environment Sensor"
        },
        "unique_id": "AA_BB_CC_DD_EE_00__vcc_voltage_low_threshold",
        "state_topic": "homeassistant/number/AA_BB_CC_DD_EE_00/vcc_voltage_low_threshold/state",
        "command_topic": "homeassistant/number/AA_BB_CC_DD_EE_00/vcc_voltage_low_threshold/state",
        "payload_not_available": "",
        "entity_category": "diagnostic",
        "device_class": "voltage",
        "retain": true,
        "max": 4.5,
        "name": "VCC Voltage Low Threshold",
        "step": 0.01,
        "mode": "box",
        "min": 3
      },
      "default_state": "3.5"
    },
    {
      "type": "sensor",
      "field": "wifi_strength",
      "config_payload": {
        "device": {
          "identifiers": ["AA_BB_CC_DD_EE_00"],
          "model": "D1 Mini",
          "manufacturer": "Espressif",
          "name": "Environment Sensor"
        },
        "unique_id": "AA_BB_CC_DD_EE_00__wifi_strength",
        "unit_of_measurement": "%",
        "state_topic": "homeassistant/sensor/AA_BB_CC_DD_EE_00/wifi_strength/state",
        "payload_not_available": "",
        "entity_category": "diagnostic",
        "state_class": "measurement",
        "name": "WiFi Strength"
      }
    }
  ],
  "reply_topics": [
    {
      "type": "sensor",
      "name": "vcc_voltage/state",
      "key": "vcc/ste"
    }
  ],
  "esp_now_key_mappings": {
    "temp/ste": {
      "type": "sensor",
      "name": "temperature/state"
    },
    "humd/ste": {
      "type": "sensor",
      "name": "humidity/state"
    },
    "status/ste": {
      "type": "binary_sensor",
      "name": "sensor_status/state"
    },
    "vcc/ste": {
      "type": "sensor",
      "name": "vcc_voltage/state"
    },
    "wifi/ste": {
      "type": "sensor",
      "name": "wifi_strength/state"
    }
  },
  "wake_msg": "status/ste:on",
  "encryption_key": "key123"
}
```
