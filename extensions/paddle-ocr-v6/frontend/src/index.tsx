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
        const e = errBody?.error ?? errBody?.message
        if (typeof e === 'string') detail = e
        else if (e && typeof e === 'object') detail = e.message || e.code || detail
      } catch {
        /* ignore parse error */
      }
      return { success: false, error: detail }
    }
    const body = await res.json()
    // Host returns { success, data? } OR the raw command result.
    if (body && typeof body === 'object' && 'success' in body) {
      // Normalize error: server may return { error: { code, message } } (object)
      // which React can't render — flatten to string.
      let errorStr: string | undefined
      const e = body.error
      if (typeof e === 'string') errorStr = e
      else if (e && typeof e === 'object') errorStr = e.message || e.code
      return { success: body.success, data: body.data, error: errorStr }
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
  display: inline-flex;
  align-items: center;
  height: 18px;
  padding: 0 6px;
  border-radius: 4px;
  font-size: 10px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.3px;
  background: rgba(56, 142, 142, 0.12);
  color: var(--pocr-accent);
  border: 1px solid rgba(56, 142, 142, 0.25);
  cursor: default;
  box-sizing: border-box;
  line-height: 1;
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

/* Preview — takes all available space, image gets maximal real estate */
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
/* Show/hide bounding boxes overlay toggle (bottom-left of preview) */
.pocr-box-toggle {
  position: absolute;
  left: 6px;
  bottom: 6px;
  width: 26px;
  height: 26px;
  padding: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.45);
  color: rgba(255, 255, 255, 0.85);
  border: 1px solid rgba(255, 255, 255, 0.2);
  border-radius: 5px;
  cursor: pointer;
  transition: background 0.15s, color 0.15s, border-color 0.15s;
  box-sizing: border-box;
  z-index: 4;
}
.pocr-box-toggle:hover {
  background: rgba(0, 0, 0, 0.65);
  color: #fff;
}
.pocr-box-toggle.is-active {
  color: #5eead4;
  border-color: rgba(94, 234, 212, 0.55);
}
.pocr-box-toggle svg {
  width: 14px;
  height: 14px;
}
.pocr-loading-overlay {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  background: rgba(0,0,0,0.45);
  color: #fff;
  font-size: 12px;
  border-radius: 6px;
}

