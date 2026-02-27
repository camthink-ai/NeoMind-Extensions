/**
 * Weather Forecast V2 Dashboard Component
 * Built with NeoMind Extension SDK V2
 *
 * Features:
 * - Standardized API helpers from SDK
 * - CSS variable-based theming
 * - ABI version 3 compatible
 */

import { forwardRef, useEffect, useState, useCallback } from 'react'

// ============================================================================
// SDK Types (from @neomind/extension-sdk/frontend)
// ============================================================================

export interface ExtensionComponentProps {
  title?: string
  dataSource?: DataSource
  className?: string
  config?: Record<string, any>
}

export interface DataSource {
  type: string
  extensionId?: string
  [key: string]: any
}

interface ExtensionCommandResult<T> {
  success: boolean
  data?: T
  error?: string
}

// ============================================================================
// API Helpers
// ============================================================================

const EXTENSION_ID = 'weather-forecast-v2'

const getAuthToken = (): string | null => {
  return localStorage.getItem('neomind_token') ||
         sessionStorage.getItem('neomind_token_session') ||
         localStorage.getItem('token') ||
         null
}

const getApiBase = (): string => {
  if (typeof window !== 'undefined' && (window as any).__TAURI__) {
    return 'http://localhost:9375/api'
  }
  return '/api'
}

async function executeExtensionCommand<T>(
  extensionId: string,
  command: string,
  args: Record<string, any>
): Promise<ExtensionCommandResult<T>> {
  const token = getAuthToken()
  const apiBase = getApiBase()
  const headers: Record<string, string> = { 'Content-Type': 'application/json' }
  if (token) headers['Authorization'] = `Bearer ${token}`

  try {
    const response = await fetch(`${apiBase}/extensions/${extensionId}/command`, {
      method: 'POST',
      headers,
      body: JSON.stringify({ command, args })
    })

    if (!response.ok) {
      return {
        success: false,
        error: response.status === 401 ? 'Authentication required' : `HTTP ${response.status}`
      }
    }

    const data = await response.json()
    return data
  } catch (err) {
    return {
      success: false,
      error: err instanceof Error ? err.message : 'Network error'
    }
  }
}

// eslint-disable-next-line @typescript-eslint/no-unused-vars
async function _getExtensionMetrics(extensionId: string): Promise<ExtensionCommandResult<Record<string, any>>> {
  const token = getAuthToken()
  const apiBase = getApiBase()
  const headers: Record<string, string> = {}
  if (token) headers['Authorization'] = `Bearer ${token}`

  try {
    const response = await fetch(`${apiBase}/extensions/${extensionId}/metrics`, { headers })

    if (!response.ok) {
      return { success: false, error: `HTTP ${response.status}` }
    }

    const data = await response.json()
    return data
  } catch (err) {
    return { success: false, error: err instanceof Error ? err.message : 'Network error' }
  }
}

// ============================================================================
// Weather Types
// ============================================================================

interface WeatherData {
  city: string
  country?: string
  temperature_c: number
  feels_like_c?: number
  humidity_percent: number
  wind_speed_kmph: number
  wind_direction_deg?: number
  wind_direction?: string
  cloud_cover_percent?: number
  pressure_hpa?: number
  is_day?: boolean
  description: string
  timestamp: string
}

// ============================================================================
// Icons
// ============================================================================

