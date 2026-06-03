/**
 * Weather Forecast V2 - Compact & Atmospheric
 * Single visual plane, subtle gradients, no loading flicker
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
  defaultCity?: string
  refreshInterval?: number
  unit?: string
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

  for (let i = 0; i < retries; i++) {
    const result = await doFetch()
    if (result.success) return result
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
// Scoped CSS
// ============================================================================

const CSS_ID = 'weather-styles-v5'

const STYLES = `
.weather {
  --w-fg: var(--foreground);
  --w-muted: var(--muted-foreground);
  --w-accent: var(--primary);
  --w-on-accent: var(--primary-foreground, #ffffff);
  --w-card: var(--card);
  --w-border: var(--border);
  --w-success: var(--color-success);
  --w-warning: var(--color-warning);
  --w-error: var(--color-error);
  --w-info: var(--color-info);
  --w-cyan: var(--accent-cyan);
  --w-cyan-bg: var(--accent-cyan-bg);
  --w-purple: var(--accent-purple);
  --w-purple-bg: var(--accent-purple-bg);
  --w-orange: var(--accent-orange);
  --w-orange-bg: var(--accent-orange-bg);
  --w-emerald: var(--accent-emerald);
  --w-emerald-bg: var(--accent-emerald-bg);
  --w-indigo: var(--accent-indigo);
  --w-indigo-bg: var(--accent-indigo-bg);

  width: 100%;
  height: 100%;
  font-size: 13px;
  line-height: 1.4;
  box-sizing: border-box;
}

.dark .weather {
  --w-on-accent: var(--primary-foreground, #17172a);
}

/* Card — glass with subtle gradient */
.weather-card {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--w-card);
  backdrop-filter: blur(12px);
  -webkit-backdrop-filter: blur(12px);
  border: 1px solid var(--w-border);
  border-radius: var(--radius-xl, 12px);
  box-shadow: var(--shadow-sm);
  box-sizing: border-box;
  overflow: hidden;
  position: relative;
}

/* Subtle gradient overlay — top-left atmospheric tint */
.weather-card::before {
  content: '';
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background:
    radial-gradient(ellipse 80% 60% at 0% 0%, var(--w-cyan-bg) 0%, transparent 70%),
    radial-gradient(ellipse 60% 50% at 100% 100%, var(--w-purple-bg) 0%, transparent 70%);
  pointer-events: none;
  z-index: 0;
}

.weather-card > * {
  position: relative;
  z-index: 1;
}

/* Top: icon + temp + meta */
.weather-top {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 16px 16px 0;
}

/* Icon with pill background */
.weather-icon {
  width: 44px;
  height: 44px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-lg, 10px);
  background: var(--w-cyan-bg);
  color: var(--w-cyan);
  flex-shrink: 0;
  box-shadow: 0 2px 8px color-mix(in oklch, var(--w-cyan) 15%, transparent);
}

.dark .weather-icon {
  background: var(--w-indigo-bg);
  color: var(--w-indigo);
  box-shadow: 0 2px 8px color-mix(in oklch, var(--w-indigo) 15%, transparent);
}

.weather-icon.night {
  background: var(--w-purple-bg);
  color: var(--w-purple);
  box-shadow: 0 2px 8px color-mix(in oklch, var(--w-purple) 15%, transparent);
}

.weather-icon svg {
  width: 22px;
  height: 22px;
}

/* Temperature block */
.weather-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.weather-temp {
  font-size: 28px;
  font-weight: 700;
  line-height: 1.1;
  color: var(--w-fg);
  letter-spacing: -0.02em;
}

.weather-meta {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  color: var(--w-muted);
}

.weather-meta .weather-desc-text {
  text-transform: capitalize;
}

.weather-meta .weather-sep {
  width: 3px;
  height: 3px;
  border-radius: 50%;
  background: var(--w-muted);
  opacity: 0.4;
}

.weather-meta .weather-city {
  display: flex;
  align-items: center;
  gap: 3px;
}

.weather-meta .weather-city svg {
  width: 10px;
  height: 10px;
  opacity: 0.5;
}

/* Inline status spinner/dot */
.weather-indicator {
  flex-shrink: 0;
  align-self: flex-start;
  margin-top: 4px;
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 10px;
  color: var(--w-muted);
}

.weather-indicator-dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: var(--w-success);
  flex-shrink: 0;
}

.weather-indicator-dot.stale {
  background: var(--w-warning);
}

