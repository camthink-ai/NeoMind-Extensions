import { forwardRef, useState, useEffect, useCallback, useRef } from 'react'

export interface ExtensionComponentProps {
  title?: string
  dataSource?: {
    type: string
    deviceId?: string
    device_id?: string
    extensionId?: string
    command?: string
    config?: Record<string, any>
    [key: string]: any
  }
  className?: string
  config?: Record<string, any>
}

interface BoundingBox { x1: number; y1: number; x2: number; y2: number }
interface Point { x: number; y: number }
interface InferenceResult {
  success: boolean
  answer: string
  boxes: BoundingBox[]
  points: Point[]
  inference_time_ms: number
  error?: string
}

const DETECTION_COLORS = ['#ff6b6b', '#ffd93d', '#6bcb77', '#4d96ff', '#9b59b6', '#e84393']

const getApiHeaders = (): Record<string, string> => {
  const token = localStorage.getItem('neomind_token') || sessionStorage.getItem('neomind_token_session')
  const headers: Record<string, string> = { 'Content-Type': 'application/json' }
  if (token) headers['Authorization'] = `Bearer ${token}`
  return headers
}

const getApiBase = (): string => {
  if (typeof window !== 'undefined' && (window as any).__TAURI__) return 'http://localhost:9375/api'
  return '/api'
}

async function executeExtensionCommand<T>(
  extensionId: string, command: string, args: Record<string, any>
): Promise<{ success: boolean; data?: T; error?: string }> {
  const controller = new AbortController()
  const tid = setTimeout(() => controller.abort(), 180_000)
  try {
    const res = await fetch(`${getApiBase()}/extensions/${extensionId}/command`, {
      method: 'POST', headers: getApiHeaders(),
      body: JSON.stringify({ command, args }), signal: controller.signal,
    })
    if (!res.ok) return { success: false, error: res.status === 401 ? 'Auth required' : `HTTP ${res.status}` }
    return res.json()
  } catch (e) {
    return { success: false, error: controller.signal.aborted ? 'Timeout (180s)' : e instanceof Error ? e.message : 'Error' }
  } finally { clearTimeout(tid) }
}

function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const r = new FileReader()
    r.onload = () => resolve((r.result as string).split(',')[1])
    r.onerror = reject
    r.readAsDataURL(file)
  })
}

function parseAnswerLabels(answer: string): string[] {
  return Array.from(answer.matchAll(/<ref>(.*?)<\/ref>/g)).map(m => m[1]).filter(Boolean)
}

