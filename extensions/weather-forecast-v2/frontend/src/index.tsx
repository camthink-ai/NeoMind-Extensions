/**
 * Weather Forecast V2 - Compact Edition
 * Matches NeoMind dashboard design system
 */

import { forwardRef, useEffect, useState, useCallback, useRef, useMemo } from 'react'

// ============================================================================
// Types
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

interface WeatherData {
  city: string
  country?: string
  temperature_c: number
  feels_like_c?: number
  humidity_percent: number
  wind_speed_kmph: number
  wind_direction?: string
  cloud_cover_percent?: number
  pressure_hpa?: number
  description: string
  is_day?: boolean
}

// ============================================================================
// API
// ============================================================================

const EXTENSION_ID = 'weather-forecast-v2'

const getApiHeaders = () => {
  const token = localStorage.getItem('neomind_token') || sessionStorage.getItem('neomind_token_session')
  const headers: Record<string, string> = { 'Content-Type': 'application/json' }
  if (token) headers['Authorization'] = `Bearer ${token}`
  return headers
}

const getApiBase = () => (window as any).__TAURI__ ? 'http://localhost:9375/api' : '/api'

async function fetchWeather(extensionId: string, city: string, retries = 3): Promise<{ success: boolean; data?: WeatherData; error?: string }> {
  const doFetch = async (): Promise<{ success: boolean; data?: WeatherData; error?: string }> => {
    try {
      const res = await fetch(`${getApiBase()}/extensions/${extensionId}/command`, {
        method: 'POST',
        headers: getApiHeaders(),
        body: JSON.stringify({ command: 'get_weather', args: { city } })
      })
      if (!res.ok) return { success: false, error: `HTTP ${res.status}` }
      return res.json()
    } catch (e) {
      return { success: false, error: e instanceof Error ? e.message : 'Network error' }
    }
  }

  // Try with retries for extension cold start
  for (let i = 0; i < retries; i++) {
    const result = await doFetch()
    if (result.success) return result

    // If it's an extension initialization error, wait and retry
    const isInitError = result.error?.includes('Invalid response') ||
                        result.error?.includes('NotRunning') ||
                        result.error?.includes('INTERNAL_ERROR')

    if (isInitError && i < retries - 1) {
      await new Promise(r => setTimeout(r, 500 * (i + 1)))
      continue
    }

    return result
  }

  return { success: false, error: 'Failed after retries' }
}

// ============================================================================
// Styles (minimal, design-system aligned)
// ============================================================================

const CSS_ID = 'wfc-styles-v2'

const STYLES = `
.wfc {
  --wfc-fg: hsl(240 10% 10%);
  --wfc-muted: hsl(240 5% 45%);
  --wfc-accent: hsl(221 83% 53%);
  --wfc-card: rgba(255,255,255,0.5);
  --wfc-border: rgba(0,0,0,0.06);
  width: 100%;
  height: 100%;
  font-size: 12px;
}
.dark .wfc {
  --wfc-fg: hsl(0 0% 95%);
  --wfc-muted: hsl(0 0% 60%);
  --wfc-card: rgba(30,30,30,0.5);
  --wfc-border: rgba(255,255,255,0.08);
}
.wfc-card {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: 10px;
  background: var(--wfc-card);
  backdrop-filter: blur(12px);
  border: 1px solid var(--wfc-border);
  border-radius: 8px;
  box-sizing: border-box;
}
.wfc-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 8px;
}
.wfc-location {
  display: flex;
  align-items: center;
  gap: 4px;
  color: var(--wfc-muted);
  font-size: 11px;
}
.wfc-location svg { width: 12px; height: 12px; opacity: 0.7; }

.wfc-main {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 6px;
}
.wfc-icon { width: 32px; height: 32px; color: var(--wfc-accent); flex-shrink: 0; }
.wfc-temp-wrap { display: flex; flex-direction: column; }
.wfc-temp { font-size: 24px; font-weight: 700; color: var(--wfc-fg); line-height: 1; }
.wfc-desc { font-size: 10px; color: var(--wfc-muted); margin-top: 1px; text-transform: capitalize; }

.wfc-stats {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 4px;
  flex: 1;
}
.wfc-stat {
  display: flex;
  align-items: center;
  gap: 3px;
  padding: 3px 4px;
  background: rgba(0,0,0,0.03);
  border-radius: 3px;
}
.dark .wfc-stat { background: rgba(255,255,255,0.03); }
.wfc-stat-icon { width: 12px; height: 12px; flex-shrink: 0; }
.wfc-stat-icon svg { width: 100%; height: 100%; }
.wfc-stat-val { font-size: 10px; font-weight: 600; color: var(--wfc-fg); }
.wfc-stat-label { font-size: 8px; color: var(--wfc-muted); text-transform: uppercase; letter-spacing: 0.2px; }

.wfc-footer {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
  margin-top: 6px;
  font-size: 9px;
  color: var(--wfc-muted);
}
.wfc-dot { width: 5px; height: 5px; border-radius: 50%; background: #22c55e; }
.wfc-dot.stale { background: #f59e0b; }

.wfc-loading, .wfc-error {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  flex: 1;
  color: var(--wfc-muted);
  font-size: 11px;
  gap: 6px;
}
.wfc-spinner {
  width: 24px; height: 24px;
  border: 2px solid var(--wfc-border);
  border-top-color: var(--wfc-accent);
  border-radius: 50%;
  animation: wfc-spin 0.7s linear infinite;
}
`