.weather-indicator-spinner {
  width: 12px;
  height: 12px;
  border: 2px solid var(--w-border);
  border-top-color: var(--w-accent);
  border-radius: 50%;
  animation: weather-spin 0.6s linear infinite;
  flex-shrink: 0;
}

@keyframes weather-spin {
  to { transform: rotate(360deg); }
}

/* Stats — 3 column, compact with colored pills */
.weather-stats {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 4px;
  padding: 12px 14px 0;
}

.weather-stat {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  padding: 8px 4px;
  border-radius: var(--radius-md, 8px);
  background: color-mix(in oklch, var(--muted) 40%, transparent);
  transition: background var(--duration-fast, 150ms) var(--ease-out, cubic-bezier(0.16, 1, 0.3, 1));
}

.weather-stat:hover {
  background: color-mix(in oklch, var(--muted) 60%, transparent);
}

.weather-stat-val {
  font-size: 13px;
  font-weight: 600;
  color: var(--w-fg);
  line-height: 1.3;
}

.weather-stat-label {
  font-size: 10px;
  color: var(--w-muted);
  letter-spacing: 0.2px;
}


/* Error */
.weather-error-inline {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 24px 16px;
}

.weather-error-text {
  font-size: 12px;
  color: var(--w-error);
}

.weather-error-btn {
  padding: 4px 12px;
  border-radius: var(--radius-sm, 6px);
  border: 1px solid var(--w-border);
  background: transparent;
  color: var(--w-fg);
  font-size: 11px;
  cursor: pointer;
  transition: background var(--duration-fast, 150ms);
}

