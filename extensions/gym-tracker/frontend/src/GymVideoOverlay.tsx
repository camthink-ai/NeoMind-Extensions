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
  FRAME_DATA_TYPE,
  AVC_DATA_TYPE,
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
import { GymSelect } from './GymSelect'

const STYLE_ID = 'gym-monitor-styles-v1'

type Status = 'idle' | 'connecting' | 'streaming' | 'error'
type EditKind = 'zones' | 'lines' | 'members' | 'exclude'

const KPT_MIN_SCORE = 0.2

// Mosaic cell size in canvas px — coarse enough to obscure identity, fine
// enough to still read "there is a face here".
const MOSAIC_CELL = 14
// Face boxes arrive at the detect cadence (every Nth frame); pad each box
// so a head moving between detections stays covered.
const MOSAIC_PAD = 0.12

// Zone equipment presets — canonical equipment_type values that hit the
// exercise-classification map in exercise.rs (zone_exercise). Chinese
// labels for the editor; values feed analytics.
const EQUIPMENT_PRESETS: Array<[string, string]> = [
  ['跑步机', 'treadmill'],
  ['椭圆机', 'elliptical'],
  ['动感单车', 'spin_bike'],
  ['划船机', 'rowing'],
  ['爬楼机', 'stair_climber'],
  ['深蹲架', 'squat_rack'],
  ['卧推凳', 'bench'],
  ['硬拉台', 'deadlift_platform'],
  ['单杠', 'pullup_bar'],
  ['龙门架', 'cable_machine'],
  ['夹胸机', 'chest_fly_machine'],
  ['练腿架', 'leg_press'],
  ['瑜伽垫', 'mat'],
  ['壶铃区', 'kettlebell'],
  ['哑铃区', 'dumbbell'],
  ['自由重量', 'free_weights'],
  ['其他', 'equipment'],
]

// ---- overlay/video time alignment ----
// The video path (RTSP/file → decode → JPEG → WS) lags the analytics path by
// Video arrives at ~4-5 Hz (~200 ms between frames) while the pose loop
// runs a touch faster, so track history USUALLY straddles any shown frame.
// The rule that keeps boxes glued to the picture: interpolate at the shown
// frame's own timestamp (see draw()). PLAY_DELAY is only an out-of-order
// guard — the smallest value that keeps frame order stable without
// deliberately ageing the video (a large fixed delay is perceived as lag).
const PLAY_DELAY = 0.03 // sec
// Beyond the newest track sample the rig extrapolates along its last
// velocities. The cap must cover the real inference-to-preview gap
// (~150-350 ms with the m tile model); below it the overlay clamps and
// visibly trails the person.
const EXTRAP_MAX_MS = 0.5 // sec

// ---- binary push frames (double-base64 killer) ----
// Platform wire format, opt-in via init config `{"binary":true}`:
//   [kind u8=1][ver u8=1][seq u64 BE][meta_len u32 BE][meta JSON][payload]
// For gym-tracker the payload is the app container:
//   [meta_len u32 BE][meta JSON (the FrameBundle minus img_b64)][JPEG bytes]
// Text sessions (legacy servers) carry the same container base64-wrapped in
// push_output.data — parseFrameContainer serves both legs unchanged.
const BIN_KIND_PUSH = 1
const BIN_VERSION = 1
const BIN_HEADER_LEN = 14

const textDecoder = new TextDecoder()

function bytesFromB64(b64: string): Uint8Array | null {
  try {
    const bin = atob(b64)
    const bytes = new Uint8Array(bin.length)
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i)
    return bytes
  } catch {
    return null
  }
}

/** Split an `application/x-neomind-frame` container into bundle + JPEG. */
function parseFrameContainer(bytes: Uint8Array): { bundle: FrameBundle; jpeg: Uint8Array } | null {
  if (bytes.length < 4) return null
  const metaLen = ((bytes[0] << 24) | (bytes[1] << 16) | (bytes[2] << 8) | bytes[3]) >>> 0
  if (4 + metaLen > bytes.length) return null
  try {
    const bundle = JSON.parse(textDecoder.decode(bytes.subarray(4, 4 + metaLen))) as FrameBundle
    return { bundle, jpeg: bytes.subarray(4 + metaLen) }
  } catch {
    return null
  }
}

/** Decode a server→client binary push frame → (platform meta, payload). */
function parseBinaryPushFrame(buf: ArrayBuffer): { meta: Record<string, any>; payload: Uint8Array; seq: number } | null {
  if (buf.byteLength < BIN_HEADER_LEN) return null
  const v = new DataView(buf)
  const kind = v.getUint8(0)
  const version = v.getUint8(1)
  if (kind !== BIN_KIND_PUSH || version !== BIN_VERSION) return null
  const seq = Number(v.getBigUint64(2))
  const metaLen = v.getUint32(10)
  if (BIN_HEADER_LEN + metaLen > buf.byteLength) return null
  try {
    const meta = JSON.parse(textDecoder.decode(new Uint8Array(buf, BIN_HEADER_LEN, metaLen)))
    return { meta, payload: new Uint8Array(buf, BIN_HEADER_LEN + metaLen), seq }
  } catch {
    return null
  }
}

// ---- hardware H.264 preview (WebCodecs) ----
// The device relays the vc8000e hardware encoder's sub stream as Annex-B
// access units (`video/avc` frames, same container layout). WebCodecs wants
// AVCC (length-prefixed NALs) plus an avcC description — both derived
// client-side from the first keyframe. The low-rate JPEG frames keep
// arriving as fallback and take over automatically whenever the decoder is
// unavailable, errored or starved.

/** Split one Annex-B access unit into NAL units (start codes stripped). */
function splitAnnexB(data: Uint8Array): Uint8Array[] {
  const starts: number[] = []
  for (let i = 0; i + 2 < data.length; i++) {
    if (data[i] === 0 && data[i + 1] === 0 && data[i + 2] === 1) {
      starts.push(i)
      i += 2
    }
  }
  const nals: Uint8Array[] = []
  for (let k = 0; k < starts.length; k++) {
    // EXACT bytes — no trailing-zero trimming: cabac_zero_words are part
    // of the bitstream and stripping them corrupts decode.
    const s = starts[k] + 3
    const end = k + 1 < starts.length ? starts[k + 1] : data.length
    if (end > s) nals.push(data.subarray(s, end))
  }
  return nals
}

/** Re-pack one Annex-B access unit as AVCC (4-byte BE length prefixes). */
function annexBToAvcc(data: Uint8Array): Uint8Array | null {
  const nals = splitAnnexB(data)
  if (!nals.length) return null
  let len = 0
  for (const n of nals) len += 4 + n.length
  const out = new Uint8Array(len)
  let o = 0
  for (const n of nals) {
    out[o++] = (n.length >>> 24) & 0xff
    out[o++] = (n.length >>> 16) & 0xff
    out[o++] = (n.length >>> 8) & 0xff
    out[o++] = n.length & 0xff
    out.set(n, o)
    o += n.length
  }
  return out
}

/** Extract SPS+PPS from a keyframe → avcC record + codec string. */
function buildAvcC(data: Uint8Array): { codec: string; avcC: Uint8Array } | null {
  let sps: Uint8Array | null = null
  let pps: Uint8Array | null = null
  for (const n of splitAnnexB(data)) {
    const type = n[0] & 0x1f
    if (type === 7 && !sps) sps = n
    else if (type === 8 && !pps) pps = n
  }
  if (!sps || !pps || sps.length < 4) return null
  const hex = (b: number) => b.toString(16).padStart(2, '0')
  const codec = `avc1.${hex(sps[1])}${hex(sps[2])}${hex(sps[3])}`
  const avcC = new Uint8Array(11 + sps.length + pps.length)
  avcC[0] = 1
  avcC[1] = sps[1]
  avcC[2] = sps[2]
  avcC[3] = sps[3]
  avcC[4] = 0xfc | 3 // lengthSizeMinusOne = 3 → 4-byte lengths
  avcC[5] = 0xe0 | 1 // numSPS
  avcC[6] = sps.length >> 8
  avcC[7] = sps.length & 0xff
  avcC.set(sps, 8)
  avcC[8 + sps.length] = 1 // numPPS
  avcC[9 + sps.length] = pps.length >> 8
  avcC[10 + sps.length] = pps.length & 0xff
  avcC.set(pps, 11 + sps.length)
  return { codec, avcC }
}

/** In-order H.264 decode pump. Configures lazily on the first keyframe
 *  (SPS/PPS ride in-band); resets — and waits for the NEXT keyframe — on
 *  error or queue overrun, so a stalled consumer degrades to the JPEG
 *  fallback instead of lagging forever. */
class H264Decoder {
  private dec: VideoDecoder | null = null
  private metaQueue: Array<Record<string, any> | undefined> = []
  private lastPts = 0
  onFrame: ((frame: VideoFrame, meta: Record<string, any>) => void) | null = null

  get active(): boolean {
    return this.dec != null
  }

  feed(meta: Record<string, any>, nalu: Uint8Array): void {
    if (typeof VideoDecoder === 'undefined' || typeof EncodedVideoChunk === 'undefined') return
    if (!this.dec) {
      if (!meta.key) return // mid-GOP join — wait for a keyframe
      const cfg = buildAvcC(nalu)
      if (!cfg) return
      try {
        this.dec = new VideoDecoder({
          output: (frame) => {
            const m = this.metaQueue.shift()
            if (m) this.onFrame?.(frame, m)
            else frame.close()
          },
          error: () => this.reset(),
        })
        this.dec.configure({
          codec: cfg.codec,
          description: cfg.avcC as unknown as BufferSource,
          optimizeForLatency: true,
        })
      } catch {
        this.dec = null
        return
      }
    }
    if (this.dec.decodeQueueSize > 60) {
      this.reset() // consumer stalled — resync on the next keyframe
      return
    }
    const avcc = annexBToAvcc(nalu)
    if (!avcc) return
    // timestamps must strictly increase — clamp encoder hiccups
    let pts = Math.floor((meta.pts_ns || meta.ts_ns || 0) / 1000)
    if (pts <= this.lastPts) pts = this.lastPts + 1
    this.lastPts = pts
    this.metaQueue.push(meta)
    try {
      this.dec.decode(
        new EncodedVideoChunk({
          type: meta.key ? 'key' : 'delta',
          timestamp: pts,
          data: avcc as unknown as BufferSource,
        })
      )
    } catch {
      this.metaQueue.pop()
    }
  }