function injectStyles() {
  if (typeof document === 'undefined' || document.getElementById(CSS_ID)) return
  const style = document.createElement('style')
  style.id = CSS_ID
  style.textContent = STYLES
  document.head.appendChild(style)
}

// ============================================================================
// Icons (inline SVG paths)
// ============================================================================

const ICONS: Record<string, string> = {
  location: '<path d="M21 10c0 7-9 13-9 13s-9-6-9-13a9 9 0 0 1 18 0z"/><circle cx="12" cy="10" r="3"/>',
  refresh: '<path d="M21 12a9 9 0 1 1-9-9c2.5 0 4.9 1 6.7 2.7L21 8M21 3v5h-5"/>',
  droplet: '<path d="M12 22a7 7 0 0 0 7-7c0-2-1-4-3-5.5s-3.5-4-4-6.5c-.5 2.5-2 5-4 6.5C6 11 5 13 5 15a7 7 0 0 0 7 7z"/>',
  wind: '<path d="M17.7 7.7a2.5 2.5 0 1 1 1.8 4.3H2M9.6 4.6A2 2 0 1 1 11 8H2M12.6 19.4A2 2 0 1 0 14 16H2"/>',
  gauge: '<path d="M12 16v-4M12 8h.01M22 12a10 10 0 1 1-20 0 10 10 0 0 1 20 0z"/>',
  compass: '<circle cx="12" cy="12" r="10"/><polygon points="16.2 7.8 14.1 14.1 7.8 16.2 9.9 9.9 16.2 7.8" fill="currentColor" stroke="none"/>',
  cloud: '<path d="M18 10h-1.3A8 8 0 1 0 9 20h9a5 5 0 0 0 0-10z"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M6.3 17.7l-1.4 1.4M19.1 4.9l-1.4 1.4"/>',
  moon: '<path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9z"/>',
  cloudSun: '<path d="M12 2v2M4.9 4.9l1.4 1.4M20 12h2M19.1 4.9l-1.4 1.4M17.5 19H9a6 6 0 1 1 3.3-11A5 5 0 0 1 17.5 19z"/>',
  cloudRain: '<path d="M16 13v8M8 13v8M12 15v8M20 16.6A5 5 0 0 0 18 7h-1.3a8 8 0 1 0-12.7 8"/>',
  thermometer: '<path d="M14 4v10.5a4 4 0 1 1-4 0V4a2 2 0 0 1 4 0z"/>',
}

const Icon = ({ name, className = '' }: { name: string; className?: string }) => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" className={className}
    dangerouslySetInnerHTML={{ __html: ICONS[name] || ICONS.cloud }} />
)

const getWeatherIcon = (desc: string, isDay?: boolean) => {
  const d = desc.toLowerCase()
  const day = isDay !== false
  if (d.includes('clear') || d.includes('sunny')) return day ? 'sun' : 'moon'
  if (d.includes('rain') || d.includes('drizzle')) return 'cloudRain'
  if (d.includes('cloud')) return day ? 'cloudSun' : 'cloud'
  return day ? 'cloudSun' : 'cloud'
}

const ICON_COLORS: Record<string, string> = {
  humidity: '#3b82f6', wind: '#06b6d4', feels: '#f97316', pressure: '#10b981', direction: '#8b5cf6', cloud: '#64748b'
}

// ============================================================================
// Component
// ============================================================================

export interface WeatherCardProps extends ExtensionComponentProps {
  defaultCity?: string
  refreshInterval?: number
  unit?: 'celsius' | 'fahrenheit'
}