const Icons = {
  sun: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
      <circle cx="12" cy="12" r="4" />
      <path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41" />
    </svg>
  ),
  moon: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
      <path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9z" />
    </svg>
  ),
  cloud: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
      <path d="M18 10h-1.26A8 8 0 1 0 9 20h9a5 5 0 0 0 0-10z" />
    </svg>
  ),
  'cloud-sun': (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
      <path d="M12 2v2M4.93 4.93l1.41 1.41M20 12h2M19.07 4.93l-1.41 1.41" />
      <path d="M17.5 19H9a6 6 0 1 1 3.34-11A5 5 0 0 1 17.5 19z" />
    </svg>
  ),
  'cloud-rain': (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
      <path d="M16 13V21M8 13V21M12 15V23" />
      <path d="M20 16.58A5 5 0 0 0 18 7h-1.26A8 8 0 1 0 4 15.25" />
    </svg>
  ),
  'cloud-snow': (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
      <path d="M20 17.58A5 5 0 0 0 18 8h-1.26A8 8 0 1 0 4 16.25" />
      <path d="M8 16h.01M8 20h.01M12 18h.01M12 22h.01M16 16h.01M16 20h.01" />
    </svg>
  ),
  'cloud-lightning': (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
      <path d="M19 16.9A5 5 0 0 0 18 7h-1.26a8 8 0 1 0-11.62 9" />
      <polyline points="13 11 9 17 15 17 11 23" />
    </svg>
  ),
  refresh: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
      <path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8" />
      <path d="M21 3v5h-5" />
    </svg>
  ),
  droplet: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
      <path d="M12 22a7 7 0 0 0 7-7c0-2-1-3.9-3-5.5s-3.5-4-4-6.5c-.5 2.5-2 4.9-4 6.5C6 11.1 5 13 5 15a7 7 0 0 0 7 7z" />
    </svg>
  ),
  wind: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
      <path d="M17.7 7.7a2.5 2.5 0 1 1 1.8 4.3H2" />
      <path d="M9.6 4.6A2 2 0 1 1 11 8H2" />
      <path d="M12.6 19.4A2 2 0 1 0 14 16H2" />
    </svg>
  ),
  compass: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
      <circle cx="12" cy="12" r="10" />
      <polygon points="16.24 7.76 14.12 14.12 7.76 16.24 9.88 9.88 16.24 7.76" fill="currentColor" stroke="none" />
    </svg>
  ),
  gauge: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
      <path d="M12 16v-4M12 8h.01M22 12a10 10 0 1 1-20 0 10 10 0 0 1 20 0z" />
    </svg>
  ),
  cloudCover: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
      <path d="M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9z" />
    </svg>
  ),
  thermometer: (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
      <path d="M14 4v10.54a4 4 0 1 1-4 0V4a2 2 0 0 1 4 0z" />
    </svg>
  ),
}

const IconColors = {
  humidity: '#3b82f6',
  wind: '#06b6d4',
  feelsLike: '#f97316',
  windDir: '#8b5cf6',
  cloud: '#64748b',
  pressure: '#10b981',
}

const getWeatherIcon = (description: string | undefined, isDay?: boolean): JSX.Element => {
  const desc = (description || '').toLowerCase()
  const isDayTime = isDay !== undefined ? isDay : true

  if (desc.includes('clear') || desc.includes('sunny')) return isDayTime ? Icons.sun : Icons.moon
  if (desc.includes('rain') || desc.includes('drizzle') || desc.includes('shower')) return Icons['cloud-rain']
  if (desc.includes('snow')) return Icons['cloud-snow']
  if (desc.includes('thunder')) return Icons['cloud-lightning']
  if (desc.includes('cloud') && (desc.includes('partly') || desc.includes('mainly'))) return isDayTime ? Icons['cloud-sun'] : Icons.cloud
  if (desc.includes('cloud') || desc.includes('overcast')) return Icons.cloud
  return isDayTime ? Icons['cloud-sun'] : Icons.cloud
}

// ============================================================================
// Weather Card Component
// ============================================================================

export interface WeatherCardProps extends ExtensionComponentProps {
  defaultCity?: string
  refreshInterval?: number
  unit?: 'celsius' | 'fahrenheit'
}

