# NeoMind Device Ecosystem Extensions Design

**Date:** 2026-05-27
**Status:** Draft
**Author:** NeoMind Team

## Overview

Three new NeoMind extensions to expand device connectivity beyond the platform's built-in MQTT/Webhook/BLE capabilities:

1. **modbus-bridge** — Industrial protocol bridge (Modbus TCP/RTU)
2. **lorawan-bridge** — LoRaWAN sensor gateway (ChirpStack/TTN integration)
3. **homeassistant-bridge** — Home Assistant ecosystem connector (3000+ device integrations)

These extensions focus purely on **device data acquisition and control** — reading sensor data, writing control commands, and bridging devices into NeoMind's metric system.

**Design principle:** Extensions are data providers only. All cross-device automation, event correlation, and workflow logic belongs in NeoMind's built-in rule engine.

## Common Architecture

### NeoMind Extension Standards

All three extensions follow these conventions from existing extensions:

- **FFI:** `neomind_export!()` macro for ABI version 3 exports
- **State:** `RwLock` for config, `Atomic*` for metrics/counters
- **HTTP:** Use `ureq` (sync) for REST calls — avoid async HTTP in cdylib
- **Tokio:** Minimal Tokio features (`rt`, `sync`) — do NOT create own runtime
- **Panic:** `panic = "unwind"` in Cargo profiles (required for safe extension unloading)
- **Frontend:** UMD build with React/ReactDOM as externals, NeoMind CSS variables only
- **Naming:** `neomind_extension_{name}` for lib, `neomind-extension-sdk` for SDK dep

### Tokio Runtime Strategy

Extensions are loaded as cdylib by NeoMind's extension runner. They cannot create their own Tokio runtime. Strategy per extension:

| Extension | Async Need | Strategy |
|-----------|-----------|----------|
| modbus-bridge | Modbus polling | `tokio-modbus` `sync` feature — no runtime needed |
| lorawan-bridge | MQTT event loop | `rumqttc` requires runtime — spawn on host runtime via `tokio::runtime::Handle` |
| homeassistant-bridge | WebSocket + REST | WebSocket via `tokio-tungstenite` on host runtime; REST via sync `ureq` |

For lorawan-bridge and homeassistant-bridge, the extension will use `tokio::runtime::Handle::try_current()` to access the host's Tokio runtime. The neomind-extension-sdk already depends on Tokio for `RwLock`, so a runtime should be available.

### Device Template & Auto-Registration

All three extensions integrate with NeoMind's device management system:

**Flow: Install extension → Configure connection → Devices auto-appear**

1. **Template Registration**: On initialization, each extension registers `DeviceTypeTemplate`s for the device types it supports (e.g., "modbus_tcp_device", "lorawan_sensor", "ha_entity")
2. **Auto-Discovery**: When the extension detects a new device (first Modbus response, first LoRaWAN uplink, HA entity sync), it auto-registers the device in NeoMind
3. **Zero-config onboarding**: User only needs to configure the connection parameters (IP, broker URL, HA token). Device discovery and registration happens automatically.

Each extension section below includes its device template definitions and auto-registration strategy.

---

## 1. modbus-bridge

### Purpose

Bridge Modbus TCP and RTU (serial) devices into NeoMind. Modbus is the most widely deployed industrial protocol, covering PLCs, power meters, environment sensors, VFDs, and controllers.

### Architecture

```
Modbus Devices (PLC, meters, sensors)
    ↓ Modbus TCP / RTU over RS485
modbus-bridge (cdylib)
    ├── tokio-modbus (sync feature — no Tokio runtime)
    ├── Register map (user-configured via JSON)
    ├── Polling loop (std::thread, not tokio)
    ├── Device template registration (on init)
    ├── Auto-discovery (scan slave IDs)
    └── register values → NeoMind metrics + device data
```

### Device Template

Extension registers a generic `modbus_device` template on initialization. Each added device creates a NeoMind device instance with metrics derived from its register map.

