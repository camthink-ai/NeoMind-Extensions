# Uink-RMS Bridge

Bridge extension for Uink-RMS e-paper displays, providing device registration, telemetry collection, and image push capabilities.

## Features

- JWT authentication with automatic refresh token renewal
- Device template registration (`uink_epaper`)
- Batch device sync from Uink-RMS to NeoMind
- Telemetry collection (battery, temperature, signal strength, etc.)
- Image push to e-paper displays (multipart/form-data)
- Content-to-image conversion (text, markdown, HTML -> image -> push)
- Configurable dithering and resize modes

## Installation

```bash
# Build the extension
./build.sh --single uink-rms-bridge

# Or dev build with auto-install
./build.sh --dev --single uink-rms-bridge
```

## Commands

| Command | Description | Key Parameters |
|---------|-------------|----------------|
| `sync_devices` | Sync Uink devices from RMS to NeoMind (registers template + devices) | - |
| `list_devices` | List all synced e-paper devices with IDs, names, model, and online status | - |
| `push_content` | Push text, markdown, HTML, or image content to a display | `device_id`, `content_type`, `content` |
| `push_image` | Push an image to a display | `device_id`, `image_url` or `image_base64`, `dither_algorithm`, `resize_mode`, `padding_color` |
| `get_display_size` | Get display resolution for a device | `device_id` |
| `get_display` | Get current and pending display content for a device | `device_id` |
| `refresh_status` | Trigger a status refresh for a device | `device_id` |
| `refresh_auth` | Force refresh the JWT authentication token | - |

## Metrics

| Metric | Type | Unit | Description |
|--------|------|------|-------------|
| `sync_count` | Integer | count | Number of device sync operations |
| `push_count` | Integer | count | Number of image/content pushes |
| `device_count` | Integer | count | Number of synced devices |
| `error_count` | Integer | count | Number of operation errors |

## Frontend Component

**DisplayEditorCard** — An interactive card for viewing and pushing e-paper display content. Supports text, markdown, HTML, and image URL content types. Bound to a device data source for targeting specific e-paper displays.

- Default size: 380 x 420 px
- Auto-refresh every 30 seconds
- Requires device data source binding

## License

Apache-2.0
