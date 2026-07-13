# BACnet Bridge

Connect NeoMind to BACnet/IP building-automation networks for device discovery, sensor polling, change-of-value subscriptions, and control writes.

## Features

- BACnet/IP over UDP (default port 47808), hand-written APDU encode/decode with no external BACnet dependency
- Who-Is / I-Am broadcast discovery (capped at 500 devices)
- ReadProperty, ReadPropertyMultiple, and WriteProperty with BACnet priority array (1–16)
- SubscribeCOV (confirmed + unconfirmed) for push-based change-of-value updates
- Background listener thread for inbound COV notifications plus per-device polling threads
- Object types: analog / binary / multi-state input, output, and value
- Automatic NeoMind device registration and per-device metric publishing
- Analog Output/Value present-value writes coerced to REAL (tag 4) per ASHRAE 135-2020

## Installation

```bash
# Build from repository root
./build.sh --single bacnet-bridge

# Or build with Cargo directly
cargo build --release -p bacnet-bridge
```

## Commands

| Command | Description | Parameters |
|---------|-------------|------------|
| `discover` | Send Who-Is broadcast to discover devices | `low_id` (integer, optional, default 0), `high_id` (integer, optional, default 4194303), `timeout_ms` (integer, optional, default 3000) — wait window for I-Am responses |
| `read_property` | Read a property from a BACnet object | `device_id` (integer, required), `object_type` (enum, required, default `analog_input`), `instance` (integer, required), `property_id` (integer, optional, default 85 — 85=present_value, 77=object_name, 28=description, 117=units) |
| `read_property_multiple` | Read multiple properties in one request | `device_id` (integer, required), `objects` (JSON array, required) — items: `{object_type, instance, properties: [property_id, ...]}` |
| `write_property` | Write a value to a BACnet object | `device_id` (integer, required), `object_type` (enum, required, default `analog_output` — output/value types only), `instance` (integer, required), `property_id` (integer, optional, default 85), `value` (string, required), `priority` (integer, optional, default 8, 1–16) |
| `subscribe_cov` | Subscribe to Change-of-Value notifications | `device_id` (integer, required), `object_type` (enum, required, default `analog_input`), `instance` (integer, required), `lifetime` (integer, optional, default 0 — seconds, 0 = indefinite), `confirmed` (enum, optional, default `true`) |
| `unsubscribe_cov` | Cancel a COV subscription | `subscriber_id` (integer, required) — subscriber process ID returned by `subscribe_cov` |
| `add_device` | Manually add a device and start polling | `device` (JSON, required) — see Device Configuration |
| `remove_device` | Remove a device and stop polling | `device_id` (integer, required) |
| `list_devices` | List all discovered and configured devices | None |
| `get_device` | Get details of a device including its objects | `device_id` (integer, required) |
| `list_objects` | List all objects for a device | `device_id` (integer, required) |
| `get_status` | Extension status: devices and COV subscriptions | None |
| `configure` | Apply extension-level configuration | None |

## Metrics

| Metric | Display Name | Type | Unit | Range |
|--------|-------------|------|------|-------|
| `total_commands` | Total Commands | Integer | - | - |
| `connected_devices` | Connected Devices | Integer | - | - |
| `cov_subscriptions` | COV Subscriptions | Integer | - | - |

## Object Types

| Type | Description |
|------|-------------|
| `analog_input` / `analog_output` / `analog_value` | Numeric (floating-point) points |
| `binary_input` / `binary_output` / `binary_value` | Boolean (on/off) points |
| `multi_state_input` / `multi_state_output` / `multi_state_value` | Enumerated state points |

Writes are limited to the `output` / `value` variants (e.g. `analog_output`, `binary_value`); input objects are read-only.

## Device Configuration

`add_device` takes a `device` JSON object:

```json
{
  "device_id": 100,
  "ip": "192.168.1.100",
  "port": 47808,
  "name": "HVAC Controller",
  "poll_interval_ms": 5000,
  "objects": [
    { "object_type": "analog_input", "instance": 1, "name": "Temperature", "units": "degC" },
    { "object_type": "analog_input", "instance": 2, "name": "Humidity", "units": "%" },
    { "object_type": "binary_output", "instance": 1, "name": "Fan Status" }
  ]
}
```

## Configuration Parameters

| Parameter | Type | Default | Range | Description |
|-----------|------|---------|-------|-------------|
| `bindAddress` | String | `0.0.0.0` | — | Local IP address to bind for BACnet/IP |
| `bindPort` | Integer | `47808` | 1–65535 | UDP port for BACnet/IP |
| `defaultTimeoutMs` | Integer | `3000` | 100–30000 | Default request timeout (ms) |
| `pollIntervalMs` | Integer | `10000` | 1000–60000 | Default polling interval (ms) |

## Requirements

- BACnet/IP devices (sensors, actuators, controllers) reachable on the local network
- Network access to send/receive UDP on port 47808 (broadcast enabled for discovery)

## License

Apache-2.0
