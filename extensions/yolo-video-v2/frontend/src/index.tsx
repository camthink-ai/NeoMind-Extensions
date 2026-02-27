/**
 * YOLO Video V2 Dashboard Component
 * Built with NeoMind Extension SDK V2
 *
 * Features:
 * - Real-time video stream display
 * - Detection statistics panel
 * - Responsive and adaptive UI
 * - CSS variable-based theming
 * - ABI version 3 compatible
 * - YOLOv11 real-time detection
 */

import { useState, useEffect, useRef, useCallback, useMemo } from 'react'

// ============================================================================
// SDK Types
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

interface ExtensionCommandResult<T> {
  success: boolean
  data?: T
  error?: string
}

// ============================================================================
// API Helpers
// ============================================================================

const EXTENSION_ID = 'yolo-video-v2'

const getAuthToken = (): string | null => {
  return localStorage.getItem('neomind_token') ||
         sessionStorage.getItem('neomind_token_session') ||
         localStorage.getItem('token') ||
         null
}

const getApiBase = (): string => {
  if (typeof window !== 'undefined' && (window as any).__TAURI__) {
    return 'http://localhost:9375/api'
  }
  return '/api'
}

async function executeExtensionCommand<T>(
  extensionId: string,
  command: string,
  args: Record<string, any>
): Promise<ExtensionCommandResult<T>> {
  const token = getAuthToken()
  const apiBase = getApiBase()
  const headers: Record<string, string> = { 'Content-Type': 'application/json' }
  if (token) headers['Authorization'] = `Bearer ${token}`

  try {
    const response = await fetch(`${apiBase}/extensions/${extensionId}/command`, {
      method: 'POST',
      headers,
      body: JSON.stringify({ command, args })
    })

    if (!response.ok) {
      return {
        success: false,
        error: response.status === 401 ? 'Authentication required' : `HTTP ${response.status}`
      }
    }

    const data = await response.json()
    return data
  } catch (err) {
    return {
      success: false,
      error: err instanceof Error ? err.message : 'Network error'
    }
  }
}

// ============================================================================
// Types
// ============================================================================

interface StreamInfo {
  stream_id: string
  stream_url: string
  status: string
  width: number
  height: number
}

interface StreamStats {
  stream_id: string
  frame_count: number
  fps: number
  total_detections: number
  detected_objects: Record<string, number>
}

// ============================================================================
// Constants
// ============================================================================

const DETECTION_COLORS = [
  '#ef4444', '#f97316', '#eab308', '#22c55e', '#06b6d4',
  '#3b82f6', '#8b5cf6', '#ec4899', '#f43f5e', '#84cc16'
]

// ============================================================================
// Icons
// ============================================================================

const VideoIcon = () => (
  <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
    <path strokeLinecap="round" strokeLinejoin="round" d="M15.75 10.5l4.72-4.72a.75.75 0 011.28.53v11.38a.75.75 0 01-1.28.53l-4.72-4.72M4.5 18.75h9a2.25 2.25 0 002.25-2.25v-9a2.25 2.25 0 00-2.25-2.25h-9A2.25 2.25 0 002.25 7.5v9a2.25 2.25 0 002.25 2.25z" />
  </svg>
)

const PlayIcon = () => (
  <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
    <path strokeLinecap="round" strokeLinejoin="round" d="M5.25 5.653c0-.856.917-1.398 1.667-.986l11.54 6.348a1.125 1.125 0 010 1.971l-11.54 6.347a1.125 1.125 0 01-1.667-.985V5.653z" />
  </svg>
)

const StopIcon = () => (
  <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
    <path strokeLinecap="round" strokeLinejoin="round" d="M5.25 7.5A2.25 2.25 0 017.5 5.25h9a2.25 2.25 0 012.25 2.25v9a2.25 2.25 0 01-2.25 2.25h-9a2.25 2.25 0 01-2.25-2.25v-9z" />
  </svg>
)

const TargetIcon = () => (
  <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
    <path strokeLinecap="round" strokeLinejoin="round" d="M15 10.5a3 3 0 11-6 0 3 3 0 016 0z" />
    <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 10.5c0 7.142-7.5 11.25-7.5 11.25S4.5 17.642 4.5 10.5a7.5 7.5 0 1115 0z" />
  </svg>
)

const SpeedIcon = () => (
  <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
    <path strokeLinecap="round" strokeLinejoin="round" d="M3.75 13.5l10.5-11.25L12 10.5h8.25L9.75 21.75 12 13.5H3.75z" />
  </svg>
)

