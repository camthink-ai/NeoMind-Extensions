/**
 * GymVideoOverlay — all-in-one gym monitor.
 *
 * One canvas, stacked layers:
 *  1. live video (push session on the stream-player extension — the camera
 *     frame IS the configuration background, so zones/lines are drawn
 *     against the real scene, not an empty grid);
 *  1b. face mosaic — detected face regions pixelated in-place (privacy,
 *     default ON; drawn before any analytics so overlays stay readable);
 *  2. heatmap overlay (today's foot-position histogram, toggle);
 *  3. zones — arbitrary polygons (N vertices) with live occupancy computed
 *     client-side (foot-in-polygon, same rule as the extension metrics),
 *     each zone labelled on the video with its live count;
 *  4. crossing lines — with live in/out count badges at the midpoint;
 *  5. trails — fading foot-history polylines per track (from get_live_state);
 *  6. AI overlay — per-person bbox + COCO-17 skeleton + foot markers.
 *
 * Edit mode: pick 「分区」 or 「画线」, click on the video. Zones: N vertices →
 * 「闭合」. Lines: 2 points, second click auto-closes (a→b is the direction
 * reference; in = crossing towards the left of a→b). Save persists the FULL
 * set via set_roi_zones / set_lines. Everything keeps updating live while
 * editing.
 */

import { forwardRef, useCallback, useEffect, useRef, useState } from 'react'
import {
  DEFAULT_EXTENSION_ID,
  ExtensionComponentProps,
  LineDef,
  LineStats,
  LiveState,
  Member,
  SKELETON_EDGES,
  VIDEO_EXTENSION_ID,
  Bbox,
  Point,
  Zone,
  deleteMember,
  fetchCrossings,
  fetchFrame,
  FrameBundle,
  fetchHeatmap,
  fetchLines,
  fetchLiveState,
  fetchMembers,
  fetchZones,
  getToken,
  injectStyles,
  mergeMembers,
  memberPhotoSrc,
  pointInPolygon,
  registerMember,
  renameMember,
  runExtensionCommand,
  setMemberPhoto,
} from './common'
import STYLES from './styles.css?raw'

const STYLE_ID = 'gym-monitor-styles-v1'

type Status = 'idle' | 'connecting' | 'streaming' | 'error'
type EditKind = 'zones' | 'lines' | 'members'

const KPT_MIN_SCORE = 0.2

// Mosaic cell size in canvas px — coarse enough to obscure identity, fine
// enough to still read "there is a face here".
const MOSAIC_CELL = 14
// Face boxes arrive at the detect cadence (every Nth frame); pad each box
// so a head moving between detections stays covered.
const MOSAIC_PAD = 0.12

// ---- overlay/video time alignment ----
// The video path (RTSP/file → decode → JPEG → WS) lags the analytics path by
// a variable 0.3–1.5 s, so drawing the NEWEST sample on the NEWEST frame
// misplaces boxes. Render at (now − OVERLAY_DELAY_MS) instead, interpolating
// between history samples (smooth motion) and extrapolating at most
// EXTRAP_MAX_MS when data is momentarily behind the picture.
const OVERLAY_DELAY_MS = 150
const EXTRAP_MAX_MS = 450

interface HistEntry { t: number; bbox: Bbox; foot?: Point | null }

/** Interpolated bbox at wall-time `target` from a sample history. */
function bboxAt(hist: HistEntry[], target: number): Bbox | null {
  if (hist.length === 0) return null
  const first = hist[0]
  const last = hist[hist.length - 1]
  let a: HistEntry, b: HistEntry, f: number
  if (target <= first.t) {
    a = first
    b = hist[Math.min(1, hist.length - 1)]
    f = 0
  } else if (target >= last.t) {
    const p = hist.length >= 2 ? hist[hist.length - 2] : last
    const span = Math.max(1, last.t - p.t)
    const over = Math.min(target - last.t, EXTRAP_MAX_MS)
    a = p
    b = last
    f = 1 + over / span
  } else {
    a = first
    b = last
    f = 0
    for (let i = 1; i < hist.length; i++) {
      if (hist[i].t >= target) {
        a = hist[i - 1]
        b = hist[i]
        f = (target - a.t) / Math.max(1, b.t - a.t)
        break
      }
    }
  }
  return {
    x: a.bbox.x + (b.bbox.x - a.bbox.x) * f,
    y: a.bbox.y + (b.bbox.y - a.bbox.y) * f,
    w: a.bbox.w + (b.bbox.w - a.bbox.w) * f,
    h: a.bbox.h + (b.bbox.h - a.bbox.h) * f,
  }
}

interface DraftZone extends Zone {
  isNew?: boolean
}
interface DraftLine extends LineDef {
  isNew?: boolean
}

interface HeatmapData {
  cols: number
  rows: number
  grid: number[]
}

