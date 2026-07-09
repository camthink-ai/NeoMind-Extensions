/**
 * PaddleOCR-VL — Tester card with mode switching + interactive bbox.
 *
 * Modes:
 *   - Text  → /recognize    (text_blocks with bbox + full_text)
 *   - Table → /recognize_table (rendered HTML)
 *   - Keys  → /extract_keys   (key-value fields)
 *
 * Inline settings (per-session, not persisted):
 *   language, auto-rotate, de-warp
 */

import { forwardRef, useState, useCallback, useRef, useMemo, useEffect } from 'react'

export interface ExtensionComponentProps {
  title?: string
  className?: string
}

interface TextBlock {
  text: string
  confidence: number
  bbox: { x: number; y: number; width: number; height: number }
}

interface OcrTextResult {
  text_blocks: TextBlock[]
  full_text: string
  processing_time_ms: number
}

interface TableResult {
  html: string
  processing_time_ms: number
}

interface KieResult {
  fields: Record<string, string>
  processing_time_ms: number
}

type Mode = 'text' | 'table' | 'keys'

const EXTENSION_ID = 'paddle-ocr-vl'

const LANGUAGES = [
  { value: 'ch', label: '中文' },
  { value: 'en', label: 'English' },
  { value: 'japan', label: '日本語' },
  { value: 'korean', label: '한국어' },
  { value: 'teletext', label: 'Teletext' },
  { value: 'multilingual', label: 'Multilingual' },
]

const MODE_TABS: { id: Mode; label: string; icon: string }[] = [
  { id: 'text', label: 'Text', icon: 'M4 7h16M4 12h10M4 17h16' },
  { id: 'table', label: 'Table', icon: 'M3 3h18v18H3zM3 9h18M3 15h18M9 3v18M15 3v18' },
  { id: 'keys', label: 'Keys', icon: 'M4 6h16M4 12h10M4 18h7' },
]

const getApiBase = (): string => {
  if (typeof window !== 'undefined' && (window as any).__TAURI__) {
    return 'http://localhost:9375/api'
  }
  return '/api'
}

async function runCommand<T>(
  command: string,
  args: Record<string, any>,
): Promise<{ success: boolean; data?: T; error?: string }> {
  try {
    const headers: Record<string, string> = { 'Content-Type': 'application/json' }
    const token =
      localStorage.getItem('neomind_token') ||
      sessionStorage.getItem('neomind_token_session')
    if (token) headers['Authorization'] = `Bearer ${token}`

    const r = await fetch(
      `${getApiBase()}/extensions/${EXTENSION_ID}/command`,
      { method: 'POST', headers, body: JSON.stringify({ command, args }) },
    )
    if (!r.ok) {
      const txt = await r.text().catch(() => '')
      return { success: false, error: `HTTP ${r.status}: ${txt.slice(0, 120)}` }
    }
    const body = await r.json()
    // Normalize: server may return error as object { code, message }
    if (body && typeof body === 'object' && body.success === false) {
      const e = body.error
      const errorStr = typeof e === 'string' ? e : (e && typeof e === 'object' ? e.message || e.code : undefined)
      return { success: false, error: errorStr || 'Command failed' }
    }
    return body
  } catch (e) {
    return { success: false, error: e instanceof Error ? e.message : 'Network error' }
  }
}

// Image compression — prevents HTTP 413 (server body limit 10 MB).
// Downscale to 2048px max + re-encode JPEG, stepping quality until ≤6 MB base64.
// Fill white before drawing so transparent PNG → white (not black) background.
const COMPRESS_MAX_DIM = 2048
const COMPRESS_SAFE_CHARS = 6 * 1024 * 1024

function compressImageForUpload(file: File): Promise<{ base64: string; dataUrl: string } | { error: string }> {
  return new Promise((resolve) => {
    const url = URL.createObjectURL(file)
    const img = new Image()
    img.onload = () => {
      URL.revokeObjectURL(url)
      let tw = img.naturalWidth
      let th = img.naturalHeight
      const longest = Math.max(tw, th)
      if (longest > COMPRESS_MAX_DIM) {
        const scale = COMPRESS_MAX_DIM / longest
        tw = Math.round(tw * scale)
        th = Math.round(th * scale)
      }
      const canvas = document.createElement('canvas')
      canvas.width = tw
      canvas.height = th
      const ctx = canvas.getContext('2d')
      if (!ctx) { resolve({ error: 'Canvas unavailable' }); return }
      ctx.fillStyle = '#ffffff'
      ctx.fillRect(0, 0, tw, th)
      ctx.drawImage(img, 0, 0, tw, th)
      let dataUrl = ''
      for (const q of [0.9, 0.75, 0.6, 0.45]) {
        dataUrl = canvas.toDataURL('image/jpeg', q)
        if (dataUrl.length - 23 <= COMPRESS_SAFE_CHARS) break
      }
      resolve({ base64: dataUrl.split(',')[1] || '', dataUrl })
    }
    img.onerror = () => { URL.revokeObjectURL(url); resolve({ error: 'Failed to decode' }) }
    img.src = url
  })
}