```json
{
  "device_type": "modbus_device",
  "name": "Modbus Device",
  "description": "Generic Modbus TCP/RTU device with configurable register mapping",
  "categories": ["industrial", "sensor"],
  "metrics": [
    { "name": "connection", "description": "Connection status", "data_type": "String", "read_only": true },
    { "name": "poll_errors", "description": "Poll error count", "data_type": "Integer", "read_only": true }
  ],
  "commands": [
    { "name": "write_register", "description": "Write holding register" },
    { "name": "write_coil", "description": "Write coil" }
  ]
}
```

User-defined registers are added as additional metrics on the device instance at registration time.

### Auto-Registration Flow

```
User configures: IP/Serial + Slave ID + Register Map
    ↓
add_device command
    ↓
Extension tests connection (read register 0)
    ↓ Success
Register device in NeoMind with template + register-derived metrics
    ↓
Start polling loop
    ↓
Metrics auto-flow into NeoMind dashboards
```

**Simpler alternative**: Pre-built templates for common Modbus devices:
- `modbus_power_meter` — 3-phase power meter (voltage, current, power, energy)
- `modbus_env_sensor` — Temperature/humidity/CO2 sensor
- `modbus_io_module` — Digital I/O module (relay inputs/outputs)

Users select a template, enter IP and slave ID, device auto-registers.

### Commands

| Command | Parameters | Description |
|---------|-----------|-------------|
| `add_device` | `{ip?, port?, slave_id, serial_port?, baud_rate?, mode: "tcp"\|"rtu"}` | Add a Modbus device |
| `remove_device` | `{device_id}` | Remove device and stop polling |
| `read_registers` | `{device_id, start: u16, count: u16}` | Immediate register read |
| `write_register` | `{device_id, address: u16, value: u16}` | Write single holding register |
| `write_registers` | `{device_id, start: u16, values: [u16]}` | Write multiple holding registers |
| `write_coil` | `{device_id, address: u16, value: bool}` | Write single coil |
| `write_coils` | `{device_id, start: u16, values: [bool]}` | Write multiple coils |
| `list_devices` | `{}` | List all configured devices |
| `update_polling` | `{device_id, interval_ms: u64}` | Change polling interval |
| `set_register_map` | `{device_id, registers: [{address, name, data_type, scale, unit}]}` | Configure register-to-metric mapping |

### Register Map Configuration

```json
{
  "registers": [
    {
      "address": 0,
      "count": 1,
      "name": "temperature",
      "data_type": "int16",
      "scale": 0.1,
      "unit": "°C"
    },
    {
      "address": 1,
      "count": 1,
      "name": "humidity",
      "data_type": "uint16",
      "scale": 0.1,
      "unit": "%"
    },
    {
      "address": 10,
      "count": 2,
      "name": "power",
      "data_type": "float32",
      "unit": "kW"
    }
  ]
}
```

Supported data types: `uint16`, `int16`, `uint32`, `int32`, `float32` (two registers), `bool` (coil).

### Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `modbus.{device_id}.{register_name}` | Gauge | Decoded register value |
| `modbus.{device_id}.connection` | State | "connected" / "disconnected" |
| `modbus.{device_id}.poll_errors` | Counter | Failed poll count since last success |
| `modbus.{device_id}.last_poll_ms` | Gauge | Last poll round-trip time |

### Config Parameters

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `default_tcp_port` | Integer | No | 502 | Default Modbus TCP port |
| `default_poll_interval_ms` | Integer | No | 5000 | Default polling interval |
| `default_timeout_ms` | Integer | No | 3000 | Default connection timeout |
| `max_devices` | Integer | No | 64 | Maximum concurrent devices |

### Dependencies

```toml
[dependencies]
neomind-extension-sdk = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
async-trait = { workspace = true }
chrono = "0.4"

tokio-modbus = { version = "0.17", default-features = false, features = ["tcp-sync", "rtu-sync"] }
# rtu-sync pulls in serialport automatically
```

Note: Using `tcp-sync` and `rtu-sync` features which provide synchronous Modbus clients without requiring a Tokio runtime. Polling runs on `std::thread`.

### Frontend Component

