// API helpers for invoking deepstream extension commands.
//
// Pattern mirrors yolo-video-v2/frontend/src/index.tsx:
//   const isTauri = !!(window as any).__TAURI_INTERNALS__
//   const host = isTauri ? 'localhost:9375' : window.location.host
//   `${protocol}//${host}/api/extensions/${extensionId}/command`
//
// Auth token (when present) is forwarded as `Authorization: Bearer <token>`
// exactly like yolo-video-v2 does for its update_stream_config call — without
// it, command requests 401 silently and the user-visible state never updates.

import type { Stream, ModelInfo, SystemStatus } from './types';

/** Extension id used in the command URL. */
export const DEEPSTREAM_EXTENSION_ID = 'deepstream';

/**
 * Resolve the API origin (no trailing /api). In Tauri the host app is served
 * from the tauri:// scheme but the REST API lives on localhost:9375; in a
 * browser deployment the API is same-origin with the dashboard.
 */
export function getApiOrigin(): string {
  const isTauri = typeof window !== 'undefined' && !!(window as any).__TAURI_INTERNALS__;
  if (isTauri) {
    return 'http://localhost:9375';
  }
  if (typeof window !== 'undefined') {
    const proto = window.location.protocol === 'https:' ? 'https:' : 'http:';
    return `${proto}//${window.location.host}`;
  }
  // SSR / non-browser fallback — should not normally be hit.
  return 'http://localhost:9375';
}

/** Same-origin `/api` base used by command + snapshot calls. */
export function getApiBase(): string {
  return `${getApiOrigin()}/api`;
}

/** Read the NeoMind auth token from any of the storage keys the host uses. */
export function getAuthToken(): string | null {
  if (typeof window === 'undefined') return null;
  return (
    localStorage.getItem('neomind_token') ||
    sessionStorage.getItem('neomind_token_session') ||
    null
  );
}

/** Build the headers required for an authenticated command request. */
function authHeaders(): Record<string, string> {
  const headers: Record<string, string> = { 'Content-Type': 'application/json' };
  const token = getAuthToken();
  if (token) headers['Authorization'] = `Bearer ${token}`;
  return headers;
}

/**
 * Generic extension command invocation.
 *
 * The host wraps every command result; on success `{ success: true, data: ... }`,
 * on error `{ success: false, error: "..." }`. We surface both shapes — callers
 * can either `.then(r => r.data)` (trusting the host) or branch on `success`.
 */
export async function executeCommand<T = unknown>(
  extensionId: string,
  command: string,
  args: Record<string, unknown> = {},
): Promise<{ success: boolean; data?: T; error?: string }> {
  const response = await fetch(`${getApiBase()}/extensions/${extensionId}/command`, {
    method: 'POST',
    headers: authHeaders(),
    body: JSON.stringify({ command, args }),
  });

  if (!response.ok) {
    let body = '';
    try { body = await response.text(); } catch { /* ignore */ }
    return {
      success: false,
      error: `HTTP ${response.status}${body ? `: ${body}` : ''}`,
    };
  }

  // The host wraps command results in { success: true, data: <payload>, meta }.
  // Unwrap the host envelope so callers receive the raw extension payload.
  const json = await response.json() as any;
  if (json && typeof json === 'object' && 'success' in json && 'data' in json) {
    return { success: json.success !== false, data: json.data as T, error: json.error };
  }
  return { success: true, data: json as T };
}

// ---------------------------------------------------------------------------
// Server configuration — allows the DeepStream sidecar to run on a different
// host than the NeoMind dashboard (e.g., Jetson at 192.168.93.20 while NeoMind
// runs on the user's Mac). When `host` is empty/undefined, URLs are derived
// from the dashboard's own origin (backward-compat).
// ---------------------------------------------------------------------------

export interface ServerConfig {
  /** DeepStream server IP or hostname (e.g., "192.168.93.20"). */
  host?: string;
  /** Snapshot HTTP port (default 8555). */
  snapshotPort?: number;
  /** RTSP port (default 8554). */
  rtspPort?: number;
}

/**
 * Snapshot URL with cache-bust tick. When `server.host` is set, the snapshot
 * is fetched directly from the DeepStream server. Otherwise the URL is derived
 * from the dashboard's origin (same host, snapshot port appended).
 */
