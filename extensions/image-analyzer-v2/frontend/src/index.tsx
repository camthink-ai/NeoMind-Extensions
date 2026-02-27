/**
 * Image Analyzer V2 Dashboard Component
 * Built with NeoMind Extension SDK V2
 *
 * Features:
 * - Before/After image comparison
 * - Detection results visualization
 * - Responsive and adaptive UI
 * - CSS variable-based theming
 * - ABI version 3 compatible
 * - YOLOv8 object detection
 */

import { forwardRef, useEffect, useState, useRef, useCallback, useMemo } from 'react'

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

const EXTENSION_ID = 'image-analyzer-v2'

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

async function getExtensionMetrics(extensionId: string): Promise<ExtensionCommandResult<Record<string, any>>> {
  const token = getAuthToken()
  const apiBase = getApiBase()
  const headers: Record<string, string> = {}
  if (token) headers['Authorization'] = `Bearer ${token}`

  try {
    const response = await fetch(`${apiBase}/extensions/${extensionId}/metrics`, { headers })
    if (!response.ok) return { success: false, error: `HTTP ${response.status}` }
    const data = await response.json()
    return data
  } catch (err) {
    return { success: false, error: err instanceof Error ? err.message : 'Network error' }
  }
}

// ============================================================================
// Types
// ============================================================================

interface Detection {
  label: string
  confidence: number
  bbox: { x: number; y: number; width: number; height: number }
}

interface AnalysisResult {
  objects: Detection[]
  description: string
  processing_time_ms: number
}

interface MetricsData {
  images_processed?: number
  avg_processing_time_ms?: number
  total_detections?: number
}

// ============================================================================
// Constants
// ============================================================================

const COLORS = [
  '#ef4444', '#f97316', '#eab308', '#22c55e', '#06b6d4',
  '#3b82f6', '#8b5cf6', '#ec4899', '#f43f5e', '#84cc16'
]

// ============================================================================
// Icons
// ============================================================================

const UploadIcon = () => (
  <svg className="w-6 h-6" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
    <path strokeLinecap="round" strokeLinejoin="round" d="M3 16.5v2.25A2.25 2.25 0 005.25 21h13.5A2.25 2.25 0 0021 18.75V16.5m-13.5-9L12 3m0 0l4.5 4.5M12 3v13.5" />
  </svg>
)

const ImageIcon = () => (
  <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
    <path strokeLinecap="round" strokeLinejoin="round" d="M2.25 15.75l5.159-5.159a2.25 2.25 0 013.182 0l5.159 5.159m-1.5-1.5l1.409-1.409a2.25 2.25 0 013.182 0l2.909 2.909m-18 3.75h16.5a1.5 1.5 0 001.5-1.5V6a1.5 1.5 0 00-1.5-1.5H3.75A1.5 1.5 0 002.25 6v12a1.5 1.5 0 001.5 1.5zm10.5-11.25h.008v.008h-.008V8.25zm.375 0a.375.375 0 11-.75 0 .375.375 0 01.75 0z" />
  </svg>
)