// ── Canvas overlay for detections ──
function DetectionOverlay({ boxes, points, imageWidth, imageHeight }: {
  boxes: BoundingBox[]; points: Point[]; imageWidth: number; imageHeight: number
}) {
  const containerRef = useRef<HTMLDivElement>(null)
  const canvasRef = useRef<HTMLCanvasElement>(null)

  const draw = useCallback(() => {
    const container = containerRef.current
    const canvas = canvasRef.current
    if (!container || !canvas || !imageWidth || !imageHeight) return
    const ctx = canvas.getContext('2d')
    if (!ctx) return

    const cW = container.clientWidth, cH = container.clientHeight
    const imgA = imageWidth / imageHeight, cA = cW / cH
    let rW: number, rH: number, oX: number, oY: number
    if (imgA > cA) { rW = cW; rH = cW / imgA; oX = 0; oY = (cH - rH) / 2 }
    else { rH = cH; rW = cH * imgA; oX = (cW - rW) / 2; oY = 0 }

    canvas.width = cW; canvas.height = cH
    ctx.clearRect(0, 0, cW, cH)
    const sX = rW / imageWidth, sY = rH / imageHeight

    boxes.forEach((box, i) => {
      const c = DETECTION_COLORS[i % DETECTION_COLORS.length]
      const x = box.x1 * sX + oX, y = box.y1 * sY + oY
      const w = (box.x2 - box.x1) * sX, h = (box.y2 - box.y1) * sY

      ctx.fillStyle = c + '12'
      ctx.fillRect(x, y, w, h)

      ctx.strokeStyle = c
      ctx.lineWidth = Math.max(1.5, rW / 300)
      ctx.strokeRect(x, y, w, h)

      const cl = Math.min(w, h) * 0.18
      ctx.lineWidth = Math.max(2, rW / 180)
      ctx.strokeStyle = c
      ctx.lineCap = 'round'
      ctx.beginPath(); ctx.moveTo(x, y + cl); ctx.lineTo(x, y); ctx.lineTo(x + cl, y); ctx.stroke()
      ctx.beginPath(); ctx.moveTo(x + w - cl, y); ctx.lineTo(x + w, y); ctx.lineTo(x + w, y + cl); ctx.stroke()
      ctx.beginPath(); ctx.moveTo(x, y + h - cl); ctx.lineTo(x, y + h); ctx.lineTo(x + cl, y + h); ctx.stroke()
      ctx.beginPath(); ctx.moveTo(x + w - cl, y + h); ctx.lineTo(x + w, y + h); ctx.lineTo(x + w, y + h - cl); ctx.stroke()
      ctx.lineCap = 'butt'

      const fs = Math.max(8, rW / 60)
      ctx.font = `600 ${fs}px -apple-system, system-ui, sans-serif`
      const label = `${i + 1}`
      const tw = ctx.measureText(label).width
      const pad = fs * 0.4
      const bH = fs + pad * 2, bW = tw + pad * 2, r = bH * 0.35
      const bx = x, by = y - bH - 3
      ctx.fillStyle = c
      ctx.beginPath()
      ctx.moveTo(bx + r, by); ctx.lineTo(bx + bW - r, by)
      ctx.quadraticCurveTo(bx + bW, by, bx + bW, by + r)
      ctx.lineTo(bx + bW, by + bH - r)
      ctx.quadraticCurveTo(bx + bW, by + bH, bx + bW - r, by + bH)
      ctx.lineTo(bx + r, by + bH)
      ctx.quadraticCurveTo(bx, by + bH, bx, by + bH - r)
      ctx.lineTo(bx, by + r)
      ctx.quadraticCurveTo(bx, by, bx + r, by)
      ctx.fill()
      ctx.fillStyle = '#fff'
      ctx.fillText(label, bx + pad, by + fs + pad * 0.4)
    })

    points.forEach((pt, i) => {
      const c = DETECTION_COLORS[i % DETECTION_COLORS.length]
      const px = pt.x * sX + oX, py = pt.y * sY + oY, rr = Math.max(4, rW / 120)
      ctx.beginPath(); ctx.arc(px, py, rr * 2, 0, Math.PI * 2)
      ctx.fillStyle = c + '25'; ctx.fill()
      ctx.beginPath(); ctx.arc(px, py, rr, 0, Math.PI * 2)
      ctx.fillStyle = c; ctx.fill()
      ctx.strokeStyle = '#fff'; ctx.lineWidth = 1.5; ctx.stroke()
    })
  }, [boxes, points, imageWidth, imageHeight])

  useEffect(() => {
    draw()
  }, [draw])

  // Re-draw on container resize
  useEffect(() => {
    const container = containerRef.current
    if (!container) return
    const ro = new ResizeObserver(() => draw())
    ro.observe(container)
    return () => ro.disconnect()
  }, [draw])

  return (
    <div ref={containerRef} style={{ position: 'absolute', inset: 0, zIndex: 2 }}>
      <canvas ref={canvasRef} style={{ position: 'absolute', inset: 0, width: '100%', height: '100%', pointerEvents: 'none' }} />
    </div>
  )
}

const MODES = [
  { value: 'detect', label: 'Detect', ph: 'person, car', input: true },
  { value: 'ground', label: 'Locate', ph: 'red shirts', input: true },
  { value: 'detect_text', label: 'OCR', ph: '', input: false },
  { value: 'ground_gui', label: 'UI', ph: 'search btn', input: true },
  { value: 'point', label: 'Point', ph: 'traffic light', input: true },
]

