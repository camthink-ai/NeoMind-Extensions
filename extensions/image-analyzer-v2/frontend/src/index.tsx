/**
 * Image Analyzer V2
 * Matches NeoMind dashboard design system
 */

import { forwardRef, useEffect, useState, useRef, useCallback, useMemo } from 'react'

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
  label: string
  confidence: number
  bbox: { x: number; y: number; width: number; height: number } | null
}

interface AnalysisResult {
  objects: Detection[]
  description: string
  processing_time_ms: number
  model_loaded: boolean
  model_error?: string
}

// ============================================================================
// API
// ============================================================================

const EXTENSION_ID = 'image-analyzer-v2'

const getApiHeaders = () => {
  const token = localStorage.getItem('neomind_token') || sessionStorage.getItem('neomind_token_session')
  const headers: Record<string, string> = { 'Content-Type': 'application/json' }
  if (token) headers['Authorization'] = `Bearer ${token}`
  return headers
}

const getApiBase = () => (window as any).__TAURI__ ? 'http://localhost:9375/api' : '/api'

async function analyzeImage(extensionId: string, imageBase64: string): Promise<{ success: boolean; data?: AnalysisResult; error?: string }> {
  try {
    const res = await fetch(`${getApiBase()}/extensions/${extensionId}/command`, {
      method: 'POST',
      headers: getApiHeaders(),
      body: JSON.stringify({ command: 'analyze_image', args: { image: imageBase64 } })
    })
    if (!res.ok) return { success: false, error: `HTTP ${res.status}` }
    return res.json()
  } catch (e) {
    return { success: false, error: e instanceof Error ? e.message : 'Network error' }
  }
}

// ============================================================================
// Styles
// ============================================================================

const CSS_ID = 'ia-styles-v2'

