/**
 * YOLO Video V2 - Dashboard Edition
 * Matches NeoMind dashboard design system with compact, elegant layout
 */

import { useState, useEffect, useRef, useCallback } from 'react'

// ============================================================================
// Types
// ============================================================================

export interface ExtensionComponentProps {
  title?: string
  dataSource?: DataSource
  className?: string
  config?: Record<string, any>
}

export interface DataSource {
  type: string
  extensionId?: string
  [key: string]: any
}

interface Detection {
  id: number
  label: string
  confidence: number
  bbox: { x: number; y: number; width: number; height: number }
  class_id: number
}

type StreamMode = 'camera' | 'network'

// ============================================================================
// Constants & Styles
// ============================================================================

const EXTENSION_ID = 'yolo-video-v2'
const CSS_ID = 'yolo-styles-v2'

const STYLES = `
.yolo {
  --yolo-fg: hsl(240 10% 10%);
  --yolo-muted: hsl(240 5% 45%);
  --yolo-accent: hsl(221 83% 53%);
  --yolo-success: #22c55e;
  --yolo-warning: #f59e0b;
  --yolo-card: rgba(255,255,255,0.5);
  --yolo-border: rgba(0,0,0,0.06);
  width: 100%;
  height: 100%;
  font-size: 12px;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
}
.dark .yolo {
  --yolo-fg: hsl(0 0% 95%);
  --yolo-muted: hsl(0 0% 60%);
  --yolo-card: rgba(30,30,30,0.5);
  --yolo-border: rgba(255,255,255,0.08);
}

.yolo-card {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--yolo-card);
  backdrop-filter: blur(12px);
  border: 1px solid var(--yolo-border);
  border-radius: 8px;
  overflow: hidden;
  box-sizing: border-box;
}

/* Header */
.yolo-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 10px;
  border-bottom: 1px solid var(--yolo-border);
}
.yolo-title {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--yolo-fg);
  font-size: 12px;
  font-weight: 600;
}
.yolo-title-icon {
  width: 16px;
  height: 16px;
  color: var(--yolo-accent);
}
.yolo-controls {
  display: flex;
  align-items: center;
  gap: 6px;
}
.yolo-status {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 10px;
  color: var(--yolo-muted);
}
.yolo-status-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--yolo-success);
  animation: yolo-pulse 2s ease-in-out infinite;
}
.yolo-status-dot.yolo-status-warning { background: var(--yolo-warning); animation: yolo-blink 1s infinite; }
.yolo-status-dot.yolo-status-error { background: #ef4444; animation: none; }
@keyframes yolo-pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.5; }
}
@keyframes yolo-blink {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.3; }
}
.yolo-btn {
  padding: 4px 10px;
  font-size: 11px;
  font-weight: 500;
  color: white;
  background: var(--yolo-accent);
  border: none;
  border-radius: 4px;
  cursor: pointer;
  transition: opacity 0.2s;
}
.yolo-btn:hover { opacity: 0.9; }
.yolo-btn-stop {
  background: #ef4444;
}

/* Video Display */
.yolo-video-wrap {
  position: relative;
  flex: 1;
  background: #000;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  min-height: 200px;
}
.yolo-video-frame {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.yolo-video-placeholder {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  color: rgba(255,255,255,0.4);
  gap: 8px;
  padding: 20px;
  text-align: center;
}
.yolo-video-icon {
  width: 48px;
  height: 48px;
  opacity: 0.3;
}
.yolo-video-text {
  font-size: 11px;
  line-height: 1.5;
}
.yolo-video-loading {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  background: rgba(0,0,0,0.7);
  color: white;
  gap: 8px;
}
.yolo-spinner {
  width: 24px;
  height: 24px;
  border: 2px solid rgba(255,255,255,0.2);
  border-top-color: white;
  border-radius: 50%;
  animation: yolo-spin 0.7s linear infinite;
}
@keyframes yolo-spin {
  to { transform: rotate(360deg); }
}

/* Stats Bar */
.yolo-stats {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 10px;
  border-top: 1px solid var(--yolo-border);
  gap: 8px;
  font-size: 10px;
}
.yolo-stat-group {
  display: flex;
  align-items: center;
  gap: 8px;
}
.yolo-stat {
  display: flex;
  align-items: center;
  gap: 3px;
  color: var(--yolo-muted);
}
.yolo-stat-icon {
  width: 12px;
  height: 12px;
  flex-shrink: 0;
}
.yolo-stat-val {
  font-weight: 600;
  color: var(--yolo-fg);
}

/* Detections */
.yolo-detections {
  padding: 6px 10px;
  border-top: 1px solid var(--yolo-border);
  max-height: 60px;
  overflow-y: auto;
}
.yolo-detections-title {
  font-size: 9px;
  color: var(--yolo-muted);
  text-transform: uppercase;
  letter-spacing: 0.3px;
  margin-bottom: 4px;
}
.yolo-detections-list {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}
.yolo-detection-tag {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 2px 6px;
  font-size: 10px;
  font-weight: 500;
  border-radius: 3px;
  white-space: nowrap;
}

/* Error */
.yolo-error {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  background: rgba(0,0,0,0.8);
  color: #ef4444;
  padding: 20px;
  text-align: center;
  z-index: 10;
}
.yolo-error-icon {
  width: 32px;
  height: 32px;
  margin-bottom: 8px;
}
.yolo-error-text {
  font-size: 11px;
  line-height: 1.5;
  max-width: 300px;
}

/* Scrollbar */
.yolo-detections::-webkit-scrollbar {
  width: 4px;
}
.yolo-detections::-webkit-scrollbar-track {
  background: transparent;
}
.yolo-detections::-webkit-scrollbar-thumb {
  background: var(--yolo-border);
  border-radius: 2px;
}
.dark .yolo-detections::-webkit-scrollbar-thumb {
  background: rgba(255,255,255,0.1);
}
`

