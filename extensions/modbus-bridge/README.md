# Modbus Bridge

Connect NeoMind to Modbus TCP/RTU devices for register polling, data decoding, and read/write control.

## Features

- Dual protocol support: Modbus TCP and Modbus RTU (serial)
- Multi-device management with independent polling loops
- Flexible register map with type decoding (uint16, int16, uint32, float32, etc.)
- All four register types: Holding, Input, Coils, Discrete Inputs
- Configurable polling interval per device
- Persistent connections with automatic reconnect on failure
- Protocol-compliant register count validation (125 max for registers, 2000 for coils)
- Read and write operations for on-demand device interaction

## Installation

```bash
# Build from repository root
./build.sh --single modbus-bridge

# Or build with Cargo directly
cargo build --release -p modbus-bridge
```

## Commands

| Command | Description | Parameters |
|---------|-------------|------------|
| `add_device` | Add a Modbus device for polling | `device_id` (string, required), `mode` (string, required) - "tcp" or "rtu", `slave_id` (integer, required, 1-247), `ip` (string, required for TCP), `port` (integer, optional, default 502), `serial_port` (string, required for RTU), `baud_rate` (integer, optional, default 9600), `poll_interval_ms` (integer, optional, default 1000), `timeout_ms` (integer, optional, default 3000), `registers` (array, optional) - Register definitions |
| `remove_device` | Remove a device and stop polling | `device_id` (string, required) |
| `list_devices` | List all configured devices | None |
| `get_device_data` | Get latest polled data for a device | `device_id` (string, required) |
| `read_registers` | Read holding registers on demand | `device_id` (string, required), `address` (integer, required), `count` (integer, required, max 125) |
| `write_register` | Write a single holding register | `device_id` (string, required), `address` (integer, required), `value` (integer, required) |
| `write_registers` | Write multiple holding registers | `device_id` (string, required), `address` (integer, required), `values` (array of integers, required) |
| `write_coil` | Write a single coil | `device_id` (string, required), `address` (integer, required), `value` (boolean, required) |
| `write_coils` | Write multiple coils | `device_id` (string, required), `address` (integer, required), `values` (array of booleans, required) |
| `update_polling` | Change polling interval for a device | `device_id` (string, required), `poll_interval_ms` (integer, required) |
| `set_register_map` | Update register map for a device | `device_id` (string, required), `registers` (array, required) - Register definitions |
| `configure` | Update bridge configuration | `device_id` (string, optional), `poll_interval_ms` (integer, optional) |

## Metrics

| Metric | Display Name | Type | Unit | Range |
|--------|-------------|------|------|-------|
| `device_count` | Device Count | Integer | - | 0 to 1000 |
| `total_poll_errors` | Total Poll Errors | Integer | - | 0 to 1000000 |
| `connected_devices` | Connected Devices | Integer | - | 0 to 1000 |

## Register Map Definition

Each register entry defines how to read and decode a Modbus register:

```json
{
  "name": "temperature",
  "address": 0,
  "count": 1,
  "register_type": "holding",
  "data_type": "int16",
  "scale": 0.1,
  "unit": "°C"
}
```

### Register Types

| Type | Function Code | Max Count | Description |
|------|--------------|-----------|-------------|
| `holding` | FC03 | 125 | Read/Write holding registers |
| `input` | FC04 | 125 | Read-only input registers |
| `coil` | FC01 | 2000 | Read/Write coils (boolean) |
| `discrete_input` | FC02 | 2000 | Read-only discrete inputs (boolean) |

### Data Types

| Type | Bytes | Description |
|------|-------|-------------|
| `uint16` | 2 | Unsigned 16-bit integer |
| `int16` | 2 | Signed 16-bit integer |
| `uint32` | 4 | Unsigned 32-bit integer (2 registers) |
| `int32` | 4 | Signed 32-bit integer (2 registers) |
| `float32` | 4 | IEEE 754 float (2 registers) |

### Scale Factor

The `scale` field multiplies the raw register value. Use it to convert integer registers to real-world values:
- Temperature sensor: raw `245`, scale `0.1` → `24.5 °C`
- Voltage sensor: raw `2301`, scale `0.001` → `2.301 V`

## Requirements

- Modbus TCP device accessible via network, or
- Modbus RTU device connected via serial port
- Correct slave ID (unit ID) for each device

## License

Apache-2.0
