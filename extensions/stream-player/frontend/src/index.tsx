/**
 * Stream Player - Universal Video Player Extension
 *
 * Renders video streams from RTSP/RTMP/HLS/File sources.
 * Supports data source binding — bind a device metric (e.g. stream_url)
 * to auto-populate the source URL.
 */

import React, { forwardRef, useState, useEffect, useRef, useCallback } from 'react'

// ============================================================================
// Types
// ============================================================================

interface DataSourceBinding {
  type: string
  // Device-metric binding
  sourceId?: string
  deviceId?: string
  property?: string
  // Extension binding
  extensionId?: string
  extensionMetric?: string
  // Static value
  staticValue?: unknown
  // Other fields
  [key: string]: unknown
}

export interface ExtensionComponentProps {
  title?: string
  dataSource?: DataSourceBinding
  className?: string
  config?: {
    defaultSource?: string
    targetFps?: number
    outputWidth?: number
    outputHeight?: number
  }
}

type PlayerStatus = 'idle' | 'connecting' | 'streaming' | 'paused' | 'error' | 'ended'

// ============================================================================
// Constants
// ============================================================================

const EXTENSION_ID = 'stream-player'

// ============================================================================
// SVG Icons
// ============================================================================

const PlayIcon = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <polygon points="5 3 19 12 5 21 5 3" />
  </svg>
)

const StopIcon = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <rect x="3" y="3" width="18" height="18" rx="2" ry="2" />
  </svg>
)

const LinkIcon = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71" />
    <path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71" />
  </svg>
)

const DatabaseIcon = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <ellipse cx="12" cy="5" rx="9" ry="3" />
    <path d="M3 5V19A9 3 0 0 0 21 19V5" />
    <path d="M3 12A9 3 0 0 0 21 12" />
  </svg>
)

// ============================================================================
// API helpers
// ============================================================================

function getApiBase(): string {
  if (typeof window !== 'undefined' && (window as any).__TAURI_INTERNALS__) {
    return 'http://localhost:9375/api'
  }
  return '/api'
}

async function fetchDeviceMetric(deviceId: string, property: string): Promise<string | null> {
  try {
    const token = localStorage.getItem('neomind_token')
      || sessionStorage.getItem('neomind_token_session')
    const headers: Record<string, string> = { 'Content-Type': 'application/json' }
    if (token) headers['Authorization'] = `Bearer ${token}`

    const resp = await fetch(`${getApiBase()}/devices/${encodeURIComponent(deviceId)}/current`, { headers })
    if (!resp.ok) return null

    const data = await resp.json()
    // Navigate to metrics.{property}.value
    const metrics = data?.metrics || data?.data?.metrics
    if (!metrics) return null

    const metric = metrics[property]
    if (metric == null) return null

    // Metric can be { value: "rtsp://..." } or just the value directly
    const value = typeof metric === 'object' && metric !== null && 'value' in metric
      ? metric.value
      : metric

    return typeof value === 'string' ? value : String(value)
  } catch {
    return null
  }
}

// ============================================================================
// Resolve data source to stream URL
// ============================================================================

function resolveBoundSource(ds: DataSourceBinding | undefined): {
  bound: boolean
  deviceId?: string
  property?: string
  staticUrl?: string
} {
  if (!ds) return { bound: false }

  // Device-metric binding: type starts with "device"
  if (ds.type?.startsWith('device') && (ds.sourceId || ds.deviceId) && ds.property) {
    return {
      bound: true,
      deviceId: ds.sourceId || ds.deviceId,
      property: ds.property,
    }
  }

  // Static value binding
  if (ds.staticValue && typeof ds.staticValue === 'string' && ds.staticValue.trim()) {
    return { bound: true, staticUrl: ds.staticValue }
  }

  return { bound: false }
}

// ============================================================================
// Styles
// ============================================================================