function injectStyles() {
  if (typeof document === 'undefined' || document.getElementById(CSS_ID)) return
  const style = document.createElement('style')
  style.id = CSS_ID
  style.textContent = STYLES
  document.head.appendChild(style)
}

// ============================================================================
// Icons
// ============================================================================

const ICONS: Record<string, string> = {
  video: '<path d="M23 7l-7 5 7 5V7z"/><rect x="1" y="5" width="15" height="14" rx="2" ry="2"/>',
  play: '<polygon points="5 3 19 12 5 21 5 3"/>',
  stop: '<rect x="3" y="3" width="18" height="18" rx="2" ry="2"/>',
  camera: '<path d="M23 19a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4l2-3h6l2 3h4a2 2 0 0 1 2 2z"/><circle cx="12" cy="13" r="4"/>',
  activity: '<polyline points="22 12 18 12 15 21 9 3 6 12 2 12"/>',
  clock: '<circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/>',
  eye: '<path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/>',
  layers: '<polygon points="12 2 2 7 12 12 22 7 12 2"/><polyline points="2 17 12 22 22 17"/><polyline points="2 12 12 17 22 12"/>',
  alert: '<circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/>',
}

const Icon = ({ name, className = '', style }: { name: string; className?: string; style?: React.CSSProperties }) => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"
    strokeLinecap="round" strokeLinejoin="round" className={className} style={style}
    dangerouslySetInnerHTML={{ __html: ICONS[name] || ICONS.video }} />
)

// ============================================================================
// Detection Colors
// ============================================================================

