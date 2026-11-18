# DB schema

We will create mqtt table and device table in sqlite to store all required data.

## MQTT table

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
```

## devices table

```sql
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
)
```
