# Serial Communication Standard

## Format

It will have the same format as ESP Now msg `[<mac>]"<field_1>/<topic>:<value_1>","<field_2>/<topic>:<value_2>"`

## Receiving Serial Msg

1. log the serial received
2. parse the received serial into mqtt msgs by `iot_device_configs.topic_mappings` json
3. publish the mqtt msg one by one
   a. also update records in `mqtt_topic_values` table

## Sending Serial Msg

This will trigger when there is an update from mqtt

0. got mqtt msg
1. update records in `mqtt_topic_values` table
2. load all related mqtt values from db by `mac`
3. compose the serial message by `iot_device_configs.reply_topics` (no length limit)
4. send the composed serial message to hub

> [!note]
> hub expected to split the msg into multiple if it is too long

## Host sending Special Setup Msg

When the hub first time connect to host or when host restarted, host should send the setup msg to hub to tell hub what devices are available

1. query all not deleted device config
2. send to hub with following format: `[<mac>]setup/<wake_msg>`
   a. hub should store the mac to wake msg mapping in memory.
3. send the iot command to hub.
   a. hub should store the mac to command mapping in memory. If a ESP Now msg received containing wake_msg with matching mac, hub should reply the stored command immediately.
