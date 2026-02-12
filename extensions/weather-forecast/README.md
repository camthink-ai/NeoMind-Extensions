# Weather Forecast Extension

Weather data and forecasts for global cities.

## Features

- **Current Weather**: Get real-time weather for any city worldwide
- **Forecasts**: 3-day weather predictions
- **Metrics**: Temperature, humidity, wind speed, cloud cover
- **AI Integration**: Tools designed for AI agent use

## Installation

This extension is part of the NeoMind Extensions workspace. Build from the root:

```bash
cd ~/NeoMind-Extension
cargo build --release -p neomind-weather-forecast
./build.sh
```

## Usage

### Via API

```bash
# Query current weather
curl -X POST http://localhost:9375/api/extensions/neomind.weather.forecast/command \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -d '{
    "command": "refresh",
    "args": {"city": "Beijing"}
  }'
```

### Via AI Agent

The extension automatically registers tools that AI agents can use:

```
User: What's the weather in Tokyo?
Agent: [Calls query_weather tool] Currently in Tokyo: 18°C, Clear, Humidity: 45%
```

## Configuration

Set the `OPENWEATHER_API_KEY` environment variable for real data:

```bash
export OPENWEATHER_API_KEY="your_api_key_here"
```

Without an API key, the extension operates in demo mode with simulated data.

## Capabilities

| Capability | Type | Description |
|-----------|------|-------------|
| `query_weather` | Tool | Get current weather for any city |
| `query_forecast` | Tool | Get 3-day weather forecast |
| `refresh` | Command | Force refresh cached data |
| `set_city` | Command | Change default city |

## Metrics

| Metric | Unit | Description |
|--------|------|-------------|
| `temperature_c` | °C | Current temperature |
| `humidity_percent` | % | Relative humidity |
| `wind_speed_kmph` | km/h | Wind speed |
| `cloud_cover_percent` | % | Cloud coverage |
