# Weather Forecast Frontend

Built for the NeoMind extension runtime.

## Quick Start

```bash
# Install dependencies
npm install

# Build for production
npm run build
```

## Output

- `dist/weather-forecast-components.umd.js` - UMD bundle for dashboard

## Component

### WeatherCard

Display real-time weather data with temperature, humidity, wind speed, and more.

**Props:**
- `title` - Card title (default: "Weather Forecast")
- `dataSource` - Data source configuration with `extensionId`
- `defaultCity` - Default city to display (default: "Beijing")
- `refreshInterval` - Auto-refresh interval in ms (default: 300000)
- `unit` - Temperature unit: "celsius" or "fahrenheit" (default: "celsius")

## Usage

```tsx
import { WeatherCard } from '@neomind/weather-forecast-frontend'

<WeatherCard
  dataSource={{ extensionId: 'weather-forecast' }}
  defaultCity="Shanghai"
  unit="celsius"
/>
```

## API

The component uses the standard SDK API helpers:

### `executeExtensionCommand<T>(extensionId, command, args)`

Execute a command on the extension:

```typescript
const result = await executeExtensionCommand<WeatherData>(
  'weather-forecast',
  'get_weather',
  { city: 'Beijing' }
)
```