const STYLES = `
.ia {
  --ia-fg: hsl(240 10% 10%);
  --ia-muted: hsl(240 5% 45%);
  --ia-accent: hsl(142 70% 65%);
  --ia-card: rgba(255,255,255,0.5);
  --ia-border: rgba(0,0,0,0.06);
  --ia-hover: rgba(0,0,0,0.03);
  width: 100%;
  height: 100%;
  font-size: 12px;
}
.dark .ia {
  --ia-fg: hsl(0 0% 95%);
  --ia-muted: hsl(0 0% 60%);
  --ia-card: rgba(30,30,30,0.5);
  --ia-border: rgba(255,255,255,0.08);
  --ia-hover: rgba(255,255,255,0.03);
}
.ia-card {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: 10px;
  background: var(--ia-card);
  backdrop-filter: blur(12px);
  border: 1px solid var(--ia-border);
  border-radius: 8px;
  box-sizing: border-box;
}
.ia-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 8px;
}
.ia-title {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--ia-fg);
  font-size: 13px;
  font-weight: 600;
}
.ia-badge {
  padding: 2px 6px;
  background: rgba(142, 70, 65, 0.1);
  color: var(--ia-accent);
  border-radius: 4px;
  font-size: 9px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.3px;
}
.ia-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 8px;
  overflow-y: auto;
}

.ia-upload {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  flex: 1;
  border: 2px dashed var(--ia-border);
  border-radius: 8px;
  cursor: pointer;
  transition: all 0.2s;
  min-height: 140px;
}
.ia-upload:hover {
  border-color: var(--ia-accent);
  background: var(--ia-hover);
}
.ia-upload-icon {
  width: 40px;
  height: 40px;
  color: var(--ia-muted);
  margin-bottom: 8px;
}
.ia-upload-text {
  color: var(--ia-muted);
  font-size: 11px;
}
.ia-upload-hint {
  color: var(--ia-muted);
  font-size: 9px;
  opacity: 0.6;
  margin-top: 2px;
}

.ia-preview {
  position: relative;
  border-radius: 6px;
  overflow: hidden;
  background: rgba(0,0,0,0.1);
}
.dark .ia-preview {
  background: rgba(0,0,0,0.3);
}
.ia-canvas {
  width: 100%;
  height: auto;
  display: block;
}
.ia-image {
  width: 100%;
  height: auto;
  display: block;
  border-radius: 6px;
}

.ia-stats {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: 6px;
}
.ia-stat {
  padding: 6px;
  background: var(--ia-hover);
  border: 1px solid var(--ia-border);
  border-radius: 6px;
}
.ia-stat-label {
  font-size: 9px;
  color: var(--ia-muted);
  text-transform: uppercase;
  letter-spacing: 0.3px;
  margin-bottom: 2px;
}
.ia-stat-value {
  font-size: 16px;
  font-weight: 700;
  color: var(--ia-fg);
}

.ia-objects {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}
.ia-object-tag {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 3px 6px;
  background: rgba(142, 70, 65, 0.1);
  border: 1px solid rgba(142, 70, 65, 0.2);
  border-radius: 4px;
  font-size: 10px;
  color: var(--ia-accent);
}

.ia-actions {
  display: flex;
  gap: 6px;
}
.ia-btn {
  flex: 1;
  padding: 6px 12px;
  border: 1px solid var(--ia-border);
  border-radius: 6px;
  font-size: 11px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.2s;
  background: transparent;
  color: var(--ia-fg);
}
.ia-btn:hover {
  background: var(--ia-hover);
}
.ia-btn-primary {
  background: var(--ia-accent);
  border-color: var(--ia-accent);
  color: #000;
}
.ia-btn-primary:hover {
  opacity: 0.9;
  background: var(--ia-accent);
}
.ia-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.ia-error {
  padding: 8px;
  background: rgba(239, 68, 68, 0.1);
  border: 1px solid rgba(239, 68, 68, 0.2);
  border-radius: 6px;
  color: #ef4444;
  font-size: 10px;
}

.ia-loading {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  flex: 1;
  gap: 8px;
  color: var(--ia-muted);
}
.ia-spinner {
  width: 24px;
  height: 24px;
  border: 2px solid var(--ia-border);
  border-top-color: var(--ia-accent);
  border-radius: 50%;
  animation: ia-spin 0.7s linear infinite;
}
@keyframes ia-spin {
  to { transform: rotate(360deg); }
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
// Icons (inline SVG paths)
// ============================================================================

const ICONS: Record<string, string> = {
  image: '<rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/>',
  upload: '<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/>',
  zap: '<polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/>',
  box: '<path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z"/><polyline points="3.27 6.96 12 12.01 20.73 6.96"/><line x1="12" y1="22.08" x2="12" y2="12"/>',
  clock: '<circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/>',
  x: '<line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>',
  refresh: '<path d="M21 12a9 9 0 1 1-9-9c2.5 0 4.9 1 6.7 2.7L21 8M21 3v5h-5"/>',
}

const Icon = ({ name, className = '', style }: { name: string; className?: string; style?: React.CSSProperties }) => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={className} style={style}
    dangerouslySetInnerHTML={{ __html: ICONS[name] || ICONS.image }} />
)

// ============================================================================
// Component
// ============================================================================

export const ImageAnalyzer = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function ImageAnalyzer(props, ref) {
    const { title = 'Image Analyzer', dataSource, className = '' } = props

    useEffect(() => injectStyles(), [])

    const extensionId = dataSource?.extensionId || EXTENSION_ID

    const [image, setImage] = useState<string | null>(null)
    const [result, setResult] = useState<AnalysisResult | null>(null)
    const [loading, setLoading] = useState(false)
    const [error, setError] = useState<string | null>(null)

    const canvasRef = useRef<HTMLCanvasElement>(null)
    const fileInputRef = useRef<HTMLInputElement>(null)

    const handleFileSelect = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0]
      if (!file) return
      if (!file.type.startsWith('image/')) {
        setError('Please select an image file')
        return
      }
      const reader = new FileReader()
      reader.onload = (ev) => {
        const base64 = (ev.target?.result as string)?.split(',')[1]
        if (base64) {
          setImage(base64)
          setResult(null)
          setError(null)
        }
      }
      reader.readAsDataURL(file)
    }, [])

    const handleAnalyze = useCallback(async () => {
      if (!image) return
      setLoading(true)
      setError(null)
      const res = await analyzeImage(extensionId, image)
      if (res.success && res.data) {
        setResult(res.data)
      } else {
        setError(res.error || 'Analysis failed')
      }
      setLoading(false)
    }, [extensionId, image])

    const handleClear = useCallback(() => {
      setImage(null)
      setResult(null)
      setError(null)
      if (canvasRef.current) {
        const ctx = canvasRef.current.getContext('2d')
        if (ctx) ctx.clearRect(0, 0, canvasRef.current.width, canvasRef.current.height)
      }
      if (fileInputRef.current) fileInputRef.current.value = ''
    }, [])

    // Draw detections on canvas
    useEffect(() => {
      if (!result || !image || !canvasRef.current) return
      const canvas = canvasRef.current
      const ctx = canvas.getContext('2d')
      if (!ctx) return

      const img = new Image()
      img.onload = () => {
        canvas.width = img.width
        canvas.height = img.height
        ctx.drawImage(img, 0, 0)

        result.objects.forEach((det) => {
          if (!det.bbox) return
          const { x, y, width, height } = det.bbox

          // Draw box
          ctx.strokeStyle = 'hsl(142, 70%, 65%)'
          ctx.lineWidth = 3
          ctx.strokeRect(x, y, width, height)

          // Draw label
          const label = `${det.label} ${(det.confidence * 100).toFixed(0)}%`
          ctx.font = '14px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'
          const metrics = ctx.measureText(label)
          const padding = 6

          ctx.fillStyle = 'rgba(20, 20, 20, 0.9)'
          ctx.fillRect(x, y - 24, metrics.width + padding * 2, 24)

          ctx.fillStyle = 'hsl(142, 70%, 65%)'
          ctx.fillText(label, x + padding, y - 7)
        })
      }
      img.src = `data:image/jpeg;base64,${image}`
    }, [result, image])

    const objectCounts = useMemo(() => {
      if (!result) return {}
      return result.objects.reduce((acc, obj) => {
        acc[obj.label] = (acc[obj.label] || 0) + 1
        return acc
      }, {} as Record<string, number>)
    }, [result])

    return (
      <div ref={ref} className={`ia ${className}`}>
        <div className="ia-card">
          {/* Header */}
          <div className="ia-header">
            <div className="ia-title">
              <Icon name="image" style={{ width: '16px', height: '16px' }} />
              <span>{title}</span>
            </div>
            <div className="ia-badge">YOLOv8</div>
          </div>

          {/* Content */}
          <div className="ia-content">
            {!image ? (
              /* Upload */
              <div
                className="ia-upload"
                onClick={() => fileInputRef.current?.click()}
              >
                <input
                  ref={fileInputRef}
                  type="file"
                  accept="image/*"
                  onChange={handleFileSelect}
                  style={{ display: 'none' }}
                />
                <Icon name="upload" className="ia-upload-icon" />
                <div className="ia-upload-text">Click to upload image</div>
                <div className="ia-upload-hint">Supports JPG, PNG, WebP</div>
              </div>
            ) : loading ? (
              /* Loading */
              <div className="ia-loading">
                <div className="ia-spinner" />
                <span>Analyzing...</span>
              </div>
            ) : (
              /* Results */
              <>
                {/* Preview - show original image or canvas with detections */}
                <div className="ia-preview">
                  {result ? (
                    <canvas ref={canvasRef} className="ia-canvas" />
                  ) : (
                    <img
                      src={`data:image/jpeg;base64,${image}`}
                      alt="Uploaded"
                      className="ia-image"
                    />
                  )}
                </div>

                {/* Stats */}
                {result && (
                  <>
                    <div className="ia-stats">
                      <div className="ia-stat">
                        <div className="ia-stat-label">Objects</div>
                        <div className="ia-stat-value">{result.objects.length}</div>
                      </div>
                      <div className="ia-stat">
                        <div className="ia-stat-label">Time</div>
                        <div className="ia-stat-value">{result.processing_time_ms}ms</div>
                      </div>
                    </div>

                    {/* Object tags */}
                    {Object.keys(objectCounts).length > 0 && (
                      <div className="ia-objects">
                        {Object.entries(objectCounts).map(([label, count]) => (
                          <div key={label} className="ia-object-tag">
                            <Icon name="box" style={{ width: '12px', height: '12px' }} />
                            <span>{label} ×{count}</span>
                          </div>
                        ))}
                      </div>
                    )}

                    {/* Model error */}
                    {!result.model_loaded && (
                      <div className="ia-error">
                        Model not loaded: {result.model_error || 'Unknown error'}
                      </div>
                    )}
                  </>
                )}

                {/* Error */}
                {error && (
                  <div className="ia-error">{error}</div>
                )}

                {/* Actions */}
                <div className="ia-actions">
                  <button onClick={handleClear} className="ia-btn">
                    <Icon name="x" style={{ width: '14px', height: '14px', display: 'inline', verticalAlign: 'middle', marginRight: '4px' }} />
                    Clear
                  </button>
                  <button onClick={handleAnalyze} disabled={loading} className="ia-btn ia-btn-primary">
                    <Icon name="zap" style={{ width: '14px', height: '14px', display: 'inline', verticalAlign: 'middle', marginRight: '4px' }} />
                    Analyze
                  </button>
                </div>
              </>
            )}
          </div>
        </div>
      </div>
    )
  }
)

ImageAnalyzer.displayName = 'ImageAnalyzer'
export default { ImageAnalyzer }