const DETECTION_COLORS = [
  { bg: 'rgba(239, 68, 68, 0.15)', fg: '#ef4444', border: '#ef4444' },   // red
  { bg: 'rgba(34, 197, 94, 0.15)', fg: '#22c55e', border: '#22c55e' },   // green
  { bg: 'rgba(59, 130, 246, 0.15)', fg: '#3b82f6', border: '#3b82f6' },  // blue
  { bg: 'rgba(249, 115, 22, 0.15)', fg: '#f97316', border: '#f97316' },  // orange
  { bg: 'rgba(168, 85, 247, 0.15)', fg: '#a855f7', border: '#a855f7' },  // purple
  { bg: 'rgba(6, 182, 212, 0.15)', fg: '#06b6d4', border: '#06b6d4' },   // cyan
  { bg: 'rgba(236, 72, 153, 0.15)', fg: '#ec4899', border: '#ec4899' },  // pink
  { bg: 'rgba(234, 179, 8, 0.15)', fg: '#eab308', border: '#eab308' },   // yellow
  { bg: 'rgba(20, 184, 166, 0.15)', fg: '#14b8a6', border: '#14b8a6' },  // teal
  { bg: 'rgba(244, 63, 94, 0.15)', fg: '#f43f5e', border: '#f43f5e' },   // rose
]

// ============================================================================
// Component
// ============================================================================