const PaddleOcrCard = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function PaddleOcrCard(props, ref) {
    const { title = 'PaddleOCR-VL', className = '' } = props

    // --- image state ---
    const [imageBase64, setImageBase64] = useState('')
    const [previewUrl, setPreviewUrl] = useState('')
    const [dims, setDims] = useState<{ w: number; h: number }>({ w: 0, h: 0 })

    // --- per-request options ---
    const [mode, setMode] = useState<Mode>('text')
    const [language, setLanguage] = useState('ch')
    const [autoRotate, setAutoRotate] = useState(false)
    const [dewarp, setDewarp] = useState(false)
    const [showSettings, setShowSettings] = useState(false)

    // --- results (kept separate per mode so switching back preserves them) ---
    const [textResult, setTextResult] = useState<OcrTextResult | null>(null)
    const [tableResult, setTableResult] = useState<TableResult | null>(null)
    const [kieResult, setKieResult] = useState<KieResult | null>(null)
    const [textView, setTextView] = useState<'blocks' | 'plain'>('blocks')

    const [loading, setLoading] = useState(false)
    const [error, setError] = useState<string | null>(null)
    const [dragOver, setDragOver] = useState(false)
    const [activeBlock, setActiveBlock] = useState<number | null>(null)
    const [copied, setCopied] = useState(false)

    const fileRef = useRef<HTMLInputElement>(null)
    const settingsRef = useRef<HTMLDivElement>(null)

    // close settings popover on outside click
    useEffect(() => {
      if (!showSettings) return
      const onClick = (e: MouseEvent) => {
        if (settingsRef.current && !settingsRef.current.contains(e.target as Node)) {
          setShowSettings(false)
        }
      }
      document.addEventListener('mousedown', onClick)
      return () => document.removeEventListener('mousedown', onClick)
    }, [showSettings])

    const onFile = useCallback(async (file: File) => {
      if (!file.type.startsWith('image/')) {
        setError(`Not an image: ${file.type}`)
        return
      }
      if (file.size > 30 * 1024 * 1024) {
        setError('Image too large (max 30 MB before compression)')
        return
      }
      const out = await compressImageForUpload(file)
      if ('error' in out) { setError(out.error); return }
      setPreviewUrl(out.dataUrl)
      setImageBase64(out.base64)
      setTextResult(null)
      setTableResult(null)
      setKieResult(null)
      setError(null)
      setActiveBlock(null)
      const img = new Image()
      img.onload = () => setDims({ w: img.naturalWidth, h: img.naturalHeight })
      img.src = out.dataUrl
    }, [])

    const run = useCallback(async () => {
      if (!imageBase64) return
      setLoading(true)
      setError(null)
      setActiveBlock(null)
      try {
        if (mode === 'text') {
          const r = await runCommand<OcrTextResult>('recognize', {
            image_base64: imageBase64,
            image_width: dims.w || undefined,
            image_height: dims.h || undefined,
            language,
            use_doc_orientation_classify: autoRotate,
            use_doc_unwarping: dewarp,
          })
          if (!r.success || !r.data) throw new Error(r.error || 'OCR failed')
          setTextResult(r.data)
        } else if (mode === 'table') {
          const r = await runCommand<TableResult>('recognize_table', {
            image_base64: imageBase64,
          })
          if (!r.success || !r.data) throw new Error(r.error || 'Table recognition failed')
          setTableResult(r.data)
        } else {
          const r = await runCommand<KieResult>('extract_keys', {
            image_base64: imageBase64,
          })
          if (!r.success || !r.data) throw new Error(r.error || 'KIE failed')
          setKieResult(r.data)
        }
      } catch (e: any) {
        setError(e?.message || String(e))
      } finally {
        setLoading(false)
      }
    }, [imageBase64, dims, mode, language, autoRotate, dewarp])

    const clear = useCallback(() => {
      setPreviewUrl('')
      setImageBase64('')
      setTextResult(null)
      setTableResult(null)
      setKieResult(null)
      setError(null)
      setActiveBlock(null)
    }, [])

    const copyText = useCallback(async () => {
      const txt = textResult?.full_text || ''
      if (!txt) return
      try {
        await navigator.clipboard.writeText(txt)
        setCopied(true)
        setTimeout(() => setCopied(false), 1500)
      } catch {}
    }, [textResult])

    const processingTime = useMemo(() => {
      if (mode === 'text' && textResult) return textResult.processing_time_ms
      if (mode === 'table' && tableResult) return tableResult.processing_time_ms
      if (mode === 'keys' && kieResult) return kieResult.processing_time_ms
      return null
    }, [mode, textResult, tableResult, kieResult])

    const blockCount = textResult?.text_blocks?.length || 0
    const hasResult = !!(textResult || tableResult || kieResult)

    return (
      <div ref={ref} className={`pocr-card ${className}`}>
        <style>{STYLES}</style>

        {/* Header: title + mode tabs + settings */}
        <div className="pocr-header">
          <span className="pocr-title">{title}</span>
          <div className="pocr-mode-tabs">
            {MODE_TABS.map((t) => (
              <button
                key={t.id}
                className={`pocr-tab ${mode === t.id ? 'active' : ''}`}
                onClick={() => setMode(t.id)}
                title={t.label}
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <path d={t.icon} />
                </svg>
              </button>
            ))}
          </div>
          <div className="pocr-header-right" ref={settingsRef}>
            {processingTime != null && (
              <span className="pocr-meta">
                {mode === 'text' ? `${blockCount} blocks` : mode === 'table' ? 'table' : 'fields'}
                {' · '}
                {(processingTime / 1000).toFixed(1)}s
              </span>
            )}
            <button
              className="pocr-icon-btn"
              onClick={() => setShowSettings((v) => !v)}
              title="Settings"
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <circle cx="12" cy="12" r="3" />
                <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
              </svg>
            </button>
            {showSettings && (
              <div className="pocr-popover">
                <div className="pocr-popover-row">
                  <label className="pocr-popover-label">Language</label>
                  <div className="pocr-select-wrap">
                    <select
                      className="pocr-select"
                      value={language}
                      onChange={(e) => setLanguage(e.target.value)}
                    >
                      {LANGUAGES.map((l) => (
                        <option key={l.value} value={l.value}>{l.label}</option>
                      ))}
                    </select>
                    <svg className="pocr-select-chevron" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                      <polyline points="6 9 12 15 18 9" />
                    </svg>
                  </div>
                </div>
                <div className="pocr-popover-row">
                  <label className="pocr-popover-label">Auto-rotate</label>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={autoRotate}
                    className={`pocr-switch ${autoRotate ? 'on' : ''}`}
                    onClick={() => setAutoRotate((v) => !v)}
                  >
                    <span className="pocr-switch-thumb" />
                  </button>
                </div>
                <div className="pocr-popover-row">
                  <label className="pocr-popover-label">De-warp</label>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={dewarp}
                    className={`pocr-switch ${dewarp ? 'on' : ''}`}
                    onClick={() => setDewarp((v) => !v)}
                  >
                    <span className="pocr-switch-thumb" />
                  </button>
                </div>
              </div>
            )}
          </div>
        </div>

        {/* Body: image (top) + result (bottom), both flex */}
        {!previewUrl ? (
          <div
            className={`pocr-upload ${dragOver ? 'pocr-drag' : ''}`}
            onDragOver={(e) => { e.preventDefault(); setDragOver(true) }}
            onDragLeave={() => setDragOver(false)}
            onDrop={(e) => {
              e.preventDefault()
              setDragOver(false)
              const f = e.dataTransfer.files?.[0]
              if (f) onFile(f)
            }}
            onClick={() => fileRef.current?.click()}
          >
            <div className="pocr-upload-icon">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M17 8l-5-5-5 5M12 3v12" />
              </svg>
            </div>
            <div className="pocr-upload-text">Drop image or click to upload</div>
            <div className="pocr-upload-hint">PNG / JPG / WEBP</div>
            <input
              ref={fileRef}
              type="file"
              accept="image/*"
              className="pocr-file-input"
              onChange={(e) => { const f = e.target.files?.[0]; if (f) onFile(f) }}
            />
          </div>
        ) : (
          <>
            {/* Image panel */}
            <div className="pocr-image-panel">
              <div className="pocr-preview-image-wrap">
                <img src={previewUrl} alt="" className="pocr-image" />
                {textResult?.text_blocks?.length ? (
                  <div className="pocr-overlay">
                    {textResult.text_blocks.map((b, i) => (
                      <div
                        key={i}
                        className={`pocr-box ${activeBlock === i ? 'pocr-box-active' : ''}`}
                        style={{
                          left: `${b.bbox.x * 100}%`,
                          top: `${b.bbox.y * 100}%`,
                          width: `${b.bbox.width * 100}%`,
                          height: `${b.bbox.height * 100}%`,
                        }}
                      />
                    ))}
                  </div>
                ) : null}
                <button className="pocr-clear" onClick={clear} title="Clear">✕</button>
              </div>
            </div>

            {/* Result panel */}
            <div className="pocr-result-panel">
              {error && <div className="pocr-error">{error}</div>}

              {!loading && !error && !hasResult && (
                <div className="pocr-empty">
                  Click <strong>Run</strong> to recognize {mode === 'text' ? 'text' : mode === 'table' ? 'tables' : 'key fields'}.
                </div>
              )}

              {/* TEXT mode */}
              {mode === 'text' && textResult && (
                <>
                  <div className="pocr-result-toolbar">
                    <div className="pocr-seg">
                      <button className={textView === 'blocks' ? 'active' : ''} onClick={() => setTextView('blocks')}>
                        Blocks ({blockCount})
                      </button>
                      <button className={textView === 'plain' ? 'active' : ''} onClick={() => setTextView('plain')}>
                        Plain text
                      </button>
                    </div>
                    <button className="pocr-copy-btn" onClick={copyText} title="Copy text">
                      {copied ? '✓ Copied' : 'Copy'}
                    </button>
                  </div>
                  {textView === 'blocks' ? (
                    <div className="pocr-block-list">
                      {textResult.text_blocks.length === 0 ? (
                        <div className="pocr-empty">No text blocks detected.</div>
                      ) : textResult.text_blocks.map((b, i) => (
                        <div
                          key={i}
                          className={`pocr-block-item ${activeBlock === i ? 'active' : ''}`}
                          onMouseEnter={() => setActiveBlock(i)}
                          onMouseLeave={() => setActiveBlock(null)}
                          onClick={() => setActiveBlock(activeBlock === i ? null : i)}
                        >
                          <span className="pocr-block-idx">{i + 1}</span>
                          <span className="pocr-block-text">{b.text}</span>
                        </div>
                      ))}
                    </div>
                  ) : (
                    <pre className="pocr-plain-text">{textResult.full_text || '(empty)'}</pre>
                  )}
                </>
              )}

              {/* TABLE mode */}
              {mode === 'table' && tableResult && (
                tableResult.html ? (
                  <div className="pocr-table-wrap" dangerouslySetInnerHTML={{ __html: tableResult.html }} />
                ) : (
                  <div className="pocr-empty">No table detected in this image.</div>
                )
              )}

              {/* KEYS mode */}
              {mode === 'keys' && kieResult && (
                <div className="pocr-kv-list">
                  {Object.keys(kieResult.fields).length === 0 ? (
                    <div className="pocr-empty">No fields extracted.</div>
                  ) : Object.entries(kieResult.fields).map(([k, v]) => (
                    <div key={k} className="pocr-kv-item">
                      <span className="pocr-kv-key">{k}</span>
                      <span className="pocr-kv-val">{String(v)}</span>
                    </div>
                  ))}
                </div>
              )}

              {loading && (
                <div className="pocr-loading">
                  <div className="pocr-spinner" />
                  <span>Recognizing…</span>
                </div>
              )}
            </div>

            {/* Run button */}
            <button className="pocr-run" onClick={run} disabled={loading || !imageBase64}>
              {loading ? 'Running…' : `Run ${mode === 'text' ? 'OCR' : mode === 'table' ? 'Table' : 'KIE'}`}
            </button>
          </>
        )}
      </div>
    )
  },
)

