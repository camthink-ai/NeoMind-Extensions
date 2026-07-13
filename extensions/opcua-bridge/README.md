# OPC-UA Bridge

Connect NeoMind to OPC-UA industrial servers for address-space browsing, node read/write, and data-change subscriptions.

## Features

- Connect to any OPC-UA server (`opc.tcp://`) with selectable security mode
- Security modes: `none`, `sign`, `sign_and_encrypt`; optional username/password authentication
- Browse the address space from any node (configurable depth, up to 10 levels)
- Read and write node values with optional explicit data type
- Subscribe / unsubscribe to data-change notifications (sampling interval 50–60000 ms)
- Auto-reconnect on connection loss (configurable, default on)
- Async client runs on a dedicated background thread; synchronous node cache for metrics
- Browsed nodes auto-register as NeoMind devices; per-node metrics published continuously

## Installation

```bash
# Build from repository root
./build.sh --single opcua-bridge

# Or build with Cargo directly
cargo build --release -p opcua-bridge
```

## Commands

| Command | Description | Parameters |
|---------|-------------|------------|
| `connect` | Connect to an OPC-UA server | `server_url` (string, required, e.g. `opc.tcp://host:4840`), `security_mode` (enum, optional, default `none` — `none` / `sign` / `sign_and_encrypt`), `username` (string, optional), `password` (string, optional) |
| `disconnect` | Disconnect from the server | None |
| `browse` | Browse the address space from a node | `node_id` (string, optional, default `i=84` — root), `max_depth` (integer, optional, default 1, 1–10) |
| `read` | Read current values from nodes | `node_ids` (JSON array or comma-separated string, required) |
| `write` | Write a value to a node | `node_id` (string, required), `value` (string, required), `data_type` (string, optional, e.g. `Float`, `Int32`) |
| `subscribe` | Subscribe to data-change notifications | `node_ids` (JSON array or comma-separated string, required), `interval_ms` (integer, optional, default 1000, 50–60000) |
| `unsubscribe` | Unsubscribe from notifications | `node_ids` (JSON array or comma-separated string, required) |
| `list_subscriptions` | List all active subscriptions | None |
| `list_nodes` | List all cached nodes | None |
| `get_node` | Get details of a cached node | `node_id` (string, required) |
| `get_status` | Get connection and cache status | None |

## Metrics

| Metric | Display Name | Type | Unit | Range |
|--------|-------------|------|------|-------|
| `total_commands` | Total Commands | Integer | - | - |
| `connected` | Connected | Integer | - | - |
| `nodes_count` | Cached Nodes | Integer | - | - |
| `subscriptions_count` | Active Subscriptions | Integer | - | - |

## Node IDs

Node IDs use standard OPC-UA notation:

| Form | Example | Meaning |
|------|---------|---------|
| Numeric | `i=84` | Root folder (Objects is `i=85`) |
| Namespaced string | `ns=2;s=Temperature` | String node in namespace 2 |

`read`, `subscribe`, and `unsubscribe` accept either a JSON array (`["i=2258", "ns=2;s=Temp"]`) or a comma-separated string (`"i=2258, ns=2;s=Temp"`).

## Configuration Parameters

| Parameter | Type | Default | Range | Description |
|-----------|------|---------|-------|-------------|
| `sessionTimeout` | Integer | `30000` | 1000–300000 | OPC-UA session timeout (ms) |
| `autoReconnect` | String | `true` | `true` / `false` | Reconnect automatically on connection loss |

## Requirements

- An OPC-UA server reachable on the network (e.g. `opc.tcp://host:4840`)
- Credentials and certificates as required by the chosen security mode

## License

Apache-2.0