const ClockIcon = () => (
  <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
    <path strokeLinecap="round" strokeLinejoin="round" d="M12 6v6h4.5m4.5 0a9 9 0 11-18 0 9 9 0 0118 0z" />
  </svg>
)

const ChartIcon = () => (
  <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
    <path strokeLinecap="round" strokeLinejoin="round" d="M3 13.125C3 12.504 3.504 12 4.125 12h2.25c.621 0 1.125.504 1.125 1.125v6.75C7.5 20.496 6.996 21 6.375 21h-2.25A1.125 1.125 0 013 19.875v-6.75zM9.75 8.625c0-.621.504-1.125 1.125-1.125h2.25c.621 0 1.125.504 1.125 1.125v11.25c0 .621-.504 1.125-1.125 1.125h-2.25a1.125 1.125 0 01-1.125-1.125V8.625zM16.5 4.125c0-.621.504-1.125 1.125-1.125h2.25C20.496 3 21 3.504 21 4.125v15.75c0 .621-.504 1.125-1.125 1.125h-2.25a1.125 1.125 0 01-1.125-1.125V4.125z" />
  </svg>
)

const SettingsIcon = () => (
  <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
    <path strokeLinecap="round" strokeLinejoin="round" d="M9.594 3.94c.09-.542.56-.94 1.11-.94h2.593c.55 0 1.02.398 1.11.94l.213 1.281c.063.374.313.686.645.87.074.04.147.083.22.127.324.196.72.257 1.075.124l1.217-.456a1.125 1.125 0 011.37.49l1.296 2.247a1.125 1.125 0 01-.26 1.431l-1.003.827c-.293.24-.438.613-.431.992a6.759 6.759 0 010 .255c-.007.378.138.75.43.99l1.005.828c.424.35.534.954.26 1.43l-1.298 2.247a1.125 1.125 0 01-1.369.491l-1.217-.456c-.355-.133-.75-.072-1.076.124a6.57 6.57 0 01-.22.128c-.331.183-.581.495-.644.869l-.213 1.28c-.09.543-.56.941-1.11.941h-2.594c-.55 0-1.02-.398-1.11-.94l-.213-1.281c-.062-.374-.312-.686-.644-.87a6.52 6.52 0 01-.22-.127c-.325-.196-.72-.257-1.076-.124l-1.217.456a1.125 1.125 0 01-1.369-.49l-1.297-2.247a1.125 1.125 0 01.26-1.431l1.004-.827c.292-.24.437-.613.43-.992a6.932 6.932 0 010-.255c.007-.378-.138-.75-.43-.99l-1.004-.828a1.125 1.125 0 01-.26-1.43l1.297-2.247a1.125 1.125 0 011.37-.491l1.216.456c.356.133.751.072 1.076-.124.072-.044.146-.087.22-.128.332-.183.582-.495.644-.869l.214-1.281z" />
    <path strokeLinecap="round" strokeLinejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
  </svg>
)

// ============================================================================
// YOLO Video Display Component
// ============================================================================

export interface YoloVideoDisplayProps extends ExtensionComponentProps {
  sourceUrl?: string
  videoSource?: 'camera' | 'rtsp' | 'file' | 'hls'
  confidenceThreshold?: number
  maxObjects?: number
  targetFps?: number
  drawBoxes?: boolean
  showStats?: boolean
}

