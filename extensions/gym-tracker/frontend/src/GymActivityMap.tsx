/**
 * GymTrailsCard + GymHeatCard — trails and heatmap as SEPARATE widgets,
 * both overlaid on the live camera image (get_frame's JPEG preview,
 * polled at a low rate — no video pipeline needed).
 */
import { forwardRef, useCallback, useEffect, useRef, useState } from 'react'
import {
  DEFAULT_EXTENSION_ID,
  ExtensionComponentProps,
  FrameBundle,
  fetchFrame,
  injectStyles,
  runExtensionCommand,
} from './common'
import STYLES from './styles.css?raw'

const STYLE_ID = 'gym-activity-styles-v1'

interface Zone { id: string; name: string; equipment_type: string; polygon: number[][]; enabled: boolean | number }
interface Heat { cols: number; rows: number; grid: number[] }
interface LiveTrack { track_id: number; foot?: { x: number; y: number }; trail?: Array<{ x: number; y: number }> }
interface WindowData {
  start: number; end: number; cols: number; rows: number
  grid: number[]; samples: number; tracks: number
  trails: Array<{ track_id: number; pts: Array<{ x: number; y: number }> }>
}
/** window spans the bar offers */
const SPANS: Array<[number, string]> = [
  [1800, '30分'], [3600, '1时'], [4 * 3600, '4时'], [12 * 3600, '12时'], [24 * 3600, '24时'],
]
const fmtClock = (ts: number) => {
  const d = new Date(ts * 1000)
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
}

/** Shared timeline: LIVE toggle + span chips + a scrub slider over 24h.
 *  When scrubbing, `start/end` describe the queried window; null = live. */
function TimeBar({ span, setSpan, scrub, setScrub, samples }: {
  span: number
  setSpan: (s: number) => void
  scrub: number | null
  setScrub: (v: number | null) => void
  samples: number
}) {
  const now = Math.floor(Date.now() / 1000)
  const maxStart = now - span
  const pos = scrub == null ? 1 : Math.max(0, Math.min(1, (now - span - scrub) / Math.max(1, maxStart - scrub + span - span || 1)))
  // slider maps 0..1 → window end offset from now (0 = newest, 1 = oldest)
  const sliderVal = scrub == null ? 0 : Math.min(1, (now - (scrub + span)) / (24 * 3600 - span))
  return (
    <div className="gym-timebar">
      <button
        className={`gym-ov-tg ${scrub == null ? 'on' : ''}`}
        onClick={() => setScrub(null)}
        title="回到实时"
      >实时</button>
      <div className="gym-timebar-spans">
        {SPANS.map(([s, label]) => (
          <button key={s} className={`gym-ov-tg ${span === s ? 'on' : ''}`}
            onClick={() => { setSpan(s); setScrub(null) }}>{label}</button>
        ))}
      </div>
      <input
        className="gym-timebar-slider"
        type="range" min={0} max={1} step={0.001}
        value={sliderVal}
        onChange={(e) => {
          const v = Number(e.target.value)
          // dragging right moves the window into the past
          setScrub(v <= 0.001 ? null : now - span - Math.round(v * (24 * 3600 - span)))
        }}
      />
      <span className="gym-timebar-label">
        {scrub == null ? '实时' : `${fmtClock(scrub)}–${fmtClock(scrub + span)}`}
        {samples > 0 && <em>{samples} 样本</em>}
      </span>
      {voidPos(pos)}
    </div>
  )
}
// keeps the unused pos var referenced without tripping lint
const voidPos = (_: number) => null

/** blue → cyan → yellow → red ramp, alpha by intensity */
function heatColor(v: number): string {
  const r = Math.round(Math.min(255, v < 0.5 ? v * 2 * 60 : 60 + (v - 0.5) * 2 * 195))
  const g = Math.round(Math.min(255, v < 0.5 ? 90 + v * 2 * 165 : 255 - (v - 0.5) * 2 * 115))
  const b = Math.round(v < 0.25 ? 255 - v * 4 * 55 : v < 0.5 ? 145 - (v - 0.25) * 4 * 145 : 0)
  return `rgba(${r},${g},${b},${(0.25 + v * 0.55).toFixed(2)})`
}