const STYLES = `
.sp {
  --sp-fg: var(--foreground);
  --sp-muted: var(--muted-foreground);
  --sp-accent: var(--primary);
  --sp-accent-fg: var(--primary-foreground);
  --sp-bg: var(--card);
  --sp-border: var(--border);
  --sp-success: var(--color-success, #22c55e);
  --sp-danger: var(--color-error, #ef4444);
  --sp-warning: var(--color-warning, #f59e0b);

  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  border-radius: var(--radius, 8px);
  overflow: hidden;
  background: var(--sp-bg);
  color: var(--sp-fg);
  font-family: inherit;
  font-size: 13px;
  border: 1px solid var(--sp-border);
}

.sp-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 12px;
  border-bottom: 1px solid var(--sp-border);
  gap: 8px;
  flex-shrink: 0;
}

.sp-header-left {
  display: flex;
  align-items: center;
  gap: 8px;
  font-weight: 600;
  font-size: 13px;
}

.sp-header-right {
  display: flex;
  align-items: center;
  gap: 6px;
}

.sp-live-badge {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 2px 8px;
  border-radius: 9999px;
  font-size: 10px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.sp-live-badge.live {
  background: var(--sp-danger);
  color: #fff;
}

.sp-live-badge.live .sp-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: #fff;
  animation: sp-pulse 1.5s ease-in-out infinite;
}

.sp-live-badge.connecting {
  background: var(--sp-warning);
  color: #000;
}

.sp-live-badge.ended {
  background: var(--sp-muted);
  color: var(--sp-bg);
}

.sp-live-badge.error {
  background: var(--sp-danger);
  color: #fff;
}

.sp-live-badge.idle {
  background: var(--sp-border);
  color: var(--sp-muted);
}

@keyframes sp-pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.4; }
}

.sp-canvas-wrap {
  flex: 1;
  min-height: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: #000;
  position: relative;
  overflow: hidden;
}

.sp-canvas-wrap canvas {
  max-width: 100%;
  max-height: 100%;
  object-fit: contain;
}

.sp-placeholder {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  color: var(--sp-muted);
  font-size: 13px;
  padding: 20px;
  text-align: center;
}

.sp-placeholder svg {
  opacity: 0.3;
}

.sp-controls {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 12px;
  border-top: 1px solid var(--sp-border);
  flex-shrink: 0;
}

.sp-url-input {
  flex: 1;
  min-width: 0;
  padding: 6px 10px;
  border-radius: var(--radius, 6px);
  border: 1px solid var(--sp-border);
  background: var(--sp-bg);
  color: var(--sp-fg);
  font-size: 12px;
  font-family: monospace;
  outline: none;
  transition: border-color 0.15s;
}

.sp-url-input:focus {
  border-color: var(--sp-accent);
}

.sp-url-input::placeholder {
  color: var(--sp-muted);
}

/* Bound data source input — read-only style with accent border */
.sp-url-input.bound {
  border-color: var(--sp-accent);
  background: color-mix(in srgb, var(--sp-accent) 8%, var(--sp-bg));
  opacity: 0.9;
  cursor: default;
}

.sp-url-input:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.sp-bound-tag {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 2px 6px;
  border-radius: 4px;
  font-size: 9px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.5px;
  background: var(--sp-accent);
  color: var(--sp-accent-fg);
  white-space: nowrap;
  flex-shrink: 0;
}

.sp-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
  padding: 6px 10px;
  border-radius: var(--radius, 6px);
  border: 1px solid var(--sp-border);
  background: var(--sp-bg);
  color: var(--sp-fg);
  cursor: pointer;
  font-size: 12px;
  transition: all 0.15s;
  white-space: nowrap;
}

.sp-btn:hover {
  background: var(--sp-accent);
  color: var(--sp-accent-fg);
  border-color: var(--sp-accent);
}

.sp-btn.primary {
  background: var(--sp-accent);
  color: var(--sp-accent-fg);
  border-color: var(--sp-accent);
}

.sp-btn.primary:hover {
  opacity: 0.9;
}

.sp-btn.danger {
  border-color: var(--sp-danger);
  color: var(--sp-danger);
}

.sp-btn.danger:hover {
  background: var(--sp-danger);
  color: #fff;
}

.sp-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.sp-stats {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 10px;
  color: var(--sp-muted);
  white-space: nowrap;
}

.sp-stats span {
  font-variant-numeric: tabular-nums;
}

.sp-error {
  padding: 4px 12px;
  font-size: 11px;
  color: var(--sp-danger);
  background: rgba(239, 68, 68, 0.1);
  border-top: 1px solid rgba(239, 68, 68, 0.2);
}
`

