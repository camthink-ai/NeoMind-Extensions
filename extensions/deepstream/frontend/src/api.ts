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
function getAuthToken(): string | null {
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

  // The host's command endpoint returns the raw command payload on success
  // (it does NOT wrap in { success, data }), so we synthesize that shape for
  // uniform handling. Errors from the extension runner arrive as HTTP 4xx/5xx
  // and are caught by the !response.ok branch above.
  const data = await response.json() as T;
  return { success: true, data };
}

// ---------------------------------------------------------------------------
// Snapshot / RTSP URL helpers
// ---------------------------------------------------------------------------

/**
 * Snapshot URL with cache-bust tick. The snapshot HTTP server runs alongside
 * the sidecar on a configurable port (default 8555) — callers usually learn
 * the port from the HelloAck frame. We default to 8555 to match the sidecar
 * bootstrap config.
 */
export function getSnapshotUrl(
  streamId: string,
  token: string,
  tick: number,
  snapshotPort = 8555,
): string {
  const origin = getApiOrigin();
  // Replace the API port with the snapshot port. getApiOrigin returns
  // `${proto}//${host}` (no port unless the host already has one), so we
  // append `:snapshotPort` for both Tauri and browser deployments.
  const hostWithPort = origin.replace(/^(https?:\/\/[^/:]+)(:\d+)?(.*)$/, `$1:${snapshotPort}`);
  return `${hostWithPort}/snapshot/${encodeURIComponent(streamId)}.jpg?token=${encodeURIComponent(token)}&t=${tick}`;
}

/**
 * RTSP URL for direct VLC / video-player handoff. The sidecar's RTSP server
 * defaults to port 8554 and mounts each stream at `/ds/<stream_id>`.
 */
export function getRtspUrl(
  streamId: string,
  host: string = typeof window !== 'undefined' ? window.location.hostname : 'localhost',
  port = 8554,
): string {
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
    executeCommand<{ stream_id: string; rtsp_url: string }>(
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
   *  on this projection — use getStreamInfo for the full record. */
  listStreams: () =>
    executeCommand<{ streams: Stream[] }>(
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
