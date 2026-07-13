# ONVIF Bridge

Connect NeoMind to ONVIF-compliant IP cameras for discovery, RTSP stream retrieval, snapshot access, and PTZ control.

## Features

- WS-Discovery (UDP multicast) for automatic camera detection on the local network
- Manual camera registration by URL/IP with automatic enrichment (device info, media profiles, stream URIs)
- RTSP stream URI retrieval per media profile (RTP-Unicast or RTP-Multicast)
- Snapshot URI retrieval for still-image capture
- Full PTZ control: relative move, absolute move, stop, go home, list/goto presets, status
- Per-camera WS-UsernameToken (WS-Security) authentication
- Automatic NeoMind device registration and per-device metric publishing
- Sync HTTP client (ureq) for SOAP calls — safe inside the cdylib runtime

## Installation

```bash
# Build from repository root
./build.sh --single onvif-bridge

# Or build with Cargo directly
cargo build --release -p onvif-bridge
```

## Commands

| Command | Description | Parameters |
|---------|-------------|------------|
| `discover` | Discover ONVIF cameras via WS-Discovery | `timeout_ms` (integer, optional, default 5000) |
| `add_device` | Manually add a camera by URL/IP | `device` (JSON, required) — see Device Configuration |
| `remove_device` | Remove a camera | `device_id` (string, required) |
| `list_devices` | List all configured cameras and status | None |
| `get_device` | Get details of a specific camera | `device_id` (string, required) |
| `get_stream_uri` | Get the RTSP stream URI for a profile | `device_id` (string, required), `profile_token` (string, optional — uses first profile if omitted), `stream_type` (enum, optional, default `RTP-Unicast` — `RTP-Unicast` / `RTP-Multicast`) |
| `get_snapshot` | Get the snapshot URI for a profile | `device_id` (string, required), `profile_token` (string, optional) |
| `ptz_move` | Move PTZ by relative offset | `device_id` (string, required), `profile_token` (string, optional), `pan` (float, optional, default 0.0, -1.0 to 1.0), `tilt` (float, optional, default 0.0, -1.0 to 1.0), `zoom` (float, optional, default 0.0, -1.0 to 1.0), `speed` (float, optional, default 0.5, 0.0 to 1.0) |
| `ptz_absolute` | Move PTZ to absolute position | `device_id` (string, required), `profile_token` (string, optional), `pan` (float, required, -1.0 to 1.0), `tilt` (float, required, -1.0 to 1.0), `zoom` (float, optional, default 0.0, 0.0 to 1.0), `speed` (float, optional, default 0.5, 0.0 to 1.0) |
| `ptz_stop` | Stop current PTZ movement | `device_id` (string, required), `profile_token` (string, optional) |
| `ptz_home` | Move camera to home position | `device_id` (string, required), `profile_token` (string, optional) |
| `list_presets` | List PTZ presets for a profile | `device_id` (string, required), `profile_token` (string, optional) |
| `goto_preset` | Move to a saved PTZ preset | `device_id` (string, required), `profile_token` (string, optional), `preset_token` (string, required) |
| `get_status` | Get current PTZ status | `device_id` (string, required), `profile_token` (string, optional) |
| `configure` | Apply extension-level configuration | None |

## Metrics

| Metric | Display Name | Type | Unit | Range |
|--------|-------------|------|------|-------|
| `total_commands` | Total Commands | Integer | - | - |
| `connected_devices` | Connected Devices | Integer | - | - |

## Device Configuration

`add_device` takes a `device` JSON object:

```json
{
  "device_id": "cam-001",
  "name": "Front Door",
  "device_url": "http://192.168.1.50/onvif/device_service",
  "username": "admin",
  "password": "secret"
}
```

`device_id` must be 1–64 characters, alphanumeric / `-` / `_` only. On add, the bridge enriches the device with manufacturer, model, firmware, serial, media profiles, stream/snapshot URIs, and PTZ capability. The device is registered even if enrichment calls partially fail.

PTZ commands return an error if the camera does not advertise PTZ support.

## Configuration Parameters

| Parameter | Type | Default | Range | Description |
|-----------|------|---------|-------|-------------|
| `discoveryTimeoutMs` | Integer | `5000` | 1000–30000 | WS-Discovery probe timeout (ms) |
| `defaultUsername` | String | — | — | Default username for discovered cameras |
| `defaultPassword` | String | — | — | Default password for discovered cameras |

## Requirements

- ONVIF-compliant IP cameras on the local network (Profile S for streaming)
- HTTP access to camera SOAP device/media/PTZ service endpoints
- Username/password for cameras with authentication enabled

## License

Apache-2.0
