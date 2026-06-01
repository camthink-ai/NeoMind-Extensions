# LoRaWAN Bridge

Connect NeoMind to LoRaWAN Network Servers for IoT device data collection, payload decoding, and downlink command injection.

## Features

- Multi-network server support: ChirpStack v3, ChirpStack v4, The Things Network (TTN)
- Built-in Cayenne LPP payload decoder with GPS coordinate support
- Custom binary decoder with configurable field definitions (offset, length, data type, scale)
- Automatic device discovery from MQTT uplink messages
- Downlink queue management with FPort validation (1-223)
- Real-time RSSI/SNR signal quality monitoring
- MQTT auto-reconnect with subscription recovery

## Installation

```bash
# Build from repository root
./build.sh --single lorawan-bridge

# Or build with Cargo directly
cargo build --release -p lorawan-bridge
```

## Commands

| Command | Description | Parameters |
|---------|-------------|------------|
| `connect` | Connect to a LoRaWAN Network Server | `ns_type` (string, required) - "chirpstack", "chirpstack_v4", or "ttn", `broker_url` (string, required) - MQTT broker URL, `username` (string, optional), `password` (string, optional), `application_id` (string, required), `tenant_id` (string, optional) - ChirpStack tenant ID, `ns_api_url` (string, optional) - API URL for downlink, `default_decoder` (string, optional) - "cayenne" or "custom", default "cayenne" |
| `disconnect` | Disconnect from Network Server | None |
| `list_devices` | List all discovered LoRa devices | None |
| `get_device` | Get details of a specific device | `dev_eui` (string, required) - Device EUI |
| `send_downlink` | Send downlink payload to a device | `dev_eui` (string, required), `f_port` (integer, required, 1-223), `payload_hex` (string, required) - Hex-encoded payload, `confirmed` (boolean, optional, default false) |
| `set_decoder` | Set custom decoder for a device | `dev_eui` (string, required), `decoder_type` (string, required) - "cayenne" or "custom", `fields` (array, optional) - Custom field definitions |
| `remove_device` | Remove a tracked device | `dev_eui` (string, required) |
| `get_status` | Get connection status and device count | None |
| `configure` | Update bridge configuration | `auto_discover` (boolean, optional), `default_decoder` (string, optional) |

## Metrics

| Metric | Display Name | Type | Unit | Range |
|--------|-------------|------|------|-------|
| `connected` | MQTT Connected | Boolean | - | - |
| `device_count` | Device Count | Integer | - | 0 to 10000 |
| `messages_received` | Messages Received | Integer | - | 0 to 1000000 |
| `decode_errors` | Decode Errors | Integer | - | 0 to 100000 |
| `last_message_ts` | Last Message Timestamp | Integer | ms | - |

## Network Server Compatibility

### ChirpStack v3
- MQTT topic: `application/{id}/device/+/event/up`
- Top-level `devEui` in uplink JSON
- Downlink API: `/api/devices/{devEui}/queue` with `deviceQueueItem` fields

### ChirpStack v4
- MQTT topic: `application/{id}/device/+/event/up` (same as v3)
- Nested `deviceInfo.devEui` in uplink JSON
- Downlink API: gRPC-gateway with `queueItem` fields (snake_case), `Grpc-Metadata-Authorization` header

### The Things Network (TTN)
- MQTT topic: `v3/{app_id}@{tenant_id}/devices/{dev_id}/up`
- QoS 0 only (TTN MQTT limitation)
- Uses `uplink_message.decoded_payload` for pre-decoded data

## Custom Decoder

Define field mappings for proprietary binary payloads:

```json
{
  "fields": [
    {"offset": 0, "length": 2, "name": "temperature", "type": "int16", "scale": 0.01, "unit": "°C"},
    {"offset": 2, "length": 1, "name": "humidity", "type": "uint8", "scale": 1.0, "unit": "%"}
  ]
}
```

Supported data types: `uint8`, `uint16`, `int16`, `uint32`, `int32`

## Requirements

- MQTT broker accessible from NeoMind edge device
- LoRaWAN Network Server (ChirpStack v3/v4 or TTN) configured with MQTT integration
- API key for downlink (ChirpStack v4 requires Bearer token with gRPC-gateway access)

## License

Apache-2.0
