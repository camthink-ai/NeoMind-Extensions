/**
 * GymTrafficChart — presence trend over the last N hours.
 *
 * Queries `gym.present_count` metric history (telemetry written by the
 * platform's 60 s extension-metrics collector) and renders a dependency-free
 * SVG area/line chart with min/max/avg summary.
 */

import { forwardRef, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  DEFAULT_EXTENSION_ID,
  ExtensionComponentProps,
  MetricPoint,
  fetchLiveState,
  fetchMetricHistory,
  fetchExtensionUiConfig,
  injectStyles
} from './common'
import STYLES from './styles.css?raw'
import { useLang } from './i18n'

const STYLE_ID = 'gym-traffic-styles-v1'

const TrendIcon = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"
    strokeLinecap="round" strokeLinejoin="round">
    <polyline points="22 12 18 12 15 21 9 3 6 12 2 12" />
  </svg>
)

function formatTick(tsMs: number): string {
  const d = new Date(tsMs)
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
}

export const GymTrafficChart = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function GymTrafficChart(props, ref) {
    const { dataSource, className = '', hours = 6 } = props
    const extensionId = dataSource?.extensionId || DEFAULT_EXTENSION_ID
    const windowHours = Math.min(24, Math.max(1, Number(hours) || 6))
    // global ui.language from the EXTENSION config (card lang overrides)
    const [gLang, setGLang] = useState<string | undefined>(undefined)
    useEffect(() => {
      let alive = true
      fetchExtensionUiConfig(extensionId).then((c: { ui?: { language?: string } }) => {
        if (alive) setGLang(c.ui?.language)
      })
      return () => { alive = false }
    }, [extensionId])
    const { t } = useLang(props.config as Record<string, unknown>, gLang)

    useEffect(() => injectStyles(STYLE_ID, STYLES), [])

    const [points, setPoints] = useState<MetricPoint[] | null>(null)
    const [current, setCurrent] = useState<number | null>(null)
    const mountedRef = useRef(true)
    useEffect(() => {
      mountedRef.current = true
      return () => { mountedRef.current = false }
    }, [])

    const refresh = useCallback(async () => {
      const [hist, live] = await Promise.all([
        fetchMetricHistory(extensionId, 'gym.present_count', windowHours),
        fetchLiveState(extensionId),
      ])
      if (!mountedRef.current) return
      setPoints(hist)
      if (live.success && live.data) setCurrent(live.data.present_count)
    }, [extensionId, windowHours])

    useEffect(() => {
      const t = setTimeout(() => { if (mountedRef.current) refresh() }, 300)
      return () => clearTimeout(t)
    }, [refresh])

    useEffect(() => {
      // 5 s cadence: the "now" counter comes from get_live_state, so a
      // short poll keeps it near-live; the metric history is still
      // platform-sampled (≈1 pt/min) and just rides along.
      const id = setInterval(() => { if (mountedRef.current) refresh() }, 5000)
      return () => clearInterval(id)
    }, [refresh])

    const stats = useMemo(() => {
      if (!points || points.length === 0) return null
      const values = points.map((p) => p.value)
      const peak = Math.max(...values)
      const avg = values.reduce((s, v) => s + v, 0) / values.length
      return { peak, avg }
    }, [points])

    // hover readout: pointer position → nearest metric point
    const plotRef = useRef<HTMLDivElement>(null)
    const [hover, setHover] = useState<{ x: number; idx: number } | null>(null)
    const onPlotMove = useCallback((e: React.PointerEvent) => {
      const el = plotRef.current
      if (!el || !points || points.length < 2) return
      const rect = el.getBoundingClientRect()
      const fx = Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width))
      const ts = points.map((p) => p.timestamp)
      const t0 = Math.min(...ts), t1 = Math.max(...ts)
      const want = t0 + fx * (t1 - t0)
      // nearest point (binary-search-ish linear scan is fine ≤ 1440 pts)
      let best = 0, bd = Infinity
      for (let i = 0; i < ts.length; i++) {
        const d = Math.abs(ts[i] - want)
        if (d < bd) { bd = d; best = i }
      }
      setHover({ x: fx, idx: best })
    }, [points])
    const onPlotLeave = useCallback(() => setHover(null), [])

    // SVG geometry — padded plot area inside a 100×40 viewBox, scaled by CSS.
    const W = 100
    const H = 40
    const PAD = { l: 2, r: 2, t: 4, b: 6 }

    const path = useMemo(() => {
      if (!points || points.length < 2) return null
      const ts = points.map((p) => p.timestamp)
      const t0 = Math.min(...ts)
      const t1 = Math.max(...ts)
      const span = Math.max(1, t1 - t0)
      const vmax = Math.max(1, ...points.map((p) => p.value))
      const x = (t: number) => PAD.l + ((t - t0) / span) * (W - PAD.l - PAD.r)
      const y = (v: number) =>
        H - PAD.b - (v / vmax) * (H - PAD.t - PAD.b)
      const coords = points.map((p) => [x(p.timestamp), y(p.value)] as const)
      const line = coords
        .map(([cx, cy], i) => `${i === 0 ? 'M' : 'L'}${cx.toFixed(2)},${cy.toFixed(2)}`)
        .join(' ')
      const area = `${line} L${coords[coords.length - 1][0].toFixed(2)},${(H - PAD.b).toFixed(2)} L${coords[0][0].toFixed(2)},${(H - PAD.b).toFixed(2)} Z`
      const ticks = [t0, t0 + span / 2, t1].map((t) => ({
        x: x(t),
        label: formatTick(t),
      }))
      return { line, area, coords, ticks, vmax }
    }, [points])

    return (
      <div ref={ref} className={`gym-traffic ${className}`}>
        <div className="gym-traffic-card">
          <div className="gym-traffic-header">
            <div className="gym-traffic-title">
              <TrendIcon />
              <span>Gym · {t('trafficTitle')}</span>
            </div>
            <span className="gym-traffic-window">{t('lastHours', { n: windowHours })}</span>
          </div>

          <div className="gym-traffic-body">
            <div className="gym-traffic-now">
              <span className="gym-traffic-now-value">
                {current ?? '—'}
              </span>
              <span className="gym-traffic-now-label">{t('peopleInGym')}</span>
              {stats && (
                <span className="gym-traffic-stats">
                  peak {stats.peak} · avg {stats.avg.toFixed(1)}
                </span>
              )}
            </div>

            {points === null ? (
              <div className="gym-traffic-plot">
                <div className="gym-live-spinner" />
              </div>
            ) : !path ? (
              <div className="gym-traffic-plot">
                <span className="gym-traffic-empty">
                  Collecting data… (one point per minute)
                </span>
              </div>
            ) : (
              <div
                className="gym-traffic-plot"
                ref={plotRef}
                onPointerMove={path ? onPlotMove : undefined}
                onPointerLeave={path ? onPlotLeave : undefined}
              >
                <svg
                  className="gym-traffic-svg"
                  viewBox={`0 0 ${W} ${H}`}
                  preserveAspectRatio="none"
                  role="img"
                  aria-label="presence trend"
                >
                  <path className="gym-traffic-area" d={path.area} />
                  <path className="gym-traffic-line" d={path.line} />
                  {path.coords.length <= 400 &&
                    path.coords.map(([cx, cy], i) => (
                      <circle key={i} className="gym-traffic-pt" cx={cx} cy={cy} r="0.5" />
                    ))}
                  {hover && path.coords[hover.idx] && (
                    <>
                      <line
                        className="gym-traffic-xhair"
                        x1={path.coords[hover.idx][0]} y1={0}
                        x2={path.coords[hover.idx][0]} y2={H - 6}
                      />
                      <circle
                        className="gym-traffic-hover-pt"
                        cx={path.coords[hover.idx][0]}
                        cy={path.coords[hover.idx][1]} r={1.2}
                      />
                    </>
                  )}
                </svg>
                {hover && points[hover.idx] && (
                  <div
                    className="gym-traffic-tip"
                    style={{
                      left: `${Math.min(86, Math.max(2, path.coords[hover.idx][0]))}%`,
                    }}
                  >
                    <b>{points[hover.idx].value}</b>
                    <span>{formatTick(points[hover.idx].timestamp)}</span>
                  </div>
                )}
                <div className="gym-traffic-ticks">
                  {path.ticks.map((t, i) => (
                    <span key={i} style={{ left: `${t.x}%` }}>{t.label}</span>
                  ))}
                </div>
              </div>
            )}
          </div>
        </div>
      </div>
    )
  }
)

GymTrafficChart.displayName = 'GymTrafficChart'
