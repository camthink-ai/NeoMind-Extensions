/**
 * Shared helpers for the gym-tracker component suite.
 *
 * All components talk to the same host REST API (proven pattern from
 * GymLiveState / weather-forecast): token from localStorage, extension
 * command dispatch, metric history query.
 */

export interface DataSource {
  type: string
  extensionId?: string
  command?: string
  [key: string]: any
}

export interface ExtensionComponentProps {
  title?: string
  dataSource?: DataSource
  className?: string
  /** configSchema fields are spread as individual props by the host */
  [key: string]: any
}

export interface Bbox {
  x: number
  y: number
  w: number
  h: number
}

export interface Point {
  x: number
  y: number
}

/** COCO-17 keypoint triplets as produced by gym-bridge (score 0 = missing). */
export type Kpt = [number, number, number]

export interface Pose {
  kpts: Kpt[]
  score: number
}

export interface Track {
  track_id: number
  bbox?: Bbox | null
  foot?: Point | null
  pose?: Pose | null
  face?: unknown
  /** Device-tracker EMA velocity, normalized units/sec (absent pre-2.9 producers). */
  vel?: [number, number] | null
  /** Per-track TRUE capture timestamp ns — far-field tile tracks carry the
   *  tile grab time (older than the frame ts); keying history by it removes
   *  the far-person box trail. Absent on older producers. */
  ts?: number | null
  /** True when the device attached a body-ReID embedding (P3). */
  has_emb?: boolean
  /** Matched member via nearest-L2 over the member library (P3); null = unknown. */
  member?: { id: string; name: string; dist: number; via?: string } | null
  /** Live workout (P4): classified exercise + rep/zone state. `zone_hold`
   *  = continuous seconds the SAME track has held its current zone — the
   *  equipment board's BUSY gate (a passer-by must not light a machine). */
  exercise?: {
    name: string; reps: number; sets: number; zone: string | null
    zone_hold?: number
  } | null
  /** Device exercise-engine metrics (14-action rep/hold FSM + windowed
   *  joint-angle stats). Present on 0.7.2+ producers once a track has
   *  ~8 frames of window data; preferred over `exercise` when both exist. */
  ex?: {
    reps: number
    depth_deg?: number | null
    symmetry_deg?: number | null
    tempo_hz?: number | null
    knee_min_deg?: number | null
    detected?: string | null
    lean_deg?: number | null
  } | null
}

export interface LiveState {
  present_count: number
  tracks: Track[]
  /** Frame-level face boxes (P2 privacy mosaic); absent on older extensions. */
  faces?: FaceBox[]
  members_count?: number
}

export interface FaceBox {
  bbox: Bbox
  det: number
}

/** get_frame / WS-push bundle — img_b64 is null until a PREVIEW-enabled
 * producer connects (binary push sessions deliver the JPEG separately, so
 * img_b64 is absent there). */
export interface FrameBundle {
  img_b64?: string | null
  /** Device capture time of the preview JPEG (ns since epoch). */
  ts_ns?: number
  /** TRUE capture time of the carried track positions (ns since epoch). */
  tracks_ts?: number
  faces?: FaceBox[]
  tracks?: Track[]
  present_count?: number
  members_count?: number
}

/** data_type of the binary frame container pushed by the extension. */
export const FRAME_DATA_TYPE = 'application/x-neomind-frame'

/** data_type of hardware-H.264 relay frames (Annex-B access units). */
export const AVC_DATA_TYPE = 'video/avc'
/** tracks/faces-only bundle (change-driven; video frames carry no tracks). */
export const TRACKS_DATA_TYPE = 'application/x-neomind-tracks'

export interface Zone {
  id: string
  name: string
  equipment_type: string
  polygon: number[][] | [number, number][]
  enabled: boolean | number
}

export const DEFAULT_EXTENSION_ID = 'gym-tracker'

/** Stream-player extension id — the video source for the overlay component. */
export const VIDEO_EXTENSION_ID = 'stream-player'

export const getApiBase = (): string =>
  (window as any).__TAURI_INTERNALS__
    ? 'http://localhost:9375/api'
    : '/api'

export const getApiHeaders = (): Record<string, string> => {
  const token =
    localStorage.getItem('neomind_token') ||
    sessionStorage.getItem('neomind_session_token') ||
    sessionStorage.getItem('neomind_token_session')
  const headers: Record<string, string> = { 'Content-Type': 'application/json' }
  if (token) headers['Authorization'] = `Bearer ${token}`
  return headers
}