/**
 * Canvas that paints the latest camera JPEG as a dimmed background and
 * re-runs the layer painter on every image/data change.
 */
function useFrameCanvas(
  canvasRef: React.RefObject<HTMLCanvasElement | null>,
  extensionId: string,
  frameMs: number,
  paint: (ctx: CanvasRenderingContext2D, W: number, H: number, bg: HTMLImageElement | null) => void,
  deps: unknown[],
) {
  const bgRef = useRef<HTMLImageElement | null>(null)
  const paintRef = useRef(paint)
  paintRef.current = paint

  const draw = useCallback(() => {
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
    ctx.fillStyle = '#0a0a0c'
    ctx.fillRect(0, 0, cw, ch)
    paintRef.current(ctx, cw, ch, bgRef.current)
  }, [canvasRef])

  useEffect(() => {
    let alive = true
    const tick = async () => {
      const r = await fetchFrame(extensionId)
      const b64 = (r as { data?: FrameBundle | null })?.data?.img_b64
      if (!alive) return
      if (b64) {
        const img = new Image()
        img.onload = () => {
          if (!alive) return
          bgRef.current = img
          draw()
        }
        img.src = `data:image/jpeg;base64,${b64}`
      } else {
        bgRef.current = null
        draw()
      }
    }
    tick()
    const id = setInterval(tick, frameMs)
    return () => { alive = false; clearInterval(id) }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [extensionId, frameMs, draw, ...deps])
}

/** Dimmed cover-fit camera background shared by both cards. */
function paintBackground(
  ctx: CanvasRenderingContext2D, W: number, H: number, bg: HTMLImageElement | null
) {
  if (!bg) return
  const s = Math.max(W / bg.naturalWidth, H / bg.naturalHeight)
  const dw = bg.naturalWidth * s
  const dh = bg.naturalHeight * s
  ctx.globalAlpha = 0.55
  ctx.drawImage(bg, (W - dw) / 2, (H - dh) / 2, dw, dh)
  ctx.globalAlpha = 1
}

function useZones(extensionId: string) {
  const zonesRef = useRef<Zone[]>([])
  useEffect(() => {
    let alive = true
    const load = async () => {
      const r = await runExtensionCommand<{ zones: Zone[] }>(extensionId, 'get_roi_zones', {})
      if (alive && r.success && r.data) {
        zonesRef.current = r.data.zones.filter((z) => z.equipment_type !== 'exclusion')
      }
    }
    load()
    const id = setInterval(load, 30000)
    return () => { alive = false; clearInterval(id) }
  }, [extensionId])
  return zonesRef
}

function drawZoneOutlines(ctx: CanvasRenderingContext2D, W: number, H: number, zones: Zone[]) {
  ctx.lineWidth = 1
  ctx.strokeStyle = 'rgba(226, 232, 240, 0.35)'
  ctx.setLineDash([5, 4])
  for (const z of zones) {
    if (!z.polygon || z.polygon.length < 3) continue
    ctx.beginPath()
    z.polygon.forEach((p, i) =>
      i === 0 ? ctx.moveTo(p[0] * W, p[1] * H) : ctx.lineTo(p[0] * W, p[1] * H)
    )
    ctx.closePath()
    ctx.stroke()
  }
  ctx.setLineDash([])
}

const TrailsIcon = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"
    strokeLinecap="round" strokeLinejoin="round">
    <path d="M3 17c4-8 8 4 12-4 2.5-5 4-3 6-6" />
    <circle cx="21" cy="7" r="1.6" fill="currentColor" stroke="none" />
  </svg>
)

