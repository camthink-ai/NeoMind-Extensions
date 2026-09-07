/**
 * GymActivityMap — trails + heatmap on their own card (moved off the
 * Monitor). No video dependency: a dark canvas with faint zone outlines,
 * today's foot-position heatmap (get_heatmap, 64×36) and the live fading
 * trails from get_live_state. Toggles via widget config.
 */
import { forwardRef, useCallback, useEffect, useRef, useState } from 'react'
import {
  DEFAULT_EXTENSION_ID,
  ExtensionComponentProps,
  injectStyles,
  runExtensionCommand,
} from './common'
import STYLES from './styles.css?raw'

const STYLE_ID = 'gym-activity-styles-v1'

const MapIcon = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"
    strokeLinecap="round" strokeLinejoin="round">
    <path d="M9 20l-5.5-2.5v-13L9 7l6-2.5L20.5 7v13L15 17.5 9 20z" />
    <line x1="9" y1="7" x2="9" y2="20" />
    <line x1="15" y1="4.5" x2="15" y2="17.5" />
  </svg>
)

interface Zone { id: string; name: string; equipment_type: string; polygon: number[][]; enabled: boolean | number }
interface Heat { cols: number; rows: number; grid: number[] }
interface LiveTrack { track_id: number; foot?: { x: number; y: number }; trail?: Array<{ x: number; y: number }> }

/** blue → cyan → yellow → red ramp, alpha by intensity */
function heatColor(v: number): string {
  const r = Math.round(Math.min(255, v < 0.5 ? v * 2 * 60 : 60 + (v - 0.5) * 2 * 195))
  const g = Math.round(Math.min(255, v < 0.5 ? 90 + v * 2 * 165 : 255 - (v - 0.5) * 2 * 115))
  const b = Math.round(v < 0.25 ? 255 - v * 4 * 55 : v < 0.5 ? 145 - (v - 0.25) * 4 * 145 : 0)
  return `rgba(${r},${g},${b},${(0.18 + v * 0.55).toFixed(2)})`
}