export const YoloVideoDisplay = function YoloVideoDisplay({
  title = 'YOLO Detection',
  dataSource,
  className = '',
  confidenceThreshold = 0.5,
  maxObjects = 20,
  sourceUrl = 'camera://0',
}: ExtensionComponentProps & {
  sourceUrl?: string
  confidenceThreshold?: number
  maxObjects?: number
}) {
  // Setup
  useEffect(() => { injectStyles() }, [])

  // Determine mode
  const isNetworkStream = sourceUrl.startsWith('rtsp://')
    || sourceUrl.startsWith('rtmp://')
    || sourceUrl.startsWith('hls://')
    || sourceUrl.includes('.m3u8')
  const mode: StreamMode = isNetworkStream ? 'network' : 'camera'

  // State
  const [isRunning, setIsRunning] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [sessionTime, setSessionTime] = useState(0)
  const [fps, setFps] = useState(0)
  const [frameCount, setFrameCount] = useState(0)
  const [detections, setDetections] = useState<Detection[]>([])
  const [frameData, setFrameData] = useState<string | null>(null)
  const [cameraPermission, setCameraPermission] = useState<'pending' | 'granted' | 'denied'>('pending')
  const [streamStatus, setStreamStatus] = useState<'idle' | 'streaming' | 'reconnecting' | 'error'>('idle')

  // Refs
  const videoRef = useRef<HTMLVideoElement>(null)
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const streamRef = useRef<MediaStream | null>(null)
  const wsRef = useRef<WebSocket | null>(null)
  const sessionTimerRef = useRef<ReturnType<typeof setInterval> | null>(null)
  const frameTimerRef = useRef<ReturnType<typeof setInterval> | null>(null)
  const fpsCounterRef = useRef({ frames: 0, lastTime: Date.now() })
  const sequenceRef = useRef(0)
  const sessionIdRef = useRef<string | null>(null)
  const sendingRef = useRef(false)
  const isFrameSendingRef = useRef(false)  // Lock for frame sending
  const lastFrameTimeRef = useRef(0)  // Last frame send time for throttling
  const lockTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)  // Safety timeout for lock

  const extensionId = dataSource?.extensionId || EXTENSION_ID

  // WebSocket URL
  const getWebSocketUrl = useCallback(() => {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    const host = window.location.host
    return `${protocol}//${host}/api/extensions/${extensionId}/stream`
  }, [extensionId])

  // Capture and send frame (camera mode)
  const captureAndSendFrame = useCallback(() => {
    if (!sendingRef.current) return

    // Skip if previous frame is still being processed
    if (isFrameSendingRef.current) {
      return
    }

    // Throttle to max 20 FPS (matching backend 50ms threshold)
    const now = Date.now()
    if (now - lastFrameTimeRef.current < 50) {
      return
    }

    const video = videoRef.current
    const canvas = canvasRef.current
    if (!video || !canvas || video.paused || video.ended) return

    const ctx = canvas.getContext('2d')
    if (!ctx) return

    ctx.drawImage(video, 0, 0, canvas.width, canvas.height)

    const ws = wsRef.current
    const sessionId = sessionIdRef.current

    if (ws?.readyState === WebSocket.OPEN && sessionId) {
      isFrameSendingRef.current = true  // Acquire lock
      lastFrameTimeRef.current = now

      // Safety timeout: auto-release lock after 200ms if callback doesn't fire
      if (lockTimeoutRef.current) {
        clearTimeout(lockTimeoutRef.current)
      }
      lockTimeoutRef.current = setTimeout(() => {
        if (isFrameSendingRef.current) {
          console.warn('[YOLO] Frame lock timeout, auto-releasing')
          isFrameSendingRef.current = false
        }
      }, 200)

      canvas.toBlob((blob) => {
        // Clear safety timeout
        if (lockTimeoutRef.current) {
          clearTimeout(lockTimeoutRef.current)
          lockTimeoutRef.current = null
        }
        // Always release lock first
        isFrameSendingRef.current = false

        if (!sendingRef.current) return  // Component may have stopped

        if (blob && wsRef.current?.readyState === WebSocket.OPEN && sessionIdRef.current) {
          blob.arrayBuffer().then(buffer => {
            const sequence = sequenceRef.current++
            const header = new ArrayBuffer(8)
            new DataView(header).setBigUint64(0, BigInt(sequence), false)

            const frame = new Uint8Array(8 + buffer.byteLength)
            frame.set(new Uint8Array(header), 0)
            frame.set(new Uint8Array(buffer), 8)

            wsRef.current?.send(frame)
          }).catch((err) => {
            console.warn('[YOLO] Failed to send frame:', err)
          })
        }
      }, 'image/jpeg', 0.8)
    }
  }, [])

  // Start camera
  const startCamera = useCallback(async () => {
    try {
      setCameraPermission('pending')
      const stream = await navigator.mediaDevices.getUserMedia({
        video: { width: { ideal: 640 }, height: { ideal: 480 }, facingMode: 'user' },
        audio: false
      })

      setCameraPermission('granted')
      streamRef.current = stream

      if (videoRef.current) {
        videoRef.current.srcObject = stream
        await videoRef.current.play()
      }

      return true
    } catch (e) {
      setCameraPermission('denied')
      if (e instanceof Error) {
        if (e.name === 'NotAllowedError') {
          setError('Camera permission denied')
        } else if (e.name === 'NotFoundError') {
          setError('No camera found')
        } else {
          setError(`Camera error: ${e.message}`)
        }
      }
      return false
    }
  }, [])

  // Stop camera
  const stopCamera = useCallback(() => {
    sendingRef.current = false
    isFrameSendingRef.current = false  // Reset frame sending lock
    lastFrameTimeRef.current = 0  // Reset throttle timer

    // Clear safety timeout
    if (lockTimeoutRef.current) {
      clearTimeout(lockTimeoutRef.current)
      lockTimeoutRef.current = null
    }

    if (streamRef.current) {
      streamRef.current.getTracks().forEach(track => track.stop())
      streamRef.current = null
    }

    if (videoRef.current) {
      videoRef.current.srcObject = null
    }
  }, [])

  // Connect WebSocket
  const connectWebSocket = useCallback(() => {
    const url = getWebSocketUrl()
    const ws = new WebSocket(url)
    ws.binaryType = 'arraybuffer'

    ws.onopen = () => {
      const initMsg = {
        type: 'init',
        config: {
          source_url: sourceUrl,
          confidence_threshold: confidenceThreshold,
          max_objects: maxObjects
        }
      }
      ws.send(JSON.stringify(initMsg))
    }

    ws.onmessage = (event) => {
      // Binary responses are not used - all results come as JSON text messages
      // This is because metadata (including detections) must be sent with the frame
      if (event.data instanceof ArrayBuffer) {
        // Skip binary responses - they don't contain detection metadata
        console.debug('[YOLO] Received binary response (no metadata), skipping')
        return
      }

      // Text message (JSON)
      if (typeof event.data !== 'string') return

      try {
        const msg = JSON.parse(event.data)

        switch (msg.type) {
          case 'session_created':
            sessionIdRef.current = msg.session_id
            setIsRunning(true)
            setSessionTime(0)
            sessionTimerRef.current = setInterval(() => setSessionTime(t => t + 1), 1000)

            // For camera mode, start capture loop (50ms interval for max 20 FPS)
            if (mode === 'camera') {
              sendingRef.current = true
              frameTimerRef.current = setInterval(captureAndSendFrame, 50)
            }
            break

          case 'push_output':
            // Check for status/error JSON messages (backend stream status)
            if (msg.data_type === 'application/json' && msg.data) {
              try {
                const statusData = typeof msg.data === 'string' ? JSON.parse(msg.data) : msg.data
                if (statusData.type === 'status' && statusData.status) {
                  setStreamStatus(statusData.status as any)
                } else if (statusData.type === 'error') {
                  setStreamStatus('error')
                  setError(statusData.message || 'Stream error')
                }
              } catch { /* ignore parse errors */ }
              break
            }
            // Image frame from backend stream
            if (msg.data && msg.data_type === 'image/jpeg') {
              setStreamStatus('streaming')
              setFrameData(msg.data)
              updateFps()
              if (msg.metadata?.detections) {
                setDetections(msg.metadata.detections)
              }
            }
            break

          case 'result':
            // Processing result from server - data is already base64 encoded
            if (msg.data) {
              // ✨ FIX: Check if frame was skipped before setting image data
              // Skipped frames return JSON metadata instead of image data
              const isSkipped = msg.skipped === true ||
                                (typeof msg.data === 'string' &&
                                 (msg.data.startsWith('{') ||
                                  msg.metadata?.skipped === true));

              if (isSkipped) {
                // Frame was skipped (rate limiting), keep displaying last frame
                console.debug('[YOLO] Frame skipped by backend, keeping previous frame')
                // Don't update frameData - keep showing the last valid frame
                if (msg.metadata?.detections) {
                  setDetections(msg.metadata.detections)
                }
              } else if (typeof msg.data === 'string' && msg.data.length > 0) {
                // Valid image data (base64 string)
                setFrameData(msg.data)
                updateFps()
                setFrameCount(prev => prev + 1)

                if (msg.metadata?.detections) {
                  setDetections(msg.metadata.detections)
                }
              } else {
                console.warn('[YOLO] Received invalid frame data:', typeof msg.data, msg.data?.substring(0, 100))
              }
            }
            break

          case 'error':
            // Ignore frame rate throttling errors (these are normal during high load)
            if (msg.message && msg.message.includes('Frame rate too high')) {
              console.debug('[YOLO] Frame dropped due to rate limiting (normal)')
              break
            }
            // Show other errors to user
            setError(`${msg.code}: ${msg.message}`)
            break

          case 'session_closed':
            setIsRunning(false)
            sessionIdRef.current = null
            break
        }
      } catch (e) {
        console.error('[YOLO] Failed to parse message:', e)
      }
    }

    ws.onerror = (e) => {
      console.error('[YOLO] WebSocket error:', e)
      setError('WebSocket connection error')
    }

    ws.onclose = () => {
      wsRef.current = null
      setIsRunning(false)
      setStreamStatus('idle')
      sessionIdRef.current = null
      sendingRef.current = false

      if (sessionTimerRef.current) {
        clearInterval(sessionTimerRef.current)
        sessionTimerRef.current = null
      }

      if (frameTimerRef.current) {
        clearInterval(frameTimerRef.current)
        frameTimerRef.current = null
      }
    }

    wsRef.current = ws
  }, [getWebSocketUrl, sourceUrl, confidenceThreshold, maxObjects, mode, captureAndSendFrame])

  // Update FPS
  const updateFps = () => {
    fpsCounterRef.current.frames++
    const now = Date.now()
    const elapsed = now - fpsCounterRef.current.lastTime
    if (elapsed >= 1000) {
      setFps(Math.round(fpsCounterRef.current.frames * 1000 / elapsed))
      fpsCounterRef.current.frames = 0
      fpsCounterRef.current.lastTime = now
    }
  }

  // Disconnect WebSocket
  const disconnectWebSocket = useCallback(() => {
    if (wsRef.current) {
      if (wsRef.current.readyState === WebSocket.OPEN) {
        wsRef.current.send(JSON.stringify({ type: 'close' }))
      }
      wsRef.current.close()
      wsRef.current = null
    }

    if (sessionTimerRef.current) {
      clearInterval(sessionTimerRef.current)
      sessionTimerRef.current = null
    }

    if (frameTimerRef.current) {
      clearInterval(frameTimerRef.current)
      frameTimerRef.current = null
    }

    setIsRunning(false)
    sessionIdRef.current = null
    setDetections([])
  }, [])

  // Start stream
  const startStream = useCallback(async () => {
    setError(null)
    setFrameData(null)
    setFps(0)
    setFrameCount(0)
    fpsCounterRef.current = { frames: 0, lastTime: Date.now() }

    if (mode === 'camera') {
      const cameraOk = await startCamera()
      if (!cameraOk) return
    }

    connectWebSocket()
  }, [mode, startCamera, connectWebSocket])

  // Stop stream
  const stopStream = useCallback(() => {
    if (mode === 'camera') {
      stopCamera()
    }
    disconnectWebSocket()
    setStreamStatus('idle')
    setDetections([])
    setFps(0)
    setFrameCount(0)
    setSessionTime(0)
    setFrameData(null)
  }, [mode, stopCamera, disconnectWebSocket])

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      stopStream()
    }
  }, [stopStream])

  // Format time
  const formatTime = (seconds: number) => {
    const mins = Math.floor(seconds / 60)
    const secs = seconds % 60
    return `${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`
  }

  // Get mode label
  const getModeLabel = () => {
    if (mode === 'network') {
      if (isRunning && streamStatus === 'reconnecting') return 'Reconnecting...'
      if (isRunning && streamStatus === 'error') return 'Error'
      if (sourceUrl.startsWith('rtsp://')) return 'RTSP'
      if (sourceUrl.startsWith('rtmp://')) return 'RTMP'
      if (sourceUrl.startsWith('hls://') || sourceUrl.includes('.m3u8')) return 'HLS'
      return 'Network'
    }
    return 'CAM'
  }

  // Render
  return (
    <div className={`yolo ${className}`}>
      <div className="yolo-card">
        {/* Header */}
        <div className="yolo-header">
          <div className="yolo-title">
            <Icon name="camera" className="yolo-title-icon" />
            {title}
          </div>
          <div className="yolo-controls">
            {isRunning && (
              <div className="yolo-status">
                <span className={`yolo-status-dot${streamStatus === 'reconnecting' ? ' yolo-status-warning' : streamStatus === 'error' ? ' yolo-status-error' : ''}`} />
                {getModeLabel()}
              </div>
            )}
            {!isRunning ? (
              <button onClick={startStream} className="yolo-btn">
                <Icon name="play" style={{ width: 12, height: 12, display: 'inline', verticalAlign: 'middle', marginRight: 2 }} />
                Start
              </button>
            ) : (
              <button onClick={stopStream} className="yolo-btn yolo-btn-stop">
                <Icon name="stop" style={{ width: 12, height: 12, display: 'inline', verticalAlign: 'middle', marginRight: 2 }} />
                Stop
              </button>
            )}
          </div>
        </div>

        {/* Video Display */}
        <div className="yolo-video-wrap">
          {/* Hidden elements for camera capture */}
          {mode === 'camera' && (
            <>
              <video ref={videoRef} className="hidden" playsInline muted />
              <canvas ref={canvasRef} width={640} height={480} className="hidden" />
            </>
          )}

          {/* Display processed frame */}
          {frameData && (
            <img
              src={`data:image/jpeg;base64,${frameData}`}
              alt="Detection Frame"
              className="yolo-video-frame"
            />
          )}

          {/* Error overlay */}
          {error && (
            <div className="yolo-error">
              <Icon name="alert" className="yolo-error-icon" />
              <div className="yolo-error-text">{error}</div>
            </div>
          )}

          {/* Placeholder */}
          {!isRunning && !error && (
            <div className="yolo-video-placeholder">
              <Icon name="video" className="yolo-video-icon" />
              <div className="yolo-video-text">
                {mode === 'camera'
                  ? 'Click Start to begin detection'
                  : `Click Start to connect to ${sourceUrl}`}
              </div>
            </div>
          )}

          {/* Loading - show when waiting for first frame */}
          {isRunning && !frameData && !error && (
            <div className="yolo-video-loading">
              <div className="yolo-spinner" />
              <div className="yolo-video-text">
                {mode === 'camera' ? 'Starting camera...' : 'Connecting...'}
              </div>
            </div>
          )}
        </div>

        {/* Stats Bar */}
        {isRunning && (
          <div className="yolo-stats">
            <div className="yolo-stat-group">
              <div className="yolo-stat">
                <Icon name="clock" className="yolo-stat-icon" />
                <span className="yolo-stat-val">{formatTime(sessionTime)}</span>
              </div>
              <div className="yolo-stat">
                <Icon name="activity" className="yolo-stat-icon" />
                <span className="yolo-stat-val">{fps}</span>
                <span>FPS</span>
              </div>
              <div className="yolo-stat">
                <Icon name="layers" className="yolo-stat-icon" />
                <span className="yolo-stat-val">{frameCount}</span>
                <span>frames</span>
              </div>
            </div>
            <div className="yolo-stat">
              <Icon name="eye" className="yolo-stat-icon" />
              <span className="yolo-stat-val">{detections.length}</span>
              <span>objects</span>
            </div>
          </div>
        )}

        {/* Detections */}
        {isRunning && detections.length > 0 && (
          <div className="yolo-detections">
            <div className="yolo-detections-title">Detected Objects</div>
            <div className="yolo-detections-list">
              {detections.slice(0, 8).map((det, i) => {
                const color = DETECTION_COLORS[i % DETECTION_COLORS.length]
                return (
                  <span
                    key={det.id || i}
                    className="yolo-detection-tag"
                    style={{
                      backgroundColor: color.bg,
                      color: color.fg,
                      border: `1px solid ${color.border}33`
                    }}
                  >
                    {det.label}
                    <span style={{ opacity: 0.7 }}>
                      {Math.round(det.confidence * 100)}%
                    </span>
                  </span>
                )
              })}
            </div>
          </div>
        )}
      </div>
    </div>
  )
}

// ============================================================================
// Export variants
// ============================================================================

export const YoloVideoCard = (props: ExtensionComponentProps) => (
  <div style={{ height: '100%', minHeight: 300 }}>
    <YoloVideoDisplay {...props} title={props.title || 'YOLO Detection'} />
  </div>
)

export const YoloVideoWidget = (props: ExtensionComponentProps) => (
  <div style={{ height: 280 }}>
    <YoloVideoDisplay {...props} title={props.title || 'YOLO'} />
  </div>
)

export const YoloVideoPanel = (props: ExtensionComponentProps) => (
  <div style={{ height: '100%', minHeight: 500 }}>
    <YoloVideoDisplay {...props} title={props.title || 'YOLO Video Detection'} />
  </div>
)

export default YoloVideoDisplay
export const Component = YoloVideoDisplay
export const Card = YoloVideoCard
export const Widget = YoloVideoWidget
export const Panel = YoloVideoPanel