/* Results — floating panel bottom-right, collapsible */
.pocr-results-toggle {
  position: absolute;
  right: 8px;
  bottom: 8px;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 5px 10px;
  background: rgba(20, 20, 20, 0.78);
  color: #fff;
  border: 1px solid rgba(255,255,255,0.12);
  border-radius: 999px;
  font-size: 11px;
  font-weight: 500;
  cursor: pointer;
  backdrop-filter: blur(10px);
  -webkit-backdrop-filter: blur(10px);
  transition: background 0.15s;
  user-select: none;
  z-index: 5;
}
.pocr-results-toggle:hover {
  background: rgba(30, 30, 30, 0.92);
}
.pocr-results-toggle svg {
  width: 12px;
  height: 12px;
  transition: transform 0.18s;
}
.pocr-results-toggle.collapsed svg {
  transform: rotate(-90deg);
}
.pocr-results-toggle .pocr-toggle-count {
  opacity: 0.7;
  font-variant-numeric: tabular-nums;
}
.pocr-results-panel {
  position: absolute;
  right: 8px;
  bottom: 42px;
  left: 8px;
  max-height: 60%;
  background: var(--pocr-card);
  border: 1px solid var(--pocr-border);
  border-radius: 8px;
  box-shadow: 0 8px 24px rgba(0,0,0,0.18);
  display: flex;
  flex-direction: column;
  overflow: hidden;
  z-index: 5;
  animation: pocr-fade-in 0.16s ease-out;
}
@keyframes pocr-fade-in {
  from { opacity: 0; transform: translateY(4px); }
  to   { opacity: 1; transform: translateY(0); }
}
.pocr-results-panel.hidden { display: none; }
.pocr-results-close {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  padding: 0;
  background: transparent;
  border: none;
  border-radius: 4px;
  color: var(--pocr-muted);
  cursor: pointer;
  flex-shrink: 0;
}
.pocr-results-close:hover {
  background: var(--pocr-hover);
  color: var(--pocr-fg);
}
.pocr-results-close svg { width: 12px; height: 12px; }
.pocr-results-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 8px 10px;
  border-bottom: 1px solid var(--pocr-border);
  flex-shrink: 0;
}
.pocr-stats {
  display: flex;
  gap: 12px;
  font-size: 10px;
  color: var(--pocr-muted);
  flex-wrap: wrap;
}
.pocr-stat-value {
  color: var(--pocr-fg);
  font-weight: 600;
}
.pocr-results-body {
  padding: 8px 10px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 6px;
  flex: 1;
  min-height: 0;
}
.pocr-results-body::-webkit-scrollbar { width: 4px; }
.pocr-results-body::-webkit-scrollbar-thumb {
  background: rgba(142,142,142,0.4);
  border-radius: 2px;
}
.pocr-fulltext {
  background: var(--pocr-hover);
  border-radius: 4px;
  padding: 6px 8px;
  font-size: 11px;
  color: var(--pocr-fg);
  white-space: pre-wrap;
  word-break: break-word;
  max-height: 120px;
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

/* Custom dropdown (web component) — replaces native <select> */
.pocr-dropdown {
  position: relative;
  display: inline-flex;
  align-items: center;
  height: 18px;
}
.pocr-dropdown-trigger {
  display: inline-flex;
  align-items: center;
  justify-content: space-between;
  gap: 4px;
  height: 18px;
  min-height: 18px;
  padding: 0 6px;
  -webkit-appearance: none;
  appearance: none;
  background: transparent;
  color: var(--pocr-fg);
  border: 1px solid var(--pocr-border);
  border-radius: 4px;
  font-size: 10px;
  line-height: 1;
  cursor: pointer;
  transition: background 0.15s, border-color 0.15s;
  user-select: none;
  box-sizing: border-box;
  white-space: nowrap;
}
.pocr-dropdown-trigger:hover {
  background: var(--pocr-hover);
  border-color: var(--pocr-accent);
}
.pocr-dropdown-trigger:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.pocr-dropdown-trigger svg {
  width: 9px;
  height: 9px;
  transition: transform 0.15s;
}
.pocr-dropdown.open .pocr-dropdown-trigger svg {
  transform: rotate(180deg);
}
.pocr-dropdown-menu {
  position: absolute;
  top: calc(100% + 4px);
  right: 0;
  min-width: 100%;
  background: var(--pocr-card);
  border: 1px solid var(--pocr-border);
  border-radius: 6px;
  box-shadow: 0 8px 24px rgba(0,0,0,0.16);
  padding: 4px;
  display: flex;
  flex-direction: column;
  gap: 1px;
  z-index: 20;
  animation: pocr-fade-in 0.14s ease-out;
}
.pocr-dropdown-option {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 5px 8px;
  border-radius: 4px;
  font-size: 10px;
  color: var(--pocr-fg);
  cursor: pointer;
  white-space: nowrap;
  user-select: none;
}
.pocr-dropdown-option:hover {
  background: var(--pocr-hover);
}
.pocr-dropdown-option.selected {
  background: color-mix(in srgb, var(--pocr-accent) 14%, transparent);
  color: var(--pocr-accent);
  font-weight: 600;
}
.pocr-dropdown-option-size {
  color: var(--pocr-muted);
  font-size: 9px;
  font-variant-numeric: tabular-nums;
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
  'chevron-down': '<polyline points="6 9 12 15 18 9"/>',
  refresh:
    '<path d="M21 12a9 9 0 1 1-9-9c2.5 0 4.9 1 6.7 2.7L21 8M21 3v5h-5"/>',
  eye: '<path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/>',
  'eye-off':
    '<path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24"/><line x1="1" y1="1" x2="23" y2="23"/>',
}

const TIER_OPTIONS: { value: string; label: string; size: string }[] = [
  { value: 'auto', label: 'auto', size: '' },
  { value: 'tiny', label: 'tiny', size: '~6 MB' },
  { value: 'small', label: 'small', size: '~18 MB' },
  { value: 'medium', label: 'medium', size: '~132 MB' },
]

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
// Image compression — prevents HTTP 413 (server body limit 10 MB)
// ============================================================================
//
// Why this exists: the host's `/api/extensions/:id/command` route enforces a
// 10 MB request-body limit (MAX_REQUEST_BODY_SIZE). A raw 8 MB photo becomes
// ~10.7 MB after base64 encoding (×4/3 expansion) + JSON envelope → 413.
//
// PP-OCRv6's detector (DB) resizes its input to ≤960 px internally, so any
// image larger than ~2048 px on its longest side is pure bandwidth + memory
// waste. We downscale to 2048 px max and re-encode as JPEG (stepping quality
// down until under a safe size). OCR accuracy is unaffected — the model never
// sees the extra pixels anyway.

const MAX_UPLOAD_DIMENSION = 2048
// Base64 char budget. Final JSON payload ≈ base64 + ~100 B envelope.
// 6 MB base64 → ~8 MB JSON → comfortably under the 10 MB server limit.
const SAFE_BASE64_CHARS = 6 * 1024 * 1024

async function compressImageForUpload(
  file: File,
): Promise<{ base64: string; mime: string } | { error: string }> {
  return new Promise((resolve) => {
    const url = URL.createObjectURL(file)
    const img = new Image()
    img.onload = () => {
      URL.revokeObjectURL(url)
      const ow = img.naturalWidth
      const oh = img.naturalHeight
      // Cap longest side; preserve aspect ratio
      let tw = ow
      let th = oh
      const longest = Math.max(ow, oh)
      if (longest > MAX_UPLOAD_DIMENSION) {
        const scale = MAX_UPLOAD_DIMENSION / longest
        tw = Math.round(ow * scale)
        th = Math.round(oh * scale)
      }
      const canvas = document.createElement('canvas')
      canvas.width = tw
      canvas.height = th
      const ctx = canvas.getContext('2d')
      if (!ctx) {
        resolve({ error: 'Canvas 2D context unavailable' })
        return
      }
      // JPEG has no alpha channel. Transparent PNG pixels default to black
      // on canvas → dark-on-transparent text becomes invisible. Fill white
      // (paper background) first so transparency flattens to white.
      ctx.fillStyle = '#ffffff'
      ctx.fillRect(0, 0, tw, th)
      ctx.drawImage(img, 0, 0, tw, th)
      // Step JPEG quality down until the base64 fits the safe budget.
      // 0.9 is visually lossless for OCR; lower rungs are fallbacks for
      // pathological inputs (e.g. 8000×8000 screenshots).
      const qualities = [0.9, 0.75, 0.6, 0.45]
      let base64 = ''
      for (const q of qualities) {
        const dataUrl = canvas.toDataURL('image/jpeg', q)
        base64 = dataUrl.slice(dataUrl.indexOf(',') + 1)
        if (base64.length <= SAFE_BASE64_CHARS) break
      }
      resolve({ base64, mime: 'image/jpeg' })
    }
    img.onerror = () => {
      URL.revokeObjectURL(url)
      resolve({ error: 'Failed to decode image' })
    }
    img.src = url
  })
}

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
    const [drawBoxes, setDrawBoxes] = useState<boolean>(
      config.drawBoxes !== false,
    ) // default true, toggleable at runtime
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
    const [tierMenuOpen, setTierMenuOpen] = useState(false)
    const [resultsOpen, setResultsOpen] = useState(true)
    const [compressing, setCompressing] = useState(false)

    const canvasRef = useRef<HTMLCanvasElement>(null)
    const fileInputRef = useRef<HTMLInputElement>(null)
    const dropdownRef = useRef<HTMLDivElement>(null)

    // Close tier dropdown on outside click
    useEffect(() => {
      if (!tierMenuOpen) return
      const handler = (e: MouseEvent) => {
        if (
          dropdownRef.current &&
          !dropdownRef.current.contains(e.target as Node)
        ) {
          setTierMenuOpen(false)
        }
      }
      document.addEventListener('mousedown', handler)
      return () => document.removeEventListener('mousedown', handler)
    }, [tierMenuOpen])

    // -------------------------------------------------------------
    // Image loading (file picker + drag/drop + paste)
    // -------------------------------------------------------------

    const acceptFile = useCallback(async (file: File) => {
      if (!file.type.startsWith('image/')) {
        setError('Please select an image file (PNG / JPEG / WebP)')
        return
      }
      // Hard cap on the *source* file to bound decode cost. Compression then
      // shrinks the upload well under the 10 MB server body limit.
      if (file.size > 30 * 1024 * 1024) {
        setError('Image too large (max 30 MB before compression)')
        return
      }
      setError(null)
      setCompressing(true)
      const out = await compressImageForUpload(file)
      setCompressing(false)
      if ('error' in out) {
        setError(out.error)
        return
      }
      setImage(out.base64)
      setImageMime(out.mime)
      setResult(null)
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
              <div
                ref={dropdownRef}
                className={`pocr-dropdown ${tierMenuOpen ? 'open' : ''}`}
              >
                <button
                  type="button"
                  className="pocr-dropdown-trigger"
                  onClick={() => setTierMenuOpen((v) => !v)}
                  disabled={tierSwitching}
                  title="Switch model tier (tiny ships in .nep; small/medium lazy-download)"
                >
                  <span>{tier}</span>
                  <Icon name="chevron-down" />
                </button>
                {tierMenuOpen && (
                  <div className="pocr-dropdown-menu" role="listbox">
                    {TIER_OPTIONS.map((opt) => (
                      <div
                        key={opt.value}
                        role="option"
                        aria-selected={tier === opt.value}
                        className={`pocr-dropdown-option ${
                          tier === opt.value ? 'selected' : ''
                        }`}
                        onClick={() => {
                          setTierMenuOpen(false)
                          if (opt.value !== tier) handleSwitchTier(opt.value)
                        }}
                      >
                        <span>{opt.label}</span>
                        <span className="pocr-dropdown-option-size">
                          {opt.size}
                        </span>
                      </div>
                    ))}
                  </div>
                )}
              </div>
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
                  Supports JPG, PNG, WebP — auto-compressed on upload
                </div>
              </div>
            ) : (
              <>
                <div className="pocr-preview-wrapper">
                  <canvas ref={canvasRef} className="pocr-canvas" />
                  {(loading || compressing) && (
                    <div className="pocr-loading-overlay">
                      <div className="pocr-spinner" />
                      <span>{compressing ? 'Compressing…' : 'Recognizing…'}</span>
                    </div>
                  )}

                  {/* Show/hide bounding boxes toggle (overlay, bottom-left) */}
                  <button
                    onClick={() => setDrawBoxes((v) => !v)}
                    className={
                      'pocr-box-toggle' + (drawBoxes ? ' is-active' : '')
                    }
                    type="button"
                    title={
                      drawBoxes ? 'Hide bounding boxes' : 'Show bounding boxes'
                    }
                    aria-label={
                      drawBoxes ? 'Hide bounding boxes' : 'Show bounding boxes'
                    }
                    aria-pressed={drawBoxes}
                  >
                    <Icon name={drawBoxes ? 'eye' : 'eye-off'} />
                  </button>

                  {result && !error && (
                    <>
                      <button
                        type="button"
                        className={`pocr-results-toggle ${
                          resultsOpen ? '' : 'collapsed'
                        }`}
                        onClick={() => setResultsOpen((v) => !v)}
                        title={resultsOpen ? 'Collapse results' : 'Expand results'}
                      >
                        <Icon name="chevron-down" />
                        <span>
                          {result.total_blocks > 0
                            ? `${result.total_blocks} block${
                                result.total_blocks === 1 ? '' : 's'
                              }`
                            : 'No text'}
                        </span>
                        <span className="pocr-toggle-count">
                          {(result.avg_confidence * 100).toFixed(0)}%
                        </span>
                      </button>
                      {resultsOpen && (
                        <div className="pocr-results-panel">
                          <div className="pocr-results-header">
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
                            <button
                              type="button"
                              className="pocr-results-close"
                              onClick={() => setResultsOpen(false)}
                              title="Collapse"
                            >
                              <Icon name="x" />
                            </button>
                          </div>
                          <div className="pocr-results-body">
                            {result.full_text ? (
                              <pre className="pocr-fulltext">
                                {result.full_text}
                              </pre>
                            ) : (
                              <div className="pocr-empty">
                                No text recognized
                              </div>
                            )}

                            {result.text_blocks.length > 0 && (
                              <div className="pocr-blocks-list">
                                {result.text_blocks
                                  .filter((b) => b.text)
                                  .slice(0, 50)
                                  .map((blk, i) => (
                                    <div
                                      key={i}
                                      className="pocr-block-item"
                                    >
                                      <span
                                        className="pocr-block-text"
                                        title={blk.text}
                                      >
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
                        </div>
                      )}
                    </>
                  )}
                </div>

                {error && <div className="pocr-error">{error}</div>}

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