export const LocateCard = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function LocateCard(props, ref) {
    const { title = 'LocateAnything', dataSource, className = '' } = props
    const extId = dataSource?.extensionId || 'locate-anything-v2'
    const [mode, setMode] = useState('detect')
    const [imageSrc, setImageSrc] = useState<string | null>(null)
    const [imageBase64, setImageBase64] = useState<string | null>(null)
    const [query, setQuery] = useState('')
    const [result, setResult] = useState<InferenceResult | null>(null)
    const [loading, setLoading] = useState(false)
    const [error, setError] = useState<string | null>(null)
    const [imgSize, setImgSize] = useState({ w: 0, h: 0 })
    const [showDetail, setShowDetail] = useState(false)
    const fileRef = useRef<HTMLInputElement>(null)
    const cur = MODES.find(m => m.value === mode)!

    const onFile = useCallback(async (e: React.ChangeEvent<HTMLInputElement>) => {
      const f = e.target.files?.[0]; if (!f) return
      const url = URL.createObjectURL(f)
      setImageSrc(url); setResult(null); setError(null); setShowDetail(false)
      const b64 = await fileToBase64(f); setImageBase64(b64)
      const img = new Image()
      img.onload = () => setImgSize({ w: img.naturalWidth, h: img.naturalHeight })
      img.src = url
    }, [])

    const analyze = useCallback(async () => {
      if (!imageBase64) { setError('Upload an image first'); return }
      setLoading(true); setError(null); setResult(null); setShowDetail(false)
      try {
        let args: Record<string, any> = { image_base64: imageBase64 }
        if (mode === 'detect') args.categories = query || 'object'
        else if (['ground', 'ground_gui', 'point'].includes(mode)) {
          args.phrase = query || 'object'
          if (mode === 'ground_gui') args.output_type = 'box'
        }
        const r = await executeExtensionCommand<InferenceResult>(extId, mode, args)
        if (r.success && r.data) {
          const d = r.data
          if (d.success === false) setError(d.error || 'Inference failed')
          else if (!d.boxes?.length && !d.points?.length && d.answer?.includes('None'))
            setError('No objects detected')
          else setResult(d)
        } else setError(r.error || 'Failed')
      } catch (e: any) { setError(e.message || 'Error') }
      finally { setLoading(false) }
    }, [imageBase64, mode, query, extId])

    const clear = useCallback(() => {
      setImageSrc(null); setImageBase64(null); setResult(null); setError(null); setShowDetail(false)
      if (fileRef.current) fileRef.current.value = ''
    }, [])

    const boxes = result?.boxes ?? [], pts = result?.points ?? []
    const labels = parseAnswerLabels(result?.answer ?? '')
    const uid = `loc-f-${extId}`

    return (
      <div ref={ref} className={`loc ${className}`}>
        <style>{`
          .loc {
            --g-bg: rgba(0, 0, 0, 0.38);
            --g-blur: blur(22px) saturate(1.15);
            --g-border: rgba(255, 255, 255, 0.09);
            --g-border-hi: rgba(255, 255, 255, 0.18);
            --g-shine: inset 0 1px 0 rgba(255, 255, 255, 0.08);
            --g-text: rgba(255, 255, 255, 0.85);
            --g-text-dim: rgba(255, 255, 255, 0.45);

            position: relative;
            height: 100%;
            overflow: hidden;
            border-radius: 12px;
            background: linear-gradient(145deg, #0b0b1a 0%, #111128 50%, #0b0b1a 100%);
            font-family: -apple-system, BlinkMacSystemFont, 'SF Pro Display', system-ui, sans-serif;
            font-size: 11px;
            color: var(--g-text);
            box-sizing: border-box;
            user-select: none;
          }

          /* ── Image layer ── */
          .loc-img { position: absolute; inset: 0; z-index: 1; }
          .loc-img img {
            width: 100%; height: 100%; object-fit: contain; display: block;
            background: #060610;
          }
          /* Vignette for glass readability */
          .loc-img::after {
            content: ''; position: absolute; inset: 0; z-index: 1; pointer-events: none;
            background: radial-gradient(ellipse at center, transparent 40%, rgba(0,0,0,0.35) 100%);
          }

          /* ── Image action buttons (in top bar) ── */
          .loc-acts {
            display: flex; gap: 4px; flex-shrink: 0;
          }
          .loc-ib {
            width: 20px; height: 20px; border-radius: 50%;
            background: var(--g-bg); backdrop-filter: var(--g-blur); -webkit-backdrop-filter: var(--g-blur);
            border: 1px solid var(--g-border); box-shadow: var(--g-shine);
            color: var(--g-text-dim); font-size: 12px; line-height: 1;
            display: flex; align-items: center; justify-content: center;
            cursor: pointer; transition: all 0.2s; padding: 0;
          }
          .loc-ib:hover { background: rgba(255,255,255,0.15); color: #fff; }
          .loc-ib svg { width: 10px; height: 10px; }

          /* ── Empty state ── */
          .loc-empty { position: absolute; inset: 0; z-index: 1; display: flex; align-items: center; justify-content: center; }
          .loc-empty input { display: none; }
          .loc-up {
            width: 72px; height: 72px; border-radius: 50%;
            background: rgba(255,255,255,0.04);
            backdrop-filter: var(--g-blur); -webkit-backdrop-filter: var(--g-blur);
            border: 1px solid var(--g-border); box-shadow: var(--g-shine);
            display: flex; flex-direction: column; align-items: center; justify-content: center;
            gap: 5px; cursor: pointer; color: var(--g-text-dim);
            transition: all 0.3s ease;
          }
          .loc-up svg { width: 20px; height: 20px; opacity: 0.6; transition: opacity 0.3s; }
          .loc-up span { font-size: 8px; letter-spacing: 0.8px; text-transform: uppercase; font-weight: 600; }
          .loc-up:hover {
            background: rgba(255,255,255,0.09); border-color: var(--g-border-hi);
            color: var(--g-text); transform: scale(1.06);
          }
          .loc-up:hover svg { opacity: 0.9; }

          /* ── Floating top bar ── */
          .loc-top {
            position: absolute; top: 5px; left: 5px; right: 5px; z-index: 10;
            display: flex; align-items: center; justify-content: space-between;
          }
          .loc-modes {
            display: flex; gap: 1px; border-radius: 6px; padding: 2px;
            background: var(--g-bg); backdrop-filter: var(--g-blur); -webkit-backdrop-filter: var(--g-blur);
            border: 1px solid var(--g-border); box-shadow: var(--g-shine);
          }
          .loc-mp {
            padding: 2px 7px; border-radius: 4px; border: none;
            background: transparent; color: var(--g-text-dim);
            font-size: 8px; font-weight: 600; letter-spacing: 0.3px;
            cursor: pointer; transition: all 0.2s; white-space: nowrap;
          }
          .loc-mp:hover { color: var(--g-text); }
          .loc-mp.on {
            background: rgba(255,255,255,0.14); color: #fff;
            box-shadow: 0 1px 6px rgba(0,0,0,0.2);
          }

          /* ── Stat badges ── */
          .loc-stats {
            display: flex; gap: 3px; flex-shrink: 0;
          }
          .loc-stat {
            background: var(--g-bg); backdrop-filter: var(--g-blur); -webkit-backdrop-filter: var(--g-blur);
            border: 1px solid var(--g-border); box-shadow: var(--g-shine);
            border-radius: 999px; padding: 2px 7px;
            font-size: 8px; font-weight: 500; color: var(--g-text-dim);
            letter-spacing: 0.2px; font-variant-numeric: tabular-nums;
            display: flex; align-items: center; gap: 3px;
          }
          .loc-stat-dot { width: 5px; height: 5px; border-radius: 50%; flex-shrink: 0; }

          /* ── Floating bottom bar ── */
          .loc-bot {
            position: absolute; bottom: 5px; left: 5px; right: 5px; z-index: 10;
            display: flex; gap: 3px; align-items: center;
          }
          .loc-in {
            flex: 1; min-width: 0; padding: 4px 10px; border-radius: 999px;
            background: var(--g-bg); backdrop-filter: var(--g-blur); -webkit-backdrop-filter: var(--g-blur);
            border: 1px solid var(--g-border); box-shadow: var(--g-shine);
            color: var(--g-text); font-size: 10px; outline: none;
            transition: border-color 0.2s;
          }
          .loc-in::placeholder { color: var(--g-text-dim); }
          .loc-in:focus { border-color: var(--g-border-hi); }

          .loc-go {
            width: 28px; height: 28px; border-radius: 50%; flex-shrink: 0;
            background: rgba(255,255,255,0.1); backdrop-filter: var(--g-blur); -webkit-backdrop-filter: var(--g-blur);
            border: 1px solid var(--g-border-hi); box-shadow: var(--g-shine);
            color: #fff; cursor: pointer;
            display: flex; align-items: center; justify-content: center;
            transition: all 0.2s; padding: 0;
          }
          .loc-go:hover:not(:disabled) { background: rgba(255,255,255,0.22); }
          .loc-go:disabled { opacity: 0.25; cursor: not-allowed; }
          .loc-go svg { width: 12px; height: 12px; }

          /* ── Result detail panel ── */
          .loc-detail {
            position: absolute; top: 38px; left: 5px; right: 5px; z-index: 10;
            max-height: 140px; overflow-y: auto;
            background: var(--g-bg); backdrop-filter: var(--g-blur); -webkit-backdrop-filter: var(--g-blur);
            border: 1px solid var(--g-border); box-shadow: var(--g-shine);
            border-radius: 10px; padding: 8px 10px;
            animation: locDown 0.2s ease-out;
          }
          .loc-detail::-webkit-scrollbar { width: 2px; }
          .loc-detail::-webkit-scrollbar-thumb { background: rgba(255,255,255,0.12); border-radius: 2px; }
          .loc-detail-text {
            font-size: 8px; line-height: 1.5; color: var(--g-text-dim);
            white-space: pre-wrap; word-break: break-word;
          }
          .loc-labels { display: flex; flex-wrap: wrap; gap: 3px; margin-bottom: 5px; }
          .loc-tag {
            font-size: 8px; font-weight: 600; padding: 1px 6px;
            border-radius: 999px; border: 1px solid; letter-spacing: 0.2px;
            background: rgba(255,255,255,0.06); color: var(--g-text);
          }

          /* ── Detail toggle ── */
          .loc-dt {
            position: absolute; top: 38px; right: 5px; z-index: 10;
            background: var(--g-bg); backdrop-filter: var(--g-blur); -webkit-backdrop-filter: var(--g-blur);
            border: 1px solid var(--g-border); box-shadow: var(--g-shine);
            border-radius: 999px; padding: 3px 9px;
            font-size: 8px; font-weight: 600; color: var(--g-text);
            display: flex; align-items: center; gap: 4px;
            cursor: pointer; transition: background 0.2s;
          }
          .loc-dt:hover { background: rgba(0,0,0,0.55); }
          .loc-dt svg {
            width: 8px; height: 8px; opacity: 0.5;
            transform: rotate(0deg); transition: transform 0.2s;
          }
          .loc-dt.open svg { transform: rotate(180deg); }

          /* ── Error toast ── */
          .loc-err {
            position: absolute; top: 50%; left: 50%; transform: translate(-50%, -50%); z-index: 20;
            background: rgba(30, 0, 0, 0.6); backdrop-filter: var(--g-blur); -webkit-backdrop-filter: var(--g-blur);
            border: 1px solid rgba(255, 80, 80, 0.18); box-shadow: var(--g-shine);
            border-radius: 10px; padding: 8px 16px; max-width: 85%; text-align: center;
            font-size: 10px; color: #ff9a9a; animation: locIn 0.2s ease-out;
          }

          /* ── Loading shimmer ── */
          .loc-ld {
            position: absolute; inset: 0; z-index: 15; pointer-events: none;
            background: rgba(0,0,0,0.08);
          }
          .loc-ld::after {
            content: ''; position: absolute; inset: 0;
            background: linear-gradient(105deg, transparent 40%, rgba(255,255,255,0.04) 50%, transparent 60%);
            animation: locShimmer 1.8s ease-in-out infinite;
          }

          @keyframes locUp { from { opacity: 0; transform: translateY(6px); } to { opacity: 1; transform: translateY(0); } }
          @keyframes locDown { from { opacity: 0; transform: translateY(-4px); } to { opacity: 1; transform: translateY(0); } }
          @keyframes locIn { from { opacity: 0; } to { opacity: 1; } }
          @keyframes locShimmer { 0% { transform: translateX(-100%); } 100% { transform: translateX(100%); } }
          @keyframes locSpin { to { transform: rotate(360deg); } }
          .loc-spin { animation: locSpin 0.7s linear infinite; }
        `}</style>

        {/* ── Image layer ── */}
        {imageSrc ? (
          <div className="loc-img">
            <img src={imageSrc} alt="" onLoad={e => {
              const i = e.target as HTMLImageElement
              setImgSize({ w: i.naturalWidth, h: i.naturalHeight })
            }} />
            {result && (boxes.length > 0 || pts.length > 0) && (
              <DetectionOverlay boxes={boxes} points={pts} imageWidth={imgSize.w} imageHeight={imgSize.h} />
            )}
          </div>
        ) : (
          <div className="loc-empty">
            <input ref={fileRef} type="file" accept="image/*" onChange={onFile} id={uid} />
            <label htmlFor={uid} className="loc-up">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><polyline points="17 8 12 3 7 8" /><line x1="12" y1="3" x2="12" y2="15" />
              </svg>
              <span>Upload</span>
            </label>
          </div>
        )}

        {/* ── Loading shimmer ── */}
        {loading && <div className="loc-ld" />}

        {/* ── Top bar: modes + actions + stats ── */}
        <div className="loc-top">
          <div style={{ display: 'flex', alignItems: 'center', gap: 6, flex: 1, minWidth: 0 }}>
            <div className="loc-modes">
              {MODES.map(m => (
                <button key={m.value}
                  className={`loc-mp ${mode === m.value ? 'on' : ''}`}
                  onClick={() => { setMode(m.value); setResult(null); setError(null); setShowDetail(false) }}
                >{m.label}</button>
              ))}
            </div>
            {imageSrc && (
              <div className="loc-acts">
                <button className="loc-ib" onClick={() => fileRef.current?.click()} title="Change image">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round"><polyline points="23 4 23 10 17 10" /><path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10" /></svg>
                </button>
                <button className="loc-ib" onClick={clear} title="Remove image">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round"><line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" /></svg>
                </button>
              </div>
            )}
          </div>
          <div className="loc-stats">
            {result && result.inference_time_ms > 0 && (
              <div className="loc-stat">{(result.inference_time_ms / 1000).toFixed(1)}s</div>
            )}
            {result && boxes.length > 0 && (
              <div className="loc-stat">
                <span className="loc-stat-dot" style={{ background: DETECTION_COLORS[0] }} />
                {boxes.length}
              </div>
            )}
          </div>
        </div>

        {/* ── Bottom bar: input + analyze ── */}
        <div className="loc-bot">
          {cur.input && (
            <input className="loc-in" placeholder={cur.ph} value={query}
              onChange={e => setQuery(e.target.value)}
              onKeyDown={e => e.key === 'Enter' && analyze()}
            />
          )}
          <button className="loc-go" onClick={analyze} disabled={loading || !imageBase64}>
            {loading ? (
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" className="loc-spin">
                <path d="M21 12a9 9 0 1 1-6.219-8.56" />
              </svg>
            ) : (
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round">
                <circle cx="11" cy="11" r="8" /><line x1="21" y1="21" x2="16.65" y2="16.65" />
              </svg>
            )}
          </button>
        </div>

        {/* ── Result detail toggle ── */}
        {result && (boxes.length > 0 || pts.length > 0) && !showDetail && (
          <div className="loc-dt" onClick={() => setShowDetail(true)}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
              <polyline points="6 9 12 15 18 9" />
            </svg>
            {labels.length > 0 ? labels.slice(0, 3).join(', ') : `${boxes.length} found`}
          </div>
        )}

        {/* ── Expanded detail panel ── */}
        {showDetail && result && (
          <div className="loc-detail">
            <div style={{ display: 'flex', alignItems: 'center', gap: 4, marginBottom: 5, cursor: 'pointer' }}
              onClick={() => setShowDetail(false)}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5"
                style={{ width: 8, height: 8, opacity: 0.5, transform: 'rotate(180deg)', flexShrink: 0 }}>
                <polyline points="6 9 12 15 18 9" />
              </svg>
              <span style={{ fontSize: 8, fontWeight: 600, color: 'var(--g-text)', opacity: 0.7 }}>
                {labels.length > 0 ? labels.slice(0, 3).join(', ') : `${boxes.length} found`}
              </span>
            </div>
            {labels.length > 0 && (
              <div className="loc-labels">
                {labels.map((l, i) => (
                  <span key={i} className="loc-tag" style={{ borderColor: DETECTION_COLORS[i % DETECTION_COLORS.length] + '60' }}>{l}</span>
                ))}
              </div>
            )}
            <div className="loc-detail-text">{result.answer}</div>
          </div>
        )}

        {/* ── Error ── */}
        {error && <div className="loc-err">{error}</div>}
      </div>
    )
  }
)

export default { LocateCard }