**`ModbusDeviceCard`** — Card displaying:
- Connection status indicator (green/red dot)
- Register values in a table/grid layout
- Quick control buttons for writable registers/coils
- Configuration dialog for register map editing

Config schema:
```json
{
  "deviceMode": {
    "type": "string",
    "enum": ["tcp", "rtu"],
    "enumTitles": ["Modbus TCP", "Modbus RTU (Serial)"],
    "default": "tcp"
  },
  "tcpAddress": { "type": "string", "title": "IP Address" },
  "tcpPort": { "type": "integer", "default": 502, "title": "Port" },
  "serialPort": { "type": "string", "title": "Serial Port", "description": "e.g. /dev/ttyUSB0, COM3" },
  "baudRate": {
    "type": "integer",
    "enum": [9600, 19200, 38400, 57600, 115200],
    "default": 9600,
    "title": "Baud Rate"
  },
  "slaveId": { "type": "integer", "title": "Slave ID", "default": 1 },
  "pollInterval": { "type": "integer", "title": "Poll Interval (ms)", "default": 5000 }
}
```

---

## 2. lorawan-bridge

### Purpose

Connect NeoMind to LoRaWAN Network Servers (ChirpStack, The Things Stack) via MQTT integration. This enables monitoring and controlling LoRa sensors across large areas (5-15km range) for agriculture, utilities, and smart city deployments.

### Architecture

```
LoRa End Devices (sensors)
    ↓ LoRa radio
LoRaWAN Gateways
    ↓ MQTT / UDP
LoRaWAN Network Server (ChirpStack / TTN)
    ↓ MQTT Integration (separate broker connection)
lorawan-bridge (cdylib)
    ├── rumqttc (MQTT client to NS broker)
    ├── Payload decoders (Cayenne LPP + custom)
    ├── Device template registration (on init)
    ├── Auto-discovery from uplinks (new device → auto-register)
    └── decoded data → NeoMind metrics + device data
```

### Device Template

Extension registers a generic `lorawan_sensor` template. Each new device discovered from an uplink auto-creates a NeoMind device instance with metrics derived from the decoded payload.

```json
{
  "device_type": "lorawan_sensor",
  "name": "LoRaWAN Sensor",
  "description": "LoRaWAN end device with auto-decoded payload",
  "categories": ["iot", "sensor", "lorawan"],
  "metrics": [
    { "name": "rssi", "description": "Signal strength", "data_type": "Integer", "unit": "dBm", "read_only": true },
    { "name": "snr", "description": "Signal-to-noise ratio", "data_type": "Float", "read_only": true },
    { "name": "battery", "description": "Battery level", "data_type": "Integer", "unit": "%", "read_only": true },
    { "name": "f_cnt", "description": "Frame counter", "data_type": "Integer", "read_only": true }
  ],
  "commands": [
    { "name": "send_downlink", "description": "Send downlink payload to device" }
  ]
}
```

Decoded sensor fields (temperature, humidity, etc.) are added as additional metrics on the device instance at discovery time.

### Auto-Registration Flow

```
User configures: NS type + broker URL + application ID
    ↓
Extension subscribes to NS uplink topics
    ↓
First uplink from unknown DevEUI arrives
    ↓
Auto-decode payload (Cayenne LPP or custom decoder)
    ↓
Auto-register device in NeoMind:
  - Device ID: DevEUI
  - Type: lorawan_sensor
  - Metrics: rssi + snr + battery + decoded fields
    ↓
All subsequent uplinks → metrics auto-flow into NeoMind
```

**Key:** User only configures the NS connection. All LoRa devices in that application auto-appear in NeoMind within seconds of their first uplink.

### Network Server Support

#### ChirpStack v4

MQTT topics:
```
# Uplink
application/{application_id}/device/{dev_eui}/event/up

# Device status (battery, margin)
application/{application_id}/device/{dev_eui}/event/status

# Join
application/{application_id}/device/{dev_eui}/event/join

# Downlink (publish to queue)
application/{application_id}/device/{dev_eui}/command/down
```

