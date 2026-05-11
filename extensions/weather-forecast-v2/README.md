# Weather Forecast V2

Real-time weather forecast with multi-city support, Open-Meteo API integration, and metric data export for NeoMind dashboards.

## Features

- Real-time weather data from Open-Meteo API (no API key required)
- Multi-city support with configurable default city
- Comprehensive weather metrics: temperature, humidity, wind, cloud cover, pressure
- Automatic data caching for metric collection
- Day/night indicator with weather code descriptions
- Configurable refresh interval and temperature unit (Celsius/Fahrenheit)

## Installation

```bash
# Build from repository root
./build.sh --single weather-forecast-v2

# Or build with Cargo directly
cargo build --release -p weather-forecast-v2
```

## Commands

| Command | Description | Parameters |
|---------|-------------|------------|
| `get_weather` | Get current weather for a city | `city` (string, required) - City name |
| `refresh` | Refresh weather for the default city | None |
| `set_default_city` | Change the default city | `city` (string, required) - City name |

## Metrics

| Metric | Display Name | Type | Unit | Range |
|--------|-------------|------|------|-------|
| `temperature_c` | Temperature | Float | °C | -100 to 100 |
| `feels_like_c` | Feels Like | Float | °C | -100 to 100 |
| `humidity_percent` | Humidity | Integer | % | 0 to 100 |
| `wind_speed_kmph` | Wind Speed | Float | km/h | 0 to 500 |
| `wind_direction_deg` | Wind Direction | Integer | ° | 0 to 360 |
| `cloud_cover_percent` | Cloud Cover | Integer | % | 0 to 100 |
| `pressure_hpa` | Pressure | Float | hPa | 800 to 1200 |
| `request_count` | Request Count | Integer | - | - |
| `last_update_ts` | Last Update Timestamp | Integer | ms | - |

## Frontend Component

**WeatherCard** - A card component that displays real-time weather data including temperature, humidity, wind speed, and weather conditions. Supports configurable default city, refresh interval (default 5 min), and temperature unit. Uses NeoMind CSS variables for light/dark mode compatibility.

## License

Apache-2.0