export const WeatherCard = forwardRef<HTMLDivElement, WeatherCardProps>(
  function WeatherCard(props, ref) {
    const { dataSource, className = '', config, defaultCity: propCity = 'Beijing', unit = 'celsius' } = props

    useEffect(() => injectStyles(), [])

    const city = config?.defaultCity || propCity
    const extensionId = dataSource?.extensionId || EXTENSION_ID

    const [weather, setWeather] = useState<WeatherData | null>(null)
    const [loading, setLoading] = useState(false)
    const [error, setError] = useState<string | null>(null)
    const [updated, setUpdated] = useState<Date | null>(null)

    const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
    const mountedRef = useRef(true)

    useEffect(() => {
      mountedRef.current = true
      return () => { mountedRef.current = false; timerRef.current && clearTimeout(timerRef.current) }
    }, [])

    // Fetch with debounce
    useEffect(() => {
      if (timerRef.current) clearTimeout(timerRef.current)
      timerRef.current = setTimeout(async () => {
        if (!mountedRef.current) return
        setLoading(true)
        const result = await fetchWeather(extensionId, city)
        if (!mountedRef.current) return
        if (result.success && result.data) {
          setWeather(result.data)
          setUpdated(new Date())
          setError(null)
        } else {
          setError(result.error || 'Failed')
        }
        setLoading(false)
      }, 400)
      return () => { if (timerRef.current) clearTimeout(timerRef.current) }
    }, [extensionId, city])

    // Auto refresh
    useEffect(() => {
      const interval = config?.refreshInterval || 300000
      if (interval <= 0) return
      const id = setInterval(async () => {
        if (!mountedRef.current) return
        const result = await fetchWeather(extensionId, city)
        if (result.success && result.data) {
          setWeather(result.data)
          setUpdated(new Date())
        }
      }, interval)
      return () => clearInterval(id)
    }, [extensionId, city, config?.refreshInterval])

    const handleRefresh = useCallback(async () => {
      setLoading(true)
      const result = await fetchWeather(extensionId, city)
      if (result.success && result.data) {
        setWeather(result.data)
        setUpdated(new Date())
        setError(null)
      } else {
        setError(result.error || 'Failed')
      }
      setLoading(false)
    }, [extensionId, city])

    const formatTemp = (t: number) => unit === 'fahrenheit' ? `${Math.round(t * 9/5 + 32)}°` : `${Math.round(t)}°`
    const formatTime = (d: Date | null) => d ? d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) : ''

    const iconKey = useMemo(() => weather ? getWeatherIcon(weather.description, weather.is_day) : 'cloud', [weather])
    const isStale = useMemo(() => updated && Date.now() - updated.getTime() > 600000, [updated])

    return (
      <div ref={ref} className={`wfc ${className}`}>
        <div className="wfc-card">
          {/* Header */}
          <div className="wfc-header">
            <div className="wfc-location">
              <Icon name="location" />
              <span>{weather?.city || city}</span>
            </div>
          </div>

          {/* Content */}
          {loading && !weather ? (
            <div className="wfc-loading">
              <div className="wfc-spinner" />
              <span>Loading...</span>
            </div>
          ) : error && !weather ? (
            <div className="wfc-error">
              <span>{error}</span>
              <button onClick={handleRefresh} style={{ padding: '4px 10px', fontSize: '11px', borderRadius: '4px', border: '1px solid var(--wfc-border)', background: 'transparent', cursor: 'pointer' }}>Retry</button>
            </div>
          ) : weather ? (
            <>
              {/* Main */}
              <div className="wfc-main">
                <div className="wfc-icon">
                  <Icon name={iconKey} />
                </div>
                <div className="wfc-temp-wrap">
                  <div className="wfc-temp">{formatTemp(weather.temperature_c)}</div>
                  <div className="wfc-desc">{weather.description}</div>
                </div>
              </div>

              {/* Stats */}
              <div className="wfc-stats">
                <div className="wfc-stat">
                  <div className="wfc-stat-icon" style={{ color: ICON_COLORS.humidity }}><Icon name="droplet" /></div>
                  <div>
                    <div className="wfc-stat-val">{weather.humidity_percent}%</div>
                    <div className="wfc-stat-label">Humidity</div>
                  </div>
                </div>
                <div className="wfc-stat">
                  <div className="wfc-stat-icon" style={{ color: ICON_COLORS.wind }}><Icon name="wind" /></div>
                  <div>
                    <div className="wfc-stat-val">{Math.round(weather.wind_speed_kmph)}</div>
                    <div className="wfc-stat-label">km/h</div>
                  </div>
                </div>
                <div className="wfc-stat">
                  <div className="wfc-stat-icon" style={{ color: ICON_COLORS.direction }}><Icon name="compass" /></div>
                  <div>
                    <div className="wfc-stat-val">{weather.wind_direction || '-'}</div>
                    <div className="wfc-stat-label">Wind</div>
                  </div>
                </div>
                <div className="wfc-stat">
                  <div className="wfc-stat-icon" style={{ color: ICON_COLORS.feels }}><Icon name="thermometer" /></div>
                  <div>
                    <div className="wfc-stat-val">{weather.feels_like_c ? formatTemp(weather.feels_like_c) : '-'}</div>
                    <div className="wfc-stat-label">Feels</div>
                  </div>
                </div>
                <div className="wfc-stat">
                  <div className="wfc-stat-icon" style={{ color: ICON_COLORS.cloud }}><Icon name="cloud" /></div>
                  <div>
                    <div className="wfc-stat-val">{weather.cloud_cover_percent ?? '-'}%</div>
                    <div className="wfc-stat-label">Cloud</div>
                  </div>
                </div>
                <div className="wfc-stat">
                  <div className="wfc-stat-icon" style={{ color: ICON_COLORS.pressure }}><Icon name="gauge" /></div>
                  <div>
                    <div className="wfc-stat-val">{weather.pressure_hpa ? Math.round(weather.pressure_hpa) : '-'}</div>
                    <div className="wfc-stat-label">hPa</div>
                  </div>
                </div>
              </div>

              {/* Footer */}
              <div className="wfc-footer">
                <div className={`wfc-dot ${isStale ? 'stale' : ''}`} />
                <span>{updated ? `Updated ${formatTime(updated)}` : 'Not updated'}</span>
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