const TargetIcon = () => (
  <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
    <path strokeLinecap="round" strokeLinejoin="round" d="M15 10.5a3 3 0 11-6 0 3 3 0 016 0z" />
    <path strokeLinecap="round" strokeLinejoin="round" d="M19.5 10.5c0 7.142-7.5 11.25-7.5 11.25S4.5 17.642 4.5 10.5a7.5 7.5 0 1115 0z" />
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

// ============================================================================
// Image Analyzer Component
// ============================================================================

export interface ImageAnalyzerProps extends ExtensionComponentProps {
  showMetrics?: boolean
  maxImageSize?: number
  confidenceThreshold?: number
}

export const ImageAnalyzer = forwardRef<HTMLDivElement, ImageAnalyzerProps>(
  function ImageAnalyzer(props, ref) {
    const {
      title = 'Image Analyzer',
      dataSource,
      className = '',
      showMetrics = true,
      maxImageSize = 10 * 1024 * 1024,
      confidenceThreshold = 0.25
    } = props

    const [image, setImage] = useState<string | null>(null)
    const [imageSize, setImageSize] = useState<{ width: number; height: number } | null>(null)
    const [result, setResult] = useState<AnalysisResult | null>(null)
    const [analyzing, setAnalyzing] = useState(false)
    const [error, setError] = useState<string | null>(null)
    const [metrics, setMetrics] = useState<MetricsData>({})
    const [isDragging, setIsDragging] = useState(false)
    const [viewMode, setViewMode] = useState<'original' | 'detected' | 'split'>('split')
    const [splitPosition, setSplitPosition] = useState(50)
    const fileInputRef = useRef<HTMLInputElement>(null)
    const containerRef = useRef<HTMLDivElement>(null)

    const extensionId = dataSource?.extensionId || EXTENSION_ID

    const fetchMetrics = useCallback(async () => {
      const res = await getExtensionMetrics(extensionId)
      if (res.success && res.data) setMetrics(res.data)
    }, [extensionId])

    const handleImageUpload = useCallback((file: File) => {
      if (file.size > maxImageSize) {
        setError(`File too large (max ${maxImageSize / 1024 / 1024}MB)`)
        return
      }
      if (!file.type.startsWith('image/')) {
        setError('Please upload an image file')
        return
      }
      const reader = new FileReader()
      reader.onload = (e) => {
        const dataUrl = e.target?.result as string
        setImage(dataUrl)
        setResult(null)
        setError(null)
        const img = new Image()
        img.onload = () => setImageSize({ width: img.width, height: img.height })
        img.src = dataUrl
      }
      reader.readAsDataURL(file)
    }, [maxImageSize])

    const analyzeImage = useCallback(async () => {
      if (!image) return
      setAnalyzing(true)
      setError(null)

      try {
        const response = await fetch(image)
        const blob = await response.blob()
        const base64 = await new Promise<string>((resolve) => {
          const reader = new FileReader()
          reader.onloadend = () => resolve(reader.result as string)
          reader.readAsDataURL(blob)
        })

        const res = await executeExtensionCommand<AnalysisResult>(
          extensionId,
          'analyze_image',
          { image: base64.split(',')[1] }
        )

        if (res.success && res.data) {
          const data = res.data as any
          setResult({
            objects: (data.objects || []).map((obj: any) => ({
              label: obj.label || 'object',
              confidence: obj.confidence || 0,
              bbox: obj.bbox || { x: 0, y: 0, width: 0, height: 0 }
            })),
            description: data.description || '',
            processing_time_ms: data.processing_time_ms || 0
          })
          fetchMetrics()
        } else {
          setError(res.error || 'Analysis failed')
        }
      } catch {
        setError('Connection error')
      } finally {
        setAnalyzing(false)
      }
    }, [image, extensionId, fetchMetrics])

    useEffect(() => {
      fetchMetrics()
      const interval = setInterval(fetchMetrics, 5000)
      return () => clearInterval(interval)
    }, [fetchMetrics])

    const handleDrop = useCallback((e: React.DragEvent) => {
      e.preventDefault()
      setIsDragging(false)
      const file = e.dataTransfer.files[0]
      if (file) handleImageUpload(file)
    }, [handleImageUpload])

    const filteredDetections = useMemo(() => {
      if (!result) return []
      return result.objects.filter(d => d.confidence >= confidenceThreshold)
    }, [result, confidenceThreshold])

    // Group detections by label
    const detectionStats = useMemo(() => {
      const stats: Record<string, number> = {}
      filteredDetections.forEach(d => {
        stats[d.label] = (stats[d.label] || 0) + 1
      })
      return Object.entries(stats).sort((a, b) => b[1] - a[1])
    }, [filteredDetections])

    const renderBoundingBoxes = useCallback((forDisplay: boolean = true) => {
      if (!result || !imageSize) return null

      return filteredDetections.map((d, i) => {
        const color = COLORS[i % COLORS.length]
        const x = (d.bbox.x / imageSize.width) * 100
        const y = (d.bbox.y / imageSize.height) * 100
        const w = (d.bbox.width / imageSize.width) * 100
        const h = (d.bbox.height / imageSize.height) * 100

        return (
          <div
            key={i}
            className="absolute pointer-events-none"
            style={{
              left: `${x}%`,
              top: `${y}%`,
              width: `${w}%`,
              height: `${h}%`,
              border: `2px solid ${color}`,
              boxShadow: `0 0 8px ${color}60`,
              opacity: forDisplay ? 1 : 0.3
            }}
          >
            <div
              className="absolute -top-5 left-0 px-1.5 py-0.5 text-[10px] font-bold text-white rounded whitespace-nowrap"
              style={{ backgroundColor: color }}
            >
              {d.label} {Math.round(d.confidence * 100)}%
            </div>
          </div>
        )
      })
    }, [result, imageSize, filteredDetections])

    // Clear all state
    const clearImage = () => {
      setImage(null)
      setResult(null)
      setError(null)
      setImageSize(null)
    }

    return (
      <div
        ref={(node) => {
          (containerRef as any).current = node
          if (typeof ref === 'function') ref(node)
          else if (ref) ref.current = node
        }}
        className={`ia relative overflow-hidden rounded-xl shadow-lg transition-all ${className}`}
      >
        <style>{`
          .ia { --ext-bg: rgba(15, 23, 42, 0.95); --ext-fg: #f8fafc; --ext-muted: rgba(248, 250, 252, 0.5); --ext-border: rgba(255, 255, 255, 0.1); --ext-accent: #3b82f6; --ext-glass: rgba(255, 255, 255, 0.05); font-size: 14px; }
          .ia * { box-sizing: border-box; }
          .ia-card { background: linear-gradient(135deg, rgba(15, 23, 42, 0.98), rgba(30, 41, 59, 0.95)); backdrop-filter: blur(20px); border: 1px solid var(--ext-border); border-radius: 12px; }
          .ia-btn { display: inline-flex; align-items: center; gap: 4px; padding: 6px 12px; border-radius: 8px; font-size: 12px; font-weight: 500; transition: all 0.2s; cursor: pointer; border: none; }
          .ia-btn-primary { background: var(--ext-accent); color: white; }
          .ia-btn-primary:hover { filter: brightness(1.1); }
          .ia-btn-ghost { background: rgba(255,255,255,0.1); color: rgba(255,255,255,0.8); }
          .ia-btn-ghost:hover { background: rgba(255,255,255,0.15); }
          .ia-btn-active { background: var(--ext-accent); color: white; }
          .ia-split-container { position: relative; width: 100%; overflow: hidden; }
          .ia-split-slider { position: absolute; top: 0; bottom: 0; width: 4px; background: white; cursor: ew-resize; z-index: 20; transform: translateX(-50%); box-shadow: 0 0 10px rgba(0,0,0,0.5); }
          .ia-split-slider::after { content: ''; position: absolute; top: 50%; left: 50%; transform: translate(-50%, -50%); width: 24px; height: 24px; background: white; border-radius: 50%; box-shadow: 0 2px 8px rgba(0,0,0,0.3); }
        `}</style>

        <div className="ia-card p-3 flex flex-col gap-3" style={{ minHeight: '320px' }}>
          {/* Header */}
          <div className="flex items-center justify-between flex-wrap gap-2">
            <div className="flex items-center gap-2">
              <div className="bg-blue-500/20 rounded-lg p-1.5">
                <ImageIcon />
              </div>
              <div>
                <h3 className="font-semibold text-white text-sm">{title}</h3>
                <p className="text-white/40 text-[10px]">YOLOv8 Object Detection</p>
              </div>
            </div>

            {/* Stats */}
            {showMetrics && (
              <div className="flex items-center gap-3 text-xs">
                <div className="flex items-center gap-1.5 text-white/70">
                  <TargetIcon />
                  <span className="font-medium text-white">{metrics.total_detections ?? 0}</span>
                  <span className="text-white/50">detected</span>
                </div>
                <div className="flex items-center gap-1.5 text-white/70">
                  <ClockIcon />
                  <span className="font-medium text-white">{Math.round(metrics.avg_processing_time_ms || 0)}</span>
                  <span className="text-white/50">ms avg</span>
                </div>
              </div>
            )}
          </div>

          {/* Upload Area or Image Display */}
          {!image ? (
            <div
              onDrop={handleDrop}
              onDragOver={(e) => { e.preventDefault(); setIsDragging(true) }}
              onDragLeave={() => setIsDragging(false)}
              onClick={() => fileInputRef.current?.click()}
              className={`flex-1 min-h-[180px] border-2 border-dashed rounded-xl flex flex-col items-center justify-center cursor-pointer transition-all ${
                isDragging ? 'border-blue-400 bg-blue-500/10' : 'border-white/20 hover:border-blue-400/50 hover:bg-white/5'
              }`}
            >
              <input
                ref={fileInputRef}
                type="file"
                accept="image/jpeg,image/png,image/webp"
                className="hidden"
                onChange={(e) => { const file = e.target.files?.[0]; if (file) handleImageUpload(file) }}
              />
              <div className="p-4 rounded-full bg-blue-500/10 mb-3">
                <UploadIcon />
              </div>
              <p className="text-white/80 text-sm font-medium">Drop image or click to upload</p>
              <p className="text-white/40 text-xs mt-1">JPEG, PNG, WebP • Max {maxImageSize / 1024 / 1024}MB</p>
            </div>
          ) : (
            <div className="flex-1 flex flex-col gap-3">
              {/* View Mode Toggle */}
              {result && (
                <div className="flex items-center gap-1 bg-white/5 rounded-lg p-1">
                  {[
                    { key: 'original', label: 'Original' },
                    { key: 'split', label: 'Compare' },
                    { key: 'detected', label: 'Detected' }
                  ].map(({ key, label }) => (
                    <button
                      key={key}
                      onClick={() => setViewMode(key as any)}
                      className={`ia-btn text-xs px-3 py-1 ${viewMode === key ? 'ia-btn-active' : 'ia-btn-ghost'}`}
                    >
                      {label}
                    </button>
                  ))}
                </div>
              )}

              {/* Image Display */}
              <div className="relative flex-1 min-h-[160px] bg-black/40 rounded-xl overflow-hidden">
                {viewMode === 'split' && result ? (
                  /* Split View */
                  <div className="ia-split-container w-full h-full">
                    {/* Original Image */}
                    <div className="absolute inset-0">
                      <img src={image} alt="Original" className="w-full h-full object-contain" />
                    </div>
                    {/* Detected Image with clip */}
                    <div
                      className="absolute inset-0 overflow-hidden"
                      style={{ clipPath: `inset(0 ${100 - splitPosition}% 0 0)` }}
                    >
                      <img src={image} alt="Detected" className="w-full h-full object-contain" />
                      {renderBoundingBoxes()}
                    </div>
                    {/* Slider */}
                    <div
                      className="ia-split-slider"
                      style={{ left: `${splitPosition}%` }}
                      onMouseDown={(e) => {
                        e.preventDefault()
                        const onMove = (e: MouseEvent) => {
                          const rect = containerRef.current?.querySelector('.ia-split-container')?.getBoundingClientRect()
                          if (rect) {
                            const newSplit = ((e.clientX - rect.left) / rect.width) * 100
                            setSplitPosition(Math.max(5, Math.min(95, newSplit)))
                          }
                        }
                        const onUp = () => {
                          document.removeEventListener('mousemove', onMove)
                          document.removeEventListener('mouseup', onUp)
                        }
                        document.addEventListener('mousemove', onMove)
                        document.addEventListener('mouseup', onUp)
                      }}
                    />
                    {/* Labels */}
                    <div className="absolute top-2 left-2 bg-black/60 backdrop-blur-sm rounded px-2 py-0.5 text-[10px] text-white/80">Original</div>
                    <div className="absolute top-2 right-2 bg-black/60 backdrop-blur-sm rounded px-2 py-0.5 text-[10px] text-white/80">Detected</div>
                  </div>
                ) : viewMode === 'original' ? (
                  /* Original Only */
                  <img src={image} alt="Original" className="w-full h-full object-contain" />
                ) : (
                  /* Detected Only */
                  <div className="relative w-full h-full">
                    <img src={image} alt="Detected" className="w-full h-full object-contain" />
                    {result && renderBoundingBoxes()}
                  </div>
                )}

                {/* Loading Overlay */}
                {analyzing && (
                  <div className="absolute inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-30">
                    <div className="flex flex-col items-center gap-2">
                      <div className="w-8 h-8 border-2 border-blue-400/30 border-t-blue-400 rounded-full animate-spin" />
                      <p className="text-white/80 text-xs">Analyzing...</p>
                    </div>
                  </div>
                )}
              </div>

              {/* Controls */}
              <div className="flex items-center gap-2 flex-wrap">
                <button onClick={clearImage} className="ia-btn ia-btn-ghost text-xs">
                  <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
                    <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
                  </svg>
                  Clear
                </button>

                <div className="flex-1" />

                {result && filteredDetections.length > 0 && (
                  <div className="flex items-center gap-1.5 bg-emerald-500/20 rounded-lg px-2 py-1">
                    <TargetIcon />
                    <span className="text-emerald-300 text-xs font-medium">{filteredDetections.length} objects</span>
                    <span className="text-emerald-400/50 text-[10px]">• {result.processing_time_ms}ms</span>
                  </div>
                )}

                <button onClick={analyzeImage} disabled={analyzing} className={`ia-btn ia-btn-primary text-xs ${analyzing ? 'opacity-50 cursor-not-allowed' : ''}`}>
                  {analyzing ? (
                    <>
                      <div className="w-3.5 h-3.5 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                      Analyzing
                    </>
                  ) : (
                    <>
                      <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
                        <path strokeLinecap="round" strokeLinejoin="round" d="M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.993 0l3.181 3.183a8.25 8.25 0 0013.803-3.7M4.031 9.865a8.25 8.25 0 0113.803-3.7l3.181 3.182m0-4.991v4.99" />
                      </svg>
                      Analyze
                    </>
                  )}
                </button>
              </div>

              {/* Detection Results */}
              {result && filteredDetections.length > 0 && (
                <div className="bg-white/5 rounded-xl p-3 border border-white/10">
                  <div className="flex items-center gap-2 mb-2">
                    <ChartIcon />
                    <span className="text-white/50 text-[10px] font-medium uppercase tracking-wider">Analysis Results</span>
                  </div>

                  {/* Stats Grid */}
                  <div className="grid grid-cols-3 gap-2 mb-3">
                    <div className="bg-white/5 rounded-lg p-2 text-center">
                      <div className="text-lg font-bold text-white">{filteredDetections.length}</div>
                      <div className="text-[10px] text-white/50">Objects</div>
                    </div>
                    <div className="bg-white/5 rounded-lg p-2 text-center">
                      <div className="text-lg font-bold text-white">{detectionStats.length}</div>
                      <div className="text-[10px] text-white/50">Classes</div>
                    </div>
                    <div className="bg-white/5 rounded-lg p-2 text-center">
                      <div className="text-lg font-bold text-white">{result.processing_time_ms}</div>
                      <div className="text-[10px] text-white/50">ms</div>
                    </div>
                  </div>

                  {/* Detection Tags */}
                  <div className="flex flex-wrap gap-1.5">
                    {detectionStats.map(([label, count], i) => (
                      <span
                        key={label}
                        className="px-2 py-1 rounded-md text-[11px] font-medium"
                        style={{
                          backgroundColor: `${COLORS[i % COLORS.length]}20`,
                          color: COLORS[i % COLORS.length],
                          border: `1px solid ${COLORS[i % COLORS.length]}40`
                        }}
                      >
                        {label} ×{count}
                      </span>
                    ))}
                  </div>
                </div>
              )}

              {/* Error */}
              {error && (
                <div className="bg-red-500/20 rounded-lg p-2.5 border border-red-500/30">
                  <p className="text-red-300 text-xs">⚠️ {error}</p>
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    )
  }
)

ImageAnalyzer.displayName = 'ImageAnalyzer'
export default { ImageAnalyzer }