.weather-error-btn:hover {
  background: var(--accent);
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
// Icons
// ============================================================================

const ICONS: Record<string, string> = {
  location: '<path d="M21 10c0 7-9 13-9 13s-9-6-9-13a9 9 0 0 1 18 0z"/><circle cx="12" cy="10" r="3"/>',
  cloud: '<path d="M18 10h-1.3A8 8 0 1 0 9 20h9a5 5 0 0 0 0-10z"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M6.3 17.7l-1.4 1.4M19.1 4.9l-1.4 1.4"/>',
  moon: '<path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9z"/>',
  cloudSun: '<path d="M12 2v2M4.9 4.9l1.4 1.4M20 12h2M19.1 4.9l-1.4 1.4M17.5 19H9a6 6 0 1 1 3.3-11A5 5 0 0 1 17.5 19z"/>',
  cloudRain: '<path d="M16 13v8M8 13v8M12 15v8M20 16.6A5 5 0 0 0 18 7h-1.3a8 8 0 1 0-12.7 8"/>',
}

const Icon = ({ name, className = '' }: { name: string; className?: string }) => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={className}
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

// ============================================================================
// i18n
// ============================================================================

type Locale = 'en' | 'zh'

function detectLocale(): Locale {
  const stored = localStorage.getItem('i18nextLng') || ''
  if (stored.startsWith('zh')) return 'zh'
  if (stored.startsWith('en')) return 'en'
  return navigator.language.startsWith('zh') ? 'zh' : 'en'
}

const T: Record<string, Record<Locale, string>> = {
  humidity:  { en: 'Humidity', zh: '湿度' },
  wind:      { en: 'Wind', zh: '风速' },
  direction: { en: 'Wind Dir', zh: '风向' },
  feels:     { en: 'Feels', zh: '体感' },
  cloud:     { en: 'Cloud', zh: '云量' },
  pressure:  { en: 'Pressure', zh: '气压' },
  retry:     { en: 'Retry', zh: '重试' },
  updated:   { en: 'Updated', zh: '更新' },
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
    const { dataSource, className = '', defaultCity: propCity = 'Beijing', refreshInterval: propRefreshInterval = 300000, unit = 'celsius' } = props

    useEffect(() => injectStyles(), [])

    const locale = useMemo(() => detectLocale(), [])
    const t = useCallback((key: string) => T[key]?.[locale] ?? key, [locale])

    const city = propCity
    const extensionId = dataSource?.extensionId || EXTENSION_ID

    const [weather, setWeather] = useState<WeatherData | null>(null)
    const [fetching, setFetching] = useState(false)
    const [error, setError] = useState<string | null>(null)
    const [updated, setUpdated] = useState<Date | null>(null)

    const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
    const mountedRef = useRef(true)

    useEffect(() => {
      mountedRef.current = true
      return () => { mountedRef.current = false; timerRef.current && clearTimeout(timerRef.current) }
    }, [])

    useEffect(() => {
      if (timerRef.current) clearTimeout(timerRef.current)
      timerRef.current = setTimeout(async () => {
        if (!mountedRef.current) return
        setFetching(true)
        const result = await fetchWeather(extensionId, city)
        if (!mountedRef.current) return
        if (result.success && result.data) {
          setWeather(result.data)
          setUpdated(new Date())
          setError(null)
        } else {
          setError(result.error || 'Failed')
        }
        setFetching(false)
      }, 400)
      return () => { if (timerRef.current) clearTimeout(timerRef.current) }
    }, [extensionId, city])

    useEffect(() => {
      const interval = propRefreshInterval
      if (interval <= 0) return
      const id = setInterval(async () => {
        if (!mountedRef.current) return
        const result = await fetchWeather(extensionId, city)
        if (mountedRef.current && result.success && result.data) {
          setWeather(result.data)
          setUpdated(new Date())
        }
      }, interval)
      return () => clearInterval(id)
    }, [extensionId, city, propRefreshInterval])

    const handleRetry = useCallback(async () => {
      setFetching(true)
      const result = await fetchWeather(extensionId, city)
      if (result.success && result.data) {
        setWeather(result.data)
        setUpdated(new Date())
        setError(null)
      } else {
        setError(result.error || 'Failed')
      }
      setFetching(false)
    }, [extensionId, city])

    const formatTemp = (v: number) => unit === 'fahrenheit' ? `${Math.round(v * 9/5 + 32)}°F` : `${Math.round(v)}°`
    const formatTime = (d: Date | null) => d ? d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) : ''
    const iconKey = useMemo(() => weather ? getWeatherIcon(weather.description, weather.is_day) : 'cloud', [weather])
    const isNight = weather?.is_day === false
    const isStale = updated ? Date.now() - updated.getTime() > 600000 : false

    const stats = weather ? [
      { cls: 'humidity',  val: `${weather.humidity_percent}%`, label: t('humidity') },
      { cls: 'wind',      val: `${Math.round(weather.wind_speed_kmph)} km/h`, label: t('wind') },
      { cls: 'feels',     val: weather.feels_like_c ? formatTemp(weather.feels_like_c) : '-', label: t('feels') },
      { cls: 'direction', val: weather.wind_direction || '-', label: t('direction') },
      { cls: 'cloud',     val: weather.cloud_cover_percent != null ? `${weather.cloud_cover_percent}%` : '-', label: t('cloud') },
      { cls: 'pressure',  val: weather.pressure_hpa ? `${Math.round(weather.pressure_hpa)} hPa` : '-', label: t('pressure') },
    ] : []

    return (
      <div ref={ref} className={`weather ${className}`}>
        <div className="weather-card">
          {error && !weather ? (
            <div className="weather-error-inline">
              <span className="weather-error-text">{error}</span>
              <button className="weather-error-btn" onClick={handleRetry}>{t('retry')}</button>
            </div>
          ) : (
            <>
              <div className="weather-top">
                <div className={`weather-icon ${isNight ? 'night' : ''}`}>
                  <Icon name={iconKey} />
                </div>
                <div className="weather-main">
                  <div className="weather-temp">{weather ? formatTemp(weather.temperature_c) : '--°'}</div>
                  <div className="weather-meta">
                    {weather && <span className="weather-desc-text">{weather.description}</span>}
                    {weather && <span className="weather-sep" />}
                    <span className="weather-city">
                      <Icon name="location" />
                      {weather?.city || city}
                    </span>
                  </div>
                </div>
                <div className="weather-indicator">
                  {fetching && !weather ? (
                    <div className="weather-indicator-spinner" />
                  ) : (
                    <div className={`weather-indicator-dot ${isStale ? 'stale' : ''}`} />
                  )}
                  {updated && <span>{formatTime(updated)}</span>}
                </div>
              </div>

              {weather && (
                <div className="weather-stats">
                  {stats.map((s) => (
                    <div className="weather-stat" key={s.cls}>
                      <div className="weather-stat-val">{s.val}</div>
                      <div className="weather-stat-label">{s.label}</div>
                    </div>
                  ))}
                </div>
              )}
            </>
          )}
        </div>
      </div>
    )
  }
)

WeatherCard.displayName = 'WeatherCard'
export default { WeatherCard }