export const YoloVideoDisplay = function YoloVideoDisplay({
  title = 'YOLO Video',
  dataSource,
  className = '',
  sourceUrl,
  videoSource = 'camera',
  confidenceThreshold = 0.5,
  maxObjects = 20,
  targetFps = 15,
  drawBoxes = true,
  showStats = true
}: YoloVideoDisplayProps) {
  const [isRunning, setIsRunning] = useState(false)
  const [streamInfo, setStreamInfo] = useState<StreamInfo | null>(null)
  const [stats, setStats] = useState<StreamStats | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [currentSource, setCurrentSource] = useState(videoSource)
  const [sourceUrlInput, setSourceUrlInput] = useState(sourceUrl || 'camera://0')
  const [sessionTime, setSessionTime] = useState(0)
  const [showSettings, setShowSettings] = useState(false)
  const [settings, setSettings] = useState({
    confidence: confidenceThreshold,
    maxObjects: maxObjects,
    targetFps: targetFps,
    drawBoxes: drawBoxes
  })

  const statsIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null)
  const sessionTimerRef = useRef<ReturnType<typeof setInterval> | null>(null)

  const extensionId = dataSource?.extensionId || EXTENSION_ID

  const fetchStats = useCallback(async (streamId: string) => {
    const result = await executeExtensionCommand<StreamStats>(
      extensionId,
      'get_stream_stats',
      { stream_id: streamId }
    )
    if (result.success && result.data) {
      setStats(result.data)
    }
  }, [extensionId])

  const startStream = useCallback(async () => {
    setError(null)
    setStats(null)

    const result = await executeExtensionCommand<StreamInfo>(
      extensionId,
      'start_stream',
      {
        source_url: sourceUrlInput,
        confidence_threshold: settings.confidence,
        max_objects: settings.maxObjects,
        target_fps: settings.targetFps,
        draw_boxes: settings.drawBoxes
      }
    )

    if (result.success && result.data) {
      setStreamInfo(result.data)
      setIsRunning(true)
      setSessionTime(0)

      sessionTimerRef.current = setInterval(() => {
        setSessionTime(t => t + 1)
      }, 1000)

      if (showStats) {
        statsIntervalRef.current = setInterval(() => {
          fetchStats(result.data!.stream_id)
        }, 2000)
      }
    } else {
      setError(result.error || 'Failed to start stream')
    }
  }, [extensionId, sourceUrlInput, settings, showStats, fetchStats])

  const stopStream = useCallback(async () => {
    if (!streamInfo) return

    await executeExtensionCommand(
      extensionId,
      'stop_stream',
      { stream_id: streamInfo.stream_id }
    )

    setIsRunning(false)
    setStreamInfo(null)
    setStats(null)

    if (statsIntervalRef.current) {
      clearInterval(statsIntervalRef.current)
      statsIntervalRef.current = null
    }
    if (sessionTimerRef.current) {
      clearInterval(sessionTimerRef.current)
      sessionTimerRef.current = null
    }
  }, [extensionId, streamInfo])

  useEffect(() => {
    return () => {
      if (isRunning) stopStream()
    }
  }, [isRunning, stopStream])

  const formatTime = (seconds: number) => {
    const mins = Math.floor(seconds / 60)
    const secs = seconds % 60
    return `${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`
  }

  const getStreamUrl = () => {
    if (!streamInfo) return ''
    return streamInfo.stream_url.startsWith('/') ? streamInfo.stream_url : `/${streamInfo.stream_url}`
  }

  // Sort detected objects by count
  const sortedObjects = useMemo(() => {
    if (!stats?.detected_objects) return []
    return Object.entries(stats.detected_objects)
      .sort((a, b) => b[1] - a[1])
      .slice(0, 8)
  }, [stats?.detected_objects])

  // Source presets
  const sourcePresets = [
    { key: 'camera', label: 'Camera', url: 'camera://0' },
    { key: 'rtsp', label: 'RTSP', url: 'rtsp://' },
    { key: 'hls', label: 'HLS', url: 'https://' },
    { key: 'file', label: 'File', url: 'file://' }
  ]

  return (
    <div className={`yvd relative overflow-hidden rounded-xl shadow-lg ${className}`}>
      <style>{`
        .yvd { --ext-bg: rgba(15, 23, 42, 0.95); --ext-fg: #f8fafc; --ext-muted: rgba(248, 250, 252, 0.5); --ext-border: rgba(255, 255, 255, 0.1); --ext-accent: #10b981; --ext-glass: rgba(255, 255, 255, 0.05); font-size: 14px; }
        .yvd * { box-sizing: border-box; }
        .yvd-card { background: linear-gradient(135deg, rgba(15, 23, 42, 0.98), rgba(30, 41, 59, 0.95)); backdrop-filter: blur(20px); border: 1px solid var(--ext-border); border-radius: 12px; }
        .yvd-btn { display: inline-flex; align-items: center; justify-content: center; gap: 4px; padding: 6px 12px; border-radius: 8px; font-size: 12px; font-weight: 500; transition: all 0.2s; cursor: pointer; border: none; }
        .yvd-btn-primary { background: var(--ext-accent); color: white; }
        .yvd-btn-primary:hover { filter: brightness(1.1); }
        .yvd-btn-danger { background: #ef4444; color: white; }
        .yvd-btn-danger:hover { filter: brightness(1.1); }
        .yvd-btn-ghost { background: rgba(255,255,255,0.1); color: rgba(255,255,255,0.8); }
        .yvd-btn-ghost:hover { background: rgba(255,255,255,0.15); }
        .yvd-input { background: rgba(255,255,255,0.1); border: 1px solid rgba(255,255,255,0.2); border-radius: 8px; padding: 6px 10px; font-size: 12px; color: white; outline: none; transition: border-color 0.2s; }
        .yvd-input:focus { border-color: var(--ext-accent); }
        .yvd-input::placeholder { color: rgba(255,255,255,0.4); }
        .yvd-stat-card { background: rgba(255,255,255,0.05); border-radius: 8px; padding: 8px 12px; text-align: center; }
      `}</style>

      <div className="yvd-card p-3 flex flex-col gap-3" style={{ minHeight: '380px' }}>
        {/* Header */}
        <div className="flex items-center justify-between flex-wrap gap-2">
          <div className="flex items-center gap-2">
            <div className="bg-emerald-500/20 rounded-lg p-1.5">
              <VideoIcon />
            </div>
            <div>
              <h3 className="font-semibold text-white text-sm">{title}</h3>
              <p className="text-white/40 text-[10px]">Real-time YOLOv11 Detection</p>
            </div>
          </div>

          {/* Live Stats */}
          <div className="flex items-center gap-2">
            {isRunning && (
              <div className="flex items-center gap-1.5 bg-emerald-500/20 rounded-lg px-2 py-1 border border-emerald-500/30">
                <div className="w-2 h-2 bg-emerald-400 rounded-full animate-pulse" />
                <span className="text-emerald-300 text-xs font-mono">{formatTime(sessionTime)}</span>
              </div>
            )}
            <div className="flex items-center gap-1.5 bg-white/5 rounded-lg px-2 py-1 border border-white/10">
              <TargetIcon />
              <span className="font-medium text-white text-sm">{stats?.total_detections || 0}</span>
            </div>
            {!isRunning && (
              <button onClick={() => setShowSettings(!showSettings)} className="yvd-btn yvd-btn-ghost p-1.5">
                <SettingsIcon />
              </button>
            )}
          </div>
        </div>

        {/* Settings Panel */}
        {showSettings && !isRunning && (
          <div className="bg-white/5 rounded-xl p-3 border border-white/10">
            <div className="flex items-center gap-2 mb-3">
              <SettingsIcon />
              <span className="text-white/70 text-xs font-medium">Stream Settings</span>
            </div>
            <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
              <div>
                <label className="text-white/50 text-[10px] block mb-1">Confidence</label>
                <input
                  type="number"
                  step="0.1"
                  min="0.1"
                  max="1"
                  value={settings.confidence}
                  onChange={(e) => setSettings(s => ({ ...s, confidence: parseFloat(e.target.value) || 0.5 }))}
                  className="yvd-input w-full"
                />
              </div>
              <div>
                <label className="text-white/50 text-[10px] block mb-1">Max Objects</label>
                <input
                  type="number"
                  min="1"
                  max="100"
                  value={settings.maxObjects}
                  onChange={(e) => setSettings(s => ({ ...s, maxObjects: parseInt(e.target.value) || 20 }))}
                  className="yvd-input w-full"
                />
              </div>
              <div>
                <label className="text-white/50 text-[10px] block mb-1">Target FPS</label>
                <input
                  type="number"
                  min="1"
                  max="60"
                  value={settings.targetFps}
                  onChange={(e) => setSettings(s => ({ ...s, targetFps: parseInt(e.target.value) || 15 }))}
                  className="yvd-input w-full"
                />
              </div>
              <div>
                <label className="text-white/50 text-[10px] block mb-1">Draw Boxes</label>
                <select
                  value={settings.drawBoxes ? 'yes' : 'no'}
                  onChange={(e) => setSettings(s => ({ ...s, drawBoxes: e.target.value === 'yes' }))}
                  className="yvd-input w-full"
                >
                  <option value="yes">Yes</option>
                  <option value="no">No</option>
                </select>
              </div>
            </div>
          </div>
        )}

        {/* Video Display */}
        <div className="relative aspect-video bg-black/50 rounded-xl overflow-hidden border border-white/10">
          {isRunning && streamInfo ? (
            <img
              src={getStreamUrl()}
              alt="YOLO Detection Stream"
              className="w-full h-full object-contain"
              onError={() => setError('Stream connection lost')}
            />
          ) : (
            <div className="absolute inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center">
              <div className="text-center">
                <VideoIcon />
                <p className="text-white/50 text-sm mt-2">Click Start to begin detection</p>
              </div>
            </div>
          )}

          {/* Live Indicator */}
          {isRunning && (
            <div className="absolute top-2 right-2 bg-red-500/80 backdrop-blur-sm rounded-full px-2 py-0.5">
              <div className="flex items-center gap-1">
                <div className="w-1.5 h-1.5 bg-white rounded-full animate-pulse" />
                <span className="text-white text-[10px] font-medium">LIVE</span>
              </div>
            </div>
          )}

          {/* FPS Badge */}
          {isRunning && stats && (
            <div className="absolute top-2 left-2 bg-black/60 backdrop-blur-sm rounded px-2 py-0.5">
              <span className="text-white/80 text-[10px] font-mono">{Math.round(stats.fps)} FPS</span>
            </div>
          )}
        </div>

        {/* Controls */}
        <div className="flex items-center gap-2 flex-wrap">
          {/* Source Presets */}
          <div className="flex gap-1">
            {sourcePresets.map(({ key, label, url }) => (
              <button
                key={key}
                onClick={() => {
                  if (isRunning) stopStream()
                  setCurrentSource(key as any)
                  setSourceUrlInput(url)
                }}
                className={`yvd-btn text-xs px-2.5 py-1 ${
                  currentSource === key ? 'yvd-btn-primary' : 'yvd-btn-ghost'
                }`}
              >
                {label}
              </button>
            ))}
          </div>

          {/* URL Input */}
          <input
            type="text"
            value={sourceUrlInput}
            onChange={(e) => setSourceUrlInput(e.target.value)}
            placeholder="camera://0"
            className="yvd-input flex-1 min-w-[120px]"
          />

          {/* Error Display */}
          {error && (
            <span className="text-red-400 text-[10px] truncate max-w-[100px]">{error}</span>
          )}

          {/* Start/Stop Button */}
          <button
            onClick={isRunning ? stopStream : startStream}
            className={`yvd-btn text-xs px-4 ${isRunning ? 'yvd-btn-danger' : 'yvd-btn-primary'}`}
          >
            {isRunning ? (
              <>
                <StopIcon />
                Stop
              </>
            ) : (
              <>
                <PlayIcon />
                Start
              </>
            )}
          </button>
        </div>

        {/* Stats Panel */}
        {showStats && stats && (
          <div className="bg-white/5 rounded-xl p-3 border border-white/10">
            <div className="flex items-center gap-2 mb-3">
              <ChartIcon />
              <span className="text-white/50 text-[10px] font-medium uppercase tracking-wider">Detection Statistics</span>
            </div>

            {/* Stats Grid */}
            <div className="grid grid-cols-4 gap-2 mb-3">
              <div className="yvd-stat-card">
                <div className="text-lg font-bold text-white">{stats.frame_count.toLocaleString()}</div>
                <div className="text-[10px] text-white/50">Frames</div>
              </div>
              <div className="yvd-stat-card">
                <div className="text-lg font-bold text-white">{Math.round(stats.fps)}</div>
                <div className="text-[10px] text-white/50">FPS</div>
              </div>
              <div className="yvd-stat-card">
                <div className="text-lg font-bold text-white">{stats.total_detections}</div>
                <div className="text-[10px] text-white/50">Detections</div>
              </div>
              <div className="yvd-stat-card">
                <div className="text-lg font-bold text-white">{sortedObjects.length}</div>
                <div className="text-[10px] text-white/50">Classes</div>
              </div>
            </div>

            {/* Detected Objects */}
            {sortedObjects.length > 0 && (
              <div className="flex flex-wrap gap-1.5">
                {sortedObjects.map(([label, count], i) => (
                  <span
                    key={label}
                    className="px-2 py-1 rounded-md text-[11px] font-medium transition-all hover:scale-105"
                    style={{
                      backgroundColor: `${DETECTION_COLORS[i % DETECTION_COLORS.length]}20`,
                      color: DETECTION_COLORS[i % DETECTION_COLORS.length],
                      border: `1px solid ${DETECTION_COLORS[i % DETECTION_COLORS.length]}40`
                    }}
                  >
                    {label} ×{count}
                  </span>
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  )
}

YoloVideoDisplay.displayName = 'YoloVideoDisplay'
export default { YoloVideoDisplay }