export const GymActivityMap = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function GymActivityMap(props, ref) {
    const { dataSource, className = '', config } = props
    const extensionId = dataSource?.extensionId || DEFAULT_EXTENSION_ID
    const showHeat = config?.heatmap !== false
    const showTrails = config?.trails !== false

    useEffect(() => injectStyles(STYLE_ID, STYLES), [])

    const canvasRef = useRef<HTMLCanvasElement>(null)
    const dataRef = useRef<{ zones: Zone[]; heat: Heat | null; tracks: LiveTrack[] }>({
      zones: [], heat: null, tracks: [],
    })
    const [present, setPresent] = useState<number | null>(null)
    const [error, setError] = useState<string | null>(null)
    const mountedRef = useRef(true)
    useEffect(() => {
      mountedRef.current = true
      return () => { mountedRef.current = false }
    }, [])

    const refresh = useCallback(async () => {
      const [zr, hr, lr] = await Promise.all([
        runExtensionCommand<{ zones: Zone[] }>(extensionId, 'get_roi_zones', {}),
        runExtensionCommand<Heat>(extensionId, 'get_heatmap', {}),
        runExtensionCommand<{ present_count: number; tracks: LiveTrack[] }>(extensionId, 'get_live_state', {}),
      ])
      if (!mountedRef.current) return
      const ok = zr.success && hr.success && lr.success
      setError(ok ? null : '数据加载失败')
      dataRef.current = {
        zones: (zr.data?.zones ?? []).filter((z) => z.equipment_type !== 'exclusion'),
        heat: hr.data ?? null,
        tracks: lr.data?.tracks ?? [],
      }
      if (lr.success) setPresent(lr.data?.present_count ?? 0)
    }, [extensionId])

    useEffect(() => {
      const t = setTimeout(() => { if (mountedRef.current) refresh() }, 300)
      return () => clearTimeout(t)
    }, [refresh])
    useEffect(() => {
      const id = setInterval(() => { if (mountedRef.current) refresh() }, 2000)
      return () => clearInterval(id)
    }, [refresh])

    // draw on data change (trails/heat change every poll)
    useEffect(() => {
      const canvas = canvasRef.current
      if (!canvas) return
      const cw = canvas.clientWidth || 480
      const ch = Math.round((cw * 9) / 16)
      const dpr = Math.min(2, window.devicePixelRatio || 1)
      if (canvas.width !== Math.round(cw * dpr)) {
        canvas.width = Math.round(cw * dpr)
        canvas.height = Math.round(ch * dpr)
      }
      const ctx = canvas.getContext('2d')
      if (!ctx) return
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
      const W = cw, H = ch
      // 16:9 camera frame normalized coords → letterbox-free direct map
      const X = (nx: number) => nx * W
      const Y = (ny: number) => ny * H

      ctx.fillStyle = '#0a0a0c'
      ctx.fillRect(0, 0, W, H)

      // faint zone outlines for spatial reference
      const { zones, heat, tracks } = dataRef.current
      ctx.lineWidth = 1
      ctx.strokeStyle = 'rgba(148, 163, 184, 0.28)'
      ctx.setLineDash([5, 4])
      for (const z of zones) {
        if (!z.polygon || z.polygon.length < 3) continue
        ctx.beginPath()
        z.polygon.forEach((p, i) =>
          i === 0 ? ctx.moveTo(X(p[0]), Y(p[1])) : ctx.lineTo(X(p[0]), Y(p[1]))
        )
        ctx.closePath()
        ctx.stroke()
      }
      ctx.setLineDash([])

      // heatmap: sqrt-scaled intensity (raw counts span orders of magnitude)
      if (showHeat && heat && heat.grid?.length) {
        const max = Math.max(1, ...heat.grid)
        const cw2 = W / heat.cols
        const ch2 = H / heat.rows
        for (let r = 0; r < heat.rows; r++) {
          for (let c = 0; c < heat.cols; c++) {
            const v = heat.grid[r * heat.cols + c]
            if (!v) continue
            ctx.fillStyle = heatColor(Math.sqrt(v / max))
            ctx.fillRect(c * cw2, r * ch2, cw2 + 0.5, ch2 + 0.5)
          }
        }
      }

      // trails: fading polylines + current foot dots
      if (showTrails) {
        for (const t of tracks) {
          const trail = t.trail ?? []
          for (let i = 1; i < trail.length; i++) {
            const a = ((i - 1) / trail.length) * 0.7 + 0.15
            ctx.strokeStyle = `rgba(96, 165, 250, ${a.toFixed(2)})`
            ctx.lineWidth = 1.6
            ctx.beginPath()
            ctx.moveTo(X(trail[i - 1].x), Y(trail[i - 1].y))
            ctx.lineTo(X(trail[i].x), Y(trail[i].y))
            ctx.stroke()
          }
          if (t.foot) {
            ctx.beginPath()
            ctx.arc(X(t.foot.x), Y(t.foot.y), 3, 0, Math.PI * 2)
            ctx.fillStyle = '#60a5fa'
            ctx.fill()
          }
        }
      }
    })

    return (
      <div ref={ref} className={`gym-activity ${className}`}>
        <div className="gym-traffic-card">
          <div className="gym-traffic-header">
            <div className="gym-traffic-title">
              <MapIcon />
              <span>Gym · 轨迹热力</span>
            </div>
            <span className="gym-ov-badge">
              {present != null ? `${present} 人在场 · 今日热力` : '…'}
            </span>
          </div>
          <div className="gym-activity-body">
            {error && <div className="gym-rank-empty">{error}</div>}
            <canvas ref={canvasRef} className="gym-activity-canvas" />
            <div className="gym-activity-legend">
              <span>低</span>
              <span className="gym-activity-ramp" />
              <span>高</span>
              {showTrails && <span className="gym-activity-trailhint">蓝线 = 最近轨迹</span>}
            </div>
          </div>
        </div>
      </div>
    )
  },
)

GymActivityMap.displayName = 'GymActivityMap'