export const GymTrailsCard = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function GymTrailsCard(props, ref) {
    const { dataSource, className = '' } = props
    const extensionId = dataSource?.extensionId || DEFAULT_EXTENSION_ID
    useEffect(() => injectStyles(STYLE_ID, STYLES), [])
    const canvasRef = useRef<HTMLCanvasElement>(null)
    const zonesRef = useZones(extensionId)
    const tracksRef = useRef<LiveTrack[]>([])
    const [present, setPresent] = useState<number | null>(null)
    // timeline: null scrub = live; otherwise a window start (unix s)
    const [span, setSpan] = useState(3600)
    const [scrub, setScrub] = useState<number | null>(null)
    const winRef = useRef<WindowData | null>(null)
    const [winInfo, setWinInfo] = useState<{ samples: number } | null>(null)

    const refresh = useCallback(async () => {
      const r = await runExtensionCommand<{ present_count: number; tracks: LiveTrack[] }>(
        extensionId, 'get_live_state', {})
      if (!r.success || !r.data) return
      tracksRef.current = r.data.tracks ?? []
      setPresent(r.data.present_count ?? 0)
    }, [extensionId])
    useEffect(() => {
      refresh()
      const id = setInterval(refresh, 2000)
      return () => clearInterval(id)
    }, [refresh])

    // window query while scrubbing. Re-runs periodically: a scrubbed
    // window that ends "now" keeps filling as samples land, and an empty
    // (pre-deployment) window must still re-check. 5 s cadence.
    useEffect(() => {
      if (scrub == null) { winRef.current = null; setWinInfo(null); return }
      let alive = true
      const load = async () => {
        const r = await runExtensionCommand<WindowData>(extensionId, 'get_activity_window', {
          start: scrub, end: scrub + span,
        })
        if (!alive || !r.success || !r.data) return
        winRef.current = r.data
        setWinInfo({ samples: r.data.samples })
      }
      load()
      const id = setInterval(load, 5000)
      return () => { alive = false; clearInterval(id) }
    }, [extensionId, scrub, span])

    useFrameCanvas(canvasRef, extensionId, 2500, (ctx, W, H, bg) => {
      paintBackground(ctx, W, H, bg)
      drawZoneOutlines(ctx, W, H, zonesRef.current)
      const win = scrub == null ? null : winRef.current
      const trailsSrc: Array<{ pts: Array<{ x: number; y: number }> }> = win
        ? win.trails
        : tracksRef.current.map((t) => ({ pts: t.trail ?? [] }))
      for (const t of trailsSrc) {
        const trail = t.pts
        for (let i = 1; i < trail.length; i++) {
          const a = ((i - 1) / trail.length) * 0.75 + 0.2
          ctx.strokeStyle = `rgba(96, 165, 250, ${a.toFixed(2)})`
          ctx.lineWidth = 1.8
          ctx.beginPath()
          ctx.moveTo(trail[i - 1].x * W, trail[i - 1].y * H)
          ctx.lineTo(trail[i].x * W, trail[i].y * H)
          ctx.stroke()
        }
        if (trail.length > 0) {
          const last = trail[trail.length - 1]
          ctx.beginPath()
          ctx.arc(last.x * W, last.y * H, 3.2, 0, Math.PI * 2)
          ctx.fillStyle = '#60a5fa'
          ctx.fill()
          ctx.strokeStyle = 'rgba(9, 14, 26, .8)'
          ctx.lineWidth = 1
          ctx.stroke()
        }
      }
    }, [present, scrub, winInfo])

    return (
      <div ref={ref} className={`gym-activity ${className}`}>
        <div className="gym-traffic-card">
          <div className="gym-traffic-header">
            <div className="gym-traffic-title">
              <TrailsIcon />
              <span>Gym · 轨迹</span>
            </div>
            <span className="gym-ov-badge">
              {scrub == null ? (present != null ? `${present} 人在场` : '…') : '回看'}
            </span>
          </div>
          <div className="gym-activity-body">
            <div className="gym-activity-stagewrap">
              <canvas ref={canvasRef} className="gym-activity-canvas" />
              {scrub != null && (winInfo?.samples ?? 0) === 0 && (
                <div className="gym-activity-empty">
                  该时段没有足迹记录
                  <small>足迹日志自今天起累积；拖回最右侧查看实时</small>
                </div>
              )}
            </div>
            <TimeBar span={span} setSpan={setSpan} scrub={scrub} setScrub={setScrub}
              samples={winInfo?.samples ?? 0} />
          </div>
        </div>
      </div>
    )
  },
)
GymTrailsCard.displayName = 'GymTrailsCard'