// ============================================================================
// Component
// ============================================================================

export const StreamPlayerCard = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function StreamPlayerCard(props, ref) {
    const { title = 'Stream Player', dataSource, className = '', config } = props
    const extensionId = dataSource?.extensionId || EXTENSION_ID

    // Resolve data source binding
    const binding = resolveBoundSource(dataSource)
    const isBound = binding.bound

    const [sourceUrl, setSourceUrl] = useState(config?.defaultSource || '')
    const [boundUrl, setBoundUrl] = useState<string | null>(null)
    const [status, setStatus] = useState<PlayerStatus>('idle')
    const [error, setError] = useState<string | null>(null)
    const [fps, setFps] = useState(0)
    const [frameCount, setFrameCount] = useState(0)
    const [hasVideo, setHasVideo] = useState(false)

    const wsRef = useRef<WebSocket | null>(null)
    const sessionIdRef = useRef<string | null>(null)
    const fpsCounterRef = useRef({ frames: 0, lastTime: Date.now() })
    const imgRef = useRef<HTMLImageElement>(null)
    const pendingFrameRef = useRef<string | null>(null)
    const autoStartAttempted = useRef(false)

    // The effective URL to use for streaming
    const effectiveUrl = isBound ? (boundUrl || '') : sourceUrl

    // WebSocket URL
    const getWebSocketUrl = useCallback(() => {
      const isTauri = !!(window as any).__TAURI_INTERNALS__
      const protocol = (isTauri ? false : window.location.protocol === 'https:') ? 'wss:' : 'ws:'
      const host = isTauri ? 'localhost:9375' : window.location.host
      const baseUrl = `${protocol}//${host}/api/extensions/${extensionId}/stream`
      const token = localStorage.getItem('neomind_token')
        || sessionStorage.getItem('neomind_token_session')
      if (token) {
        return `${baseUrl}?token=${encodeURIComponent(token)}`
      }
      return baseUrl
    }, [extensionId])

    // FPS counter
    const updateFps = useCallback(() => {
      fpsCounterRef.current.frames++
      const now = Date.now()
      const elapsed = now - fpsCounterRef.current.lastTime
      if (elapsed >= 1000) {
        setFps(Math.round(fpsCounterRef.current.frames * 1000 / elapsed))
        fpsCounterRef.current.frames = 0
        fpsCounterRef.current.lastTime = now
      }
      setFrameCount(prev => prev + 1)
    }, [])

    // rAF loop
    useEffect(() => {
      let rafId: number
      const tick = () => {
        if (pendingFrameRef.current && imgRef.current) {
          imgRef.current.src = pendingFrameRef.current
          pendingFrameRef.current = null
        }
        rafId = requestAnimationFrame(tick)
      }
      rafId = requestAnimationFrame(tick)
      return () => cancelAnimationFrame(rafId)
    }, [])

    // Stop streaming
    const stopStream = useCallback(() => {
      if (wsRef.current) {
        wsRef.current.close()
        wsRef.current = null
      }
      sessionIdRef.current = null
      pendingFrameRef.current = null
      if (imgRef.current) imgRef.current.src = ''
      setHasVideo(false)
      setStatus('idle')
      setError(null)
      setFps(0)
      setFrameCount(0)
    }, [])

    // Start streaming (accepts optional URL override)
    const startStream = useCallback((urlOverride?: string) => {
      const url = urlOverride || effectiveUrl
      if (!url.trim()) {
        setError('Please enter a source URL')
        return
      }

      stopStream()

      setStatus('connecting')
      setError(null)

      const ws = new WebSocket(getWebSocketUrl())
      ws.binaryType = 'arraybuffer'
      wsRef.current = ws

      ws.onopen = () => {
        const initMsg = {
          type: 'init',
          config: {
            source_url: url,
            target_fps: config?.targetFps || 24,
            output_width: config?.outputWidth || 640,
            output_height: config?.outputHeight || 480,
            video_bitrate: 1500,
            loop_file: true,
          },
        }
        ws.send(JSON.stringify(initMsg))
      }

      ws.onmessage = (event) => {
        if (typeof event.data !== 'string') return

        try {
          const msg = JSON.parse(event.data)

          switch (msg.type) {
            case 'session_created':
              sessionIdRef.current = msg.session_id
              ws.send(JSON.stringify({ type: 'start_push', session_id: msg.session_id }))
              break

            case 'push_output':
              if (msg.data_type === 'application/json' && msg.data) {
                try {
                  const statusData = typeof msg.data === 'string' ? JSON.parse(msg.data) : msg.data
                  if (statusData.type === 'status') {
                    const s = statusData.status
                    if (s === 'streaming') setStatus('streaming')
                    else if (s === 'connecting' || s === 'reconnecting') setStatus('connecting')
                    else if (s === 'ended') { setStatus('ended'); }
                    else if (s === 'looping') { /* silently continue */ }
                  } else if (statusData.type === 'error') {
                    setStatus('error')
                    setError(statusData.message || 'Stream error')
                  }
                } catch { /* ignore */ }
                break
              }

              if ((msg.data_type === 'image/jpeg' || msg.data_type === 'video/mpeg1') && msg.data) {
                pendingFrameRef.current = `data:image/jpeg;base64,${msg.data}`
                if (!hasVideo) {
                  setHasVideo(true)
                  setStatus('streaming')
                }
                updateFps()
              }
              break

            case 'error':
              setStatus('error')
              setError(`${msg.code || 'Error'}: ${msg.message || 'Unknown error'}`)
              break

            case 'session_closed':
              setStatus('idle')
              sessionIdRef.current = null
              break
          }
        } catch (e) {
          console.error('[StreamPlayer] Message parse error:', e)
        }
      }

      ws.onerror = () => {
        setStatus('error')
        setError('WebSocket connection failed')
      }

      ws.onclose = () => {
        if (status === 'streaming' || status === 'connecting') {
          setStatus('idle')
        }
        wsRef.current = null
        sessionIdRef.current = null
      }
    }, [effectiveUrl, config, extensionId, getWebSocketUrl, stopStream, updateFps, hasVideo, status])

    // ---- Data source binding: fetch device metric & auto-start ----
    useEffect(() => {
      if (!isBound) return

      let cancelled = false

      async function resolveAndStart() {
        // Static value
        if (binding.staticUrl) {
          if (!cancelled) {
            setBoundUrl(binding.staticUrl)
            setSourceUrl(binding.staticUrl)
          }
          return
        }

        // Device metric
        if (binding.deviceId && binding.property) {
          const url = await fetchDeviceMetric(binding.deviceId, binding.property)
          if (cancelled) return

          if (url) {
            setBoundUrl(url)
            setSourceUrl(url)
          } else {
            setError(`Bound metric "${binding.property}" not found on device`)
          }
        }
      }

      resolveAndStart()
      return () => { cancelled = true }
    }, [isBound, binding.staticUrl, binding.deviceId, binding.property])

    // Auto-start when bound URL becomes available
    useEffect(() => {
      if (!isBound || !boundUrl || autoStartAttempted.current) return
      autoStartAttempted.current = true
      startStream(boundUrl)
    }, [isBound, boundUrl, startStream])

    // Reset auto-start when data source changes
    useEffect(() => {
      autoStartAttempted.current = false
    }, [dataSource])

    // Cleanup on unmount
    useEffect(() => {
      return () => { stopStream() }
    }, [stopStream])

    // Handle Enter key in input (only when not bound)
    const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
      if (e.key === 'Enter' && !isBound && status !== 'streaming' && status !== 'connecting') {
        startStream()
      }
    }, [startStream, status, isBound])

    // Status badge
    const statusLabel: Record<PlayerStatus, string> = {
      idle: 'IDLE',
      connecting: 'CONNECTING',
      streaming: 'LIVE',
      paused: 'PAUSED',
      error: 'ERROR',
      ended: 'ENDED',
    }

    // Whether the input should be disabled
    const inputDisabled = isBound || status === 'streaming' || status === 'connecting'

    return (
      <div ref={ref} className={`sp ${className}`}>
        <style>{STYLES}</style>

        {/* Header */}
        <div className="sp-header">
          <div className="sp-header-left">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <circle cx="12" cy="12" r="10" />
              <polygon points="10 8 16 12 10 16 10 8" />
            </svg>
            <span>{title}</span>
          </div>
          <div className="sp-header-right">
            {status === 'streaming' && (
              <div className="sp-stats">
                <span>{fps} FPS</span>
                <span>{frameCount} frames</span>
              </div>
            )}
            <div className={`sp-live-badge ${status}`}>
              {status === 'streaming' && <span className="sp-dot" />}
              {statusLabel[status]}
            </div>
          </div>
        </div>

        {/* Video Canvas */}
        <div className="sp-canvas-wrap">
          {hasVideo && (
            <img
              ref={imgRef}
              alt="Video stream"
              style={{ maxWidth: '100%', maxHeight: '100%', objectFit: 'contain' }}
            />
          )}
          {status === 'idle' && !hasVideo && !isBound && (
            <div className="sp-placeholder">
              <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                <rect x="2" y="2" width="20" height="20" rx="2.18" ry="2.18" />
                <line x1="7" y1="2" x2="7" y2="22" />
                <line x1="17" y1="2" x2="17" y2="22" />
                <line x1="2" y1="12" x2="22" y2="12" />
                <line x1="2" y1="7" x2="7" y2="7" />
                <line x1="2" y1="17" x2="7" y2="17" />
                <line x1="17" y1="7" x2="22" y2="7" />
                <line x1="17" y1="17" x2="22" y2="17" />
              </svg>
              <span>Enter a video source URL to begin</span>
              <span style={{ fontSize: '11px' }}>rtsp://, rtmp://, hls://, http://, file://</span>
            </div>
          )}
          {status === 'idle' && !hasVideo && isBound && !boundUrl && (
            <div className="sp-placeholder">
              <DatabaseIcon />
              <span>Resolving data source...</span>
            </div>
          )}
          {status === 'idle' && !hasVideo && isBound && boundUrl && (
            <div className="sp-placeholder">
              <span>Data source bound</span>
              <span style={{ fontSize: '11px', fontFamily: 'monospace' }}>{boundUrl}</span>
            </div>
          )}
          {status === 'connecting' && (
            <div className="sp-placeholder">
              <span>Connecting to stream...</span>
            </div>
          )}
          {status === 'ended' && (
            <div className="sp-placeholder">
              <span>Stream ended</span>
            </div>
          )}
        </div>

        {/* Error bar */}
        {error && <div className="sp-error">{error}</div>}

        {/* Controls */}
        <div className="sp-controls">
          {isBound ? (
            <span style={{ display: 'flex', alignItems: 'center', color: 'var(--sp-accent)' }}>
              <DatabaseIcon />
            </span>
          ) : (
            <span style={{ display: 'flex', alignItems: 'center', color: 'var(--sp-muted)' }}>
              <LinkIcon />
            </span>
          )}
          <input
            className={`sp-url-input${isBound ? ' bound' : ''}`}
            type="text"
            value={effectiveUrl}
            onChange={e => { if (!isBound) setSourceUrl(e.target.value) }}
            onKeyDown={handleKeyDown}
            placeholder={isBound ? 'Bound to data source' : 'rtsp://host:554/stream'}
            disabled={inputDisabled}
            readOnly={isBound}
          />
          {isBound && (
            <span className="sp-bound-tag">Bound</span>
          )}
          {status === 'streaming' || status === 'connecting' ? (
            <button className="sp-btn danger" onClick={stopStream} title="Stop">
              <StopIcon />
            </button>
          ) : (
            <button
              className="sp-btn primary"
              onClick={() => startStream()}
              disabled={!effectiveUrl.trim() || inputDisabled}
              title="Play"
            >
              <PlayIcon />
            </button>
          )}
        </div>
      </div>
    )
  }
)

export default { StreamPlayerCard }
