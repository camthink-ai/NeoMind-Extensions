/**
 * PaddleOCR-v6
 * PP-OCRv6 detection + multilingual recognition card.
 *
 * Calls the Rust extension's three commands via the host's REST bridge:
 *   - recognize   (detect + crop + recognize, returns text_blocks)
 *   - switch_tier (lazy-load tiny/small/medium)
 *   - health      (load status + active tier)
 *
 * Bounding boxes from the engine are normalized [0,1]; we rescale them
 * to the rendered image dimensions when drawing on the canvas overlay.
 */

import {
  forwardRef,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'

// ============================================================================
// Types — mirror Rust structs in extensions/paddle-ocr-v6/src/lib.rs
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

interface BoundingBox {
  x: number
  y: number
  width: number
  height: number
}

interface TextBlock {
  text: string
  confidence: number
  bbox: BoundingBox
  polygon?: Array<[number, number]>
}

interface OcrResult {
  text_blocks: TextBlock[]
  full_text: string
  total_blocks: number
  avg_confidence: number
  processing_time_ms: number
  image_width: number
  image_height: number
  tier: string
}

interface HealthInfo {
  loaded: boolean
  tier: string
  configured_tier: string
  models_dir: string
  load_error: string | null
}

// ============================================================================
// API
// ============================================================================

const EXTENSION_ID = 'paddle-ocr-v6'

const getApiHeaders = () => {
  const token =
    localStorage.getItem('neomind_token') ||
    sessionStorage.getItem('neomind_token_session')
  const headers: Record<string, string> = { 'Content-Type': 'application/json' }
  if (token) headers['Authorization'] = `Bearer ${token}`
  return headers
}

const getApiBase = () =>
  (window as any).__TAURI__ ? 'http://localhost:9375/api' : '/api'

async function runCommand<T = any>(
  extensionId: string,
  command: string,
  args: Record<string, any> = {},
): Promise<{ success: boolean; data?: T; error?: string }> {
  try {
    const res = await fetch(
      `${getApiBase()}/extensions/${extensionId}/command`,
      {
        method: 'POST',
        headers: getApiHeaders(),
        body: JSON.stringify({ command, args }),
      },
    )
    if (!res.ok) {
      // Surface server error body if present (host wraps ExtensionError here)
      let detail = `HTTP ${res.status}`
      try {
        const errBody = await res.json()
        if (errBody?.error) detail = errBody.error
        else if (errBody?.message) detail = errBody.message
      } catch {
        /* ignore parse error */
      }
      return { success: false, error: detail }
    }
    const body = await res.json()
    // Host returns { success, data? } OR the raw command result.
    if (body && typeof body === 'object' && 'success' in body) {
      return body as { success: boolean; data?: T; error?: string }
    }
    return { success: true, data: body as T }
  } catch (e) {
    return { success: false, error: e instanceof Error ? e.message : 'Network error' }
  }
}

// ============================================================================
// Styles — NeoMind CSS variables, scoped under .pocr- prefix
// ============================================================================

const CSS_ID = 'pocr-styles-v1'

const STYLES = `
.pocr {
  --pocr-fg: var(--foreground);
  --pocr-muted: var(--muted-foreground);
  --pocr-accent: var(--primary);
  --pocr-card: var(--card);
  --pocr-border: var(--border);
  --pocr-hover: rgba(0,0,0,0.03);
  --pocr-on-primary: var(--primary-foreground, #ffffff);
  width: 100%;
  height: 100%;
  font-size: 12px;
  display: flex;
  flex-direction: column;
}
.dark .pocr {
  --pocr-hover: rgba(255,255,255,0.03);
  --pocr-on-primary: var(--primary-foreground, #17172a);
}
.pocr-card {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
  padding: 10px;
  background: var(--pocr-card);
  backdrop-filter: blur(12px);
  border: 1px solid var(--pocr-border);
  border-radius: 8px;
  box-sizing: border-box;
}
.pocr-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-shrink: 0;
  margin-bottom: 8px;
  gap: 8px;
}
.pocr-title {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--pocr-fg);
  font-size: 13px;
  font-weight: 600;
}
.pocr-title svg {
  width: 16px;
  height: 16px;
}
.pocr-header-actions {
  display: flex;
  align-items: center;
  gap: 6px;
}
.pocr-tier-badge {
  padding: 2px 8px;
  border-radius: 4px;
  font-size: 10px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.3px;
  background: rgba(56, 142, 142, 0.12);
  color: var(--pocr-accent);
  border: 1px solid rgba(56, 142, 142, 0.25);
  cursor: default;
}
.pocr-tier-badge.loading {
  background: rgba(245, 158, 11, 0.12);
  color: #f59e0b;
  border-color: rgba(245, 158, 11, 0.25);
}
.pocr-tier-badge.error {
  background: rgba(239, 68, 68, 0.12);
  color: #ef4444;
  border-color: rgba(239, 68, 68, 0.25);
}
.pocr-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 0;
  overflow: hidden;
}

/* Upload */
.pocr-upload {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  flex: 1;
  border: 2px dashed var(--pocr-border);
  border-radius: 8px;
  cursor: pointer;
  transition: all 0.2s;
  min-height: 180px;
  padding: 16px;
}
.pocr-upload:hover {
  border-color: var(--pocr-accent);
  background: var(--pocr-hover);
}
.pocr-upload.dragover {
  border-color: var(--pocr-accent);
  background: var(--pocr-hover);
}
.pocr-upload-icon {
  width: 40px;
  height: 40px;
  color: var(--pocr-muted);
  margin-bottom: 8px;
}
.pocr-upload-text {
  color: var(--pocr-muted);
  font-size: 11px;
}
.pocr-upload-hint {
  color: var(--pocr-muted);
  font-size: 9px;
  opacity: 0.6;
  margin-top: 2px;
}

/* Preview */
.pocr-preview-wrapper {
  flex: 1;
  min-height: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 6px;
  overflow: hidden;
  position: relative;
  background: rgba(0,0,0,0.05);
}
.dark .pocr-preview-wrapper {
  background: rgba(0,0,0,0.2);
}
.pocr-canvas {
  max-width: 100%;
  max-height: 100%;
  width: auto;
  height: auto;
  object-fit: contain;
  display: block;
}

/* Results panel — bottom of the card, scrollable */
.pocr-results {
  flex-shrink: 0;
  max-height: 38%;
  overflow-y: auto;
  border-top: 1px solid var(--pocr-border);
  padding-top: 8px;
  margin-top: 4px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.pocr-results::-webkit-scrollbar {
  width: 4px;
}
.pocr-results::-webkit-scrollbar-thumb {
  background: rgba(142, 142, 142, 0.4);
  border-radius: 2px;
}
.pocr-stats {
  display: flex;
  gap: 12px;
  flex-shrink: 0;
  font-size: 10px;
  color: var(--pocr-muted);
}
.pocr-stat-value {
  color: var(--pocr-fg);
  font-weight: 600;
}
.pocr-fulltext {
  background: var(--pocr-hover);
  border-radius: 4px;
  padding: 6px 8px;
  font-size: 11px;
  color: var(--pocr-fg);
  white-space: pre-wrap;
  word-break: break-word;
  max-height: 80px;
  overflow-y: auto;
  font-family: inherit;
  line-height: 1.4;
}
.pocr-blocks-list {
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.pocr-block-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 8px;
  padding: 3px 6px;
  border-radius: 4px;
  background: var(--pocr-hover);
  font-size: 11px;
}
.pocr-block-text {
  color: var(--pocr-fg);
  flex: 1;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.pocr-block-conf {
  color: var(--pocr-muted);
  font-size: 10px;
  flex-shrink: 0;
  font-variant-numeric: tabular-nums;
}

/* Tier selector */
.pocr-tier-select {
  background: transparent;
  color: var(--pocr-fg);
  border: 1px solid var(--pocr-border);
  border-radius: 4px;
  padding: 2px 6px;
  font-size: 10px;
  cursor: pointer;
}

/* Actions */
.pocr-actions {
  display: flex;
  gap: 6px;
  flex-shrink: 0;
  margin-top: 6px;
  padding-top: 6px;
  border-top: 1px solid var(--pocr-border);
}
.pocr-btn {
  flex: 1;
  padding: 6px 12px;
  border: 1px solid var(--pocr-border);
  border-radius: 6px;
  font-size: 11px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.2s;
  background: transparent;
  color: var(--pocr-fg);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
}
.pocr-btn:hover {
  background: var(--pocr-hover);
}
.pocr-btn-primary {
  background: var(--pocr-accent);
  border-color: var(--pocr-accent);
  color: var(--pocr-on-primary);
}
.pocr-btn-primary:hover {
  opacity: 0.9;
  background: var(--pocr-accent);
}
.pocr-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.pocr-btn svg {
  width: 13px;
  height: 13px;
}

/* Loading / error / empty states */
.pocr-loading {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  flex: 1;
  gap: 8px;
  color: var(--pocr-muted);
}
.pocr-spinner {
  width: 24px;
  height: 24px;
  border: 2px solid var(--pocr-border);
  border-top-color: var(--pocr-accent);
  border-radius: 50%;
  animation: pocr-spin 0.7s linear infinite;
}
@keyframes pocr-spin {
  to { transform: rotate(360deg); }
}
.pocr-error {
  padding: 8px;
  background: rgba(239, 68, 68, 0.1);
  border: 1px solid rgba(239, 68, 68, 0.2);
  border-radius: 6px;
  color: #ef4444;
  font-size: 10px;
  flex-shrink: 0;
  word-break: break-word;
}
.pocr-empty {
  color: var(--pocr-muted);
  font-size: 10px;
  text-align: center;
  padding: 6px;
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
// Icons (inline SVG)
// ============================================================================

const ICONS: Record<string, string> = {
  'file-text':
    '<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="16" y1="13" x2="8" y2="13"/><line x1="16" y1="17" x2="8" y2="17"/><polyline points="10 9 9 9 8 9"/>',
  upload:
    '<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/>',
  zap: '<polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/>',
  x: '<line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>',
  refresh:
    '<path d="M21 12a9 9 0 1 1-9-9c2.5 0 4.9 1 6.7 2.7L21 8M21 3v5h-5"/>',
}

const Icon = ({
  name,
  className = '',
  style,
}: {
  name: string
  className?: string
  style?: React.CSSProperties
}) => (
  <svg
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth={2}
    strokeLinecap="round"
    strokeLinejoin="round"
    className={className}
    style={style}
    dangerouslySetInnerHTML={{ __html: ICONS[name] || ICONS['file-text'] }}
  />
)

// ============================================================================
// Component
// ============================================================================

export const PaddleOcrV6Card = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function PaddleOcrV6Card(props, ref) {
    const {
      title = 'PaddleOCR v6',
      dataSource,
      className = '',
      config = {},
    } = props

    useEffect(() => injectStyles(), [])

    const extensionId = dataSource?.extensionId || EXTENSION_ID

    // user-configurable options
    const drawBoxes = config.drawBoxes !== false // default true
    const showConfidence = config.showConfidence !== false // default true
    const initialTier =
      (typeof config.tier === 'string' && config.tier) || 'auto'

    // state
    const [image, setImage] = useState<string | null>(null) // base64 (no data: prefix)
    const [imageMime, setImageMime] = useState<string>('image/jpeg')
    const [result, setResult] = useState<OcrResult | null>(null)
    const [loading, setLoading] = useState(false)
    const [error, setError] = useState<string | null>(null)
    const [tier, setTier] = useState<string>(initialTier)
    const [health, setHealth] = useState<HealthInfo | null>(null)
    const [tierSwitching, setTierSwitching] = useState(false)
    const [dragOver, setDragOver] = useState(false)

    const canvasRef = useRef<HTMLCanvasElement>(null)
    const fileInputRef = useRef<HTMLInputElement>(null)

    // -------------------------------------------------------------
    // Image loading (file picker + drag/drop + paste)
    // -------------------------------------------------------------

    const acceptFile = useCallback((file: File) => {
      if (!file.type.startsWith('image/')) {
        setError('Please select an image file (PNG / JPEG / WebP)')
        return
      }
      // Cap at ~10 MB to stay within HTTP body limits
      if (file.size > 10 * 1024 * 1024) {
        setError('Image too large (max 10 MB)')
        return
      }
      const reader = new FileReader()
      reader.onload = (ev) => {
        const dataUrl = ev.target?.result as string | undefined
        if (!dataUrl) return
        const commaIdx = dataUrl.indexOf(',')
        const base64 = commaIdx >= 0 ? dataUrl.slice(commaIdx + 1) : dataUrl
        // mime like "image/png"
        const mime = file.type || 'image/jpeg'
        setImage(base64)
        setImageMime(mime)
        setResult(null)
        setError(null)
      }
      reader.readAsDataURL(file)
    }, [])

    const handleFileSelect = useCallback(
      (e: React.ChangeEvent<HTMLInputElement>) => {
        const file = e.target.files?.[0]
        if (file) acceptFile(file)
      },
      [acceptFile],
    )

    const handleDrop = useCallback(
      (e: React.DragEvent) => {
        e.preventDefault()
        setDragOver(false)
        const file = e.dataTransfer.files?.[0]
        if (file) acceptFile(file)
      },
      [acceptFile],
    )

    const handlePaste = useCallback(
      (e: React.ClipboardEvent) => {
        const items = e.clipboardData?.items
        if (!items) return
        for (let i = 0; i < items.length; i++) {
          if (items[i].type.startsWith('image/')) {
            const file = items[i].getAsFile()
            if (file) {
              acceptFile(file)
              break
            }
          }
        }
      },
      [acceptFile],
    )

    // -------------------------------------------------------------
    // Commands
    // -------------------------------------------------------------

    const refreshHealth = useCallback(async () => {
      const res = await runCommand<HealthInfo>(extensionId, 'health', {})
      if (res.success && res.data) setHealth(res.data)
    }, [extensionId])

    // Initial health probe so the tier badge reflects reality
    useEffect(() => {
      refreshHealth().catch(() => {})
    }, [refreshHealth])

    const handleSwitchTier = useCallback(
      async (newTier: string) => {
        setTier(newTier)
        setTierSwitching(true)
        setError(null)
        const res = await runCommand(extensionId, 'switch_tier', {
          tier: newTier,
        })
        setTierSwitching(false)
        if (!res.success) {
          setError(res.error || `Failed to switch to ${newTier} tier`)
        }
        await refreshHealth()
      },
      [extensionId, refreshHealth],
    )

    const handleRecognize = useCallback(async () => {
      if (!image) return
      setLoading(true)
      setError(null)
      const res = await runCommand<OcrResult>(extensionId, 'recognize', {
        image_base64: image,
      })
      if (res.success && res.data) {
        setResult(res.data)
        // active tier may have changed (auto → concrete); refresh health
        await refreshHealth()
      } else {
        setError(res.error || 'Recognition failed')
      }
      setLoading(false)
    }, [extensionId, image, refreshHealth])

    const handleClear = useCallback(() => {
      setImage(null)
      setResult(null)
      setError(null)
      if (fileInputRef.current) fileInputRef.current.value = ''
    }, [])

    // -------------------------------------------------------------
    // Draw bounding boxes on canvas over the loaded image
    // -------------------------------------------------------------

    useEffect(() => {
      if (!image || !canvasRef.current) return
      const canvas = canvasRef.current
      const ctx = canvas.getContext('2d')
      if (!ctx) return

      const img = new Image()
      img.onload = () => {
        canvas.width = img.naturalWidth
        canvas.height = img.naturalHeight
        ctx.drawImage(img, 0, 0)

        if (!drawBoxes || !result || result.text_blocks.length === 0) return

        const W = img.naturalWidth
        const H = img.naturalHeight
        // Pick a stroke width proportional to image size (not too thick)
        const stroke = Math.max(2, Math.round(Math.min(W, H) / 250))
        const fontPx = Math.max(14, Math.round(Math.min(W, H) / 40))

        result.text_blocks.forEach((blk) => {
          // skip empty recognitions — they're placeholders for failed crops
          if (!blk.text) return
          const { x, y, width, height } = blk.bbox
          const px = x * W
          const py = y * H
          const pw = width * W
          const ph = height * H

          // Box — green, distinct from object-detection colors
          ctx.strokeStyle = 'hsl(142, 70%, 55%)'
          ctx.lineWidth = stroke
          ctx.strokeRect(px, py, pw, ph)

          // Label background
          const conf =
            showConfidence && blk.confidence > 0
              ? ` ${(blk.confidence * 100).toFixed(0)}%`
              : ''
          const label = `${blk.text}${conf}`
          ctx.font = `${fontPx}px -apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif`
          const metrics = ctx.measureText(label)
          const labelH = fontPx + 6
          const labelW = metrics.width + 12
          // Anchor label at top-left of box; if it would clip above, drop it inside
          const labelY = py - labelH < 0 ? py : py - labelH

          ctx.fillStyle = 'rgba(20, 20, 20, 0.85)'
          ctx.fillRect(px, labelY, Math.min(labelW, W - px), labelH)
          ctx.fillStyle = 'hsl(142, 70%, 70%)'
          ctx.fillText(label, px + 6, labelY + fontPx + 1)
        })
      }
      img.onerror = () => {
        /* ignore — canvas stays blank */
      }
      img.src = `data:${imageMime};base64,${image}`
    }, [image, imageMime, result, drawBoxes, showConfidence])

    // -------------------------------------------------------------
    // Derived
    // -------------------------------------------------------------

    const tierBadgeClass = useMemo(() => {
      if (tierSwitching) return 'pocr-tier-badge loading'
      if (health?.load_error) return 'pocr-tier-badge error'
      if (health?.loaded) return 'pocr-tier-badge'
      return 'pocr-tier-badge loading'
    }, [tierSwitching, health])

    const tierBadgeText = useMemo(() => {
      if (tierSwitching) return 'switching…'
      if (health?.loaded) return health.tier || tier
      if (health?.load_error) return 'error'
      return tier
    }, [tierSwitching, health, tier])

    return (
      <div
        ref={ref}
        className={`pocr ${className}`}
        onPaste={handlePaste}
        tabIndex={0}
      >
        <div className="pocr-card">
          {/* Header */}
          <div className="pocr-header">
            <div className="pocr-title">
              <Icon name="file-text" />
              <span>{title}</span>
            </div>
            <div className="pocr-header-actions">
              <select
                className="pocr-tier-select"
                value={tier}
                onChange={(e) => handleSwitchTier(e.target.value)}
                disabled={tierSwitching}
                title="Switch model tier (tiny ships in .nep; small/medium lazy-download)"
              >
                <option value="auto">auto</option>
                <option value="tiny">tiny</option>
                <option value="small">small</option>
                <option value="medium">medium</option>
              </select>
              <span className={tierBadgeClass} title={health?.load_error || ''}>
                {tierBadgeText}
              </span>
            </div>
          </div>

          {/* Content */}
          <div className="pocr-content">
            {!image ? (
              <div
                className={`pocr-upload ${dragOver ? 'dragover' : ''}`}
                onClick={() => fileInputRef.current?.click()}
                onDragOver={(e) => {
                  e.preventDefault()
                  setDragOver(true)
                }}
                onDragLeave={() => setDragOver(false)}
                onDrop={handleDrop}
              >
                <input
                  ref={fileInputRef}
                  type="file"
                  accept="image/*"
                  onChange={handleFileSelect}
                  style={{ display: 'none' }}
                />
                <Icon name="upload" className="pocr-upload-icon" />
                <div className="pocr-upload-text">
                  Click / drop / paste an image
                </div>
                <div className="pocr-upload-hint">
                  Supports JPG, PNG, WebP (max 10 MB)
                </div>
              </div>
            ) : loading ? (
              <div className="pocr-loading">
                <div className="pocr-spinner" />
                <span>Recognizing…</span>
              </div>
            ) : (
              <>
                <div className="pocr-preview-wrapper">
                  <canvas ref={canvasRef} className="pocr-canvas" />
                </div>

                {error && <div className="pocr-error">{error}</div>}

                {result && (
                  <div className="pocr-results">
                    <div className="pocr-stats">
                      <span>
                        blocks:{' '}
                        <span className="pocr-stat-value">
                          {result.total_blocks}
                        </span>
                      </span>
                      <span>
                        avg conf:{' '}
                        <span className="pocr-stat-value">
                          {(result.avg_confidence * 100).toFixed(0)}%
                        </span>
                      </span>
                      <span>
                        time:{' '}
                        <span className="pocr-stat-value">
                          {result.processing_time_ms}ms
                        </span>
                      </span>
                    </div>

                    {result.full_text ? (
                      <pre className="pocr-fulltext">{result.full_text}</pre>
                    ) : (
                      <div className="pocr-empty">No text recognized</div>
                    )}

                    {result.text_blocks.length > 0 && (
                      <div className="pocr-blocks-list">
                        {result.text_blocks
                          .filter((b) => b.text)
                          .slice(0, 20)
                          .map((blk, i) => (
                            <div key={i} className="pocr-block-item">
                              <span className="pocr-block-text" title={blk.text}>
                                {blk.text}
                              </span>
                              <span className="pocr-block-conf">
                                {(blk.confidence * 100).toFixed(0)}%
                              </span>
                            </div>
                          ))}
                      </div>
                    )}
                  </div>
                )}

                {/* Actions */}
                <div className="pocr-actions">
                  <button
                    onClick={handleClear}
                    className="pocr-btn"
                    type="button"
                  >
                    <Icon name="x" />
                    Clear
                  </button>
                  <button
                    onClick={handleRecognize}
                    disabled={loading}
                    className="pocr-btn pocr-btn-primary"
                    type="button"
                  >
                    <Icon name="zap" />
                    Recognize
                  </button>
                </div>
              </>
            )}
          </div>
        </div>
      </div>
    )
  },
)

PaddleOcrV6Card.displayName = 'PaddleOcrV6Card'

export type { BoundingBox, TextBlock, OcrResult, HealthInfo }
export default { PaddleOcrV6Card }
