# Home Assistant Bridge

Connect NeoMind to Home Assistant for bidirectional device control and state synchronization via REST and WebSocket APIs.

## Features

- Dual-connection mode: REST API (simple polling) and WebSocket API (real-time event streaming)
- Automatic entity discovery and state synchronization
- Entity type filtering (light, switch, sensor, binary_sensor, climate, etc.)
- Service call support for device control (turn on/off, toggle, set values)
- WebSocket auto-reconnect with exponential backoff and retry cap
- Area-aware entity grouping (via WebSocket API)
- Configurable connection parameters (URL, token, polling interval)

## Installation

```bash
# Build from repository root
./build.sh --single homeassistant-bridge

# Or build with Cargo directly
cargo build --release -p homeassistant-bridge
```

## Commands

| Command | Description | Parameters |
|---------|-------------|------------|
| `connect` | Connect to Home Assistant | `url` (string) - HA URL, `token` (string) - Long-lived access token, `mode` (string, optional) - "rest" or "websocket", default "rest", `poll_interval_ms` (integer, optional) - Polling interval, default 5000 |
| `disconnect` | Disconnect from Home Assistant | None |
| `list_entities` | List all discovered entities | `entity_type` (string, optional) - Filter by type (light, switch, sensor, etc.) |
| `get_state` | Get current state of an entity | `entity_id` (string, required) - Entity ID (e.g. "light.living_room") |
| `call_service` | Call a Home Assistant service | `service` (string, required) - Service to call (e.g. "light.turn_on"), `entity_id` (string, required) - Target entity, `service_data` (object, optional) - Service parameters |
| `set_filters` | Set entity type filters | `entity_types` (array of strings, required) - Types to track |
| `get_status` | Get connection status and statistics | None |
| `get_areas` | Get HA areas (WebSocket mode only) | None |
| `refresh` | Force refresh all entity states | None |
| `configure` | Update connection configuration | `poll_interval_ms` (integer, optional), `filters` (array of strings, optional) |

## Metrics

| Metric | Display Name | Type | Unit | Range |
|--------|-------------|------|------|-------|
| `connected` | Connected | Boolean | - | - |
| `entity_count` | Entity Count | Integer | - | 0 to 10000 |
| `poll_errors` | Poll Errors | Integer | - | 0 to 100000 |
| `last_poll_ms` | Last Poll Duration | Integer | ms | 0 to 60000 |

## Connection Modes

### REST Mode
Simple HTTP polling using HA REST API. Suitable for basic monitoring scenarios. Polls entity states at configured interval.

### WebSocket Mode
Real-time event streaming via HA WebSocket API. Receives state changes instantly without polling. Supports area discovery and event subscriptions. Automatically reconnects on disconnection with exponential backoff (capped at 30 seconds, resets on successful auth).

## Requirements

- Home Assistant instance accessible via network
- Long-lived access token (generated in HA Profile > Security > Long-Lived Access Tokens)

## License

Apache-2.0