export function getSnapshotUrl(
  streamId: string,
  token: string,
  tick: number,
  server?: ServerConfig,
): string {
  const port = server?.snapshotPort ?? 8555;
  const path = `/snapshot/${encodeURIComponent(streamId)}.jpg?token=${encodeURIComponent(token)}&t=${tick}`;

  if (server?.host) {
    return `http://${server.host}:${port}${path}`;
  }

  // Fall back to deriving from API origin (same host, replace port).
  const origin = getApiOrigin();
  const hostWithPort = origin.replace(/^(https?:\/\/[^/:]+)(:\d+)?(.*)$/, `$1:${port}`);
  return `${hostWithPort}${path}`;
}

/**
 * RTSP URL for direct VLC / video-player handoff. When `server.host` is set,
 * the RTSP URL points to the DeepStream server; otherwise it falls back to the
 * dashboard's hostname.
 */
export function getRtspUrl(
  streamId: string,
  server?: ServerConfig,
): string {
  const port = server?.rtspPort ?? 8554;
  const host = server?.host ?? (typeof window !== 'undefined' ? window.location.hostname : 'localhost');
  return `rtsp://${host}:${port}/ds/${encodeURIComponent(streamId)}`;
}

// ---------------------------------------------------------------------------
// Typed convenience wrappers for the 10 deepstream commands
// (see lib.rs execute_command match arms for the exact payload shapes)
// ---------------------------------------------------------------------------

export interface AddStreamArgs {
  stream_id: string;
  // Accept either the wrapper form `{ stream_id, config: {...} }` or a bare
  // StreamConfig object — the host's cmd_add_stream handles both.
  config: Record<string, unknown>;
}

export interface UpdateAnalyticsArgs {
  stream_id: string;
  config: { line_crossing?: unknown[]; roi?: unknown[] };
}

export interface SetThresholdArgs {
  stream_id: string;
  conf?: number;   // default 0.5 on the host
  iou?: number;    // default 0.45 on the host
}

export interface RegisterModelArgs {
  id: string;
  name: string;
  engine_path: string;
  labels_path?: string;
  input_shape?: [number, number, number];
  precision?: 'fp16' | 'int8' | 'fp32';
}

export const dsCommands = {
  /** Add a stream — returns the sidecar-assigned rtsp_url. */
  addStream: (args: AddStreamArgs) =>
    executeCommand<{ stream_id: string; rtsp_url: string; snapshot_token: string }>(
      DEEPSTREAM_EXTENSION_ID,
      'add_stream',
      args as unknown as Record<string, unknown>,
    ),

  /** Remove a stream by id. */
  removeStream: (stream_id: string) =>
    executeCommand<{ removed: string }>(
      DEEPSTREAM_EXTENSION_ID,
      'remove_stream',
      { stream_id },
    ),

  /** Snapshot of all streams. `config` and `last_transition_at` are absent
   *  on this projection — use getStreamInfo for the full record.
   *  `server_host` is the extension-level default server address (may be empty). */
  listStreams: () =>
    executeCommand<{ streams: Stream[]; server_host?: string }>(
      DEEPSTREAM_EXTENSION_ID,
      'list_streams',
      {},
    ),

  /** Detailed info for one stream (includes source + last_transition_at). */
  getStreamInfo: (stream_id: string) =>
    executeCommand<Stream>(
      DEEPSTREAM_EXTENSION_ID,
      'get_stream_info',
      { stream_id },
    ),

  /** Hot-swap line-crossing / ROI config on a running stream. */
  updateAnalytics: (args: UpdateAnalyticsArgs) =>
    executeCommand<{ applied: string }>(
      DEEPSTREAM_EXTENSION_ID,
      'update_analytics',
      args as unknown as Record<string, unknown>,
    ),

  /** Hot-swap model conf / iou thresholds. */
  setThreshold: (args: SetThresholdArgs) =>
    executeCommand<{ applied: string }>(
      DEEPSTREAM_EXTENSION_ID,
      'set_threshold',
      args as unknown as Record<string, unknown>,
    ),

  /** List preset + user-registered models. */
  listModels: () =>
    executeCommand<{ models: ModelInfo[] }>(
      DEEPSTREAM_EXTENSION_ID,
      'list_models',
      {},
    ),

  /** Register a user-provided model. */
  registerModel: (args: RegisterModelArgs) =>
    executeCommand<{ registered: string }>(
      DEEPSTREAM_EXTENSION_ID,
      'register_model',
      args as unknown as Record<string, unknown>,
    ),

  /** Restart the Python sidecar (replays active streams). */
  restartSidecar: () =>
    executeCommand<unknown>(DEEPSTREAM_EXTENSION_ID, 'restart_sidecar', {}),

  /** Run pre-flight checks. */
  diagnose: () =>
    executeCommand<SystemStatus>(DEEPSTREAM_EXTENSION_ID, 'diagnose', {}),
};