  reset(): void {
    this.metaQueue.length = 0
    if (this.dec) {
      try { this.dec.close() } catch { /* already closed */ }
      this.dec = null
    }
  }
}

interface HistEntry { t: number; bbox: Bbox; foot?: Point | null; kpts?: [number, number, number][] | null; vel?: [number, number] | null }

/** Interpolated bbox + keypoints + foot at device-time `target` from the
 *  per-track history. Beyond the newest sample the bbox extrapolates
 *  linearly (velocity from the last span, EXTRAP_MAX_MS cap) while limbs
 *  hold their newest pose — extrapolated skeletons flail. */
function sampleAt(hist: HistEntry[], target: number, kptMin: number):
  { bbox: Bbox; kpts?: [number, number, number][]; foot?: Point | null } | null {
  if (hist.length === 0) return null
  const first = hist[0]
  const last = hist[hist.length - 1]
  let a: HistEntry, b: HistEntry, f: number
  // extrapolating: target is newer than the newest sample (the inference
  // chain runs ~150-350 ms behind the preview stream)
  let beyond = 0
  if (target <= first.t) {
    a = first
    b = hist[Math.min(1, hist.length - 1)]
    f = 0
  } else if (target >= last.t) {
    const p = hist.length >= 2 ? hist[hist.length - 2] : last
    const span = Math.max(1, last.t - p.t)
    beyond = Math.min(target - last.t, EXTRAP_MAX_MS)
    a = p
    b = last
    f = 1 + beyond / span
    // device-supplied velocity (normalized/sec, EMA over many frames)
    // beats the noisy two-point span slope when available; blend 70/30
    // toward it (the span slope still carries the newest acceleration)
    if (b.vel) {
      const s = Math.max(1e-6, span)
      const [vx, vy] = b.vel
      // express the device velocity as an equivalent factor: where the
      // last span's slope would land vs. vel*span — blend 70/30 toward
      // the device value (span slope carries the newest acceleration)
      const slopeX = (b.bbox.x - a.bbox.x) / s
      const slopeY = (b.bbox.y - a.bbox.y) / s
      const bx = 0.3 * slopeX + 0.7 * vx
      const by = 0.3 * slopeY + 0.7 * vy
      const cx0 = b.bbox.x + b.bbox.w / 2
      const cy0 = b.bbox.y + b.bbox.h / 2
      const ex = Math.min(0.15, Math.max(-0.15, bx * beyond))
      const ey = Math.min(0.15, Math.max(-0.15, by * beyond))
      const bbox = {
        x: b.bbox.x + ex,
        y: b.bbox.y + ey,
        w: b.bbox.w,
        h: b.bbox.h,
      }
      const dxBody = ex
      const dyBody = ey
      const MAX_KPT_ADV = 0.12
      const kb = b.kpts
      let kpts: [number, number, number][] | undefined
      if (kb && kb.length) {
        const clamp01v = (v: number) => Math.min(1, Math.max(0, v))
        kpts = kb.map((k2) => [
          clamp01v(k2[0] + dxBody),
          clamp01v(k2[1] + dyBody),
          k2[2],
        ] as [number, number, number])
      }
      const foot = b.foot
        ? {
            x: Math.min(1, Math.max(0, b.foot.x + ex)),
            y: Math.min(1, Math.max(0, b.foot.y + ey)),
          }
        : b.foot
      return { bbox, kpts, foot }
    }
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
  const lerp = (pa: number, pb: number) => pa + (pb - pa) * f
  const bbox = {
    x: lerp(a.bbox.x, b.bbox.x),
    y: lerp(a.bbox.y, b.bbox.y),
    w: lerp(a.bbox.w, b.bbox.w),
    h: lerp(a.bbox.h, b.bbox.h),
  }
  const clamp01v = (v: number) => Math.min(1, Math.max(0, v))
  // bbox-center advance over `beyond` — the fallback translation for any
  // held point so the whole rig rides the extrapolation instead of the
  // skeleton standing still while the box walks ahead
  const dxBody = beyond > 0 ? bbox.x + bbox.w / 2 - (b.bbox.x + b.bbox.w / 2) : 0
  const dyBody = beyond > 0 ? bbox.y + bbox.h / 2 - (b.bbox.y + b.bbox.h / 2) : 0
  const MAX_KPT_ADV = 0.12 // normalized clamp per extrapolated point
  let kpts: [number, number, number][] | undefined
  const ka = a.kpts
  const kb = b.kpts
  if (kb && kb.length) {
    const span = Math.max(1, b.t - a.t)
    kpts = kb.map((k2, i) => {
      const k1 = ka && ka[i]
      if (k1 && k1[2] > kptMin && k2[2] > kptMin) {
        if (beyond > 0) {
          // true per-point extrapolation: own velocity over the last span,
          // clamped — limbs that were moving keep moving
          const vx = (k2[0] - k1[0]) / span
          const vy = (k2[1] - k1[1]) / span
          return [
            clamp01v(k2[0] + Math.max(-MAX_KPT_ADV, Math.min(MAX_KPT_ADV, vx * beyond))),
            clamp01v(k2[1] + Math.max(-MAX_KPT_ADV, Math.min(MAX_KPT_ADV, vy * beyond))),
            k2[2],
          ] as [number, number, number]
        }
        return [lerp(k1[0], k2[0]), lerp(k1[1], k2[1]), k2[2]] as [number, number, number]
      }
      // held point (invisible on one side): translate with the body
      return [clamp01v(k2[0] + dxBody), clamp01v(k2[1] + dyBody), k2[2]] as [number, number, number]
    })
  }
  let foot: Point | null | undefined
  if (a.foot && b.foot) {
    if (beyond > 0) {
      const fx = (b.foot.x - a.foot.x) / Math.max(1, b.t - a.t)
      const fy = (b.foot.y - a.foot.y) / Math.max(1, b.t - a.t)
      foot = {
        x: clamp01v(b.foot.x + Math.max(-0.1, Math.min(0.1, fx * beyond))),
        y: clamp01v(b.foot.y + Math.max(-0.1, Math.min(0.1, fy * beyond))),
      }
    } else {
      foot = { x: lerp(a.foot.x, b.foot.x), y: lerp(a.foot.y, b.foot.y) }
    }
  } else {
    foot = b.foot
      ? { x: clamp01v(b.foot.x + dxBody), y: clamp01v(b.foot.y + dyBody) }
      : a.foot
  }
  return { bbox, kpts, foot }
}

interface DraftZone extends Zone {
  isNew?: boolean
}
interface DraftLine extends LineDef {
  isNew?: boolean
}

/** One in-progress edit drag. `orig` holds the pre-drag geometry so moves
 *  are recomputed from it (no compounding drift), and `snap` the full zones
 *  array for 撤销. */
type DragState =
  | { kind: 'vertex'; zoneId: string; idx: number; snap: DraftZone[] }
  | { kind: 'mid-insert'; zoneId: string; after: number; snap: DraftZone[] }
  | { kind: 'poly'; zoneId: string; orig: number[][]; nx0: number; ny0: number; snap: DraftZone[] }
  | { kind: 'draft-pt'; idx: number }
  | { kind: 'line-end'; lineId: string; end: 'a' | 'b'; snap: DraftLine[] }
  | { kind: 'line-move'; lineId: string; orig: [number[], number[]]; nx0: number; ny0: number; snap: DraftLine[] }

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
      config,
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

    const [showTrails, setShowTrails] = useState(false)
    const [showBoxes, setShowBoxes] = useState(config?.showBoxes !== false)
    const [showPose, setShowPose] = useState(config?.showSkeleton !== false)
    const [showZones, setShowZones] = useState(true)
    const [showHeatmap, setShowHeatmap] = useState(false)
    const [mosaic, setMosaic] = useState(true)
    const [heat, setHeat] = useState<HeatmapData | null>(null)
    // zone/line selected from the edit list — gets a highlight on canvas
    // (and, in the compact list below, expands the single inspector row)
    const [selZoneId, setSelZoneId] = useState<string | null>(null)
    const [selLineId, setSelLineId] = useState<string | null>(null)
    const [selMemberId, setSelMemberId] = useState<string | null>(null)
    // right-side edit-list drawer (card-anchored); collapsible to free the
    // canvas while drawing
    const [listOpen, setListOpen] = useState(true)

    const [saving, setSaving] = useState(false)
    // wipe guard: set_roi_zones/set_lines are full-replace, so a save with
    // an EMPTY list erases everything. If the local list is empty but the
    // user deleted nothing this session, the emptiness is a load failure /
    // remount artifact — skip that write instead of wiping the library.
    const deletedRef = useRef({ zone: false, line: false })
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
    const selZoneRef = useRef<string | null>(null)
    // live polygon/line drag (edit mode): see onPointerDown
    const dragRef = useRef<{ st: DragState; moved: boolean; sx: number; sy: number } | null>(null)
    // a completed drag sets this; the next click must not add a draft point
    const suppressClickRef = useRef(false)
    // geometry snapshot taken when a drag/vertex-delete starts — 撤销 restores
    // it once the draft stacks are empty
    const lastDragSnapRef = useRef<{ zones?: DraftZone[]; lines?: DraftLine[] } | null>(null)
    const showRef = useRef({ trails: true, boxes: true, pose: true, zones: true, heat: false, mosaic: true })
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
    const trackHistRef = useRef<Map<number, HistEntry[]>>(new Map())

    useEffect(() => { zonesRef.current = zones }, [zones])
    useEffect(() => { linesRef.current = lines }, [lines])
    useEffect(() => { crossingsRef.current = crossings }, [crossings])
    useEffect(() => { draftRef.current = draft }, [draft])
    useEffect(() => { draftLineRef.current = draftLine }, [draftLine])
    useEffect(() => { modeRef.current = mode }, [mode])
    useEffect(() => { editKindRef.current = editKind }, [editKind])
    useEffect(() => { showRef.current = { trails: showTrails, boxes: showBoxes, pose: showPose, zones: showZones, heat: showHeatmap, mosaic } }, [showTrails, showBoxes, showPose, showZones, showHeatmap, mosaic])
    useEffect(() => { selZoneRef.current = selZoneId }, [selZoneId])
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
          const now = performance.now() / 1000
          const hist = trackHistRef.current
          const seen = new Set<number>()
          for (const t of r.data.tracks ?? []) {
            if (!t.bbox) continue
            seen.add(t.track_id)
            let arr = hist.get(t.track_id)
            if (!arr) { arr = []; hist.set(t.track_id, arr) }
            arr.push({ t: now, bbox: t.bbox, foot: t.foot })
            while (arr.length > 0 && now - arr[0].t > 3) arr.shift()
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
    // device ts_ns of the last applied frame — the dedup key for binary
    // frames (no img_b64 string to compare against)
    const lastFrameTsNsRef = useRef<number | null>(null)
    // ts_ns (seconds) of the currently displayed device frame, when present
    const lastTsRef = useRef<number | null>(null)
    // TRUE ts (sec) of the latest track keyframe — drives the adaptive video
    // delay so the shown frame lands behind it (interpolation, not extrapolation)
    const lastTracksTsRef = useRef(0)
    const emaGapRef = useRef<number | null>(null)
    // rolling window of recent video-vs-tracks gaps — the delay derives
    // from the window MAX, not an EMA: the gap oscillates by up to one
    // inference period (EMA sits mid-range) and a mid-range delay leaves
    // extrapolation windows of up to half a period — the residual lag
    // users still saw. Max+margin guarantees the playhead stays behind
    // the newest track sample, i.e. pure interpolation, always.
    const gapWinRef = useRef<number[]>([])
    // jitter buffer: decoded frames (ts sec, ImageBitmap | VideoFrame |
    // HTMLImageElement) in arrival order; bitmaps/frames are closed on
    // eviction (GPU-backed memory)
    const frameBufRef = useRef<Array<{ t: number; img: HTMLImageElement | ImageBitmap | VideoFrame }>>([])
    // hardware H.264 decode state (video/avc frames)
    const h264Ref = useRef<H264Decoder | null>(null)
    const h264ActiveRef = useRef(false)
    const lastPushSeqRef = useRef<number | null>(null)
    // wall-clock anchor for the encoder PTS timeline (pts_ns has an
    // arbitrary origin; ts_ns is the device wall clock). Slowly adapted
    // EMA so systematic relay-latency changes are followed, read-jitter
    // is smoothed away.
    const ptsAnchorRef = useRef<number | null>(null)

    // current interpolation delay (sec) — written by draw(), read by the
    // jitter-buffer eviction so retention always covers the playhead
    const delayEstRef = useRef(1.5)
    const closeFrameImg = (img: HTMLImageElement | ImageBitmap | VideoFrame) => {
      if (img instanceof ImageBitmap) img.close()
      else if (typeof VideoFrame !== 'undefined' && img instanceof VideoFrame) img.close()
    }

    /** Push one decoded frame into the jitter buffer (called async after
     * bitmap/VideoFrame decode resolves — ordering tolerance matches the old
     * Image.onload behavior; the draw loop picks by ts, not index). */
    const pushDecodedFrame = useCallback((frameTs: number, img: HTMLImageElement | ImageBitmap | VideoFrame) => {
      if (!mountedRef.current) return
      const buf = frameBufRef.current
      buf.push({ t: frameTs, img })
      // retention must cover the interpolation delay (draw() writes
      // delayEstRef): the playhead sits `delay` behind newest, so a
      // fixed 1.5 s trim would clamp playback ahead of track-now and
      // reintroduce the box-vs-picture slide the delay cap fixes.
      const retain = Math.min(3.0, Math.max(1.5, delayEstRef.current + 0.35))
      while (buf.length > 0 && frameTs - buf[0].t > retain) {
        const evicted = buf.shift()
        if (evicted) closeFrameImg(evicted.img)
      }
      const c = fpsCounterRef.current
      c.frames++
      const now = Date.now()
      if (now - c.last >= 1000) {
        setVideoFps(Math.round((c.frames * 1000) / (now - c.last)))
        c.frames = 0
        c.last = now
      }
      dirtyRef.current = true
      // rAF is PAUSED in non-composited webviews (Electron IAB) — kick the
      // draw directly; the dirty flag makes this a no-op when rAF is alive
      drawRef.current?.()
    }, [])

    /** Decode JPEG bytes off the main thread when the engine supports it
     * (every modern WKWebView/Chromium does); fall back to a data-URL
     * <img> otherwise. createImageBitmap replaces the objectURL+Image dance:
     * no revoke bookkeeping, decode never blocks the main thread. */
    const decodeJpeg = useCallback((bytes: Uint8Array, frameTs: number, b64Fallback?: string) => {
      if (typeof createImageBitmap === 'function') {
        createImageBitmap(new Blob([bytes as unknown as BlobPart], { type: 'image/jpeg' }))
          .then((bmp) => pushDecodedFrame(frameTs, bmp))
          .catch(() => { /* decode failure — drop the frame */ })
        return
      }
      if (!b64Fallback) return
      const im = new Image()
      im.onload = () => pushDecodedFrame(frameTs, im)
      im.src = `data:image/jpeg;base64,${b64Fallback}`
    }, [pushDecodedFrame])

    /** Track-history + live-state update from a bundle meta — shared by the
     * JPEG frames, the H.264 frames (which carry the same meta fields) and
     * the REST fallback. */
    const applyTrackMeta = useCallback((data: FrameBundle) => {
      const hist = trackHistRef.current
      const tsNs = data.ts_ns
      // ts-keyed from the device clock: each preview frame carries the
      // latest tracks + its own ts — the local history built from these is
      // the interpolation source (exact, and no server-side hist needed).
      // Falls back to receive-time keys when ts is absent (old producers).
      if (tsNs) {
        // device-clock keyframes: tracks only change at inference rate
        // (~5 Hz) while preview frames arrive faster — append a history
        // entry ONLY when the position actually moved (plus a 500 ms
        // heartbeat so a stationary person doesn't age out). A staircase
        // history (identical positions repeated) zeroes the interpolation
        // velocity and the overlay visibly trails the video.
        // TRUE capture time of the positions — the preview ts is one
        // inference-latency ahead of when these positions were real
        const tracksTs = (data.tracks_ts ?? 0) / 1e6
        const tSec = tracksTs > 0 ? tracksTs : tsNs / 1e6
        if (tracksTs > 0 && tracksTs > lastTracksTsRef.current)
          lastTracksTsRef.current = tracksTs
        const seen = new Set<number>()
        for (const t of data.tracks ?? []) {
          if (!t.bbox) continue
          seen.add(t.track_id)
          let arr = hist.get(t.track_id)
          if (!arr) { arr = []; hist.set(t.track_id, arr) }
          const last = arr[arr.length - 1]
          const moved = !last
            || Math.abs(last.bbox.x - t.bbox.x) > 1e-4
            || Math.abs(last.bbox.y - t.bbox.y) > 1e-4
            || Math.abs(last.bbox.w - t.bbox.w) > 1e-4
            || Math.abs(last.bbox.h - t.bbox.h) > 1e-4
          if (moved || tSec - last.t > 0.5) {
            arr.push({ t: tSec, bbox: t.bbox, foot: t.foot, kpts: t.pose?.kpts ?? null, vel: t.vel ?? null })
          }
          while (arr.length > 0 && tSec - arr[0].t > 3) arr.shift()
        }
        for (const k of [...hist.keys()]) if (!seen.has(k)) hist.delete(k)
      } else {
        const nowH = performance.now() / 1000
        const seen = new Set<number>()
        for (const t of data.tracks ?? []) {
          if (!t.bbox) continue
          seen.add(t.track_id)
          let arr = hist.get(t.track_id)
          if (!arr) { arr = []; hist.set(t.track_id, arr) }
          arr.push({ t: nowH, bbox: t.bbox, foot: t.foot })
          while (arr.length > 0 && nowH - arr[0].t > 3) arr.shift()
        }
        for (const k of [...hist.keys()]) if (!seen.has(k)) hist.delete(k)
      }
      stateRef.current = {
        present_count: data.present_count ?? data.tracks?.length ?? 0,
        tracks: data.tracks ?? [],
        faces: data.faces ?? [],
      } as LiveState
      setPresent(data.present_count ?? data.tracks?.length ?? 0)
    }, [])

    /** One hardware H.264 frame (Annex-B AU + bundle-shaped meta). Meta is
     *  processed immediately (tracks ride at the relay rate); the NALU goes
     *  through the WebCodecs pump and lands in the jitter buffer as a
     *  VideoFrame. While the decoder is actively producing, JPEG frames are
     *  held back as the fallback. */
    const handleH264Frame = useCallback((meta: Record<string, any>, nalu: Uint8Array, pushSeq?: number) => {
      if (!deviceFramesRef.current) {
        deviceFramesRef.current = true
        if (wsRef.current) { try { wsRef.current.close() } catch { /* already closed */ } wsRef.current = null }
      }
      setStatus('streaming')
      applyTrackMeta(meta as FrameBundle)
      // DEVICE-seq gap (meta.h264_seq, monotonic per relay session) = a
      // frame was lost anywhere upstream. The platform session seq is
      // contiguous BY CONSTRUCTION and hides loss; the device seq is the
      // truthful end-to-end signal. Feeding the decoder past a gap decodes
      // smear until the next keyframe; resetting bounds the corruption to
      // ≤1 GOP and re-syncs cleanly. A seq that goes backwards = relay
      // session restart, not a gap.
      const devSeq = Number(meta.h264_seq ?? 0) || pushSeq || 0
      if (devSeq > 0) {
        const prev = lastPushSeqRef.current
        if (prev != null && devSeq > prev + 1) {
          h264Ref.current?.reset()
          lastPushSeqRef.current = null
        } else if (prev == null || devSeq >= prev) {
          lastPushSeqRef.current = devSeq
        }
      }
      if (!h264Ref.current) h264Ref.current = new H264Decoder()
      const dec = h264Ref.current
      dec.onFrame = (vf, m) => {
        h264ActiveRef.current = true
        // Key the jitter buffer by ENCODER PTS (capture clock), not the
        // relay-read ts: the read side wobbles with socket scheduling
        // (GIL, batching) and that wobble lands directly in the overlay's
        // alignment. pts is monotonic and jitter-free; a slow EMA anchor
        // maps it onto the device wall clock the track history uses.
        const pts = Number(m.pts_ns ?? 0)
        const tsNs = Number(m.ts_ns ?? 0) || Date.now() * 1e6
        let t: number
        if (pts > 0) {
          const a = ptsAnchorRef.current
          ptsAnchorRef.current = a == null ? tsNs - pts
            : a + ((tsNs - pts) - a) * 0.02
          t = (pts + (ptsAnchorRef.current ?? 0)) / 1e6
        } else {
          t = tsNs / 1e6
        }
        // 4K VideoFrames held for the full ~2.1 s alignment delay would
        // pin ~850 MB of GPU memory (≈70 frames × 12.4 MB). Transcode to
        // a ≤1280-wide ImageBitmap immediately and close the source
        // frame — the buffer then holds ~2.8 MB bitmaps and the draw
        // loop blits bitmap→widget in one GPU op instead of scaling 4K
        // every rAF. Falls back to a plain copy, then to the raw
        // VideoFrame, when resize options are unsupported.
        if (typeof createImageBitmap === 'function') {
          const dw = vf.displayWidth
          const opts = dw > 1280
            ? { resizeWidth: 1280, resizeHeight: Math.max(2, Math.round(1280 * vf.displayHeight / dw)), resizeQuality: 'low' as const }
            : undefined
          createImageBitmap(vf, opts ?? {})
            .then(bm => {
              vf.close()
              if (mountedRef.current) pushDecodedFrame(t, bm)
              else bm.close()
            })
            .catch(() => createImageBitmap(vf)
              .then(bm => {
                vf.close()
                if (mountedRef.current) pushDecodedFrame(t, bm)
                else bm.close()
              })
              .catch(() => pushDecodedFrame(t, vf)))
        } else {
          pushDecodedFrame(t, vf)
        }
      }
      dec.feed(meta, nalu)
      // after a reset (error/stall) the decoder waits for a keyframe —
      // unblock the JPEG fallback for that window
      h264ActiveRef.current = dec.active
    }, [applyTrackMeta, pushDecodedFrame])

    const applyFrameBundle = useCallback((data: FrameBundle, jpegBytes?: Uint8Array) => {
      // Dedup: device ts_ns is unique per frame and present on both the WS
      // and REST legs; fall back to img_b64 equality for ts-less producers.
      const tsNs = data.ts_ns
      if (tsNs != null) {
        if (tsNs === lastFrameTsNsRef.current) return
        lastFrameTsNsRef.current = tsNs
      } else {
        if (!data.img_b64 || data.img_b64 === lastImgRef.current) return
        lastImgRef.current = data.img_b64
      }
      if (!deviceFramesRef.current) {
        deviceFramesRef.current = true
        // stream-player video is superseded by the device frames
        if (wsRef.current) { try { wsRef.current.close() } catch { /* already closed */ } wsRef.current = null }
      }
      // every device frame proves the display pipeline is alive — also
      // overrides any stale error status from the superseded video path
      setStatus('streaming')
      const frameTs = tsNs != null ? tsNs / 1e6 : Date.now() / 1000
      // JPEG is the fallback image path: skip while the hardware H.264
      // decoder is actively producing frames (it still processes meta)
      if (!h264ActiveRef.current) {
        if (jpegBytes) {
          decodeJpeg(jpegBytes, frameTs)
        } else if (data.img_b64) {
          // legacy string leg (REST fallback / old-core Text sessions)
          const bytes = bytesFromB64(data.img_b64)
          if (bytes) decodeJpeg(bytes, frameTs, data.img_b64)
        }
      }
      applyTrackMeta(data)
    }, [decodeJpeg, applyTrackMeta])

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

      // The server kills a push session whose client stalls >2 s (throttled
      // tab, GC pause) — reconnect with backoff so playback RESUMES instead
      // of silently dropping to the (slower) polling fallback forever.
      let wsGeneration = 0
      let pollingActive = false
      const startPollingOnce = () => {
        if (pollingActive) return
        pollingActive = true
        startPolling()
      }
      const startPush = () => {
        const gen = ++wsGeneration
        const retry = (delayMs: number) => {
          if (stopped || gen !== wsGeneration) return
          timer = window.setTimeout(() => startPush(), delayMs)
        }
        try {
          const isTauri = !!(window as any).__TAURI_INTERNALS__
          const proto = (isTauri ? false : window.location.protocol === 'https:') ? 'wss:' : 'ws:'
          const host = isTauri ? 'localhost:9375' : window.location.host
          let url = `${proto}//${host}/api/extensions/${extensionId}/stream`
          const token = getToken()
          if (token) url += `?token=${encodeURIComponent(token)}`
          ws = new WebSocket(url)
          // binary push frames arrive as ArrayBuffers when negotiated
          ws.binaryType = 'arraybuffer'
          frameWsRef.current = ws
          ws.onopen = () => {
            // `binary: true` opts into raw-byte push frames. Old servers
            // treat config as free JSON and ignore the unknown key — they
            // keep sending Text+base64, which the handler below still parses.
            ws?.send(JSON.stringify({ type: 'init', config: { binary: true } }))
          }
          ws.onmessage = (event) => {
            if (!mountedRef.current) return
            // Binary leg: [14B header][meta][container] — payloads arrive as
            // raw bytes, zero base64 on the wire.
            if (event.data instanceof ArrayBuffer) {
              const parsed = parseBinaryPushFrame(event.data)
              if (parsed && parsed.meta?.data_type === FRAME_DATA_TYPE) {
                const frame = parseFrameContainer(parsed.payload)
                if (frame) applyFrameBundle(frame.bundle, frame.jpeg)
              } else if (parsed && parsed.meta?.data_type === AVC_DATA_TYPE) {
                const frame = parseFrameContainer(parsed.payload)
                if (frame) handleH264Frame(frame.bundle as unknown as Record<string, any>, frame.jpeg, parsed.seq)
              }
              return
            }
            if (typeof event.data !== 'string') return
            try {
              const msg = JSON.parse(event.data)
              if (msg.type === 'session_created') {
                // push starts at init server-side; nothing to send here
                // (a former `start_push` message never existed in the
                // server enum and was silently dropped)
              } else if (msg.type === 'push_output') {
                if (msg.data_type === FRAME_DATA_TYPE || msg.data_type === AVC_DATA_TYPE) {
                  // legacy Text leg from a new extension: the container is
                  // base64-wrapped inside the JSON envelope
                  const bytes = typeof msg.data === 'string' ? bytesFromB64(msg.data) : null
                  if (bytes) {
                    const frame = parseFrameContainer(bytes)
                    if (frame) {
                      if (msg.data_type === AVC_DATA_TYPE)
                        handleH264Frame(frame.bundle as unknown as Record<string, any>, frame.jpeg)
                      else applyFrameBundle(frame.bundle, frame.jpeg)
                    }
                  }
                } else if (msg.data_type === 'application/json') {
                  // old extension build: img_b64 embedded in the bundle
                  let bundle = null
                  try {
                    bundle = typeof msg.data === 'string'
                      ? JSON.parse(atob(msg.data))
                      : msg.data
                  } catch { /* skip malformed frame */ }
                  if (bundle) applyFrameBundle(bundle)
                }
              }
            } catch { /* malformed frame — skip */ }
          }
          ws.onerror = () => { /* handled by onclose */ }
          ws.onclose = () => {
            if (frameWsRef.current === ws) frameWsRef.current = null
            if (stopped || gen !== wsGeneration) return
            startPollingOnce() // keep frames coming while disconnected
            retry(2000)        // then rebuild the push session
          }
        } catch {
          startPollingOnce()
          retry(2000)
        }
      }
      startPush()
      return () => {
        stopped = true
        if (timer) clearTimeout(timer)
        if (ws) { try { ws.close() } catch { /* already closed */ } }
      }
    }, [extensionId, applyFrameBundle, handleH264Frame])

    // release GPU-backed decode state on unmount
    useEffect(() => () => {
      h264Ref.current?.reset()
      h264Ref.current = null
      h264ActiveRef.current = false
      for (const f of frameBufRef.current) closeFrameImg(f.img)
      frameBufRef.current.length = 0
    }, [])

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

    // counters only — safe to poll while the editor owns the local geometry
    const loadCrossings = useCallback(async () => {
      const cr = await fetchCrossings(extensionId)
      if (mountedRef.current && cr.success && cr.data) setCrossings(cr.data.lines ?? [])
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
        if (!mountedRef.current) return
        // In edit mode the local state IS the working copy — refreshing the
        // line definitions here resurrected a just-deleted line within 2.5 s
        // and made 删除 look like a no-op. Counters stay live; definitions
        // reload only outside the editor.
        if (modeRef.current === 'edit') {
          loadCrossings()
          return
        }
        loadLines()
        if (showHeatmap) {
          fetchHeatmap(extensionId).then((r) => {
            if (mountedRef.current && r.success && r.data) setHeat(r.data)
          })
        }
      }, 2500)
      return () => clearInterval(id)
    }, [loadLines, loadCrossings, showHeatmap, extensionId])

    // ---- render loop ----
    const draw = useCallback(() => {
      const canvas = canvasRef.current
      const ctx = canvas?.getContext('2d')
      if (!canvas || !ctx) return
      const nowMs = performance.now()
      if (!dirtyRef.current && nowMs - lastDrawRef.current < 250) return
      dirtyRef.current = false
      lastDrawRef.current = nowMs

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
      // Frame pick + alignment target. The overlay is drawn for the EXACT
      // moment the shown frame was captured (img.t) — that invariant is what
      // makes box-vs-picture alignment exact. The catch: the track stream
      // runs 35-250 ms (jittery) behind the preview stream, so playing the
      // NEWEST frame means the overlay must extrapolate that whole gap with
      // noisy velocities — visible trailing on motion. Instead the video is
      // held back by the SMOOTHED data gap minus a small margin, so the
      // shown frame's ts lands just BEHIND the newest track keyframe and the
      // overlay INTERPOLATES between two real samples (exact) almost always;
      // spikes fall into bounded extrapolation along the device velocity.
      const buf = frameBufRef.current
      let img: HTMLImageElement | ImageBitmap | VideoFrame | null = null
      if (buf.length > 0) {
        const newest = buf[buf.length - 1].t
        const gap = Math.max(0, newest - lastTracksTsRef.current)
        const gw = gapWinRef.current
        gw.push(gap)
        if (gw.length > 90) gw.shift()
        // PURE-INTERPOLATION MODE: delay from the rolling window MAX of
        // the video-vs-tracks gap (+ small margin), not an EMA. The gap
        // swings by up to one inference PERIOD (2 Hz cadence ⇒ ±240 ms
        // around its mean); an EMA-based delay sits mid-range and leaves
        // half-period extrapolation windows — the residual "box trails
        // person" users saw. Window-max guarantees the playhead is behind
        // the newest track sample essentially every frame: pure two-sample
        // interpolation, sub-pixel error, no velocity guessing. The cap must
        // sit ABOVE the steady-state gap or it forces the playhead AHEAD of
        // the newest track sample into clamped extrapolation — measured
        // steady gap with the 4K H.264 relay + 2.2 Hz track pipeline is
        // 1.05–2.1 s (rolling max ≈ 2.1), so a 1.2 s cap left the playhead
        // up to 0.9 s ahead of track-now and boxes slid off walkers every
        // ~1 s cycle. 2.6 s covers the operating point with margin and
        // still bounds the dead-track-stream case.
        const delay = Math.min(2.6, Math.max(0.05, Math.max(...gw) + 0.05))
        delayEstRef.current = delay
        let want = newest - delay
        // don't visibly rewind when a burst of old frames lands late
        const prevShown = lastTsRef.current
        if (prevShown != null && want < prevShown - 0.05) want = prevShown - 0.05
        for (let i = buf.length - 1; i >= 0; i--) {
          if (buf[i].t <= want + 0.004) { img = buf[i].img; lastTsRef.current = buf[i].t; break }
        }
        if (!img) { img = buf[0].img; lastTsRef.current = buf[0].t }
      }
      if (!img) img = imgRef.current
      // per-type dims: HTMLImageElement naturalWidth/Height, ImageBitmap
      // width/height, VideoFrame displayWidth/displayHeight
      const frameDims = (im: NonNullable<typeof img>): [number, number] => {
        if (im instanceof HTMLImageElement) return [im.naturalWidth, im.naturalHeight]
        if (typeof VideoFrame !== 'undefined' && im instanceof VideoFrame)
          return [im.displayWidth, im.displayHeight]
        return [(im as ImageBitmap).width, (im as ImageBitmap).height]
      }
      const [iw, ih] = img ? frameDims(img) : [0, 0]
      if (img && iw > 0) {
        const scale = Math.min(VW / iw, VH / ih)
        const l = {
          dx: (VW - iw * scale) / 2,
          dy: (VH - ih * scale) / 2,
          dw: iw * scale,
          dh: ih * scale,
        }
        layoutRef.current = l
        ctx.imageSmoothingEnabled = true
        ctx.imageSmoothingQuality = 'high'
        ctx.drawImage(img, l.dx, l.dy, l.dw, l.dh)
      }
      const { dx, dy, dw, dh } = layoutRef.current
      const X = (nx: number) => dx + nx * dw
      const Y = (ny: number) => dy + ny * dh
      // Edit-handle sizes in CSS px, converted into virtual units: virtual
      // space is fixed at 960 wide, so constant radii balloon on wide
      // widgets. S = virtual units per CSS pixel.
      const S = canvas.clientWidth > 0 ? 960 / canvas.clientWidth : 1

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
        // occupancy: foot first (ground contact); when the camera hides the
        // floor the body's bbox center stands in — a torso inside the zone
        // is occupancy too (gear blocks the ankles on this fisheye view)
        const inZone = (tr: any, poly: number[][]): boolean => {
          if (tr.foot && pointInPolygon(tr.foot.x, tr.foot.y, poly)) return true
          const b = tr.bbox
          return !!b && pointInPolygon(b.x + b.w / 2, b.y + b.h / 2, poly)
        }
        for (const z of zonesRef.current) {
          const poly = z.polygon
          if (!poly || poly.length < 3) continue
          const isExcl = z.equipment_type === 'exclusion'
          const count = isExcl ? 0 : tracks.filter((t) => inZone(t, poly)).length
          const occupied = !isExcl && count > 0
          const editing =
            modeRef.current === 'edit' &&
            (editKindRef.current === 'zones' || editKindRef.current === 'exclude')

          ctx.beginPath()
          poly.forEach((p, i) =>
            i === 0 ? ctx.moveTo(X(p[0]), Y(p[1])) : ctx.lineTo(X(p[0]), Y(p[1]))
          )
          ctx.closePath()
          ctx.fillStyle = isExcl
            ? 'rgba(239, 68, 68, 0.10)'
            : occupied
              ? 'rgba(34, 197, 94, 0.22)'
              : editing
                ? 'rgba(148, 163, 184, 0.16)'
                : 'rgba(148, 163, 184, 0.07)'
          ctx.fill()
          ctx.lineWidth = editing ? 2.5 : 2
          const sel = editing && selZoneRef.current === z.id
          ctx.strokeStyle = isExcl
            ? 'rgba(239, 68, 68, 0.75)'
            : occupied
              ? 'rgba(34, 197, 94, 0.95)'
              : sel
                ? '#3b82f6'
                : 'rgba(148, 163, 184, 0.7)'
          if (isExcl || z.enabled === false || z.enabled === 0) ctx.setLineDash([6, 5])
          ctx.stroke()
          ctx.setLineDash([])

          if (editing) {
            // draggable vertex handles: dark ring + light core (grab affordance)
            for (const p of poly) {
              ctx.beginPath()
              ctx.arc(X(p[0]), Y(p[1]), 5.5 * S, 0, Math.PI * 2)
              ctx.fillStyle = 'rgba(15, 23, 42, 0.85)'
              ctx.fill()
              ctx.beginPath()
              ctx.arc(X(p[0]), Y(p[1]), (sel ? 3.2 : 2.6) * S, 0, Math.PI * 2)
              ctx.fillStyle = sel ? '#3b82f6' : 'rgba(226, 232, 240, 0.95)'
              ctx.fill()
            }
            // edge midpoints: hollow — drag one to insert a vertex there
            for (let i = 0; i < poly.length; i++) {
              const a = poly[i]
              const b = poly[(i + 1) % poly.length]
              const mx = (a[0] + b[0]) / 2
              const my = (a[1] + b[1]) / 2
              ctx.beginPath()
              ctx.arc(X(mx), Y(my), 4.2 * S, 0, Math.PI * 2)
              ctx.fillStyle = 'rgba(15, 23, 42, 0.7)'
              ctx.fill()
              ctx.beginPath()
              ctx.arc(X(mx), Y(my), 2.4 * S, 0, Math.PI * 2)
              ctx.strokeStyle = sel ? '#93c5fd' : 'rgba(148, 163, 184, 0.9)'
              ctx.lineWidth = 1.5 * S
              ctx.stroke()
            }
          }

          const cx0 = poly.reduce((s, p) => s + p[0], 0) / poly.length
          const cy0 = poly.reduce((s, p) => s + p[1], 0) / poly.length
          const label = `${z.name}${count > 0 ? `·${count}` : ''}`
          // compact plate: zones cluster with bbox labels + line badges and
          // a 14 px plate drowned the scene; keep it a quiet tag
          ctx.font = '600 11px system-ui, sans-serif'
          const tw = ctx.measureText(label).width + 10
          ctx.fillStyle = occupied ? 'rgba(34, 197, 94, 0.88)' : 'rgba(15, 23, 42, 0.68)'
          ctx.fillRect(X(cx0) - tw / 2, Y(cy0) - 9, tw, 18)
          ctx.fillStyle = occupied ? '#04250f' : '#d1d5db'
          ctx.textAlign = 'center'
          ctx.fillText(label, X(cx0), Y(cy0) + 3.5)
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
        // endpoints — in edit mode: draggable handles (ring + core)
        for (const [px, py] of [[ax, ay], [bx, by]]) {
          if (editing) {
            ctx.beginPath()
            ctx.arc(px, py, 6 * S, 0, Math.PI * 2)
            ctx.fillStyle = 'rgba(15, 23, 42, 0.85)'
            ctx.fill()
          }
          ctx.beginPath()
          ctx.arc(px, py, (editing ? 3.4 : 3.2) * S, 0, Math.PI * 2)
          ctx.fillStyle = 'rgba(245, 158, 11, 0.95)'
          ctx.fill()
        }
        // count badge above midpoint
        const label = `${ln.name}  ↑${st?.in_count ?? 0} ↓${st?.out_count ?? 0}`
        ctx.font = '600 11px system-ui, sans-serif'
        const tw = ctx.measureText(label).width + 10
        const off = 26
        const nx = Math.sin(ang), ny = -Math.cos(ang) // normal
        const bxPos = mx + nx * off, byPos = my + ny * off
        ctx.fillStyle = 'rgba(120, 53, 15, 0.85)'
        ctx.fillRect(bxPos - tw / 2, byPos - 9, tw, 18)
        ctx.fillStyle = '#fef3c7'
        ctx.textAlign = 'center'
        ctx.fillText(label, bxPos, byPos + 3.5)
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
      // High-contrast styling: every stroke is drawn twice — a black
      // underlay slightly thicker, then a bright color on top — so boxes
      // and skeletons stay readable on both bright and dark video.
      const HC = {
        member: '#2dd4a7',   // teal-green (matched member)
        unknown: '#38e1ff',  // cyan (unknown person)
        skeleton: '#d9ff3d', // lime (skeleton edges)
        kpt: '#ffffff',      // keypoint fill
        underlay: 'rgba(0, 0, 0, 0.85)',
        foot: '#ffb020',
      }
      // four corner brackets instead of a full rectangle — lighter visual
      // weight, surveillance-style, and doesn't cover the person's body
      const drawCorners = (bx: number, by: number, bw: number, bh: number,
                           color: string) => {
        const len = Math.max(8, Math.min(22, Math.min(bw, bh) * 0.28))
        const corners: Array<[number, number, number, number]> = [
          [bx, by, len, 0], [bx, by, 0, len],                       // TL
          [bx + bw - len, by, len, 0], [bx + bw, by, 0, len],       // TR
          [bx, by + bh - len, 0, len], [bx, by + bh, len, 0],       // BL
          [bx + bw, by + bh - len, 0, len], [bx + bw - len, by + bh, len, 0], // BR
        ]
        for (const pass of [ [HC.underlay, 4.5], [color, 2.5] ] as const) {
          ctx.strokeStyle = pass[0]
          ctx.lineWidth = pass[1]
          ctx.lineCap = 'round'
          ctx.beginPath()
          for (const [sx, sy, dx, dy] of corners) {
            ctx.moveTo(sx, sy)
            ctx.lineTo(sx + dx, sy + dy)
          }
          ctx.stroke()
        }
      }
      // Both modes interpolate along the bbox history: stream-player mode
      // aligns to the displayed video time; device-frame mode targets "now"
      // so boxes keep moving smoothly between ~5 Hz device updates.
      // device-frame mode: lastTsRef is the SHOWN frame's own ts (set where
      // the frame is picked) — boxes interpolate at exactly that moment
      const drawNow = deviceFramesRef.current && lastTsRef.current != null
        ? lastTsRef.current
        : performance.now() / 1000 - 0.15
      // far-field false positives: a bbox with <3 visible keypoints is an
      // "empty box" — skip it (defensive; the device also gates at publish)
      const visibleKpts = (kpts?: [number, number, number][]) =>
        (kpts ?? []).filter(k => k[2] > KPT_MIN_SCORE).length
      const alignedTracks = tracks.filter((tr) =>
        !tr.pose || visibleKpts(tr.pose.kpts) >= 2 || tr.member
      ).map((tr) => {
        const hist = trackHistRef.current.get(tr.track_id)
        if (!tr.bbox || !hist || hist.length === 0) return tr
        const s = sampleAt(hist, drawNow, KPT_MIN_SCORE)
        if (!s) return tr
        return {
          ...tr,
          bbox: s.bbox,
          pose: tr.pose && s.kpts ? { kpts: s.kpts, score: tr.pose.score } : tr.pose,
          foot: s.foot ?? tr.foot,
        }
      })
      for (const track of alignedTracks) {
        if (show.boxes && track.bbox) {
          const { x, y, w, h } = track.bbox
          drawCorners(X(x), Y(y), w * dw, h * dh,
                      track.member ? HC.member : HC.unknown)
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
          ctx.font = '600 12px system-ui, sans-serif'
          const tw = ctx.measureText(label).width + 10
          // label plate: black base + colored edge, white text — readable
          // on any background
          ctx.fillStyle = 'rgba(0, 0, 0, 0.82)'
          ctx.fillRect(X(x) - 1, Math.max(0, Y(y) - 17), tw + 2, 16)
          ctx.fillStyle = track.member ? HC.member : HC.unknown
          ctx.fillRect(X(x) - 1, Math.max(0, Y(y) - 17), 3, 16)
          ctx.fillStyle = '#ffffff'
          ctx.fillText(label, X(x) + 6, Math.max(11, Y(y) - 5.5))
        }

        const kpts = track.pose?.kpts
        if (show.pose && kpts && kpts.length > 0) {
          const pt = (i: number) => {
            const k = kpts[i]
            return k && k[2] > KPT_MIN_SCORE ? { x: X(k[0]), y: Y(k[1]) } : null
          }
          // skeleton: black underlay pass then bright lime pass
          for (const pass of [ [HC.underlay, 5], [HC.skeleton, 2.5] ] as const) {
            ctx.strokeStyle = pass[0]
            ctx.lineWidth = pass[1]
            ctx.lineCap = 'round'
            ctx.beginPath()
            for (const [a, b] of SKELETON_EDGES) {
              const pa = pt(a)
              const pb = pt(b)
              if (!pa || !pb) continue
              ctx.moveTo(pa.x, pa.y)
              ctx.lineTo(pb.x, pb.y)
            }
            ctx.stroke()
          }
          // keypoints: white dot with dark ring — pops on any background
          for (let i = 0; i < kpts.length; i++) {
            const p = pt(i)
            if (!p) continue
            ctx.beginPath()
            ctx.arc(p.x, p.y, 4.2, 0, Math.PI * 2)
            ctx.fillStyle = HC.underlay
            ctx.fill()
            ctx.beginPath()
            ctx.arc(p.x, p.y, 3, 0, Math.PI * 2)
            ctx.fillStyle = HC.kpt
            ctx.fill()
          }
        }

        if (show.pose && track.foot) {
          const fx = X(track.foot.x)
          const fy = Y(track.foot.y)
          ctx.beginPath()
          ctx.arc(fx, fy, 7, 0, Math.PI * 2)
          ctx.fillStyle = HC.underlay
          ctx.fill()
          ctx.beginPath()
          ctx.arc(fx, fy, 4.5, 0, Math.PI * 2)
          ctx.fillStyle = HC.foot
          ctx.fill()
        }
      }

      // ---- drafts (edit mode) — zones AND exclusion areas share this ----
      if (
        modeRef.current === 'edit' &&
        (editKindRef.current === 'zones' || editKindRef.current === 'exclude')
      ) {
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
            ctx.arc(X(p[0]), Y(p[1]), 3.4 * S, 0, Math.PI * 2)
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

      // Redraw loop: rAF-paced but dirty-flagged — see dirtyRef above.
      rafRef.current = requestAnimationFrame(draw)
      fallbackRef.current = window.setInterval(() => { dirtyRef.current = true }, 250)
    }, [])

    // latest draw closure for external kicks (image onload)
    const drawRef = useRef<(() => void) | null>(null)
    // dirty-flag: rAF redraws only when a new frame arrived (or 250 ms
    // passed) — a full canvas redraw per rAF tick at 60 Hz starved the
    // main thread while frames arrived at only ~23 Hz
    const dirtyRef = useRef(true)
    const lastDrawRef = useRef(0)

    useEffect(() => {
      rafRef.current = requestAnimationFrame(draw)
      drawRef.current = draw
      return () => {
        cancelAnimationFrame(rafRef.current)
        if (fallbackRef.current) { clearInterval(fallbackRef.current); fallbackRef.current = null }
      }
    }, [draw])

    // ---- edit-mode geometry dragging ----
    // Grab radius in virtual px (the draw space is 960-wide, so this stays
    // proportional on any widget size).
    const HIT = 11
    const clamp01 = (v: number) => Math.min(1, Math.max(0, v))
    // keep a whole-polygon drag inside the frame: shift may need clamping per
    // axis when the shape reaches an edge
    const clampShift = (v: number) => Math.max(-1, Math.min(1, v))
    const cloneZones = (zs: DraftZone[]) => zs.map((z) => ({ ...z, polygon: z.polygon.map((p) => [...p]) }))
    const cloneLines = (ls: DraftLine[]) => ls.map((l) => ({ ...l, a: [...l.a] as number[], b: [...l.b] as number[] }))

    const pointerPos = (e: { clientX: number; clientY: number }) => {
      const canvas = canvasRef.current!
      const rect = canvas.getBoundingClientRect()
      const scaleX = canvas.width / rect.width
      const scaleY = canvas.height / rect.height
      const K = canvas.width / 960
      const cx = ((e.clientX - rect.left) * scaleX) / K
      const cy = ((e.clientY - rect.top) * scaleY) / K
      const { dx, dy, dw, dh } = layoutRef.current
      return { cx, cy, nx: (cx - dx) / dw, ny: (cy - dy) / dh }
    }
    const vX = (nx: number) => layoutRef.current.dx + nx * layoutRef.current.dw
    const vY = (ny: number) => layoutRef.current.dy + ny * layoutRef.current.dh

    /** Nearest zone vertex / edge-midpoint under the pointer, if any. */
    const hitZoneHandle = (cx: number, cy: number) => {
      for (const z of zonesRef.current) {
        const poly = z.polygon
        if (!poly) continue
        for (let i = 0; i < poly.length; i++) {
          if (Math.hypot(cx - vX(poly[i][0]), cy - vY(poly[i][1])) <= HIT)
            return { zone: z, kind: 'vertex' as const, idx: i }
        }
        for (let i = 0; i < poly.length; i++) {
          const a = poly[i], b = poly[(i + 1) % poly.length]
          if (Math.hypot(cx - vX((a[0] + b[0]) / 2), cy - vY((a[1] + b[1]) / 2)) <= HIT)
            return { zone: z, kind: 'mid' as const, idx: i }
        }
      }
      return null
    }
    const hitLineHandle = (cx: number, cy: number) => {
      for (const ln of linesRef.current) {
        if (Math.hypot(cx - vX(ln.a[0]), cy - vY(ln.a[1])) <= HIT) return { line: ln, end: 'a' as const }
        if (Math.hypot(cx - vX(ln.b[0]), cy - vY(ln.b[1])) <= HIT) return { line: ln, end: 'b' as const }
        // line body (segment distance) → whole-line move
        const ax = vX(ln.a[0]), ay = vY(ln.a[1]), bx = vX(ln.b[0]), by = vY(ln.b[1])
        const L2 = (bx - ax) ** 2 + (by - ay) ** 2
        const t = L2 > 0 ? Math.max(0, Math.min(1, ((cx - ax) * (bx - ax) + (cy - ay) * (by - ay)) / L2)) : 0
        if (Math.hypot(cx - (ax + t * (bx - ax)), cy - (ay + t * (by - ay))) <= HIT)
          return { line: ln, end: null }
      }
      return null
    }

    // live drag: zones/lines refs are written in lockstep with state so the
    // canvas (which reads refs) never lags a frame behind the drag
    const applyZones = (next: DraftZone[]) => { zonesRef.current = next; setZones(next) }
    const applyLines = (next: DraftLine[]) => { linesRef.current = next; setLines(next) }

    const onPointerDown = useCallback((e: React.PointerEvent<HTMLCanvasElement>) => {
      if (modeRef.current !== 'edit') return
      const { cx, cy, nx, ny } = pointerPos(e)

      const start = (st: DragState) => {
        e.currentTarget.setPointerCapture(e.pointerId)
        dragRef.current = { st, moved: false, sx: e.clientX, sy: e.clientY }
      }

      if (editKindRef.current === 'lines') {
        const hit = hitLineHandle(cx, cy)
        if (!hit) return
        if (hit.end) start({ kind: 'line-end', lineId: hit.line.id, end: hit.end, snap: cloneLines(linesRef.current) })
        else start({ kind: 'line-move', lineId: hit.line.id, orig: [[...hit.line.a], [...hit.line.b]] as [number[], number[]], nx0: nx, ny0: ny, snap: cloneLines(linesRef.current) })
        return
      }
      if (editKindRef.current !== 'zones' && editKindRef.current !== 'exclude') return

      // draft point handles first (they sit on top while drawing)
      const d = draftRef.current
      for (let i = d.length - 1; i >= 0; i--) {
        if (Math.hypot(cx - vX(d[i][0]), cy - vY(d[i][1])) <= HIT) {
          start({ kind: 'draft-pt', idx: i })
          return
        }
      }
      const hit = hitZoneHandle(cx, cy)
      if (hit?.kind === 'vertex') {
        start({ kind: 'vertex', zoneId: hit.zone.id, idx: hit.idx, snap: cloneZones(zonesRef.current) })
        return
      }
      if (hit?.kind === 'mid') {
        // the vertex is materialized only once the pointer actually moves —
        // a plain click on a midpoint must not add stray vertices
        start({ kind: 'mid-insert', zoneId: hit.zone.id, after: hit.idx, snap: cloneZones(zonesRef.current) })
        return
      }
      // inside a zone → whole-polygon move (topmost wins)
      for (let i = zonesRef.current.length - 1; i >= 0; i--) {
        const z = zonesRef.current[i]
        if (z.polygon && pointInPolygon(nx, ny, z.polygon)) {
          start({ kind: 'poly', zoneId: z.id, orig: z.polygon.map((p) => [...p]), nx0: nx, ny0: ny, snap: cloneZones(zonesRef.current) })
          return
        }
      }
    }, [])

    const onPointerMove = useCallback((e: React.PointerEvent<HTMLCanvasElement>) => {
      const canvas = canvasRef.current
      if (!canvas) return
      const drag = dragRef.current
      const { cx, cy, nx, ny } = pointerPos(e)
      if (!drag) {
        if (modeRef.current === 'edit') {
          const grab =
            editKindRef.current === 'lines'
              ? !!hitLineHandle(cx, cy)
              : !!hitZoneHandle(cx, cy) ||
                draftRef.current.some((p) => Math.hypot(cx - vX(p[0]), cy - vY(p[1])) <= HIT)
          const inside =
            (editKindRef.current === 'zones' || editKindRef.current === 'exclude') &&
            zonesRef.current.some((z) => z.polygon && pointInPolygon(nx, ny, z.polygon))
          canvas.style.cursor = grab ? 'grab' : inside ? 'move' : 'crosshair'
        } else if (canvas.style.cursor) canvas.style.cursor = ''
        return
      }
      if (Math.hypot(e.clientX - drag.sx, e.clientY - drag.sy) > 4) drag.moved = true
      if (!drag.moved) return
      const st = drag.st
      if (st.kind === 'mid-insert') {
        // first movement materializes the vertex under the pointer, then the
        // drag continues as a normal vertex drag
        const idx = st.after + 1
        applyZones(zonesRef.current.map((z) =>
          z.id === st.zoneId
            ? { ...z, polygon: [...z.polygon.slice(0, idx), [clamp01(nx), clamp01(ny)], ...z.polygon.slice(idx)] }
            : z))
        drag.st = { kind: 'vertex', zoneId: st.zoneId, idx, snap: st.snap }
      } else if (st.kind === 'vertex') {
        applyZones(zonesRef.current.map((z) =>
          z.id === st.zoneId
            ? { ...z, polygon: z.polygon.map((p, i) => (i === st.idx ? [clamp01(nx), clamp01(ny)] : p)) }
            : z))
      } else if (st.kind === 'poly') {
        const dnx = clampShift(nx - st.nx0), dny = clampShift(ny - st.ny0)
        applyZones(zonesRef.current.map((z) =>
          z.id === st.zoneId
            ? { ...z, polygon: st.orig.map((p) => [clamp01(p[0] + dnx), clamp01(p[1] + dny)]) }
            : z))
      } else if (st.kind === 'draft-pt') {
        const next = draftRef.current.map((p, i) => (i === st.idx ? [clamp01(nx), clamp01(ny)] : p))
        draftRef.current = next
        setDraft(next)
      } else if (st.kind === 'line-end') {
        applyLines(linesRef.current.map((l) =>
          l.id === st.lineId ? { ...l, [st.end]: [clamp01(nx), clamp01(ny)] } : l))
      } else if (st.kind === 'line-move') {
        const dnx = nx - st.nx0, dny = ny - st.ny0
        applyLines(linesRef.current.map((l) =>
          l.id === st.lineId
            ? { ...l, a: [clamp01(st.orig[0][0] + dnx), clamp01(st.orig[0][1] + dny)], b: [clamp01(st.orig[1][0] + dnx), clamp01(st.orig[1][1] + dny)] }
            : l))
      }
      dirtyRef.current = true
    }, [])

    const onPointerUp = useCallback(() => {
      const drag = dragRef.current
      dragRef.current = null
      const canvas = canvasRef.current
      if (canvas && modeRef.current !== 'edit') canvas.style.cursor = ''
      if (!drag || !drag.moved) return
      suppressClickRef.current = true
      const st = drag.st
      if (st.kind === 'line-end' || st.kind === 'line-move')
        lastDragSnapRef.current = { lines: st.snap }
      else if (st.kind !== 'draft-pt')
        lastDragSnapRef.current = { zones: st.snap }
      dirtyRef.current = true
    }, [])

    const onCanvasDblClick = useCallback((e: React.MouseEvent<HTMLCanvasElement>) => {
      if (
        modeRef.current !== 'edit' ||
        (editKindRef.current !== 'zones' && editKindRef.current !== 'exclude')
      )
        return
      const { cx, cy } = pointerPos(e)
      const hit = hitZoneHandle(cx, cy)
      if (hit?.kind !== 'vertex') return
      const z = hit.zone
      if (z.polygon.length <= 3) return // would degenerate — delete via the list instead
      suppressClickRef.current = true
      lastDragSnapRef.current = { zones: cloneZones(zonesRef.current) }
      applyZones(zonesRef.current.map((x) =>
        x.id === z.id ? { ...x, polygon: x.polygon.filter((_, i) => i !== hit.idx) } : x))
      dirtyRef.current = true
    }, [])

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
      // the click that ended a drag (or a dblclick's second click) must not
      // add geometry
      if (suppressClickRef.current) {
        suppressClickRef.current = false
        return
      }

      if (editKindRef.current === 'lines') {
        // clicks on an existing line's handles are for dragging, not drawing
        if (hitLineHandle(cx, cy)) return
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
        // clicks on vertex/midpoint handles are for dragging, not new points
        if (hitZoneHandle(cx, cy)) return
        setDraft((d) => [...d, p])
      }
    }, [])

    const closeDraft = useCallback(() => {
      if (draft.length < 3) return
      const excl = editKindRef.current === 'exclude'
      setZones((zs) => [
        ...zs,
        {
          id: crypto.randomUUID(),
          name: excl ? `无效区 ${zs.filter((z) => z.equipment_type === 'exclusion').length + 1}` : `Zone ${zs.length + 1}`,
          equipment_type: excl ? 'exclusion' : 'equipment',
          polygon: draft,
          enabled: true,
          isNew: true,
        },
      ])
      setDraft([])
      // the editor list sits below the tall canvas — bring it into view so
      // naming/saving is reachable without hunting for it
      requestAnimationFrame(() => {
        document.querySelector('.gym-ov-zonelist')
          ?.scrollIntoView({ behavior: 'smooth', block: 'center' })
      })
    }, [draft])

    const undo = useCallback(() => {
      if (editKind === 'lines' && draftLine.length > 0) {
        setDraftLine((d) => d.slice(0, -1))
        return
      }
      if ((editKind === 'zones' || editKind === 'exclude') && draft.length > 0) {
        setDraft((d) => d.slice(0, -1))
        return
      }
      // no draft points left → undo the last drag / vertex delete
      const snap = lastDragSnapRef.current
      if (snap) {
        if (snap.zones) applyZones(snap.zones)
        if (snap.lines) applyLines(snap.lines)
        lastDragSnapRef.current = null
        dirtyRef.current = true
      }
    }, [editKind, draft.length, draftLine.length])

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

    // 放弃：重载服务端已保存的分区/线（丢弃未保存修改）并回到查看模式
    const cancelEdit = useCallback(async () => {
      // mirror the render-time `dirty` derivation via the refs (this
      // callback is declared before the derived const)
      const dirtyNow =
        zonesRef.current.some((z) => z.isNew) || linesRef.current.some((l) => l.isNew)
      if (dirtyNow && !window.confirm('有未保存的修改，确定放弃并退出编辑？')) return
      await Promise.all([loadZones(), loadLines()])
      deletedRef.current = { zone: false, line: false }
      setDraft([]); setDraftLine([]); setSelZoneId(null); setSelLineId(null); setSelMemberId(null)
      setMode('view')
    }, [loadZones, loadLines])

    const save = useCallback(async (): Promise<boolean> => {
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
        // wipe guard: an empty list with no delete action this session is a
        // load artifact (extension reload / remount race), not intent —
        // writing it would erase the persisted set
        const zoneOk = zonePayload.length > 0 || deletedRef.current.zone
        const lineOk = linePayload.length > 0 || deletedRef.current.line
        const [zr, lr] = await Promise.all([
          zoneOk
            ? runExtensionCommand(extensionId, 'set_roi_zones', { zones: zonePayload })
            : Promise.resolve({ success: true } as const),
          lineOk
            ? runExtensionCommand(extensionId, 'set_lines', { lines: linePayload })
            : Promise.resolve({ success: true } as const),
        ])
        if (!mountedRef.current) return false
        if (zr.success && lr.success) {
          setSavedFlash(Date.now())
          deletedRef.current = { zone: false, line: false }
          loadZones()
          loadLines()
          return true
        }
        return false
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
    // exclusion zones share storage with equipment zones but are filters
    const eqZones = zones.filter((z) => z.equipment_type === 'exclusion')
    const opZones = zones.filter((z) => z.equipment_type !== 'exclusion')
    const editing = mode === 'edit'

    return (
      <div ref={ref} className={`gym-ov ${className}`}>
        <div className="gym-ov-card">
          <div className="gym-ov-header">
            <div className="gym-ov-compact">
              <span className={`gym-ov-dot ${status === 'streaming' ? 'on' : status === 'connecting' ? 'wait' : 'off'}`} />
              <span className="gym-ov-metric">{videoFps}fps</span>
              <span className="gym-ov-metric">{present}人</span>
              <span className="gym-ov-flex" />
              {!editing && (
                <>
                  {([[ 'boxes', showBoxes, setShowBoxes, '框'],
                     ['pose', showPose, setShowPose, '骨架'],
                     ['zones', showZones, setShowZones, '分区'],
                     ['mosaic', mosaic, setMosaic, '打码']] as const).map(
                    ([key, on, set, label]) => (
                      <button key={key} className={`gym-ov-tg ${on ? 'on' : ''}`}
                        onClick={() => set((v: boolean) => !v)}>{label}</button>
                    )
                  )}
                  <button className="gym-ov-tg gym-ov-tg-edit" onClick={() => { deletedRef.current = { zone: false, line: false }; setMode('edit'); setListOpen(true) }}>编辑</button>
                </>
              )}
              {editing && (
                <>
                  <span className="gym-ov-editkind">
                    {editKind === 'zones' ? '分区管理' : editKind === 'exclude' ? '无效区管理' : '计数线管理'}
                  </span>
                  <span className="gym-ov-tb-sep" />
                  <button className="gym-ov-tg" onClick={undo}
                    disabled={editKind === 'lines' ? draftLine.length === 0 : draft.length === 0}
                    title="撤销当前草稿的上一个点">撤销</button>
                  {(editKind === 'zones' || editKind === 'exclude') && (
                    <button className="gym-ov-tg" onClick={closeDraft} disabled={draft.length < 3}
                      title="把当前点串闭合为区域">闭合{draft.length}</button>
                  )}
                  <span className="gym-ov-flex" />
                  <button className={`gym-ov-tg ${listOpen ? 'on' : ''}`} onClick={() => setListOpen(!listOpen)}
                    title="收起/展开右侧列表，留出画面空间">{listOpen ? '隐藏列表' : '列表'}</button>
                  <span className="gym-ov-tb-sep" />
                  <button className="gym-ov-tg" onClick={cancelEdit}
                    title="丢弃未保存的修改，回到查看模式">取消</button>
                  <button className="gym-ov-tg" onClick={save} disabled={saving}
                    title="保存到服务器，继续编辑">{saving ? '…' : dirty ? '保存*' : '保存'}</button>
                  <button className="gym-ov-tg gym-ov-tg-save" onClick={async () => {
                    // 完成 = save & exit; on failure stay in the editor so
                    // the edit isn't silently lost
                    const ok = await save()
                    if (!ok) return
                    setMode('view'); setDraft([]); setDraftLine([]); setSelZoneId(null); setSelLineId(null); setSelMemberId(null)
                  }} disabled={saving} title="保存并返回查看模式">{saving ? '保存中…' : '完成'}</button>
                </>
              )}
            </div>
          </div>

          <div className="gym-ov-body">
            <canvas
              ref={canvasRef}
              className={`gym-ov-canvas ${editing ? 'editing' : ''}`}
              onClick={onCanvasClick}
              onDoubleClick={onCanvasDblClick}
              onPointerDown={onPointerDown}
              onPointerMove={onPointerMove}
              onPointerUp={onPointerUp}
              onPointerCancel={onPointerUp}
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
                  ? '拖顶点调形状 · 拖空心中点加点 · 区内拖动整体移动 · 双击顶点删除 · 点击空白画新分区'
                  : editKind === 'exclude'
                    ? '圈出镜子/无人区等无效区域——区域内的检测将被完全忽略（不跟踪、不计数、不入库）'
                    : editKind === 'lines'
                      ? '拖端点/线身调整 · 点击两点画计数线（a→b 为方向基准，↑=向左穿入）'
                      : `会员库（${members.length} 人）——点击条目展开编辑，回车保存改名；新人自动录入编号`}
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
            <div className={`gym-ov-zonelist ${listOpen ? '' : 'closed'}`}>
              <div className="gym-ov-drawtabs">
                <button className={`gym-ov-drawtab ${editKind === 'zones' ? 'on' : ''}`}
                  onClick={() => { setEditKind('zones'); setDraftLine([]); setSelLineId(null); setSelMemberId(null) }}>
                  分区管理<span className="gym-ov-drawtab-n">{zones.length}</span>
                </button>
                <button className={`gym-ov-drawtab ${editKind === 'lines' ? 'on' : ''}`}
                  onClick={() => { setEditKind('lines'); setDraft([]); setSelZoneId(null); setSelMemberId(null) }}>
                  计数线管理<span className="gym-ov-drawtab-n">{lines.length}</span>
                </button>
                <button className={`gym-ov-drawtab ${editKind === 'exclude' ? 'on' : ''}`}
                  onClick={() => { setEditKind('exclude'); setDraftLine([]); setSelLineId(null); setSelMemberId(null) }}>
                  无效区<span className="gym-ov-drawtab-n">{zones.filter((z) => z.equipment_type === 'exclusion').length}</span>
                </button>
              </div>
              {editKind === 'zones'
                ? (opZones.length === 0
                    ? [<span key="e" className="gym-ov-zonelist-empty">还没有分区——在画面上点出第一块器械区</span>]
                    : opZones.map((z, i) => (
                        <div key={z.id} className="gym-ov-rowwrap">
                          <div
                            className={`gym-ov-zonerow ${selZoneId === z.id ? 'sel' : ''}`}
                            onClick={() => setSelZoneId(selZoneId === z.id ? null : z.id)}>
                            <span className="gym-ov-zoneidx">{i + 1}</span>
                            <span className="gym-ov-zonename">{z.name || '未命名分区'}</span>
                            <span className="gym-ov-typetag">
                              {EQUIPMENT_PRESETS.find(([, v]) => v === z.equipment_type)?.[0] ?? z.equipment_type}
                            </span>
                          </div>
                          {selZoneId === z.id && (
                            <div className="gym-ov-rowins">
                              <input className="gym-ov-input name" value={z.name}
                                onChange={(e) =>
                                  setZones((zs) => zs.map((x) => (x.id === z.id ? { ...x, name: e.target.value } : x)))
                                } />
                              <GymSelect
                                value={z.equipment_type}
                                onChange={(v) =>
                                  setZones((zs) => zs.map((x) => (x.id === z.id ? { ...x, equipment_type: v } : x)))
                                }
                                options={[
                                  ...(!EQUIPMENT_PRESETS.some(([, v]) => v === z.equipment_type)
                                    ? [{ value: z.equipment_type, label: z.equipment_type }]
                                    : []),
                                  ...EQUIPMENT_PRESETS.map(([label, value]) => ({ value, label })),
                                ]}
                              />
                              <button className="gym-ov-btn danger"
                                onClick={() => { deletedRef.current.zone = true; setZones((zs) => zs.filter((x) => x.id !== z.id)); setSelZoneId(null) }}>删除</button>
                            </div>
                          )}
                        </div>
                      )))
                : (editKind === 'exclude'
                    ? (eqZones.length === 0
                        ? [<span key="e" className="gym-ov-zonelist-empty">还没有无效区——圈出镜子/无人区，区域内的检测将被忽略</span>]
                        : eqZones.map((z, i) => (
                            <div key={z.id} className="gym-ov-rowwrap">
                              <div
                                className={`gym-ov-zonerow ${selZoneId === z.id ? 'sel' : ''}`}
                                onClick={() => setSelZoneId(selZoneId === z.id ? null : z.id)}>
                                <span className="gym-ov-zoneidx excl">{i + 1}</span>
                                <span className="gym-ov-zonename">{z.name || '无效区'}</span>
                                <span className="gym-ov-typetag">检测忽略</span>
                              </div>
                              {selZoneId === z.id && (
                                <div className="gym-ov-rowins">
                                  <input className="gym-ov-input name" value={z.name}
                                    onChange={(e) =>
                                      setZones((zs) => zs.map((x) => (x.id === z.id ? { ...x, name: e.target.value } : x)))
                                    } />
                                  <button className="gym-ov-btn danger"
                                    onClick={() => { deletedRef.current.zone = true; setZones((zs) => zs.filter((x) => x.id !== z.id)); setSelZoneId(null) }}>删除</button>
                                </div>
                              )}
                            </div>
                          )))
                    : (editKind === 'lines'
                    ? (lines.length === 0
                        ? [<span key="e" className="gym-ov-zonelist-empty">还没有计数线——在画面上点两点画出出入口线</span>]
                        : lines.map((l, i) => {
                            const st = crossings.find((s) => s.line_id === l.id)
                            return (
                              <div key={l.id} className="gym-ov-rowwrap">
                                <div className={`gym-ov-zonerow ${selLineId === l.id ? 'sel' : ''}`}
                                  onClick={() => setSelLineId(selLineId === l.id ? null : l.id)}>
                                  <span className="gym-ov-zoneidx line">{i + 1}</span>
                                  <span className="gym-ov-zonename">{l.name || '未命名计数线'}</span>
                                  <span className="gym-ov-linecount">
                                    ↑{st?.in_count ?? 0} ↓{st?.out_count ?? 0}
                                  </span>
                                </div>
                                {selLineId === l.id && (
                                  <div className="gym-ov-rowins">
                                    <input className="gym-ov-input name" value={l.name}
                                      onChange={(e) =>
                                        setLines((ls) => ls.map((x) => (x.id === l.id ? { ...x, name: e.target.value } : x)))
                                      } />
                                    <button className="gym-ov-btn danger"
                                      onClick={() => { deletedRef.current.line = true; setLines((ls) => ls.filter((x) => x.id !== l.id)); setSelLineId(null) }}>删除</button>
                                  </div>
                                )}
                              </div>
                            )
                          }))
                    : (members.length === 0
                        ? [<span key="e" className="gym-ov-zonelist-empty">会员库为空——新人入画会自动录入特征；也可在查看模式点击人物注册</span>]
                        : members.map((m, i) => (
                            <div key={m.id} className="gym-ov-rowwrap">
                              <div className={`gym-ov-zonerow ${selMemberId === m.id ? 'sel' : ''}`}
                                onClick={() => setSelMemberId(selMemberId === m.id ? null : m.id)}>
                                {memberPhotoSrc(m.photo) ? (
                                  <img className="gym-ov-avatar" src={memberPhotoSrc(m.photo)!}
                                    alt={m.name} title={m.name} />
                                ) : (
                                  <span className="gym-ov-zoneidx member">{i + 1}</span>
                                )}
                                <span className="gym-ov-zonename">{m.name || '未命名'}</span>
                                <span className="gym-ov-typetag">
                                  {m.source === 'auto' ? '自动' : '手动'} · {m.samples ?? 1}样本
                                </span>
                              </div>
                              {selMemberId === m.id && (
                                <div className="gym-ov-rowins">
                                  <input className="gym-ov-input name" key={`rn-${m.id}`} autoFocus
                                    defaultValue={m.name}
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
                                  {merging === m.id ? (
                                    <GymSelect
                                      value=""
                                      autoOpen
                                      placeholder="并入哪位会员？"
                                      onClose={() => setMerging(null)}
                                      onChange={(v) => { if (v) doMerge(v) }}
                                      options={members
                                        .filter((x) => x.id !== m.id)
                                        .map((x) => ({ value: x.id, label: x.name }))}
                                    />
                                  ) : (
                                    <button className="gym-ov-btn" title="把此人的特征并入另一位会员（换装确认）"
                                      onClick={() => { setMerging(m.id); setSavedFlash(0) }}>并入</button>
                                  )}
                                  <button className="gym-ov-btn danger"
                                    onClick={() => { removeMember(m.id); setSelMemberId(null) }}>删除</button>
                                </div>
                              )}
                            </div>
                          )))))}
            </div>
          )}
        </div>
      </div>
    )
  }
)

GymVideoOverlay.displayName = 'GymVideoOverlay'