export const WeatherCard = forwardRef<HTMLDivElement, WeatherCardProps>(
  function WeatherCard(props, ref) {
    const {
      title: _title = 'Weather Forecast V2',
      dataSource,
      className = '',
      defaultCity = 'Beijing',
      refreshInterval = 300000,
      unit = 'celsius'
    } = props

    const [weather, setWeather] = useState<WeatherData | null>(null)
    const [loading, setLoading] = useState(false)
    const [error, setError] = useState<string | null>(null)

    const extensionId = dataSource?.extensionId || EXTENSION_ID

    const fetchWeather = useCallback(async (city: string) => {
      setLoading(true)
      setError(null)

      const result = await executeExtensionCommand<WeatherData>(
        extensionId,
        'get_weather',
        { city }
      )

      if (result.success && result.data) {
        setWeather(result.data)
      } else {
        setError(result.error || 'Failed to fetch weather')
      }

      setLoading(false)
    }, [extensionId])

    useEffect(() => {
      fetchWeather(defaultCity)
      if (refreshInterval > 0) {
        const interval = setInterval(() => fetchWeather(defaultCity), refreshInterval)
        return () => clearInterval(interval)
      }
    }, [fetchWeather, defaultCity, refreshInterval])

    const displayTemp = (tempC: number): string =>
      unit === 'fahrenheit' ? `${Math.round(tempC * 9 / 5 + 32)}°` : `${Math.round(tempC)}°`

    const coloredIcon = (icon: JSX.Element, color: string) => (
      <span style={{ color, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>{icon}</span>
    )

    return (
      <div ref={ref} className={`wfc ${className}`}>
        <style>{`
          .wfc {
            --ext-bg: rgba(255, 255, 255, 0.25);
            --ext-fg: hsl(240 10% 10%);
            --ext-muted: hsl(240 5% 40%);
            --ext-border: rgba(255, 255, 255, 0.5);
            --ext-accent: hsl(221 83% 53%);
            --ext-glass: rgba(255, 255, 255, 0.3);
            width: 100%;
            height: 100%;
          }
          .dark .wfc {
            --ext-bg: rgba(30, 30, 30, 0.4);
            --ext-fg: hsl(0 0% 95%);
            --ext-muted: hsl(0 0% 65%);
            --ext-border: rgba(255, 255, 255, 0.1);
            --ext-glass: rgba(255, 255, 255, 0.06);
          }
          .wfc-card {
            background: var(--ext-bg);
            backdrop-filter: blur(24px) saturate(180%);
            -webkit-backdrop-filter: blur(24px) saturate(180%);
            border: 1px solid var(--ext-border);
            border-radius: 16px;
            padding: 8px;
            height: 100%;
            width: 100%;
            display: flex;
            flex-direction: column;
            box-sizing: border-box;
            overflow: hidden;
            box-shadow: 0 8px 32px rgba(0, 0, 0, 0.12);
          }
          .wfc-refresh {
            width: 14px;
            height: 14px;
            background: transparent;
            border: none;
            color: var(--ext-muted);
            cursor: pointer;
            padding: 0;
            opacity: 0.5;
            transition: opacity 0.2s;
          }
          .wfc-refresh:hover { opacity: 1; color: var(--ext-accent); }
          .wfc-refresh svg { width: 12px; height: 12px; }
          .wfc-refresh.spin svg { animation: wfc-spin 1s linear infinite; }
          @keyframes wfc-spin { to { transform: rotate(360deg); } }

          .wfc-top {
            display: flex;
            align-items: center;
            justify-content: space-between;
            padding: 2px 0 0 6px;
            flex-shrink: 0;
          }
          .wfc-time {
            font-size: 9px;
            color: var(--ext-muted);
          }

          .wfc-main {
            display: flex;
            flex-direction: column;
            align-items: center;
            text-align: center;
            padding: 4px 8px;
            flex: 0 0 auto;
          }
          .wfc-location {
            font-size: clamp(11px, 3vw, 14px);
            font-weight: 600;
            color: var(--ext-fg);
            letter-spacing: -0.02em;
            margin-bottom: 2px;
          }
          .wfc-weather-row {
            display: flex;
            align-items: center;
            gap: 3px;
          }
          .wfc-icon {
            width: clamp(18px, 5vw, 24px);
            height: clamp(18px, 5vw, 24px);
            color: var(--ext-accent);
            flex-shrink: 0;
          }
          .wfc-temp {
            font-size: clamp(18px, 5vw, 24px);
            font-weight: 600;
            color: var(--ext-fg);
            line-height: 1;
          }
          .wfc-desc {
            font-size: clamp(9px, 2vw, 10px);
            color: var(--ext-muted);
            margin-top: 1px;
            text-transform: capitalize;
          }

          .wfc-grid {
            display: grid;
            grid-template-columns: repeat(3, 1fr);
            gap: 3px;
            flex: 1;
            min-height: 0;
            margin-top: 8px;
          }
          .wfc-stat {
            display: flex;
            align-items: center;
            gap: 4px;
            padding: 4px 6px;
            background: var(--ext-glass);
            border-radius: 5px;
            min-width: 0;
            overflow: hidden;
          }
          .wfc-stat-icon {
            width: 14px;
            height: 14px;
            flex-shrink: 0;
          }
          .wfc-stat-icon svg { width: 100%; height: 100%; }
          .wfc-stat-content {
            display: flex;
            flex-direction: column;
            min-width: 0;
            overflow: hidden;
          }
          .wfc-stat-value { font-size: 11px; font-weight: 600; color: var(--ext-fg); line-height: 1.2; white-space: nowrap; }
          .wfc-stat-label { font-size: 8px; color: var(--ext-muted); line-height: 1.1; white-space: nowrap; }

          @container (max-width: 200px) {
            .wfc-grid { grid-template-columns: repeat(2, 1fr); }
            .wfc-main { padding: 6px; }
          }

          @container (max-width: 120px) {
            .wfc-grid { grid-template-columns: 1fr; }
            .wfc-stat { padding: 3px 5px; }
          }

          .wfc-loading, .wfc-error {
            display: flex;
            align-items: center;
            justify-content: center;
            flex: 1;
            color: var(--ext-muted);
            font-size: 11px;
          }
          .wfc-spinner {
            width: 20px;
            height: 20px;
            border: 2px solid var(--ext-border);
            border-top-color: var(--ext-accent);
            border-radius: 50%;
            animation: wfc-spin 0.8s linear infinite;
          }
        `}</style>

        <div className="wfc-card">
          {loading && !weather ? (
            <div className="wfc-loading"><div className="wfc-spinner" /></div>
          ) : error ? (
            <div className="wfc-error">{error}</div>
          ) : weather ? (
            <>
              <div className="wfc-top">
                {weather.timestamp && (
                  <div className="wfc-time">{new Date(weather.timestamp).toLocaleTimeString()}</div>
                )}
                <button onClick={() => fetchWeather(defaultCity)} disabled={loading} className={`wfc-refresh ${loading ? 'spin' : ''}`}>
                  {Icons.refresh}
                </button>
              </div>
              <div className="wfc-main">
                <div className="wfc-location">{weather.city}</div>
                <div className="wfc-weather-row">
                  <div className="wfc-icon">{getWeatherIcon(weather.description, weather.is_day)}</div>
                  <div className="wfc-temp">{displayTemp(weather.temperature_c)}</div>
                </div>
                <div className="wfc-desc">{weather.description}</div>
              </div>

              <div className="wfc-grid">
                <div className="wfc-stat">
                  <div className="wfc-stat-icon">{coloredIcon(Icons.droplet, IconColors.humidity)}</div>
                  <div className="wfc-stat-content">
                    <div className="wfc-stat-value">{weather.humidity_percent}%</div>
                    <div className="wfc-stat-label">Humidity</div>
                  </div>
                </div>
                <div className="wfc-stat">
                  <div className="wfc-stat-icon">{coloredIcon(Icons.wind, IconColors.wind)}</div>
                  <div className="wfc-stat-content">
                    <div className="wfc-stat-value">{weather.wind_speed_kmph}</div>
                    <div className="wfc-stat-label">km/h</div>
                  </div>
                </div>
                <div className="wfc-stat">
                  <div className="wfc-stat-icon">{coloredIcon(Icons.thermometer, IconColors.feelsLike)}</div>
                  <div className="wfc-stat-content">
                    <div className="wfc-stat-value">{weather.feels_like_c ? displayTemp(weather.feels_like_c) : '-'}</div>
                    <div className="wfc-stat-label">Feels</div>
                  </div>
                </div>
                <div className="wfc-stat">
                  <div className="wfc-stat-icon">{coloredIcon(Icons.compass, IconColors.windDir)}</div>
                  <div className="wfc-stat-content">
                    <div className="wfc-stat-value">{weather.wind_direction || (weather.wind_direction_deg ? `${weather.wind_direction_deg}°` : '-')}</div>
                    <div className="wfc-stat-label">Wind</div>
                  </div>
                </div>
                <div className="wfc-stat">
                  <div className="wfc-stat-icon">{coloredIcon(Icons.cloudCover, IconColors.cloud)}</div>
                  <div className="wfc-stat-content">
                    <div className="wfc-stat-value">{weather.cloud_cover_percent ?? '-'}{weather.cloud_cover_percent !== undefined ? '%' : ''}</div>
                    <div className="wfc-stat-label">Cloud</div>
                  </div>
                </div>
                <div className="wfc-stat">
                  <div className="wfc-stat-icon">{coloredIcon(Icons.gauge, IconColors.pressure)}</div>
                  <div className="wfc-stat-content">
                    <div className="wfc-stat-value">{weather.pressure_hpa ? Math.round(weather.pressure_hpa) : '-'}</div>
                    <div className="wfc-stat-label">hPa</div>
                  </div>
                </div>
              </div>
            </>
          ) : null}
        </div>
      </div>
    )
  }
)

WeatherCard.displayName = 'WeatherCard'

export default { WeatherCard }