export const getToken = (): string | null =>
  localStorage.getItem('neomind_token') ||
  sessionStorage.getItem('neomind_token_session')

export async function runExtensionCommand<T>(
  extensionId: string,
  command: string,
  args: Record<string, any> = {}
): Promise<{ success: boolean; data?: T; error?: string }> {
  try {
    const res = await fetch(
      `${getApiBase()}/extensions/${extensionId}/command`,
      {
        method: 'POST',
        headers: getApiHeaders(),
        body: JSON.stringify({ command, args }),
      }
    )
    if (res.status === 401) {
      return { success: false, error: '登录已过期，请重新登录（401）' }
    }
    if (!res.ok) return { success: false, error: `HTTP ${res.status}` }
    return res.json()
  } catch (e) {
    return {
      success: false,
      error: e instanceof Error ? e.message : 'Network error',
    }
  }

}

export async function fetchLiveState(
  extensionId: string
): Promise<{ success: boolean; data?: LiveState; error?: string }> {
  return runExtensionCommand<LiveState>(extensionId, 'get_live_state')
}

/** Single-source frame bundle: preview JPEG + the tracks OF THAT frame. */
export async function fetchFrame(
  extensionId: string
): Promise<{ success: boolean; data?: FrameBundle | null; error?: string }> {
  return runExtensionCommand<FrameBundle | null>(extensionId, 'get_frame')
}

export async function fetchZones(
  extensionId: string
): Promise<{ success: boolean; data?: { zones: Zone[] }; error?: string }> {
  return runExtensionCommand<{ zones: Zone[] }>(extensionId, 'get_roi_zones')
}

/** Extension-level config (PUT /api/extensions/:id/config shape) — carries
 *  the GLOBAL ui.* settings (language, mosaic default). Cached briefly;
 *  widgets pass ui.language into useLang as the card-level fallback. */
let extConfigCache: { at: number; cfg: Record<string, unknown> } | null = null
export async function fetchExtensionUiConfig(
  extensionId: string
): Promise<Record<string, unknown>> {
  const now = Date.now()
  if (extConfigCache && now - extConfigCache.at < 10000) return extConfigCache.cfg
  try {
    const res = await fetch(
      `${getApiBase()}/extensions/${extensionId}/config`,
      { headers: getApiHeaders() }
    )
    if (!res.ok) return {}
    const json = await res.json()
    const cfg = json?.data?.current_config ?? {}
    extConfigCache = { at: now, cfg }
    return cfg
  } catch {
    return {}
  }
}

export interface MetricPoint {
  /** Normalized to epoch MILLISECONDS (the server stores seconds —
   * `new Date(point.timestamp)` must "just work" for callers). */
  timestamp: number
  value: number
}

export async function fetchMetricHistory(
  extensionId: string,
  metric: string,
  hours: number
): Promise<MetricPoint[]> {
  try {
    // `start`/`end` are Unix SECONDS on this API. The legacy `hours=`
    // query key is NOT part of the server's TimeRangeQuery — it was
    // silently ignored and every query fell back to a 24 h window while
    // the chart claimed "last {hours}h".
    const start = Math.floor(Date.now() / 1000) - hours * 3600
    const res = await fetch(
      `${getApiBase()}/extensions/${extensionId}/metrics/${encodeURIComponent(
        metric
      )}/data?start=${start}&limit=1000`,
      { headers: getApiHeaders() }
    )
    if (!res.ok) return []
    const json = await res.json()
    const raw: Array<{ timestamp: number; value: number }> = json?.data?.data ?? []
    // Unit-agnostic: seconds (< ~2001-09 in ms terms) become ms; already-ms
    // values (any server that switches) pass through untouched.
    return raw.map((p) => ({
      timestamp: p.timestamp > 1e12 ? p.timestamp : p.timestamp * 1000,
      value: p.value,
    }))
  } catch {
    return []
  }
}

/** Ray-casting point-in-polygon on normalized coords (mirrors geo.rs PNPOLY). */
export function pointInPolygon(
  px: number,
  py: number,
  polygon: Array<number[]> | undefined | null
): boolean {
  if (!polygon || polygon.length < 3) return false
  let inside = false
  let j = polygon.length - 1
  for (let i = 0; i < polygon.length; i++) {
    const xi = polygon[i][0]
    const yi = polygon[i][1]
    const xj = polygon[j][0]
    const yj = polygon[j][1]
    if (yi > py !== yj > py) {
      const xInt = ((xj - xi) * (py - yi)) / (yj - yi + 1e-12) + xi
      if (px < xInt) inside = !inside
    }
    j = i
  }
  return inside
}