export { PaddleOcrCard }
export default PaddleOcrCard

const STYLES = `
.pocr-card {
  --pocr-fg: var(--foreground, hsl(240 10% 10%));
  --pocr-muted: var(--muted-foreground, hsl(240 5% 40%));
  --pocr-card-bg: var(--card, rgba(255,255,255,0.6));
  --pocr-border: var(--border, rgba(0,0,0,0.08));
  --pocr-input: var(--input, rgba(0,0,0,0.10));
  --pocr-accent: var(--primary, hsl(221 83% 53%));
  --pocr-on-accent: var(--primary-foreground, #ffffff);
  --pocr-error: var(--color-error, hsl(0 72% 51%));
  --pocr-hover: var(--accent-muted-bg, rgba(0,0,0,0.04));
  --pocr-radius: var(--radius-lg, 12px);

  display: flex;
  flex-direction: column;
  height: 100%;
  width: 100%;
  padding: 10px;
  gap: 8px;
  background: var(--pocr-card-bg);
  border: 1px solid var(--pocr-border);
  border-radius: var(--pocr-radius);
  box-sizing: border-box;
  font-size: 12px;
  color: var(--pocr-fg);
}

.dark .pocr-card {
  --pocr-on-accent: var(--primary-foreground, #17172a);
}

/* ---------- header ---------- */
.pocr-header {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}
.pocr-title { font-weight: 600; font-size: 13px; }
.pocr-mode-tabs {
  display: flex;
  gap: 2px;
  padding: 2px;
  background: var(--pocr-hover);
  border-radius: 7px;
  flex-shrink: 0;
}
.pocr-tab {
  width: 26px; height: 24px;
  display: flex; align-items: center; justify-content: center;
  background: transparent;
  border: none;
  border-radius: 5px;
  color: var(--pocr-muted);
  cursor: pointer;
  transition: all 0.15s ease;
}
.pocr-tab svg { width: 14px; height: 14px; }
.pocr-tab:hover { color: var(--pocr-fg); }
.pocr-tab.active {
  background: var(--pocr-card-bg);
  color: var(--pocr-accent);
  box-shadow: 0 1px 2px rgba(0,0,0,0.06);
}
.pocr-header-right {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: 6px;
  position: relative;
}
.pocr-meta { font-size: 10px; color: var(--pocr-muted); }
.pocr-icon-btn {
  width: 24px; height: 24px;
  display: flex; align-items: center; justify-content: center;
  background: transparent;
  border: none;
  border-radius: 5px;
  color: var(--pocr-muted);
  cursor: pointer;
}
.pocr-icon-btn:hover { background: var(--pocr-hover); color: var(--pocr-fg); }
.pocr-icon-btn svg { width: 15px; height: 15px; }

.pocr-popover {
  position: absolute;
  top: 30px; right: 0;
  z-index: 10;
  min-width: 200px;
  padding: 10px;
  background: var(--pocr-card-bg);
  backdrop-filter: blur(12px);
  border: 1px solid var(--pocr-border);
  border-radius: var(--radius-md, 8px);
  box-shadow: var(--shadow-lg, 0 6px 20px rgba(0,0,0,0.12));
  display: flex;
  flex-direction: column;
  gap: 10px;
  animation: pocr-popover-in var(--duration-fast, 150ms) var(--ease-out, cubic-bezier(0.16, 1, 0.3, 1));
}
@keyframes pocr-popover-in {
  from { opacity: 0; transform: translateY(-4px); }
  to { opacity: 1; transform: translateY(0); }
}
.pocr-popover-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  font-size: 11px;
}
.pocr-popover-label {
  color: var(--pocr-fg);
  font-weight: 500;
  flex-shrink: 0;
}

/* ---- NeoMind-styled select (per design guide §2.5) ---- */
.pocr-select-wrap {
  position: relative;
  display: inline-flex;
  align-items: center;
}
.pocr-select {
  appearance: none;
  -webkit-appearance: none;
  -moz-appearance: none;
  font-size: 11px;
  padding: 5px 26px 5px 8px;
  border: 1px solid var(--input, var(--pocr-border));
  border-radius: var(--radius-sm, 6px);
  background: var(--pocr-card-bg);
  color: var(--pocr-fg);
  cursor: pointer;
  transition: border-color var(--duration-fast, 150ms) var(--ease-out, cubic-bezier(0.16, 1, 0.3, 1));
  outline: none;
}
.pocr-select:hover { border-color: var(--pocr-accent); }
.pocr-select:focus {
  border-color: var(--pocr-accent);
  box-shadow: 0 0 0 2px color-mix(in oklch, var(--pocr-accent) 18%, transparent);
}
.pocr-select option {
  background: var(--pocr-card-bg);
  color: var(--pocr-fg);
}
.pocr-select-chevron {
  position: absolute;
  right: 6px;
  width: 12px;
  height: 12px;
  color: var(--pocr-muted);
  pointer-events: none;
}

/* ---- Custom toggle switch (iOS-style, NeoMind-styled) ---- */
.pocr-switch {
  position: relative;
  width: 30px;
  height: 17px;
  border: none;
  border-radius: var(--radius-full, 9999px);
  background: var(--input, var(--pocr-border));
  cursor: pointer;
  padding: 0;
  flex-shrink: 0;
  transition: background var(--duration-fast, 150ms) var(--ease-out, cubic-bezier(0.16, 1, 0.3, 1));
}
.pocr-switch.on {
  background: var(--pocr-accent);
}
.pocr-switch-thumb {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 13px;
  height: 13px;
  border-radius: 50%;
  background: #fff;
  box-shadow: 0 1px 2px rgba(0,0,0,0.2);
  transition: transform var(--duration-fast, 150ms) var(--ease-out, cubic-bezier(0.16, 1, 0.3, 1));
}
.pocr-switch.on .pocr-switch-thumb {
  transform: translateX(13px);
  background: var(--pocr-on-accent);
}

/* ---------- upload ---------- */
.pocr-upload {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  border: 2px dashed var(--pocr-border);
  border-radius: var(--pocr-radius);
  background: var(--pocr-hover);
  cursor: pointer;
  transition: all 0.2s ease;
}
.pocr-upload:hover, .pocr-drag {
  border-color: var(--pocr-accent);
  background: color-mix(in oklch, var(--pocr-accent) 6%, transparent);
}
.pocr-upload-icon {
  width: 34px; height: 34px;
  border-radius: 50%;
  background: color-mix(in oklch, var(--pocr-accent) 12%, transparent);
  color: var(--pocr-accent);
  display: flex; align-items: center; justify-content: center;
}
.pocr-upload-icon svg { width: 16px; height: 16px; }
.pocr-upload-text { font-size: 12px; color: var(--pocr-muted); }
.pocr-upload-hint { font-size: 10px; color: var(--pocr-muted); opacity: 0.7; }
.pocr-file-input { display: none; }

/* ---------- image panel ---------- */
.pocr-image-panel {
  flex: 1 1 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.pocr-preview-image-wrap {
  position: relative;
  flex: 1;
  min-height: 0;
  border-radius: 8px;
  overflow: hidden;
  border: 1px solid var(--pocr-border);
  background: var(--pocr-hover);
  display: flex;
  align-items: center;
  justify-content: center;
}
.pocr-image {
  display: block;
  max-width: 100%;
  max-height: 100%;
  object-fit: contain;
}
.pocr-overlay {
  position: absolute;
  inset: 0;
  pointer-events: none;
}
.pocr-box {
  position: absolute;
  border: 1.5px solid color-mix(in oklch, var(--pocr-accent) 70%, transparent);
  background: color-mix(in oklch, var(--pocr-accent) 12%, transparent);
  transition: all 0.15s ease;
}
.pocr-box-active {
  border-color: var(--pocr-accent);
  border-width: 2px;
  background: color-mix(in oklch, var(--pocr-accent) 28%, transparent);
}
.pocr-clear {
  position: absolute;
  top: 6px; right: 6px;
  width: 22px; height: 22px;
  display: flex; align-items: center; justify-content: center;
  background: rgba(0, 0, 0, 0.55);
  color: #fff;
  border: none;
  border-radius: 50%;
  font-size: 12px;
  cursor: pointer;
  opacity: 0.7;
  transition: opacity 0.15s ease;
  z-index: 2;
}
.pocr-clear:hover { opacity: 1; background: rgba(0, 0, 0, 0.8); }

/* ---------- result panel ---------- */
.pocr-result-panel {
  flex: 1 1 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
  position: relative;
}
.pocr-empty {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  text-align: center;
  color: var(--pocr-muted);
  font-size: 11px;
  padding: 12px;
}
.pocr-error {
  padding: 7px 9px;
  background: color-mix(in oklch, var(--pocr-error) 8%, transparent);
  color: var(--pocr-error);
  border-left: 3px solid var(--pocr-error);
  border-radius: 4px;
  font-size: 11px;
  word-break: break-word;
  flex-shrink: 0;
}

.pocr-result-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
  flex-shrink: 0;
}
.pocr-seg {
  display: flex;
  gap: 1px;
  padding: 2px;
  background: var(--pocr-hover);
  border-radius: 6px;
}
.pocr-seg button {
  padding: 3px 8px;
  font-size: 10px;
  background: transparent;
  border: none;
  border-radius: 4px;
  color: var(--pocr-muted);
  cursor: pointer;
}
.pocr-seg button.active {
  background: var(--pocr-card-bg);
  color: var(--pocr-fg);
  box-shadow: 0 1px 2px rgba(0,0,0,0.06);
}
.pocr-copy-btn {
  padding: 3px 8px;
  font-size: 10px;
  background: transparent;
  border: 1px solid var(--pocr-border);
  border-radius: 4px;
  color: var(--pocr-muted);
  cursor: pointer;
}
.pocr-copy-btn:hover { background: var(--pocr-hover); color: var(--pocr-fg); }

.pocr-block-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding-right: 2px;
}
.pocr-block-item {
  display: flex;
  gap: 6px;
  padding: 5px 7px;
  border-radius: 5px;
  background: var(--pocr-hover);
  cursor: pointer;
  transition: background 0.12s ease;
  align-items: flex-start;
}
.pocr-block-item:hover, .pocr-block-item.active {
  background: color-mix(in oklch, var(--pocr-accent) 14%, transparent);
}
.pocr-block-idx {
  flex-shrink: 0;
  font-size: 9px;
  font-weight: 600;
  color: var(--pocr-accent);
  background: color-mix(in oklch, var(--pocr-accent) 14%, transparent);
  border-radius: 3px;
  padding: 1px 4px;
  min-width: 18px;
  text-align: center;
  margin-top: 1px;
}
.pocr-block-text {
  font-size: 11px;
  line-height: 1.45;
  word-break: break-word;
  white-space: pre-wrap;
}

.pocr-plain-text {
  margin: 0;
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 8px;
  background: var(--pocr-hover);
  border-radius: 6px;
  white-space: pre-wrap;
  word-break: break-word;
  font-family: 'SF Mono', Monaco, monospace;
  font-size: 10px;
  line-height: 1.5;
}

.pocr-table-wrap {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 6px;
  background: var(--pocr-hover);
  border-radius: 6px;
}
.pocr-table-wrap table {
  border-collapse: collapse;
  width: 100%;
  font-size: 10px;
}
.pocr-table-wrap th, .pocr-table-wrap td {
  border: 1px solid var(--pocr-border);
  padding: 4px 6px;
  text-align: left;
}
.pocr-table-wrap th {
  background: color-mix(in oklch, var(--pocr-accent) 10%, transparent);
  font-weight: 600;
}

.pocr-kv-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.pocr-kv-item {
  display: flex;
  flex-direction: column;
  gap: 1px;
  padding: 5px 8px;
  background: var(--pocr-hover);
  border-radius: 5px;
  border-left: 2px solid var(--pocr-accent);
}
.pocr-kv-key {
  font-size: 9px;
  color: var(--pocr-muted);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}
.pocr-kv-val {
  font-size: 11px;
  word-break: break-word;
  white-space: pre-wrap;
}

.pocr-loading {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  background: color-mix(in oklch, var(--pocr-card-bg) 80%, transparent);
  backdrop-filter: blur(2px);
  font-size: 11px;
  color: var(--pocr-muted);
  z-index: 5;
}
.pocr-spinner {
  width: 14px; height: 14px;
  border: 2px solid var(--pocr-border);
  border-top-color: var(--pocr-accent);
  border-radius: 50%;
  animation: pocr-spin 0.7s linear infinite;
}
@keyframes pocr-spin { to { transform: rotate(360deg); } }

/* ---------- run button ---------- */
.pocr-run {
  padding: 8px 14px;
  background: var(--pocr-accent);
  color: var(--pocr-on-accent);
  border: none;
  border-radius: 7px;
  font-size: 11px;
  font-weight: 600;
  cursor: pointer;
  flex-shrink: 0;
  transition: opacity 0.15s ease;
}
.pocr-run:hover:not(:disabled) { opacity: 0.9; }
.pocr-run:disabled { opacity: 0.5; cursor: not-allowed; }
`
