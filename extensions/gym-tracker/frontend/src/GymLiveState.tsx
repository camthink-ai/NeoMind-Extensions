/**
 * GymLiveState — minimal P1 live-state card for the gym-tracker extension.
 *
 * Shows the current in-gym presence count and the list of active tracks
 * (track id + a tiny normalized bbox/foot indicator). Polls the extension's
 * `get_live_state` command directly via the host REST API — the same proven
 * pattern used by weather-forecast / yolo-device-inference. Works whether
 * or not a data source is bound (defaults to the gym-tracker extension id).
 */

import {
  forwardRef,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import STYLES from './styles.css?raw'

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
  command?: string
  [key: string]: any
}

interface Bbox {
  x: number
  y: number
  w: number
  h: number
}

interface Point {
  x: number
  y: number
}

interface Track {
  track_id: number
  bbox?: Bbox | null
  foot?: Point | null
}

interface LiveState {
  present_count: number
  tracks: Track[]
}

interface CommandResult {
  success: boolean
  data?: LiveState
  error?: string
}

// ============================================================================
// API — matches the host REST shape used by every other extension card
// ============================================================================

const EXTENSION_ID = 'gym-tracker'

const getApiBase = (): string =>
  (window as any).__TAURI__ ? 'http://localhost:9375/api' : '/api'

const getApiHeaders = (): Record<string, string> => {
  const token =
    localStorage.getItem('neomind_token') ||
    sessionStorage.getItem('neomind_token_session')
  const headers: Record<string, string> = { 'Content-Type': 'application/json' }
  if (token) headers['Authorization'] = `Bearer ${token}`
  return headers
}

async function fetchLiveState(
  extensionId: string
): Promise<CommandResult> {
  try {
    const res = await fetch(
      `${getApiBase()}/extensions/${extensionId}/command`,
      {
        method: 'POST',
        headers: getApiHeaders(),
        body: JSON.stringify({
          command: 'get_live_state',
          args: {},
        }),
      }
    )
    if (!res.ok) return { success: false, error: `HTTP ${res.status}` }
    return res.json()
  } catch (e) {
    return {
      success: false,
      error: e instanceof Error ? e.message : 'Network error',
    }
  }
}

// ============================================================================
// Scoped CSS — injected once, deduplicated by id
// ============================================================================

const STYLE_ID = 'gym-live-styles-v1'

function injectStyles() {
  if (typeof document === 'undefined' || document.getElementById(STYLE_ID)) return
  const style = document.createElement('style')
  style.id = STYLE_ID
  style.textContent = STYLES
  document.head.appendChild(style)
}

// ============================================================================
// Icons (inline SVG — no icon library)
// ============================================================================

const UsersIcon = ({ className = '' }: { className?: string }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2" />
    <circle cx="9" cy="7" r="4" />
    <path d="M23 21v-2a4 4 0 0 0-3-3.87" />
    <path d="M16 3.13a4 4 0 0 1 0 7.75" />
  </svg>
)

// ============================================================================
// Helpers
// ============================================================================

const clamp01 = (v: number): number => {
  if (Number.isNaN(v)) return 0
  return Math.min(1, Math.max(0, v))
}

const pct = (v: number | undefined, fallback = 0): string =>
  `${(clamp01(Number(v ?? fallback)) * 100).toFixed(1)}%`

// ============================================================================
// Mini bbox frame — renders the person's normalized position in the frame
// ============================================================================

const BboxIndicator = ({ track }: { track: Track }) => {
  const hasBbox = !!track.bbox
  const foot = track.foot
  return (
    <div className="gym-live-frame" aria-hidden="true">
      {hasBbox && (
        <div
          className="gym-live-frame-box"
          style={{
            left: pct(track.bbox!.x),
            top: pct(track.bbox!.y),
            width: pct(track.bbox!.w, 0.0001),
            height: pct(track.bbox!.h, 0.0001),
          }}
        />
      )}
      {foot && (
        <div
          className="gym-live-foot-dot"
          style={{ left: pct(foot.x), top: pct(foot.y) }}
        />
      )}
    </div>
  )
}