const HeatIcon = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"
    strokeLinecap="round" strokeLinejoin="round">
    <circle cx="12" cy="12" r="9" />
    <circle cx="12" cy="12" r="5" fill="currentColor" stroke="none" opacity=".55" />
  </svg>
)

export const GymHeatCard = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function GymHeatCard(props, ref) {
    const { dataSource, className = '', config } = props
    const extensionId = dataSource?.extensionId || DEFAULT_EXTENSION_ID
    const showZones = config?.showZones !== false
    useEffect(() => injectStyles(STYLE_ID, STYLES), [])
    const canvasRef = useRef<HTMLCanvasElement>(null)
    const zonesRef = useZones(extensionId)
    const heatRef = useRef<Heat | null>(null)
    const [peak, setPeak] = useState<number | null>(null)
    const [span, setSpan] = useState(3600)
    const [scrub, setScrub] = useState<number | null>(null)
    const winRef = useRef<WindowData | null>(null)
    const [winInfo, setWinInfo] = useState<{ samples: number } | null>(null)

    const refresh = useCallback(async () => {
      const r = await runExtensionCommand<Heat>(extensionId, 'get_heatmap', {})
      if (r.success && r.data) {
        heatRef.current = r.data
        setPeak(Math.max(0, ...(r.data.grid ?? [0])))
      }
    }, [extensionId])
    useEffect(() => {
      refresh()
      const id = setInterval(refresh, 15000)
      return () => clearInterval(id)
    }, [refresh])

    // window query while scrubbing (5 s re-check so windows ending "now"
    // fill in and empty windows recover once data lands)
    useEffect(() => {
      if (scrub == null) { winRef.current = null; setWinInfo(null); return }
      let alive = true
      const load = async () => {
        const r = await runExtensionCommand<WindowData>(extensionId, 'get_activity_window', {
          start: scrub, end: scrub + span,
        })
        if (!alive || !r.success || !r.data) return
        winRef.current = r.data
        setWinInfo({ samples: r.data.samples })
      }
      load()
      const id = setInterval(load, 5000)
      return () => { alive = false; clearInterval(id) }
    }, [extensionId, scrub, span])

    useFrameCanvas(canvasRef, extensionId, 5000, (ctx, W, H, bg) => {
      paintBackground(ctx, W, H, bg)
      const win = scrub == null ? null : winRef.current
      const heat: Heat | null = win
        ? { cols: win.cols, rows: win.rows, grid: win.grid }
        : heatRef.current
      if (heat && heat.grid?.length) {
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
      if (showZones) drawZoneOutlines(ctx, W, H, zonesRef.current)
    }, [peak, showZones, scrub, winInfo])

    return (
      <div ref={ref} className={`gym-activity ${className}`}>
        <div className="gym-traffic-card">
          <div className="gym-traffic-header">
            <div className="gym-traffic-title">
              <HeatIcon />
              <span>Gym · 热力</span>
            </div>
            <span className="gym-ov-badge">
              {scrub == null ? (peak != null ? `今日峰值 ${peak}` : '…') : '回看'}
            </span>
          </div>
          <div className="gym-activity-body">
            <div className="gym-activity-stagewrap">
            <canvas ref={canvasRef} className="gym-activity-canvas" />
            {scrub != null && (winInfo?.samples ?? 0) === 0 && (
              <div className="gym-activity-empty">
                该时段没有足迹记录
                <small>足迹日志自今天起累积；拖回最右侧查看实时</small>
              </div>
            )}
            </div>
            <div className="gym-activity-legend">
              <span>低</span>
              <span className="gym-activity-ramp" />
              <span>高</span>
            </div>
            <TimeBar span={span} setSpan={setSpan} scrub={scrub} setScrub={setScrub}
              samples={winInfo?.samples ?? 0} />
          </div>
        </div>
      </div>
    )
  },
)
GymHeatCard.displayName = 'GymHeatCard'
