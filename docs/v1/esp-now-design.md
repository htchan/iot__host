# Esp Now Communication Standard

## Msg Format

The esp now msg format will be `"<field_1>/<topic>:<value_1>","<field_2>/<topic>:<value_2>"`

- the msg is a json string array without `[`, `]`
- `<field_1>`, `<topic>`, `<value>` will be free text as meaningful and short as possible.
  - in host, we will define mapping to convert the `field` and `topic` to mqtt specific topic in device creation request. [ref](api-design.md)
  - device creation request `reply_topics` will also define which mqtt update should update to host and let host sync it to iot devices. Iot device should hardcoded the expected key and handle the value accordingly

## Msg Constraint

- each msg can only contains 224 character (because of software encryption, we cannot use full 250 bytes as content), sender should calculate the append the strings to msg one by one and compute the used length to make sure it is within length limit. Sender shoudl **split** the msg into multiple array if they exceed the length limit
- we will have software encryption. 16 bytes used as `iv`, 1 byte used as length, 4 bytes used as `boot_id`(avoid replay attach), 4 bytes used as `sequence_num` (avoid replay attack), **`224`** byte will be used as ciphertext.

```cpp
#pragma pack(push, 1)
struct SecurePacket {
    uint8_t iv[16];           // 16 bytes: Initialization Vector (Random per message)
    uint32_t boot_id;         // 4 bytes: Random ID generated on cold boot / battery swap
    uint32_t sequence_num;    // 4 bytes: Increments on each wakeup/transmission
    uint8_t actual_length;    // 1 byte: Length of the unpadded plaintext data
    uint8_t ciphertext[224];  // 224 bytes: Encrypted payload (14 AES blocks * 16 bytes)
};
#pragma pack(pop)
```