// ============================================================================
// Component
// ============================================================================

export const GymLiveState = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function GymLiveState(props, ref) {
    const { dataSource, className = '' } = props
    const extensionId = dataSource?.extensionId || EXTENSION_ID

    useEffect(() => injectStyles(), [])

    const [state, setState] = useState<LiveState | null>(null)
    const [loading, setLoading] = useState(false)
    const [error, setError] = useState<string | null>(null)
    const [updated, setUpdated] = useState<number | null>(null)

    const mountedRef = useRef(true)
    useEffect(() => {
      mountedRef.current = true
      return () => {
        mountedRef.current = false
      }
    }, [])

    const refresh = useCallback(async () => {
      // The full-screen spinner only renders while there is no data yet
      // (loading && !state), so setting this on every poll won't flicker the
      // card once the first snapshot has arrived.
      setLoading(true)
      const result = await fetchLiveState(extensionId)
      if (!mountedRef.current) return
      if (result.success && result.data) {
        setState(result.data)
        setError(null)
        setUpdated(Date.now())
      } else {
        setError(result.error || 'Failed to load live state')
      }
      setLoading(false)
    }, [extensionId])

    // Initial fetch (small delay, like weather-forecast, to avoid a flash
    // of spinner when the dashboard is still laying out cards).
    useEffect(() => {
      const t = setTimeout(() => {
        if (mountedRef.current) refresh()
      }, 300)
      return () => clearTimeout(t)
    }, [refresh])

    // Live polling — keeps presence fresh independent of the host refresh tick.
    useEffect(() => {
      const id = setInterval(() => {
        if (mountedRef.current) refresh()
      }, 2000)
      return () => clearInterval(id)
    }, [refresh])

    const tracks = useMemo(() => state?.tracks ?? [], [state])
    const presentCount = state?.present_count ?? 0
    const isStale = updated !== null && Date.now() - updated > 10000

    return (
      <div ref={ref} className={`gym-live ${className}`}>
        <div className="gym-live-card">
          {/* Header */}
          <div className="gym-live-header">
            <div className="gym-live-title">
              <UsersIcon />
              <span>Gym · Live</span>
            </div>
            <span className={`gym-live-pulse ${isStale ? 'stale' : ''}`}>
              <span className="gym-live-pulse-dot" />
              {isStale ? 'idle' : 'live'}
            </span>
          </div>

          {/* Body */}
          {error && !state ? (
            <div className="gym-live-state gym-live-error">
              <span className="gym-live-state-text">{error}</span>
              <button className="gym-live-retry" onClick={refresh}>
                Retry
              </button>
            </div>
          ) : loading && !state ? (
            <div className="gym-live-state">
              <div className="gym-live-spinner" />
              <span className="gym-live-state-text">Loading…</span>
            </div>
          ) : (
            <>
              {/* Hero count */}
              <div className="gym-live-hero">
                <span className="gym-live-count">{presentCount}</span>
                <span className="gym-live-count-label">
                  {presentCount === 1 ? 'person in gym' : 'people in gym'}
                </span>
              </div>

              {/* Track list */}
              {tracks.length === 0 ? (
                <div className="gym-live-state">
                  <UsersIcon />
                  <span className="gym-live-state-text">
                    No active tracks
                  </span>
                </div>
              ) : (
                <div className="gym-live-list">
                  {tracks.map((t) => (
                    <div className="gym-live-track" key={t.track_id}>
                      <span className="gym-live-track-id">#{t.track_id}</span>
                      <BboxIndicator track={t} />
                      <div className="gym-live-track-meta">
                        {t.bbox
                          ? `${Math.round(clamp01(t.bbox.w) * 100)}×${Math.round(
                              clamp01(t.bbox.h) * 100
                            )}%`
                          : '—'}
                      </div>
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

GymLiveState.displayName = 'GymLiveState'