export const GymVideoOverlay = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function GymVideoOverlay(props, ref) {
    const {
      dataSource,
      className = '',
      sourceUrl = '',
      targetFps = 10,
      outputWidth = 960,
      statePollMs = 600,
    } = props
    const extensionId = dataSource?.extensionId || DEFAULT_EXTENSION_ID
    const fps = Math.min(24, Math.max(1, Number(targetFps) || 10))
    const pollMs = Math.min(5000, Math.max(250, Number(statePollMs) || 600))

    useEffect(() => injectStyles(STYLE_ID, STYLES), [])

    const [status, setStatus] = useState<Status>('idle')
    const [error, setError] = useState<string | null>(null)
    const [present, setPresent] = useState(0)
    const [videoFps, setVideoFps] = useState(0)
    const [mode, setMode] = useState<'view' | 'edit'>('view')
    const [editKind, setEditKind] = useState<EditKind>('zones')

    const [zones, setZones] = useState<DraftZone[]>([])
    const [draft, setDraft] = useState<number[][]>([])
    const [lines, setLines] = useState<DraftLine[]>([])
    const [draftLine, setDraftLine] = useState<number[][]>([])
    const [crossings, setCrossings] = useState<LineStats[]>([])
    // P3: member registration — set when the user clicks a person in view
    // mode; the panel collects a name and calls register_member.
    const [register, setRegister] = useState<{ trackId: number; name: string; busy: boolean; msg: string | null } | null>(null)
    const [members, setMembers] = useState<Member[]>([])
  // P4: member being merged — holds the src id while the user picks dst
  const [merging, setMerging] = useState<string | null>(null)

    const [showTrails, setShowTrails] = useState(true)
    const [showZones, setShowZones] = useState(true)
    const [showHeatmap, setShowHeatmap] = useState(false)
    const [mosaic, setMosaic] = useState(true)
    const [heat, setHeat] = useState<HeatmapData | null>(null)

    const [saving, setSaving] = useState(false)
    const [savedFlash, setSavedFlash] = useState(0)

    const canvasRef = useRef<HTMLCanvasElement>(null)
    const wsRef = useRef<WebSocket | null>(null)
    // dedicated WS for the device-frame push stream (distinct from wsRef,
    // which belongs to the stream-player video path)
    const frameWsRef = useRef<WebSocket | null>(null)
    // scratch canvas for the face mosaic downsample (reused every frame)
    const mosaicCanvasRef = useRef<HTMLCanvasElement>(
      typeof document !== 'undefined' ? document.createElement('canvas') : null as unknown as HTMLCanvasElement
    )
    const stateRef = useRef<LiveState | null>(null)
    const zonesRef = useRef<DraftZone[]>([])
    const linesRef = useRef<DraftLine[]>([])
    const crossingsRef = useRef<LineStats[]>([])
    const draftRef = useRef<number[][]>([])
    const draftLineRef = useRef<number[][]>([])
    const modeRef = useRef<'view' | 'edit'>('view')
    const editKindRef = useRef<EditKind>('zones')
    const showRef = useRef({ trails: true, zones: true, heat: false, mosaic: true })
    const heatRef = useRef<HeatmapData | null>(null)
    const imgRef = useRef<HTMLImageElement | null>(null)
    const rafRef = useRef<number>(0)
    const layoutRef = useRef({ dx: 0, dy: 0, dw: 1, dh: 1 })
    const fpsCounterRef = useRef({ frames: 0, last: Date.now() })
    const fallbackRef = useRef<number | null>(null)
    const mountedRef = useRef(true)
    // ---- overlay/video time alignment ----
    // Video (RTSP→decode→JPEG→WS) lags the analytics pipeline by a variable
    // 0.3–1.5 s, so drawing "the newest sample" misplaces boxes. Instead each
    // track keeps a short bbox history and the renderer interpolates to
    // (now − overlayDelayMs), extrapolating up to 0.4 s when data is behind.
    const trackHistRef = useRef<Map<number, Array<{ t: number; bbox: Bbox; foot?: Point | null }>>>(new Map())

    useEffect(() => { zonesRef.current = zones }, [zones])
    useEffect(() => { linesRef.current = lines }, [lines])
    useEffect(() => { crossingsRef.current = crossings }, [crossings])
    useEffect(() => { draftRef.current = draft }, [draft])
    useEffect(() => { draftLineRef.current = draftLine }, [draftLine])
    useEffect(() => { modeRef.current = mode }, [mode])
    useEffect(() => { editKindRef.current = editKind }, [editKind])
    useEffect(() => { showRef.current = { trails: showTrails, zones: showZones, heat: showHeatmap, mosaic } }, [showTrails, showZones, showHeatmap, mosaic])
    useEffect(() => { heatRef.current = heat }, [heat])

    useEffect(() => {
      mountedRef.current = true
      return () => { mountedRef.current = false }
    }, [])

    // Backing store follows the DISPLAYED size (× DPR): the buffer aspect
    // then always equals the widget aspect, so the letterboxed video is
    // never stretched and the overlay tracks it exactly.
    useEffect(() => {
      const canvas = canvasRef.current
      if (!canvas || typeof ResizeObserver === 'undefined') return
      const fit = () => {
        const cw = canvas.clientWidth
        const ch = canvas.clientHeight
        if (cw < 2 || ch < 2) return
        const dpr = Math.min(2, window.devicePixelRatio || 1)
        let w = Math.round(cw * dpr)
        if (w > 1920) w = 1920
        const h = Math.round((w / cw) * ch)
        if (canvas.width !== w || canvas.height !== h) {
          canvas.width = w
          canvas.height = h
        }
      }
      fit()
      const ro = new ResizeObserver(fit)
      ro.observe(canvas)
      return () => ro.disconnect()
    }, [])

    // ---- data polling ----
    useEffect(() => {
      let stopped = false
      const poll = async () => {
        const r = await fetchLiveState(extensionId)
        if (stopped || !mountedRef.current) return
        if (r.success && r.data) {
          // device-frame mode owns the display (image+tracks single-source);
          // this poll only feeds auxiliary state when stream-player mode is
          // active or as a fallback.
          if (!deviceFramesRef.current) {
            stateRef.current = r.data
            setPresent(r.data.present_count)
          }
          // bbox history for time-aligned interpolation (stream-player mode)
          const now = performance.now()
          const hist = trackHistRef.current
          const seen = new Set<number>()
          for (const t of r.data.tracks ?? []) {
            if (!t.bbox) continue
            seen.add(t.track_id)
            let arr = hist.get(t.track_id)
            if (!arr) { arr = []; hist.set(t.track_id, arr) }
            arr.push({ t: now, bbox: t.bbox, foot: t.foot })
            while (arr.length > 0 && now - arr[0].t > 3000) arr.shift()
          }
          for (const k of [...hist.keys()]) if (!seen.has(k)) hist.delete(k)
        }
      }
      poll()
      const id = setInterval(poll, pollMs)
      return () => { stopped = true; clearInterval(id) }
    }, [extensionId, pollMs])

    // ---- device-frame mode: image + tracks from ONE source ----
    // When the producer attaches frame previews (PREVIEW=1), the Monitor
    // renders the preview JPEG with the tracks of that exact frame — the
    // separate video pipeline (stream-player) is bypassed entirely, so video
    // and overlay can never desync, in live AND replay modes.
    // Transport: the extension's own WS push channel (push mode). WS
    // messages are the one path browser timer-throttling cannot slow —
    // occluded tabs clamp timers/rAF to ~1 Hz but deliver WS at full rate.
    // Falls back to a response-chained REST poll when WS is unavailable.
    const deviceFramesRef = useRef(false)
    const lastImgRef = useRef<string>('')
    const applyFrameBundle = useCallback((data: FrameBundle) => {
      if (!data.img_b64 || data.img_b64 === lastImgRef.current) return
      lastImgRef.current = data.img_b64
      if (!deviceFramesRef.current) {
        deviceFramesRef.current = true
        // stream-player video is superseded by the device frames
        if (wsRef.current) { try { wsRef.current.close() } catch { /* already closed */ } wsRef.current = null }
      }
      // every device frame proves the display pipeline is alive — also
      // overrides any stale error status from the superseded video path
      setStatus('streaming')
      const im = new Image()
      im.onload = () => {
        if (!mountedRef.current) return
        imgRef.current = im
        const c = fpsCounterRef.current
        c.frames++
        const now = Date.now()
        if (now - c.last >= 1000) {
          setVideoFps(Math.round((c.frames * 1000) / (now - c.last)))
          c.frames = 0
          c.last = now
        }
        // rAF may be paused entirely in occluded tabs — draw NOW
        drawRef.current?.()
      }
      im.src = `data:image/jpeg;base64,${data.img_b64}`
      // tracks/faces OF the same frame; also feed the bbox history so the
      // renderer can interpolate BETWEEN device frames — data arrives at
      // ~5 Hz but drawing runs at display rate, keeping motion smooth.
      const nowH = performance.now()
      const hist = trackHistRef.current
      const seen = new Set<number>()
      for (const t of data.tracks ?? []) {
        if (!t.bbox) continue
        seen.add(t.track_id)
        let arr = hist.get(t.track_id)
        if (!arr) { arr = []; hist.set(t.track_id, arr) }
        arr.push({ t: nowH, bbox: t.bbox, foot: t.foot })
        while (arr.length > 0 && nowH - arr[0].t > 2000) arr.shift()
      }
      for (const k of [...hist.keys()]) if (!seen.has(k)) hist.delete(k)
      stateRef.current = {
        present_count: data.present_count ?? data.tracks?.length ?? 0,
        tracks: data.tracks ?? [],
        faces: data.faces ?? [],
      } as LiveState
      setPresent(data.present_count ?? data.tracks?.length ?? 0)
    }, [])

    useEffect(() => {
      let stopped = false
      let timer: number | undefined
      let ws: WebSocket | null = null

      const startPolling = () => {
        const poll = async () => {
          const r = await fetchFrame(extensionId)
          if (stopped || !mountedRef.current) return
          if (r.success && r.data) applyFrameBundle(r.data)
          // chain from the response (not setInterval) — throttling then only
          // delays the schedule instead of stacking missed intervals
          timer = window.setTimeout(poll, 120)
        }
        poll()
      }

      const startPush = () => {
        try {
          const isTauri = !!(window as any).__TAURI_INTERNALS__
          const proto = (isTauri ? false : window.location.protocol === 'https:') ? 'wss:' : 'ws:'
          const host = isTauri ? 'localhost:9375' : window.location.host
          let url = `${proto}//${host}/api/extensions/${extensionId}/stream`
          const token = getToken()
          if (token) url += `?token=${encodeURIComponent(token)}`
          ws = new WebSocket(url)
          frameWsRef.current = ws
          ws.onopen = () => {
            ws?.send(JSON.stringify({ type: 'init', config: {} }))
          }
          ws.onmessage = (event) => {
            if (typeof event.data !== 'string' || !mountedRef.current) return
            try {
              const msg = JSON.parse(event.data)
              if (msg.type === 'session_created') {
                ws?.send(JSON.stringify({ type: 'start_push', session_id: msg.session_id }))
              } else if (msg.type === 'push_output' && msg.data_type === 'application/json') {
                const bundle = typeof msg.data === 'string' ? JSON.parse(msg.data) : msg.data
                applyFrameBundle(bundle)
              }
            } catch { /* malformed frame — skip */ }
          }
          ws.onerror = () => { if (!stopped) startPolling() }
          ws.onclose = () => {
            if (frameWsRef.current === ws) frameWsRef.current = null
            if (!stopped) startPolling() // WS lost — degrade to polling
          }
        } catch {
          startPolling()
        }
      }
      startPush()
      return () => {
        stopped = true
        if (timer) clearTimeout(timer)
        if (ws) { try { ws.close() } catch { /* already closed */ } }
      }
    }, [extensionId, applyFrameBundle])

    const loadZones = useCallback(async () => {
      const r = await fetchZones(extensionId)
      if (mountedRef.current && r.success && r.data) setZones(r.data.zones ?? [])
    }, [extensionId])

    const loadLines = useCallback(async () => {
      const [lr, cr] = await Promise.all([
        fetchLines(extensionId),
        fetchCrossings(extensionId),
      ])
      if (!mountedRef.current) return
      if (lr.success && lr.data) setLines(lr.data.lines ?? [])
      if (cr.success && cr.data) setCrossings(cr.data.lines ?? [])
    }, [extensionId])

    const loadMembers = useCallback(async () => {
      const r = await fetchMembers(extensionId)
      if (mountedRef.current && r.success && r.data) setMembers(r.data.members ?? [])
    }, [extensionId])

    // ---- member avatar capture ----
    // Crop a normalized bbox from the RAW video frame (imgRef — never the
    // canvas, whose face regions are mosaic-pixelated) into a square JPEG
    // thumbnail. Returns raw base64 (no data: prefix) or null.
    const cropAvatar = useCallback((bbox: { x: number; y: number; w: number; h: number }): string | null => {
      const img = imgRef.current
      if (!img || img.naturalWidth === 0) return null
      // normalized coords are frame-relative → scale directly to natural px
      const cx = bbox.x * img.naturalWidth
      const cy = bbox.y * img.naturalHeight
      const cw = bbox.w * img.naturalWidth
      const ch = bbox.h * img.naturalHeight
      if (cw < 16 || ch < 16) return null
      // head-and-shoulders crop: top ~45% of the body box, slightly widened
      const hw = Math.min(img.naturalWidth, cw * 1.15)
      const hh = ch * 0.45
      const sx = Math.max(0, cx + cw / 2 - hw / 2)
      const sy = Math.max(0, cy)
      const S = 128
      const off = document.createElement('canvas')
      off.width = S; off.height = S
      const octx = off.getContext('2d')
      if (!octx) return null
      octx.fillStyle = '#111'
      octx.fillRect(0, 0, S, S)
      // square-fill: crop the smaller dimension centered
      const side = Math.min(hw, hh)
      const ox = sx + (hw - side) / 2
      const oy = sy + (hh - side) / 2
      try {
        octx.drawImage(img, ox, oy, side, side, 0, 0, S, S)
      } catch { return null }
      const url = off.toDataURL('image/jpeg', 0.82)
      return url.startsWith('data:image/jpeg;base64,') ? url.slice(23) : null
    }, [])

    // Track ids that already had a photo-capture attempt this session —
    // prevents re-cropping the same person on every poll tick.
    const photoTriedRef = useRef<Set<number>>(new Set())

    // Auto-capture: when a known member (matched by the extension) appears
    // with no avatar yet, grab a head crop from the current frame.
    const maybeAutoPhoto = useCallback(async () => {
      if (editKindRef.current === 'members') return // panel is open; user may register
      const state = stateRef.current
      if (!state) return
      const noPhoto = new Set(members.filter((m) => !m.photo).map((m) => m.id))
      if (noPhoto.size === 0) return
      const target = (state.tracks ?? []).find(
        (t) =>
          t.member?.id &&
          noPhoto.has(t.member.id) &&
          t.bbox &&
          !photoTriedRef.current.has(t.track_id)
      )
      if (!target || !target.bbox) return
      photoTriedRef.current.add(target.track_id)
      const b64 = cropAvatar(target.bbox)
      if (!b64) return
      const r = await setMemberPhoto(extensionId, target.member!.id, b64)
      if (r.success && mountedRef.current) loadMembers()
    }, [members, cropAvatar, extensionId, loadMembers])

    // Piggyback on the members poll: after each refresh, try one capture.
    useEffect(() => { maybeAutoPhoto() }, [members, maybeAutoPhoto])

    useEffect(() => {
      const t = setTimeout(() => {
        if (mountedRef.current) { loadZones(); loadLines(); loadMembers() }
      }, 300)
      return () => clearTimeout(t)
    }, [loadZones, loadLines, loadMembers])

    useEffect(() => {
      const id = setInterval(() => {
        if (mountedRef.current) {
          loadLines()
          if (showHeatmap) {
            fetchHeatmap(extensionId).then((r) => {
              if (mountedRef.current && r.success && r.data) setHeat(r.data)
            })
          }
        }
      }, 2500)
      return () => clearInterval(id)
    }, [loadLines, showHeatmap, extensionId])

    // ---- render loop ----
    const draw = useCallback(() => {
      const canvas = canvasRef.current
      const ctx = canvas?.getContext('2d')
      if (!canvas || !ctx) return

      // Virtual coordinate space: everything below draws in a 960-wide
      // viewport so fonts/line widths stay proportional regardless of the
      // backing-store resolution (which tracks the widget size × DPR via
      // ResizeObserver — the buffer aspect always equals the display aspect,
      // so the image is letterboxed, never stretched).
      const K = canvas.width / 960
      if (!Number.isFinite(K) || K <= 0) return
      ctx.setTransform(K, 0, 0, K, 0, 0)
      const VW = canvas.width / K
      const VH = canvas.height / K

      ctx.fillStyle = '#050505'
      ctx.fillRect(0, 0, VW, VH)
      const img = imgRef.current
      if (img && img.naturalWidth > 0) {
        const scale = Math.min(
          VW / img.naturalWidth,
          VH / img.naturalHeight
        )
        const l = {
          dx: (VW - img.naturalWidth * scale) / 2,
          dy: (VH - img.naturalHeight * scale) / 2,
          dw: img.naturalWidth * scale,
          dh: img.naturalHeight * scale,
        }
        layoutRef.current = l
        ctx.drawImage(img, l.dx, l.dy, l.dw, l.dh)
      }
      const { dx, dy, dw, dh } = layoutRef.current
      const X = (nx: number) => dx + nx * dw
      const Y = (ny: number) => dy + ny * dh

      const state = stateRef.current
      const tracks = state?.tracks ?? []
      const show = showRef.current

      // ---- face mosaic (privacy, default on) ----
      // Pixelate each detected face region: downsample the already-drawn
      // video through a tiny offscreen canvas, then draw it back scaled-up
      // with smoothing off. Runs right after the video layer so every
      // analytics layer (zones/lines/skeletons) renders on top of it.
      const faces = state?.faces ?? []
      if (show.mosaic && faces.length > 0) {
        for (const f of faces) {
          if (!f.bbox) continue
          const pad = MOSAIC_PAD
          const fx = X(Math.max(0, f.bbox.x - f.bbox.w * pad))
          const fy = Y(Math.max(0, f.bbox.y - f.bbox.h * pad))
          const fw = f.bbox.w * (1 + pad * 2) * dw
          const fh = f.bbox.h * (1 + pad * 2) * dh
          if (fw < 10 || fh < 10) continue
          const sw = Math.max(2, Math.round(fw / MOSAIC_CELL))
          const sh = Math.max(2, Math.round(fh / MOSAIC_CELL))
          const off = mosaicCanvasRef.current
          off.width = sw
          off.height = sh
          const octx = off.getContext('2d')
          if (!octx) continue
          octx.imageSmoothingEnabled = true
          // source rect is in REAL backing-store pixels (virtual × K); the
          // transform scales only the destination rect
          octx.drawImage(canvas, fx * K, fy * K, fw * K, fh * K, 0, 0, sw, sh)
          ctx.imageSmoothingEnabled = false
          ctx.drawImage(off, 0, 0, sw, sh, fx, fy, fw, fh)
          ctx.imageSmoothingEnabled = true
          // thin white outline marks the covered region
          ctx.strokeStyle = 'rgba(255, 255, 255, 0.7)'
          ctx.lineWidth = 1.5
          ctx.strokeRect(fx, fy, fw, fh)
        }
      }

      // ---- heatmap layer ----
      const hm = heatRef.current
      if (show.heat && hm && hm.grid) {
        const cellW = dw / hm.cols
        const cellH = dh / hm.rows
        const max = Math.max(1, ...hm.grid)
        for (let r = 0; r < hm.rows; r++) {
          for (let c = 0; c < hm.cols; c++) {
            const v = hm.grid[r * hm.cols + c]
            if (!v) continue
            const t = Math.min(1, Math.log2(1 + v) / Math.log2(1 + max))
            // blue → red ramp
            const rr = Math.round(40 + t * 215)
            const gg = Math.round(90 * (1 - t))
            const bb = Math.round(220 * (1 - t) + 30)
            ctx.fillStyle = `rgba(${rr},${gg},${bb},${0.22 + 0.4 * t})`
            ctx.fillRect(dx + c * cellW, dy + r * cellH, cellW + 0.5, cellH + 0.5)
          }
        }
      }

      // ---- zones layer ----
      if (show.zones) {
        for (const z of zonesRef.current) {
          const poly = z.polygon
          if (!poly || poly.length < 3) continue
          const count = tracks.filter(
            (t) => t.foot && pointInPolygon(t.foot.x, t.foot.y, poly)
          ).length
          const occupied = count > 0
          const editing = modeRef.current === 'edit' && editKindRef.current === 'zones'

          ctx.beginPath()
          poly.forEach((p, i) =>
            i === 0 ? ctx.moveTo(X(p[0]), Y(p[1])) : ctx.lineTo(X(p[0]), Y(p[1]))
          )
          ctx.closePath()
          ctx.fillStyle = occupied
            ? 'rgba(34, 197, 94, 0.22)'
            : editing
              ? 'rgba(148, 163, 184, 0.16)'
              : 'rgba(148, 163, 184, 0.07)'
          ctx.fill()
          ctx.lineWidth = editing ? 2.5 : 2
          ctx.strokeStyle = occupied ? 'rgba(34, 197, 94, 0.95)' : 'rgba(148, 163, 184, 0.7)'
          if (z.enabled === false || z.enabled === 0) ctx.setLineDash([6, 5])
          ctx.stroke()
          ctx.setLineDash([])

          if (editing) {
            ctx.fillStyle = 'rgba(148, 163, 184, 0.95)'
            for (const p of poly) {
              ctx.beginPath()
              ctx.arc(X(p[0]), Y(p[1]), 4, 0, Math.PI * 2)
              ctx.fill()
            }
          }

          const cx0 = poly.reduce((s, p) => s + p[0], 0) / poly.length
          const cy0 = poly.reduce((s, p) => s + p[1], 0) / poly.length
          const label = `${z.name} ${count > 0 ? `· ${count}` : ''}`
          ctx.font = 'bold 14px system-ui, sans-serif'
          const tw = ctx.measureText(label).width + 16
          ctx.fillStyle = occupied ? 'rgba(34, 197, 94, 0.9)' : 'rgba(15, 23, 42, 0.75)'
          ctx.fillRect(X(cx0) - tw / 2, Y(cy0) - 12, tw, 24)
          ctx.fillStyle = occupied ? '#04250f' : '#e2e8f0'
          ctx.textAlign = 'center'
          ctx.fillText(label, X(cx0), Y(cy0) + 5)
          ctx.textAlign = 'left'
        }
      }

      // ---- crossing lines layer ----
      for (const ln of linesRef.current) {
        const ax = X(ln.a[0]), ay = Y(ln.a[1]), bx = X(ln.b[0]), by = Y(ln.b[1])
        const st = crossingsRef.current.find((s) => s.line_id === ln.id)
        const editing = modeRef.current === 'edit' && editKindRef.current === 'lines'

        ctx.lineWidth = editing ? 3 : 2.5
        ctx.strokeStyle = 'rgba(245, 158, 11, 0.9)'
        ctx.setLineDash([10, 6])
        ctx.beginPath()
        ctx.moveTo(ax, ay)
        ctx.lineTo(bx, by)
        ctx.stroke()
        ctx.setLineDash([])
        // direction arrow at midpoint (a→b)
        const mx = (ax + bx) / 2, my = (ay + by) / 2
        const ang = Math.atan2(by - ay, bx - ax)
        ctx.fillStyle = 'rgba(245, 158, 11, 0.95)'
        ctx.beginPath()
        ctx.moveTo(mx + Math.cos(ang) * 9, my + Math.sin(ang) * 9)
        ctx.lineTo(mx + Math.cos(ang + 2.5) * 7, my + Math.sin(ang + 2.5) * 7)
        ctx.lineTo(mx + Math.cos(ang - 2.5) * 7, my + Math.sin(ang - 2.5) * 7)
        ctx.closePath()
        ctx.fill()
        // endpoints
        for (const [px, py] of [[ax, ay], [bx, by]]) {
          ctx.beginPath()
          ctx.arc(px, py, editing ? 5 : 4, 0, Math.PI * 2)
          ctx.fill()
        }
        // count badge above midpoint
        const label = `${ln.name}  ↑${st?.in_count ?? 0} ↓${st?.out_count ?? 0}`
        ctx.font = 'bold 14px system-ui, sans-serif'
        const tw = ctx.measureText(label).width + 16
        const off = 34
        const nx = Math.sin(ang), ny = -Math.cos(ang) // normal
        const bxPos = mx + nx * off, byPos = my + ny * off
        ctx.fillStyle = 'rgba(120, 53, 15, 0.85)'
        ctx.fillRect(bxPos - tw / 2, byPos - 12, tw, 24)
        ctx.fillStyle = '#fef3c7'
        ctx.textAlign = 'center'
        ctx.fillText(label, bxPos, byPos + 5)
        ctx.textAlign = 'left'
      }

      // ---- trails layer ----
      if (show.trails) {
        ctx.lineWidth = 2
        for (const t of tracks) {
          const trail: Array<{ x: number; y: number }> = (t as any).trail ?? []
          if (trail.length < 2) continue
          for (let i = 1; i < trail.length; i++) {
            const a = ((i - 1) / trail.length) * 0.75 + 0.1
            ctx.strokeStyle = `rgba(96, 165, 250, ${a.toFixed(2)})`
            ctx.beginPath()
            ctx.moveTo(X(trail[i - 1].x), Y(trail[i - 1].y))
            ctx.lineTo(X(trail[i].x), Y(trail[i].y))
            ctx.stroke()
          }
        }
      }

      // ---- AI overlay (people) ----
      // Both modes interpolate along the bbox history: stream-player mode
      // aligns to the displayed video time; device-frame mode targets "now"
      // so boxes keep moving smoothly between ~5 Hz device updates.
      const drawNow = performance.now() - (deviceFramesRef.current ? 0 : OVERLAY_DELAY_MS)
      const alignedTracks = tracks.map((tr) => {
        const hist = trackHistRef.current.get(tr.track_id)
        if (!tr.bbox || !hist || hist.length === 0) return tr
        const ib = bboxAt(hist, drawNow)
        if (!ib) return tr
        // affine (translate+scale around bbox center) mapping raw→interp so
        // skeleton points and feet ride along the interpolated box
        const cx = tr.bbox.x + tr.bbox.w / 2
        const cy = tr.bbox.y + tr.bbox.h / 2
        const sx = tr.bbox.w > 1e-6 ? ib.w / tr.bbox.w : 1
        const sy = tr.bbox.h > 1e-6 ? ib.h / tr.bbox.h : 1
        const map = (px: number, py: number) => ({
          x: cx + (px - cx) * sx + (ib.x + ib.w / 2 - cx),
          y: cy + (py - cy) * sy + (ib.y + ib.h / 2 - cy),
        })
        const kpts = tr.pose?.kpts?.map((k) => {
          const p = map(k[0], k[1])
          return [p.x, p.y, k[2]] as [number, number, number]
        })
        const foot = tr.foot ? map(tr.foot.x, tr.foot.y) : tr.foot
        return {
          ...tr,
          bbox: ib,
          pose: tr.pose && kpts ? { kpts, score: tr.pose.score } : tr.pose,
          foot,
        }
      })
      for (const track of alignedTracks) {
        if (track.bbox) {
          const { x, y, w, h } = track.bbox
          ctx.strokeStyle = 'rgba(250, 250, 250, 0.9)'
          ctx.lineWidth = 2
          ctx.strokeRect(X(x), Y(y), w * dw, h * dh)
          // member name when matched (P3) + live exercise/reps (P4)
          const ex = track.exercise
            ? track.exercise.reps > 0
              ? ` ${track.exercise.name}×${track.exercise.reps}`
              : track.exercise.name !== 'unknown'
                ? ` ${track.exercise.name}`
                : ''
            : ''
          const label = track.member?.name
            ? `${track.member.name} · #${track.track_id}${ex}`
            : `#${track.track_id}${ex}`
          ctx.font = 'bold 13px system-ui, sans-serif'
          const tw = ctx.measureText(label).width + 10
          ctx.fillStyle = track.member
            ? 'rgba(34, 197, 94, 0.9)'
            : 'rgba(250, 250, 250, 0.85)'
          ctx.fillRect(X(x), Math.max(0, Y(y) - 18), tw, 17)
          ctx.fillStyle = track.member ? '#04250f' : '#0a0a0a'
          ctx.fillText(label, X(x) + 5, Math.max(12, Y(y) - 6))
        }

        const kpts = track.pose?.kpts
        if (kpts && kpts.length > 0) {
          const pt = (i: number) => {
            const k = kpts[i]
            return k && k[2] > KPT_MIN_SCORE ? { x: X(k[0]), y: Y(k[1]) } : null
          }
          ctx.lineWidth = 2.5
          ctx.strokeStyle = 'rgba(59, 130, 246, 0.95)'
          ctx.beginPath()
          for (const [a, b] of SKELETON_EDGES) {
            const pa = pt(a)
            const pb = pt(b)
            if (!pa || !pb) continue
            ctx.moveTo(pa.x, pa.y)
            ctx.lineTo(pb.x, pb.y)
          }
          ctx.stroke()
          ctx.fillStyle = 'rgba(248, 250, 252, 0.95)'
          for (let i = 0; i < kpts.length; i++) {
            const p = pt(i)
            if (!p) continue
            ctx.beginPath()
            ctx.arc(p.x, p.y, 3.5, 0, Math.PI * 2)
            ctx.fill()
          }
        }

        if (track.foot) {
          ctx.fillStyle = 'rgba(245, 158, 11, 0.95)'
          ctx.beginPath()
          ctx.arc(X(track.foot.x), Y(track.foot.y), 5, 0, Math.PI * 2)
          ctx.fill()
        }
      }

      // ---- drafts (edit mode) ----
      if (modeRef.current === 'edit' && editKindRef.current === 'zones') {
        const d = draftRef.current
        if (d.length > 0) {
          ctx.beginPath()
          d.forEach((p, i) => (i === 0 ? ctx.moveTo(X(p[0]), Y(p[1])) : ctx.lineTo(X(p[0]), Y(p[1]))))
          if (d.length >= 3) {
            ctx.closePath()
            ctx.fillStyle = 'rgba(59, 130, 246, 0.15)'
            ctx.fill()
          }
          ctx.lineWidth = 2
          ctx.strokeStyle = 'rgba(59, 130, 246, 0.95)'
          ctx.setLineDash([8, 6])
          ctx.stroke()
          ctx.setLineDash([])
          ctx.fillStyle = '#3b82f6'
          for (const p of d) {
            ctx.beginPath()
            ctx.arc(X(p[0]), Y(p[1]), 5, 0, Math.PI * 2)
            ctx.fill()
          }
        }
      }
      if (modeRef.current === 'edit' && editKindRef.current === 'lines') {
        const d = draftLineRef.current
        if (d.length > 0) {
          ctx.lineWidth = 3
          ctx.strokeStyle = 'rgba(59, 130, 246, 0.95)'
          ctx.setLineDash([10, 6])
          ctx.beginPath()
          d.forEach((p, i) => (i === 0 ? ctx.moveTo(X(p[0]), Y(p[1])) : ctx.lineTo(X(p[0]), Y(p[1]))))
          if (d.length >= 2) ctx.lineTo(X(d[0][0]), Y(d[0][1]))
          ctx.stroke()
          ctx.setLineDash([])
          ctx.fillStyle = '#3b82f6'
          for (const p of d) {
            ctx.beginPath()
            ctx.arc(X(p[0]), Y(p[1]), 6, 0, Math.PI * 2)
            ctx.fill()
          }
          ctx.font = '12px system-ui, sans-serif'
          ctx.fillStyle = '#93c5fd'
          ctx.fillText(d.length === 1 ? '再点一点确定方向 (a→b)' : '', X(d[0][0]) + 10, Y(d[0][1]) - 8)
        }
      }

      // Redraw loop: rAF when the page composites; when rAF is throttled
      // (occluded/energy-saving tabs pause it entirely), a 250 ms interval
      // fallback keeps the canvas alive, and every arrived device frame
      // kicks an immediate draw (see device-frame poll).
      rafRef.current = requestAnimationFrame(draw)
      fallbackRef.current = window.setInterval(() => draw(), 250)
    }, [])

    // latest draw closure for external kicks (image onload)
    const drawRef = useRef<(() => void) | null>(null)

    useEffect(() => {
      rafRef.current = requestAnimationFrame(draw)
      drawRef.current = draw
      return () => {
        cancelAnimationFrame(rafRef.current)
        if (fallbackRef.current) { clearInterval(fallbackRef.current); fallbackRef.current = null }
      }
    }, [draw])

    // ---- canvas click routing ----
    const onCanvasClick = useCallback((e: React.MouseEvent<HTMLCanvasElement>) => {
      const canvas = canvasRef.current
      if (!canvas) return
      const rect = canvas.getBoundingClientRect()
      const scaleX = canvas.width / rect.width
      const scaleY = canvas.height / rect.height
      const K = canvas.width / 960 // virtual-space scale (draw uses setTransform)
      const cx = ((e.clientX - rect.left) * scaleX) / K
      const cy = ((e.clientY - rect.top) * scaleY) / K
      const { dx, dy, dw, dh } = layoutRef.current
      const nx = (cx - dx) / dw
      const ny = (cy - dy) / dh
      if (nx < 0 || nx > 1 || ny < 0 || ny > 1) return
      const p: number[] = [Number(nx.toFixed(4)), Number(ny.toFixed(4))]

      if (modeRef.current !== 'edit') {
        // View mode: click a person to register them as a member (P3).
        // Topmost (last-drawn) hit wins.
        const hit = [...(stateRef.current?.tracks ?? [])]
          .reverse()
          .find((t) => t.bbox && nx >= t.bbox.x && nx <= t.bbox.x + t.bbox.w
            && ny >= t.bbox.y && ny <= t.bbox.y + t.bbox.h)
        if (hit) {
          setRegister({ trackId: hit.track_id, name: '', busy: false, msg: null })
        }
        return
      }

      if (editKindRef.current === 'lines') {
        // two clicks complete a line: first = a, second = b
        setDraftLine((d) => {
          if (d.length === 0) return [p]
          const line: DraftLine = {
            id: crypto.randomUUID(),
            name: `Line ${linesRef.current.length + 1}`,
            a: d[0],
            b: p,
            isNew: true,
          }
          setLines((ls) => [...ls, line])
          return []
        })
      } else {
        setDraft((d) => [...d, p])
      }
    }, [])

    const closeDraft = useCallback(() => {
      if (draft.length < 3) return
      setZones((zs) => [
        ...zs,
        {
          id: crypto.randomUUID(),
          name: `Zone ${zs.length + 1}`,
          equipment_type: 'equipment',
          polygon: draft,
          enabled: true,
          isNew: true,
        },
      ])
      setDraft([])
    }, [draft])

    const undo = useCallback(() => {
      if (editKind === 'lines') setDraftLine((d) => d.slice(0, -1))
      else setDraft((d) => d.slice(0, -1))
    }, [editKind])

    const submitRegister = useCallback(async () => {
      setRegister((r) => (r ? { ...r, busy: true, msg: null } : r))
      if (!register) return
      // capture the head crop BEFORE the panel closes / track moves on
      const track = (stateRef.current?.tracks ?? []).find((t) => t.track_id === register.trackId)
      const headCrop = track?.bbox ? cropAvatar(track.bbox) : null
      const r = await registerMember(extensionId, register.trackId, register.name.trim())
      if (!mountedRef.current) return
      if (r.success) {
        if (headCrop && r.data?.member?.id) {
          await setMemberPhoto(extensionId, r.data.member.id, headCrop)
        }
        setRegister(null)
        setSavedFlash(Date.now())
        loadMembers()
      } else {
        setRegister((s) => (s ? { ...s, busy: false, msg: r.error || '注册失败' } : s))
      }
    }, [register, extensionId, loadMembers, cropAvatar])

    const removeMember = useCallback(async (id: string) => {
      await deleteMember(extensionId, id)
      if (mountedRef.current) loadMembers()
    }, [extensionId, loadMembers])

    const doMerge = useCallback(async (dstId: string) => {
      if (!merging || merging === dstId) return
      const r = await mergeMembers(extensionId, merging, dstId)
      if (mountedRef.current) {
        setMerging(null)
        if (r.success) { setSavedFlash(Date.now()); loadMembers() }
      }
    }, [merging, extensionId, loadMembers])

    const save = useCallback(async () => {
      setSaving(true)
      try {
        const zonePayload = zones.map((z) => ({
          id: z.id,
          name: z.name.trim() || `Zone ${z.id.slice(0, 4)}`,
          equipment_type: z.equipment_type,
          polygon: z.polygon,
          enabled: z.enabled === true || z.enabled === 1,
        }))
        const linePayload = lines.map((l) => ({
          id: l.id,
          name: l.name.trim() || `Line ${l.id.slice(0, 4)}`,
          a: l.a,
          b: l.b,
        }))
        const [zr, lr] = await Promise.all([
          runExtensionCommand(extensionId, 'set_roi_zones', { zones: zonePayload }),
          runExtensionCommand(extensionId, 'set_lines', { lines: linePayload }),
        ])
        if (!mountedRef.current) return
        if (zr.success && lr.success) {
          setSavedFlash(Date.now())
          loadZones()
          loadLines()
        }
      } finally {
        if (mountedRef.current) setSaving(false)
      }
    }, [zones, lines, extensionId, loadZones, loadLines])

    // ---- video session ----
    const stop = useCallback(() => {
      if (wsRef.current) {
        wsRef.current.close()
        wsRef.current = null
      }
      imgRef.current = null
      setStatus('idle')
      setVideoFps(0)
    }, [])

    const start = useCallback(() => {
      const url = String(sourceUrl || '').trim()
      if (!url) {
        setError('Configure sourceUrl (e.g. rtsp://host:8554/sub)')
        setStatus('error')
        return
      }
      stop()
      setError(null)
      setStatus('connecting')

      const isTauri = !!(window as any).__TAURI_INTERNALS__
      const proto = (isTauri ? false : window.location.protocol === 'https:') ? 'wss:' : 'ws:'
      const host = isTauri ? 'localhost:9375' : window.location.host
      let wsUrl = `${proto}//${host}/api/extensions/${VIDEO_EXTENSION_ID}/stream`
      const token = getToken()
      if (token) wsUrl += `?token=${encodeURIComponent(token)}`

      const ws = new WebSocket(wsUrl)
      ws.binaryType = 'arraybuffer'
      wsRef.current = ws

      ws.onopen = () => {
        ws.send(
          JSON.stringify({
            type: 'init',
            config: {
              source_url: url,
              target_fps: fps,
              output_width: outputWidth,
              output_height: Math.round((outputWidth * 9) / 16),
              loop_file: true,
            },
          })
        )
      }

      ws.onmessage = (event) => {
        if (typeof event.data !== 'string') return
        try {
          const msg = JSON.parse(event.data)
          if (msg.type === 'session_created') {
            ws.send(JSON.stringify({ type: 'start_push', session_id: msg.session_id }))
          } else if (msg.type === 'push_output') {
            if (msg.data_type === 'image/jpeg' && msg.data) {
              setStatus((s) => (s === 'streaming' ? s : 'streaming'))
              const im = new Image()
              im.onload = () => {
                imgRef.current = im
                const c = fpsCounterRef.current
                c.frames++
                const now = Date.now()
                if (now - c.last >= 1000) {
                  setVideoFps(Math.round((c.frames * 1000) / (now - c.last)))
                  c.frames = 0
                  c.last = now
                }
              }
              im.src = `data:image/jpeg;base64,${msg.data}`
            } else if (msg.data_type === 'application/json' && msg.data) {
              try {
                const s = typeof msg.data === 'string' ? JSON.parse(msg.data) : msg.data
                if (s?.type === 'error') {
                  setError(s.message || 'Stream error')
                  setStatus('error')
                }
              } catch { /* ignore */ }
            }
          } else if (msg.type === 'error') {
            setError(`${msg.code || 'Error'}: ${msg.message || 'unknown'}`)
            setStatus('error')
          }
        } catch { /* ignore */ }
      }

      ws.onerror = () => {
        setError('WebSocket connection failed')
        setStatus('error')
      }

      ws.onclose = () => {
        if (mountedRef.current && wsRef.current === ws) {
          wsRef.current = null
          setStatus((s) => (s === 'error' ? s : 'idle'))
        }
      }
    }, [sourceUrl, fps, outputWidth, stop])

    useEffect(() => {
      if (String(sourceUrl || '').trim()) start()
      return stop
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [sourceUrl, fps, outputWidth])

    const dirty = zones.some((z) => z.isNew) || lines.some((l) => l.isNew)
    const editing = mode === 'edit'

    return (
      <div ref={ref} className={`gym-ov ${className}`}>
        <div className="gym-ov-card">
          <div className="gym-ov-header">
            <div className="gym-ov-title">
              <span>Gym · Monitor</span>
            </div>
            <div className="gym-ov-status">
              <span className={`gym-ov-badge ${status}`}>
                {status === 'streaming' ? `${videoFps} fps` : status}
              </span>
              <span className="gym-ov-badge people">
                {present} {present === 1 ? 'person' : 'people'}
              </span>
              {!editing && (
                <>
                  <button
                    className={`gym-ov-btn ${showTrails ? 'on' : ''}`}
                    onClick={() => setShowTrails((v) => !v)}
                  >拖尾</button>
                  <button
                    className={`gym-ov-btn ${showZones ? 'on' : ''}`}
                    onClick={() => setShowZones((v) => !v)}
                  >分区</button>
                  <button
                    className={`gym-ov-btn ${showHeatmap ? 'on' : ''}`}
                    onClick={() => setShowHeatmap((v) => !v)}
                  >热力</button>
                  <button
                    className={`gym-ov-btn ${mosaic ? 'on' : ''}`}
                    title="人脸隐私打码"
                    onClick={() => setMosaic((v) => !v)}
                  >打码</button>
                  <button className="gym-ov-btn" onClick={() => setMode('edit')}>
                    编辑
                  </button>
                </>
              )}
              {editing && (
                <>
                  {savedFlash > 0 && Date.now() - savedFlash < 3000 && (
                    <span className="gym-ov-badge ok">saved ✓</span>
                  )}
                  <div className="gym-ov-seg">
                    <button className={`gym-ov-btn ${editKind === 'zones' ? 'on' : ''}`}
                      onClick={() => { setEditKind('zones'); setDraftLine([]) }}>分区</button>
                    <button className={`gym-ov-btn ${editKind === 'lines' ? 'on' : ''}`}
                      onClick={() => { setEditKind('lines'); setDraft([]) }}>画线</button>
                    <button className={`gym-ov-btn ${editKind === 'members' ? 'on' : ''}`}
                      onClick={() => { setEditKind('members'); setDraft([]); setDraftLine([]); loadMembers() }}>会员</button>
                  </div>
                  {editKind !== 'members' && (
                    <>
                      <button className="gym-ov-btn" onClick={undo}
                        disabled={editKind === 'lines' ? draftLine.length === 0 : draft.length === 0}>
                        撤销
                      </button>
                      {editKind === 'zones' && (
                        <button className="gym-ov-btn" onClick={closeDraft} disabled={draft.length < 3}>
                          闭合 ({draft.length})
                        </button>
                      )}
                    </>
                  )}
                  <button className="gym-ov-btn primary" onClick={save} disabled={saving}>
                    {saving ? '保存中…' : dirty ? '保存 *' : '保存'}
                  </button>
                  <button className="gym-ov-btn" onClick={() => {
                    setMode('view'); setDraft([]); setDraftLine([])
                  }}>完成</button>
                </>
              )}
            </div>
          </div>

          <div className="gym-ov-body">
            <canvas
              ref={canvasRef}
              className={`gym-ov-canvas ${editing ? 'editing' : ''}`}
              onClick={onCanvasClick}
            />
            {status !== 'streaming' && (
              <div className="gym-ov-veil">
                {status === 'connecting' ? (
                  <>
                    <div className="gym-live-spinner" />
                    <span>Connecting to {sourceUrl || '…'} …</span>
                  </>
                ) : status === 'error' ? (
                  <span className="gym-ov-error">{error || 'Stream error'}</span>
                ) : (
                  <span>Set sourceUrl in the component config</span>
                )}
              </div>
            )}
            {editing && (
              <div className="gym-ov-edit-tip">
                {editKind === 'zones'
                  ? '点击画面添加分区顶点（任意多边形，≥3 点）→「闭合」→ 下方命名 → 保存'
                  : editKind === 'lines'
                    ? '点击两点画计数线：第一点 a → 第二点 b（a→b 为方向基准，↑=向左穿入）'
                    : `会员库（${members.length} 人）——新人自动录入编号，选中姓名即可补填真名；回车保存`}
              </div>
            )}
            {!editing && register && (
              <div className="gym-ov-register">
                <span className="gym-ov-register-title">
                  注册会员 · 轨迹 #{register.trackId}
                </span>
                <input
                  className="gym-ov-input name"
                  autoFocus
                  placeholder="会员姓名"
                  value={register.name}
                  onChange={(e) =>
                    setRegister((r) => (r ? { ...r, name: e.target.value } : r))
                  }
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' && register.name.trim() && !register.busy) submitRegister()
                    if (e.key === 'Escape') setRegister(null)
                  }}
                />
                <button
                  className="gym-ov-btn primary"
                  disabled={!register.name.trim() || register.busy}
                  onClick={submitRegister}
                >{register.busy ? '注册中…' : '注册'}</button>
                <button className="gym-ov-btn" onClick={() => setRegister(null)}>取消</button>
                {register.msg && <span className="gym-ov-register-err">{register.msg}</span>}
              </div>
            )}
          </div>

          {editing && (
            <div className="gym-ov-zonelist">
              {editKind === 'zones'
                ? (zones.length === 0
                    ? [<span key="e" className="gym-ov-zonelist-empty">还没有分区——在画面上点出第一块器械区</span>]
                    : zones.map((z, i) => (
                        <div key={z.id} className="gym-ov-zonerow">
                          <span className="gym-ov-zoneidx">{i + 1}</span>
                          <input className="gym-ov-input name" value={z.name}
                            onChange={(e) =>
                              setZones((zs) => zs.map((x) => (x.id === z.id ? { ...x, name: e.target.value } : x)))
                            } />
                          <input className="gym-ov-input type" value={z.equipment_type}
                            placeholder="器械类型"
                            onChange={(e) =>
                              setZones((zs) => zs.map((x) => (x.id === z.id ? { ...x, equipment_type: e.target.value } : x)))
                            } />
                          <button className="gym-ov-btn danger"
                            onClick={() => setZones((zs) => zs.filter((x) => x.id !== z.id))}>删除</button>
                        </div>
                      )))
                : (editKind === 'lines'
                    ? (lines.length === 0
                        ? [<span key="e" className="gym-ov-zonelist-empty">还没有计数线——在画面上点两点画出出入口线</span>]
                        : lines.map((l, i) => {
                            const st = crossings.find((s) => s.line_id === l.id)
                            return (
                              <div key={l.id} className="gym-ov-zonerow">
                                <span className="gym-ov-zoneidx line">{i + 1}</span>
                                <input className="gym-ov-input name" value={l.name}
                                  onChange={(e) =>
                                    setLines((ls) => ls.map((x) => (x.id === l.id ? { ...x, name: e.target.value } : x)))
                                  } />
                                <span className="gym-ov-linecount">
                                  ↑{st?.in_count ?? 0} ↓{st?.out_count ?? 0}
                                </span>
                                <button className="gym-ov-btn danger"
                                  onClick={() => setLines((ls) => ls.filter((x) => x.id !== l.id))}>删除</button>
                              </div>
                            )
                          }))
                    : (members.length === 0
                        ? [<span key="e" className="gym-ov-zonelist-empty">会员库为空——新人入画会自动录入特征；也可在查看模式点击人物注册</span>]
                        : members.map((m, i) => (
                            <div key={m.id} className="gym-ov-zonerow">
                              {memberPhotoSrc(m.photo) ? (
                                <img className="gym-ov-avatar" src={memberPhotoSrc(m.photo)!}
                                  alt={m.name} title={m.name} />
                              ) : (
                                <span className="gym-ov-zoneidx member">{i + 1}</span>
                              )}
                              <input className="gym-ov-input name" defaultValue={m.name}
                                placeholder={m.source === 'auto' ? '补填姓名' : '会员姓名'}
                                onKeyDown={(e) => {
                                  if (e.key === 'Enter') (e.target as HTMLInputElement).blur()
                                }}
                                onBlur={async (e) => {
                                  const v = e.target.value.trim()
                                  if (v && v !== m.name) {
                                    await renameMember(extensionId, m.id, v)
                                    if (mountedRef.current) loadMembers()
                                  }
                                }} />
                              <span className="gym-ov-linecount">
                                {m.source === 'auto' ? '自动' : '手动'} · {m.samples ?? 1}样本
                              </span>
                              {merging === m.id ? (
                                <select
                                  className="gym-ov-input type"
                                  autoFocus
                                  value=""
                                  onChange={(e) => { if (e.target.value) doMerge(e.target.value) }}
                                  onBlur={() => setMerging(null)}
                                >
                                  <option value="">并入哪位会员？</option>
                                  {members.filter((x) => x.id !== m.id).map((x) => (
                                    <option key={x.id} value={x.id}>{x.name}</option>
                                  ))}
                                </select>
                              ) : (
                                <button className="gym-ov-btn" title="把此人的特征并入另一位会员（换装确认）"
                                  onClick={() => { setMerging(m.id); setSavedFlash(0) }}>并入</button>
                              )}
                              <button className="gym-ov-btn danger"
                                onClick={() => removeMember(m.id)}>删除</button>
                            </div>
                          ))))}
            </div>
          )}
        </div>
      </div>
    )
  }
)

GymVideoOverlay.displayName = 'GymVideoOverlay'
