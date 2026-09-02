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
  /** True when the device attached a body-ReID embedding (P3). */
  has_emb?: boolean
  /** Matched member via nearest-L2 over the member library (P3); null = unknown. */
  member?: { id: string; name: string; dist: number } | null
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

export async function fetchZones(
  extensionId: string
): Promise<{ success: boolean; data?: { zones: Zone[] }; error?: string }> {
  return runExtensionCommand<{ zones: Zone[] }>(extensionId, 'get_roi_zones')
}

export interface MetricPoint {
  timestamp: number
  value: number
}

export async function fetchMetricHistory(
  extensionId: string,
  metric: string,
  hours: number
): Promise<MetricPoint[]> {
  try {
    const res = await fetch(
      `${getApiBase()}/extensions/${extensionId}/metrics/${encodeURIComponent(
        metric
      )}/data?hours=${hours}`,
      { headers: getApiHeaders() }
    )
    if (!res.ok) return []
    const json = await res.json()
    return json?.data?.data ?? []
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
  created_at?: number | null
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

export async function deleteMember(
  extensionId: string,
  id: string
): Promise<{ success: boolean; error?: string }> {
  return runExtensionCommand(extensionId, 'delete_member', { id })
}