/** COCO-17 skeleton edges (index pairs), mirroring pose.py SKELETON_EDGES. */
export const SKELETON_EDGES: Array<[number, number]> = [
  [0, 1], [1, 3], [0, 2], [2, 4],
  [5, 6],
  [5, 7], [7, 9],
  [6, 8], [8, 10],
  [11, 12],
  [5, 11], [6, 12],
  [11, 13], [13, 15],
  [12, 14], [14, 16],
]

/** Inject a raw CSS string once, deduplicated by element id. */
export function injectStyles(id: string, css: string): void {
  if (typeof document === 'undefined' || document.getElementById(id)) return
  const style = document.createElement('style')
  style.id = id
  style.textContent = css
  document.head.appendChild(style)
}

export const clamp01 = (v: number): number => {
  if (Number.isNaN(v)) return 0
  return Math.min(1, Math.max(0, v))
}

/** Crossing line as stored by the extension (set_lines/get_lines). */
export interface LineDef {
  id: string
  name: string
  a: number[]
  b: number[]
}

export interface LineStats {
  line_id: string
  name: string
  in_count: number
  out_count: number
  day: number
}

export async function fetchLines(
  extensionId: string
): Promise<{ success: boolean; data?: { lines: LineDef[] }; error?: string }> {
  return runExtensionCommand<{ lines: LineDef[] }>(extensionId, 'get_lines')
}

export async function fetchCrossings(
  extensionId: string
): Promise<{ success: boolean; data?: { lines: LineStats[] }; error?: string }> {
  return runExtensionCommand<{ lines: LineStats[] }>(extensionId, 'get_crossings')
}

export async function fetchHeatmap(
  extensionId: string
): Promise<{ success: boolean; data?: { cols: number; rows: number; day: number; grid: number[] }; error?: string }> {
  return runExtensionCommand<{ cols: number; rows: number; day: number; grid: number[] }>(extensionId, 'get_heatmap')
}

// ---- P3: member library (body-ReID) ----

export interface Member {
  id: string
  name: string
  dim: number
  /** "manual" (registered from the Monitor) | "auto" (auto-enrolled unknown) */
  source?: string
  /** Total embeddings in the member's library (primary + accumulated). */
  samples?: number
  /** Avatar thumbnail — data URL or raw base64 JPEG; absent until captured. */
  photo?: string | null
  created_at?: number | null
}

/** Normalize a photo value (raw base64 or data URL) into an <img> src. */
export function memberPhotoSrc(photo?: string | null): string | null {
  if (!photo) return null
  return photo.startsWith('data:') ? photo : `data:image/jpeg;base64,${photo}`
}

export async function registerMember(
  extensionId: string,
  trackId: number,
  name: string
): Promise<{ success: boolean; data?: { member: { id: string; name: string; dim: number } }; error?: string }> {
  return runExtensionCommand(extensionId, 'register_member', { track_id: trackId, name })
}

export async function fetchMembers(
  extensionId: string
): Promise<{ success: boolean; data?: { members: Member[] }; error?: string }> {
  return runExtensionCommand<{ members: Member[] }>(extensionId, 'list_members')
}

export async function mergeMembers(
  extensionId: string,
  srcId: string,
  dstId: string
): Promise<{ success: boolean; data?: { name: string; samples: number }; error?: string }> {
  return runExtensionCommand(extensionId, 'merge_members', { src_id: srcId, dst_id: dstId })
}

export async function renameMember(
  extensionId: string,
  id: string,
  name: string
): Promise<{ success: boolean; error?: string }> {
  return runExtensionCommand(extensionId, 'rename_member', { id, name })
}

export async function setMemberPhoto(
  extensionId: string,
  id: string,
  /** raw base64 JPEG (no data: prefix) or null to clear */
  photoBase64: string | null
): Promise<{ success: boolean; error?: string }> {
  return runExtensionCommand(extensionId, 'set_member_photo', {
    id, photo_base64: photoBase64 ?? '',
  })
}

export async function deleteMember(
  extensionId: string,
  id: string
): Promise<{ success: boolean; error?: string }> {
  return runExtensionCommand(extensionId, 'delete_member', { id })
}