Uplink payload (JSON):
```json
{
  "devEui": "0102030405060708",
  "fPort": 2,
  "fCnt": 42,
  "data": "AQIBCg==",
  "object": { "temperature": 23.5 },
  "rxInfo": [{ "rssi": -57, "snr": 8.2, "gatewayId": "..." }],
  "txInfo": { "frequency": 868100000, "modulation": { "lora": { "bandwidth": 125, "spreadingFactor": 7 } } }
}
```

#### The Things Stack v3

MQTT topics:
```
# Uplink
v3/{application_id}@{tenant_id}/devices/{device_id}/up

# Join accept
v3/{application_id}@{tenant_id}/devices/{device_id}/join/accept

# Downlink (via API, not MQTT publish)
```

Authentication: username = `{application_id}@{tenant_id}`, password = API key (NNSXS...).

Connection: Port 8883 (TLS required).

### Commands

| Command | Parameters | Description |
|---------|-----------|-------------|
| `connect` | `{broker_url, username?, password?, ns_type: "chirpstack"\|"ttn", app_id?, tenant_id?}` | Connect to NS MQTT broker |
| `disconnect` | `{}` | Disconnect from NS |
| `list_devices` | `{}` | List discovered LoRa devices |
| `get_device` | `{dev_eui}` | Get single device details |
| `send_downlink` | `{dev_eui, payload_hex, f_port: u8, confirmed: bool}` | Send downlink via NS HTTP API |
| `set_decoder` | `{dev_eui, type: "cayenne"\|"custom", custom_map?: [...]}` | Set payload decoder for device |
| `get_gateways` | `{}` | List known gateways and status |
| `get_status` | `{}` | Connection status and stats |

### Payload Decoders

#### Cayenne LPP (Automatic)

Standard LoRaWAN payload format. Auto-decodes common channel types:

| Channel Type | Code | Fields |
|-------------|------|--------|
| Digital Input | 0x00 | digital_input |
| Digital Output | 0x01 | digital_output |
| Analog Input | 0x02 | analog_in_{ch} |
| Analog Output | 0x03 | analog_out_{ch} |
| Illuminance | 0x65 | illuminance (lux) |
| Presence | 0x66 | presence |
| Temperature | 0x67 | temperature (°C) |
| Humidity | 0x68 | humidity (%) |
| Accelerometer | 0x71 | acc_x, acc_y, acc_z |
| GPS | 0x88 | latitude, longitude, altitude |
| Barometer | 0x73 | pressure (hPa) |

#### Custom Decoder (User-Configured)

```json
{
  "decoder_type": "custom",
  "fields": [
    { "offset": 0, "length": 2, "name": "soil_moisture", "type": "uint16", "scale": 0.01, "unit": "%" },
    { "offset": 2, "length": 2, "name": "temperature", "type": "int16", "scale": 0.1, "unit": "°C" },
    { "offset": 4, "length": 1, "name": "battery", "type": "uint8", "unit": "%" }
  ]
}
```

If ChirpStack provides `object` (already decoded), the extension uses it directly and skips local decoding.

### Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `lorawan.{dev_eui}.{field}` | Gauge | Decoded sensor value |
| `lorawan.{dev_eui}.rssi` | Gauge | Signal strength (dBm) |
| `lorawan.{dev_eui}.snr` | Gauge | Signal-to-noise ratio |
| `lorawan.{dev_eui}.battery` | Gauge | Battery level (%) from status event |
| `lorawan.{dev_eui}.f_cnt` | Counter | Frame counter |
| `lorawan.gateway.{gw_id}.status` | State | Gateway online status |

### Config Parameters

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `ns_type` | String | Yes | — | "chirpstack" or "ttn" |
| `broker_url` | String | Yes | — | MQTT broker URL (e.g. mqtt://host:1883) |
| `username` | String | No | — | MQTT username |
| `password` | String | No | — | MQTT password |
| `application_id` | String | Yes | — | ChirpStack app ID or TTN app ID |
| `tenant_id` | String | No | — | TTN tenant ID (required for TTN) |
| `default_decoder` | String | No | "cayenne" | Default payload decoder |
| `auto_discover` | Boolean | No | true | Auto-register new devices |
| `ns_api_url` | String | No | — | NS HTTP API URL for downlink |

### Dependencies

```toml
[dependencies]
neomind-extension-sdk = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
async-trait = { workspace = true }
chrono = "0.4"
tokio = { version = "1", features = ["rt", "sync"] }

rumqttc = { version = "0.25", features = ["use-rustls"] }
ureq = { version = "3", features = ["json"] }  # For NS HTTP API (downlink)
```

### Frontend Component

**`LoRaWANDeviceCard`** — Card displaying:
- Device list with signal strength bars
- Real-time sensor readings per device
- Battery indicator
- Downlink command button
- Gateway status section

Config schema:
```json
{
  "nsType": {
    "type": "string",
    "enum": ["chirpstack", "ttn"],
    "enumTitles": ["ChirpStack v4", "The Things Stack v3"],
    "default": "chirpstack"
  },
  "brokerUrl": { "type": "string", "title": "MQTT Broker URL" },
  "username": { "type": "string", "title": "Username" },
  "password": { "type": "string", "title": "Password" },
  "applicationId": { "type": "string", "title": "Application ID" },
  "nsApiUrl": { "type": "string", "title": "NS API URL", "description": "For downlink commands" }
}
```

---

## 3. homeassistant-bridge

### Purpose

Connect NeoMind to Home Assistant, gaining access to 3000+ device integrations. Import HA entities (sensors, switches, lights, etc.) into NeoMind as devices with real-time state monitoring and remote control.

### Architecture

```
Home Assistant (3000+ integrations)
    ↕
homeassistant-bridge (cdylib)
    ├── WebSocket client (tokio-tungstenite, host runtime)
    │   └── Real-time state_changed subscriptions
    ├── REST client (ureq, sync)
    │   └── Service calls, entity queries
    ├── Device template registration (on init)
    ├── Auto-sync HA entities → NeoMind devices
    └── Entity state changes → NeoMind metrics
```

### Device Templates

Extension registers templates per HA domain. Each HA entity auto-creates a NeoMind device instance matching its domain type.

```json
[
  {
    "device_type": "ha_sensor",
    "name": "HA Sensor",
    "description": "Home Assistant sensor entity",
    "categories": ["home-assistant", "sensor"],
    "metrics": [
      { "name": "value", "description": "Sensor value", "data_type": "Float", "read_only": true },
      { "name": "battery", "description": "Battery level", "data_type": "Integer", "unit": "%", "read_only": true }
    ]
  },
  {
    "device_type": "ha_switch",
    "name": "HA Switch",
    "description": "Home Assistant switch/plug entity",
    "categories": ["home-assistant", "control"],
    "metrics": [
      { "name": "state", "description": "Switch state", "data_type": "String", "read_only": true }
    ],
    "commands": [
      { "name": "turn_on", "description": "Turn on" },
      { "name": "turn_off", "description": "Turn off" }
    ]
  },
  {
    "device_type": "ha_light",
    "name": "HA Light",
    "description": "Home Assistant light entity",
    "categories": ["home-assistant", "control"],
    "metrics": [
      { "name": "state", "description": "Light state", "data_type": "String", "read_only": true },
      { "name": "brightness", "description": "Brightness", "data_type": "Integer", "unit": "%", "read_only": true }
    ],
    "commands": [
      { "name": "turn_on", "description": "Turn on" },
      { "name": "turn_off", "description": "Turn off" },
      { "name": "set_brightness", "description": "Set brightness" }
    ]
  },
  {
    "device_type": "ha_climate",
    "name": "HA Climate",
    "description": "Home Assistant climate/HVAC entity",
    "categories": ["home-assistant", "control"],
    "metrics": [
      { "name": "temperature", "description": "Current temperature", "data_type": "Float", "unit": "°C", "read_only": true },
      { "name": "target_temperature", "description": "Target temperature", "data_type": "Float", "unit": "°C", "read_only": false }
    ],
    "commands": [
      { "name": "set_temperature", "description": "Set target temperature" }
    ]
  },
  {
    "device_type": "ha_lock",
    "name": "HA Lock",
    "description": "Home Assistant lock entity",
    "categories": ["home-assistant", "security"],
    "metrics": [
      { "name": "state", "description": "Lock state", "data_type": "String", "read_only": true }
    ],
    "commands": [
      { "name": "lock", "description": "Lock" },
      { "name": "unlock", "description": "Unlock" }
    ]
  }
]
```

Domain filter determines which templates are registered. Only enabled domains (e.g., sensor, light, switch) create templates and sync entities.

### Auto-Registration Flow

```
User configures: HA URL + access token + domain filter
    ↓
Extension connects via WebSocket + REST
    ↓
Register templates for enabled domains
    ↓
Fetch all entity states via REST (GET /api/states)
    ↓
For each entity matching domain filter:
  - Auto-register as NeoMind device
  - Device ID: ha_{entity_id} (e.g. ha_light_living_room)
  - Type: ha_{domain} (e.g. ha_light)
    ↓
Subscribe to WebSocket state_changed events
    ↓
All state changes → metrics auto-flow into NeoMind
```

**Key:** User enters HA URL + token + selects domains. All matching entities auto-appear as NeoMind devices instantly.

### HA API Integration

#### Authentication

Home Assistant uses long-lived access tokens:
1. User creates token in HA Profile → Security → Long-Lived Access Tokens
2. Token passed via `Authorization: Bearer <token>` header (REST) or `{"type":"auth","access_token":"..."}` message (WebSocket)

#### REST API (via sync `ureq`)

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/states` | GET | Get all entity states |
| `/api/states/{entity_id}` | GET | Get specific entity state |
| `/api/services/{domain}/{service}` | POST | Call service (turn_on, etc.) |

#### WebSocket API (via async `tokio-tungstenite`)

Connection: `ws://HOST:8123/api/websocket` (or `wss://` for TLS)

Auth flow:
```json
// Server sends on connect:
{"type": "auth_required", "ha_version": "2025.x"}

// Client responds:
{"type": "auth", "access_token": "TOKEN"}

// Server confirms:
{"type": "auth_ok", "ha_version": "2025.x"}
```

Key subscriptions:
```json
// Subscribe to all state changes
{"id": 1, "type": "subscribe_events", "event_type": "state_changed"}

// Get all current states
{"id": 2, "type": "get_states"}

// Call service
{"id": 3, "type": "call_service", "domain": "light", "service": "turn_on", "target": {"entity_id": "light.living_room"}}
```

### HA Domain Mapping

| HA Domain | Example Devices | NeoMind Metric | NeoMind Command |
|-----------|----------------|---------------|-----------------|
| `sensor` | Temperature, humidity, PM2.5 | Gauge value | — |
| `binary_sensor` | Door/window, motion, leak | State (on/off) | — |
| `light` | Smart bulbs, LED strips | State + brightness | turn_on/off, set brightness |
| `switch` | Smart plugs, relays | State (on/off) | turn_on/off |
| `climate` | HVAC, thermostat | State + temperature | set temperature, mode |
| `lock` | Smart locks | State (locked/unlocked) | lock/unlock |
| `cover` | Blinds, garage door | State + position | open/close/set_position |
| `camera` | IP cameras | Snapshot URL | — |
| `person` | Presence tracking | State (home/away) | — |

### Commands

| Command | Parameters | Description |
|---------|-----------|-------------|
| `connect` | `{url, token}` | Connect to HA (WebSocket + REST) |
| `disconnect` | `{}` | Disconnect from HA |
| `list_entities` | `{domain?}` | List HA entities (optionally filtered by domain) |
| `get_state` | `{entity_id}` | Get entity current state |
| `call_service` | `{domain, service, entity_id, service_data?}` | Call HA service |
| `set_filters` | `{domains: [string], entity_patterns: [string]}` | Set entity import filters |
| `get_areas` | `{}` | List HA areas/rooms |
| `get_status` | `{}` | HA connection status and stats |

### Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `ha.{entity_id}.state` | State | Entity state string |
| `ha.{entity_id}.value` | Gauge | Numeric sensor value |
| `ha.{entity_id}.battery` | Gauge | Battery level (%) |
| `ha.connection` | State | HA connection status |
| `ha.entities_count` | Gauge | Number of monitored entities |
| `ha.ws_events` | Counter | WebSocket events received |

### Config Parameters

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `ha_url` | String | Yes | — | HA base URL (e.g. http://192.168.1.100:8123) |
| `token` | String | Yes | — | Long-lived access token |
| `domains` | String | No | "sensor,light,switch" | Comma-separated domain filter |
| `entity_patterns` | String | No | — | Comma-separated entity ID patterns |
| `sync_interval` | Integer | No | 30 | Full state sync interval (seconds) |

### Dependencies

```toml
[dependencies]
neomind-extension-sdk = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
async-trait = { workspace = true }
chrono = "0.4"
tokio = { version = "1", features = ["rt", "sync"] }

tokio-tungstenite = { version = "0.29", features = ["rustls-tls-webpki-roots"] }
futures-util = "0.3"
ureq = { version = "3", features = ["json"] }
```

### Frontend Component

**`HADeviceCard`** — Card displaying:
- HA connection status
- Monitored entity list grouped by domain
- Quick controls (toggle switches, sliders for brightness/temperature)
- Last sync timestamp

Config schema:
```json
{
  "haUrl": { "type": "string", "title": "Home Assistant URL", "description": "e.g. http://192.168.1.100:8123" },
  "token": { "type": "string", "title": "Access Token" },
  "domains": {
    "type": "string",
    "title": "Domains",
    "description": "Comma-separated HA domains to import",
    "default": "sensor,light,switch,climate,lock"
  },
}
```
```

---

## Implementation Priority

| Batch | Extension | Complexity | Value | Rationale |
|-------|-----------|-----------|-------|-----------|
| 1 | modbus-bridge | Medium | Very High | Most requested industrial protocol; sync client avoids runtime issues |
| 1 | lorawan-bridge | Medium-High | High | Unique long-range IoT capability; complements agriculture/smart city |
| 2 | homeassistant-bridge | Medium | Very High | 3000+ device multiplier via HA ecosystem |

## Design Principle

Extensions are **data providers** — they bring external device data into NeoMind and expose control commands. All cross-device automation (e.g., "LoRa sensor threshold exceeded → trigger Modbus actuator") is handled by NeoMind's built-in rule engine, not by extensions.

## File Structure

Each extension follows the standard NeoMind structure:

```
extensions/
├── modbus-bridge/
│   ├── Cargo.toml
│   ├── src/
│   │   └── lib.rs
│   ├── metadata.json          (auto-generated)
│   └── frontend/
│       ├── frontend.json
│       ├── vite.config.ts
│       ├── package.json
│       ├── src/
│       │   └── index.tsx
│       └── dist/              (built UMD bundle)
├── lorawan-bridge/
│   ├── Cargo.toml
│   ├── src/
│   │   ├── lib.rs
│   │   └── decoders.rs       (Cayenne LPP + custom decoders)
│   ├── metadata.json
│   └── frontend/
│       └── ...
├── homeassistant-bridge/
│   ├── Cargo.toml
│   ├── src/
│   │   ├── lib.rs
│   │   ├── ws_client.rs      (WebSocket state subscription)
│   │   └── rest_client.rs    (REST API calls)
│   ├── metadata.json
│   └── frontend/
│       └── ...
└── index.json                 (updated with new extensions)
```

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|-----------|
| Tokio runtime conflicts in cdylib | Extension crash | modbus uses sync client; lorawan/HA use host runtime handle |
| Modbus device diversity | Incorrect register reads | Provide common device templates + clear error messages |
| LoRaWAN NS API version changes | Integration breakage | Abstract NS interface behind trait; test against ChirpStack v4 + TTN v3 |
| HA WebSocket reconnection | Data gaps | Exponential backoff + full state resync on reconnect |
| Serial port permissions | RTU connection failure | Document uaccess rules and Windows COM port setup |
