import { forwardRef, useState, useEffect, useRef, useCallback } from 'react'

// ============================================================================
// Types
// ============================================================================

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

interface VlmInfo {
  text: string
  frame: number
  latencyMs: number
}

interface RuntimeStats {
  fps: number
  totalFrames: number
  sessionTime: number
}

const DEFAULT_PROMPT = 'Briefly describe what is happening in the video frame, including objects, people, scene, and events.'
const EXTENSION_ID = 'video-vlm'

// Auth header for extension command API calls
const authHeader = () => {
  const token = (typeof localStorage !== 'undefined' && localStorage.getItem('neomind_token'))
    || (typeof sessionStorage !== 'undefined' && sessionStorage.getItem('neomind_token_session'))
  return token ? { Authorization: `Bearer ${token}` } as Record<string, string> : {} as Record<string, string>
}

// HTTP API base for extension command calls (matches the WS host logic)
const getApiBase = () => {
  const host = (window as any).__TAURI__ ? `${location.hostname}:9375` : location.host
  return `${location.protocol === 'https:' ? 'https' : 'http'}://${host}/api`
}


// UI is English-only (per requirements)

// ============================================================================
// Component
// ============================================================================

export const VideoVlmDisplay = forwardRef<HTMLDivElement, ExtensionComponentProps & {
  sourceUrl?: string
  prompt?: string
  analyzeEvery?: number
  maxTokens?: number
  fps?: number
}>(function VideoVlmDisplay(props, ref) {
  const { className = '', config } = props

  // Config state (from props/config or defaults)
  const cfg = config || {}
  const [sourceUrl, setSourceUrl] = useState(cfg.sourceUrl || '')
  const [prompt, setPrompt] = useState(cfg.prompt || DEFAULT_PROMPT)
  const [analyzeEvery, setAnalyzeEvery] = useState(cfg.analyzeEvery || 48)
  const [maxTokens, setMaxTokens] = useState(cfg.maxTokens || 120)
  const [voiceOn, setVoiceOn] = useState(cfg.voiceOn ?? false)
  const [loopVideo, setLoopVideo] = useState(cfg.loopVideo ?? true)
  const lastSpokenRef = useRef('')
  const lastSpeakTimeRef = useRef(0)

  // Runtime state
  const [isRunning, setIsRunning] = useState(false)
  const [connecting, setConnecting] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [info, setInfo] = useState<string | null>(null)
  const [vlm, setVlm] = useState<VlmInfo>({ text: '', frame: 0, latencyMs: 0 })
  const [vlmHistory, setVlmHistory] = useState<Array<{timestamp:number; frame:number; latency_ms:number; text:string; thumb?:string}>>([])
  const [stats, setStats] = useState<RuntimeStats>({ fps: 0, totalFrames: 0, sessionTime: 0 })
  const [hasFrame, setHasFrame] = useState(false)

  // Refs
  const wsRef = useRef<WebSocket | null>(null)
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const pendingFrameRef = useRef<string | null>(null)
  const rafRef = useRef<number>(0)
  const sessionIdRef = useRef<string | null>(null)
  const frameTimesRef = useRef<number[]>([])
  const startTimeRef = useRef<number>(0)
  const imgRef = useRef<HTMLImageElement | null>(null)

  // Voice: speak VLM text via browser speechSynthesis when enabled
  useEffect(() => {
    if (!voiceOn || !vlm.text || vlm.text === lastSpokenRef.current) return
    if (typeof speechSynthesis === 'undefined') return
    lastSpokenRef.current = vlm.text
    speechSynthesis.cancel() // cancel any pending speech
    const u = new SpeechSynthesisUtterance(vlm.text)
    u.lang = 'zh-CN'
    u.rate = 1.0
    // pick a Chinese voice if available
    const voices = speechSynthesis.getVoices()
    const zh = voices.find(v => v.lang.startsWith('zh'))
    if (zh) u.voice = zh
    speechSynthesis.speak(u)
  }, [voiceOn, vlm.text])

  // Stop speech when toggled off or component unmounts
  useEffect(() => {
    if (!voiceOn && typeof speechSynthesis !== 'undefined') {
      speechSynthesis.cancel()
    }
  }, [voiceOn])
  useEffect(() => () => { if (typeof speechSynthesis !== 'undefined') speechSynthesis.cancel() }, [])

  // Stable config refs (avoid WS reconnect on every keystroke)
  const cfgRef = useRef({ sourceUrl, prompt, analyzeEvery, maxTokens })
  useEffect(() => { cfgRef.current = { sourceUrl, prompt, analyzeEvery, maxTokens } }, [sourceUrl, prompt, analyzeEvery, maxTokens])

  // Live config apply: while the stream is running, push prompt/interval/token
  // edits to the extension via update_stream_config (debounced) — the worker
  // reads them fresh on the next analysis, no restart needed.
  // IME guard: never fire mid-composition (Chinese pinyin etc.); the final
  // onChange after compositionend re-triggers this effect.
  const composingRef = useRef(false)
  const [configApply, setConfigApply] = useState<'idle' | 'typing' | 'applied'>('idle')
  // Remounts the hidden file input after any upload outcome so re-picking
  // (even the same file) always fires onChange — the "must refresh to retry"
  // bug after a size-limit rejection.
  const [uploadReset, setUploadReset] = useState(0)
  useEffect(() => {
    if (!isRunning || !sessionIdRef.current) { setConfigApply('idle'); return }
    if (composingRef.current) return
    setConfigApply('typing')
    const sid = sessionIdRef.current
    const t = setTimeout(() => {
      fetch(`${getApiBase()}/extensions/${EXTENSION_ID}/command`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', ...authHeader() },
        body: JSON.stringify({
          command: 'update_stream_config',
          args: { stream_id: sid, prompt, analyze_every: analyzeEvery, max_tokens: maxTokens },
        }),
      })
        .then(() => setConfigApply('applied'))
        .catch(() => setConfigApply('idle'))
    }, 800)
    return () => clearTimeout(t)
  }, [prompt, analyzeEvery, maxTokens, isRunning])

  // ============================================================================
  // Frame rendering (rAF loop)
  // ============================================================================

  useEffect(() => {
    let lastDrawn = ''
    const renderLoop = () => {
      const data = pendingFrameRef.current
      const canvas = canvasRef.current
      if (data && canvas && data !== lastDrawn) {
        lastDrawn = data
        // Reuse a single Image object — creating new Image() per frame
        // at 24fps causes GC pressure and intermittent jank
        if (!imgRef.current) imgRef.current = new Image()
        const img = imgRef.current
        img.onload = () => {
          const c = canvasRef.current
          if (c) {
            if (c.width !== img.width) c.width = img.width
            if (c.height !== img.height) c.height = img.height
            const ctx = c.getContext('2d')
            if (ctx) {
              ctx.drawImage(img, 0, 0)
              pendingFrameRef.current = null // consumed
            }
          }
          setHasFrame(true)
        }
        img.src = `data:image/jpeg;base64,${data}`
      }
      rafRef.current = requestAnimationFrame(renderLoop)
    }
    rafRef.current = requestAnimationFrame(renderLoop)
    return () => cancelAnimationFrame(rafRef.current)
  }, [])

  // Session timer
  useEffect(() => {
    if (!isRunning) return
    const t = setInterval(() => {
      setStats(prev => ({ ...prev, sessionTime: prev.sessionTime + 1 }))
    }, 1000)
    return () => clearInterval(t)
  }, [isRunning])

  // ============================================================================
  // WebSocket
  // ============================================================================

  // ── Device camera (client-camera:// source) ────────────────────────────
  // Opens the browser's OWN camera via getUserMedia, captures JPEG frames on
  // a canvas and uploads them as binary WS frames (8-byte BE sequence + JPEG)
  // to the extension, which echoes them back and runs VLM analysis.
  // NOTE: getUserMedia requires a secure context — works on localhost/HTTPS/
  // Tauri, NOT on plain-HTTP LAN IP access.
  const camStreamRef = useRef<MediaStream | null>(null)
  const camVideoRef = useRef<HTMLVideoElement | null>(null)
  const camTimerRef = useRef<number | null>(null)
  const camSeqRef = useRef(0)
  // True while the local camera preview <video> should render (native fps)
  const [camPreview, setCamPreview] = useState(false)

  const stopClientCamera = useCallback(() => {
    if (camTimerRef.current !== null) { clearInterval(camTimerRef.current); camTimerRef.current = null }
    if (camStreamRef.current) {
      camStreamRef.current.getTracks().forEach(t => t.stop())
      camStreamRef.current = null
    }
    setCamPreview(false)
  }, [])

  const startClientCamera = useCallback(async (ws: WebSocket) => {
    if (!navigator.mediaDevices?.getUserMedia) {
      setError(`Device camera unavailable: this page is NOT a secure context (protocol=${location.protocol}, isSecureContext=${window.isSecureContext}). Use https://${location.host}`)
      return
    }
    try {
      const stream = await navigator.mediaDevices.getUserMedia({
        video: { facingMode: 'environment', width: { ideal: 1280 }, height: { ideal: 720 } },
        audio: false,
      })
      camStreamRef.current = stream
      setCamPreview(true) // mounts the local <video> preview (native fps)
      setHasFrame(true)
      const canvas = document.createElement('canvas')
      canvas.width = 640
      canvas.height = 360
      const ctx = canvas.getContext('2d')
      camTimerRef.current = window.setInterval(() => {
        const video = camVideoRef.current
        if (!ctx || !video || video.readyState < 2) return
        // Contain-draw (letterbox): portrait phone cameras must not be
        // squashed into the fixed 640x360 capture canvas.
        const vw = video.videoWidth || 640
        const vh = video.videoHeight || 360
        const s = Math.min(640 / vw, 360 / vh)
        const dw = vw * s, dh = vh * s
        ctx.fillStyle = '#000'
        ctx.fillRect(0, 0, 640, 360)
        ctx.drawImage(video, (640 - dw) / 2, (360 - dh) / 2, dw, dh)
        canvas.toBlob(async blob => {
          if (!blob || ws.readyState !== WebSocket.OPEN) return
          const jpeg = new Uint8Array(await blob.arrayBuffer())
          const frame = new Uint8Array(8 + jpeg.length)
          const seq = ++camSeqRef.current
          // 8-byte big-endian sequence + payload (neomind binary WS frame)
          for (let i = 0; i < 8; i++) frame[i] = (seq / 2 ** (8 * (7 - i))) & 0xFF
          frame.set(jpeg, 8)
          ws.send(frame)
        }, 'image/jpeg', 0.6)
      }, 200)
    } catch (e: any) {
      setError(`Camera failed: ${e?.message || e}`)
      stopClientCamera()
    }
  }, [stopClientCamera])

  // Attach the camera stream to the preview <video> once it mounts
  useEffect(() => {
    if (camPreview && camVideoRef.current && camStreamRef.current) {
      camVideoRef.current.srcObject = camStreamRef.current
      void camVideoRef.current.play()
    }
  }, [camPreview])

  const connect = useCallback(() => {
    const src = cfgRef.current.sourceUrl.trim()
    if (!src) {
      setError('Please enter video source URL')
      return
    }

    setConnecting(true)
    setError(null)
    setInfo(null)
    setVlm({ text: '', frame: 0, latencyMs: 0 })
    setStats({ fps: 0, totalFrames: 0, sessionTime: 0 })
    setHasFrame(false)
    startTimeRef.current = Date.now()
    frameTimesRef.current = []

    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:'
    const host = (window as any).__TAURI__ ? `${location.hostname}:9375` : location.host
    const baseUrl = `${proto}//${host}/api/extensions/${EXTENSION_ID}/stream`
    const token = localStorage.getItem('neomind_token')
      || sessionStorage.getItem('neomind_token_session')
    const url = token
      ? `${baseUrl}?token=${encodeURIComponent(token)}`
      : baseUrl

    const ws = new WebSocket(url)
    wsRef.current = ws

    ws.onopen = () => {
      ws.send(JSON.stringify({
        type: 'init',
        config: {
          source_url: src,
          prompt: cfgRef.current.prompt || DEFAULT_PROMPT,
          analyze_every: cfgRef.current.analyzeEvery,
          max_tokens: cfgRef.current.maxTokens,
          target_fps: 30, // push as fast as decode allows; analysis interval controls VLM cadence
          draw_boxes: false,
        }
      }))
      if (src.startsWith('client-camera://')) {
        void startClientCamera(ws)
      }
    }

    ws.onmessage = (event) => {
      if (event.data instanceof ArrayBuffer) return
      if (typeof event.data !== 'string') return

      try {
        const msg = JSON.parse(event.data)

        switch (msg.type) {
          case 'session_created':
            sessionIdRef.current = msg.session_id
            setIsRunning(true)
            setConnecting(false)
            break

          case 'session_closed':
          case 'end':
            setIsRunning(false)
            if (loopVideo && wsRef.current) {
              setInfo('Looping...')
              setTimeout(() => { disconnect(); connect() }, 1000)
            }
            break

          case 'error':
            setError(msg.message || msg.error || 'Unknown error')
            setIsRunning(false)
            setConnecting(false)
            break

          default:
            // Frame or result message
            if (msg.data && typeof msg.data === 'string' && msg.data.length > 100) {
              pendingFrameRef.current = msg.data

              // Update stats
              const now = performance.now()
              frameTimesRef.current.push(now)
              if (frameTimesRef.current.length > 30) frameTimesRef.current.shift()
              const times = frameTimesRef.current
              let fps = 0
              if (times.length >= 2) {
                const elapsed = (times[times.length - 1] - times[0]) / 1000
                if (elapsed > 0) fps = (times.length - 1) / elapsed
              }
              setStats(prev => ({
                fps: Math.round(fps * 10) / 10,
                totalFrames: prev.totalFrames + 1,
                sessionTime: prev.sessionTime,
              }))
            }

            // VLM metadata
            if (msg.metadata?.vlm_text) {
              setVlm({
                text: msg.metadata.vlm_text,
                frame: msg.metadata.vlm_frame ?? 0,
                latencyMs: msg.metadata.vlm_latency_ms ?? 0,
              })
            }
            // VLM history arrives as an incremental delta (newest entry only,
            // sent when a new analysis lands) — append locally and cap at 20.
            const entry = msg.metadata?.vlm_entry
            if (entry && entry !== null) {
              setVlmHistory(prev => [...prev, entry].slice(-20))
            } else if (msg.metadata?.vlm_history && msg.metadata.vlm_history !== null) {
              setVlmHistory(msg.metadata.vlm_history)
            }

            if (msg.metadata?.status === 'reconnecting') {
              setError('Reconnecting...')
            } else if (msg.metadata?.status === 'streaming') {
              setError(null)
            } else if (msg.metadata?.status === 'ended') {
              setIsRunning(false)
              if (loopVideo) {
                setInfo('Looping...')
                setTimeout(() => { disconnect(); connect() }, 1000)
              }
            }
            break
        }
      } catch { /* not JSON */ }
    }

    ws.onclose = () => {
      setIsRunning(false)
      setConnecting(false)
      wsRef.current = null
      stopClientCamera()
    }

    ws.onerror = () => {
      setError('WebSocket failed')
      setConnecting(false)
      setIsRunning(false)
    }
  }, [startClientCamera, stopClientCamera])

  const disconnect = useCallback(() => {
    if (wsRef.current) {
      wsRef.current.close()
      wsRef.current = null
    }
    stopClientCamera()
    setIsRunning(false)
    sessionIdRef.current = null
  }, [stopClientCamera])

  useEffect(() => () => { if (wsRef.current) wsRef.current.close(); stopClientCamera() }, [stopClientCamera])

  // ============================================================================
  // Render
  // ============================================================================

  const fmtTime = (s: number) => {
    const m = Math.floor(s / 60)
    const sec = s % 60
    return m > 0 ? `${m}m${sec}s` : `${sec}s`
  }

  return (
    <div ref={ref} className={`vvlm ${className}`}>
      <style>{`
        .vvlm {
          display: flex;
          gap: 0;
          height: 100%;
          background: var(--card, #1a1a2e);
          border: 1px solid var(--border, #333);
          border-radius: 12px;
          overflow: hidden;
          font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
          color: var(--foreground, #eee);
          font-size: 13px;
        }

        /* ===== Left: Config Panel ===== */
        .vvlm-left {
          width: 220px;
          flex-shrink: 0;
          display: flex;
          flex-direction: column;
          gap: 12px;
          padding: 14px;
          border-right: 1px solid var(--border, #333);
          background: var(--muted, rgba(255,255,255,0.03));
          overflow-y: auto;
        }

        .vvlm-field {
          display: flex;
          flex-direction: column;
          gap: 5px;
        }

        .vvlm-label {
          font-size: 11px;
          font-weight: 600;
          color: var(--muted-foreground, #888);
          text-transform: uppercase;
          letter-spacing: 0.5px;
        }

        .vvlm-input {
          width: 100%;
          padding: 7px 10px;
          border: 1px solid var(--border, #444);
          border-radius: 6px;
          background: var(--background, #111);
          color: var(--foreground, #eee);
          font-size: 12px;
          font-family: inherit;
          outline: none;
          box-sizing: border-box;
        }
        .vvlm-input:focus {
          border-color: var(--primary, #3b82f6);
        }
        .vvlm-input::placeholder { color: #555; }

        textarea.vvlm-input {
          resize: vertical;
          min-height: 60px;
          line-height: 1.4;
        }

        .vvlm-row {
          display: flex;
          align-items: center;
          gap: 8px;
        }
        .vvlm-row .vvlm-input { flex: 1; }
        .vvlm-hint {
          font-size: 10px;
          color: #666;
        }

        .vvlm-btn {
          display: flex;
          align-items: center;
          justify-content: center;
          gap: 6px;
          width: 100%;
          padding: 10px 0;
          border: none;
          border-radius: 8px;
          font-size: 13px;
          font-weight: 600;
          cursor: pointer;
          transition: opacity .15s;
          font-family: inherit;
          box-sizing: border-box;
        }
        .vvlm-btn:disabled { opacity: .5; cursor: not-allowed; }
        .vvlm-btn-start {
          background: var(--primary, #3b82f6);
          color: var(--primary-foreground, #fff);
        }
        .vvlm-btn-stop {
          background: #ef4444;
          color: #fff;
        }

        /* ===== Center: Video ===== */
        .vvlm-center {
          flex: 1;
          display: flex;
          align-items: center;
          justify-content: center;
          background: #000;
          position: relative;
          min-width: 0;
          overflow: hidden;
        }
        .vvlm-canvas {
          width: 100%;
          height: 100%;
          object-fit: contain;
        }
        .vvlm-placeholder {
          display: flex;
          flex-direction: column;
          align-items: center;
          gap: 8px;
          color: #555;
          font-size: 13px;
        }
        .vvlm-placeholder svg { opacity: .3; }
        .vvlm-info {
          position: absolute;
          top: 10px; left: 10px; right: 10px;
          padding: 8px 12px;
          border-radius: 6px;
          background: rgba(59,130,246,.85);
          color: #fff;
          font-size: 12px;
          z-index: 10;
        }
        .vvlm-error {
          position: absolute;
          top: 10px; left: 10px; right: 10px;
          padding: 8px 12px;
          border-radius: 6px;
          background: rgba(239,68,68,.9);
          color: #fff;
          font-size: 12px;
          z-index: 10;
        }
        .vvlm-connecting {
          position: absolute;
          inset: 0;
          display: flex;
          align-items: center;
          justify-content: center;
          background: rgba(0,0,0,.7);
          color: #aaa;
          font-size: 13px;
          z-index: 5;
        }

        /* ===== Right: VLM Info ===== */
        .vvlm-right {
          width: 240px;
          flex-shrink: 0;
          display: flex;
          flex-direction: column;
          border-left: 1px solid var(--border, #333);
          background: var(--muted, rgba(255,255,255,0.03));
          overflow: hidden;
          min-height: 0;
        }

        .vvlm-info-section {
          padding: 12px 14px;
          display: flex;
          flex-direction: column;
          gap: 6px;
        }
        .vvlm-info-section + .vvlm-info-section {
          border-top: 1px solid var(--border, #333);
        }
        .vvlm-info-section-vlm {
          flex: 1;
          min-height: 0;
          overflow: hidden;
          display: flex;
          flex-direction: column;
          gap: 6px;
          padding: 12px 14px;
        }

        .vvlm-info-title {
          font-size: 10px;
          font-weight: 700;
          color: var(--muted-foreground, #888);
          text-transform: uppercase;
          letter-spacing: 0.5px;
          flex-shrink: 0;
        }

        .vvlm-desc-text {
          font-size: 12.5px;
          line-height: 1.55;
          color: var(--foreground, #ddd);
          word-break: break-word;
          overflow-y: auto;
          flex: 1;
          min-height: 0;
        }

        .vvlm-stat-grid {
          display: grid;
          grid-template-columns: 1fr 1fr;
          gap: 6px;
        }
        .vvlm-stat {
          display: flex;
          flex-direction: column;
          gap: 2px;
          padding: 6px 8px;
          border-radius: 6px;
          background: rgba(255,255,255,0.04);
        }
        .vvlm-stat-val {
          font-size: 16px;
          font-weight: 700;
          font-variant-numeric: tabular-nums;
          color: var(--foreground, #eee);
        }
        .vvlm-stat-label {
          font-size: 10px;
          color: var(--muted-foreground, #777);
        }

        .vvlm-vlm-meta {
          display: flex;
          gap: 10px;
          font-size: 11px;
          color: var(--muted-foreground, #888);
          font-variant-numeric: tabular-nums;
        }

        .vvlm-status-bar {
          padding: 6px 14px;
          border-top: 1px solid var(--border, #333);
          display: flex;
          align-items: center;
          gap: 6px;
          font-size: 10.5px;
          color: #666;
          margin-top: auto;
        }
        .vvlm-dot {
          width: 7px; height: 7px;
          border-radius: 50%;
          flex-shrink: 0;
        }
        .vvlm-dot-on { background: #22c55e; animation: vvlm-pulse 2s infinite; }
        .vvlm-dot-off { background: #555; }
        .vvlm-dot-err { background: #ef4444; }
        @keyframes vvlm-pulse { 0%,100%{opacity:1} 50%{opacity:.4} }
      `}</style>

      {/* ===== Left: Config ===== */}
      <div className="vvlm-left">
        <div className="vvlm-field">
          <span className="vvlm-label">  {'Source'}  </span>
          <input
            className="vvlm-input"
            value={sourceUrl}
            onChange={e => setSourceUrl(e.target.value)}
            placeholder="rtsp://... / file:///... / camera://0"
            disabled={isRunning}
          />
          <div style={{display:'flex',gap:'6px',marginTop:'4px'}}>
            <button
              className="vvlm-btn"
              style={{
                flex:1, fontSize:11, padding:'5px 0', cursor:'pointer',
                background:'transparent', color:'var(--muted-foreground,#888)',
                border:'1px dashed var(--border,#444)', borderRadius:6,
              }}
              title="Use this device's camera (requires HTTPS or localhost)"
              onClick={() => {
                setSourceUrl('client-camera://0')
                setError(null)
                if (window.isSecureContext) {
                  setInfo('Device camera ready — press Play')
                } else {
                  setInfo('Device camera needs HTTPS: open https://' + location.host + ' (or localhost)')
                }
              }}
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{display:'inline',verticalAlign:'middle',marginRight:6}}>
                <path d="M23 7l-7 5 7 5V7z"/><rect x="1" y="5" width="15" height="14" rx="2" ry="2"/>
              </svg>
              Device Camera
            </button>
            <label className="vvlm-btn" style={{
              flex:1, fontSize:11, padding:'5px 0', cursor:'pointer',
              background:'transparent', color:'var(--muted-foreground,#888)',
              border:'1px dashed var(--border,#444)', borderRadius:6,
            }}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{display:'inline',verticalAlign:'middle',marginRight:6}}>
                <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>
              </svg>
              Browse
              <input
                key={uploadReset}
                type="file"
                accept="video/*"
                style={{display:'none'}}
                disabled={isRunning}
                onChange={async (e) => {
                  const inputEl = e.target
                  const f = inputEl.files?.[0]
                  if (!f) return
                  // Any failure remounts the input (uploadReset) so a re-pick —
                  // including the SAME file — always fires onChange again.
                  const resetInput = () => { inputEl.value = ''; setUploadReset(n => n + 1) }
                  if (f.size > 500 * 1024 * 1024) {
                    setError(`File too large: ${(f.size/1048576).toFixed(0)}MB (limit 500MB)`)
                    resetInput()
                    return
                  }
                  setInfo('Uploading' + ` (${(f.size/1048576).toFixed(1)}MB)`)
                  try {
                    const buf = await f.arrayBuffer()
                    const bytes = new Uint8Array(buf)
                    // Chunked upload: split into 1.5MB pieces to stay under API body limit
                    const CH = 512 * 1024 // 512KB per chunk (base64 ~700KB, under 1MB API limit)
                    const totalChunks = Math.ceil(buf.byteLength / CH)
                    const startR = await fetch(`${getApiBase()}/extensions/${EXTENSION_ID}/command`, {
                      method:'POST',
                      headers:{'Content-Type':'application/json', ...authHeader()},
                      body: JSON.stringify({ command:'upload_start', args:{ filename: f.name } })
                    }).then(r => r.json())
                    if (!startR.success) throw new Error(startR.error?.message || 'start failed')
                    const tmpPath = startR.data?.tmp
                    for (let ci = 0; ci < totalChunks; ci++) {
                      const chunk = bytes.subarray(ci * CH, Math.min((ci + 1) * CH, buf.byteLength))
                      let bin = ''
                      for (let j = 0; j < chunk.length; j += 32768) {
                        bin += String.fromCharCode(...chunk.subarray(j, Math.min(j + 32768, chunk.length)))
                      }
                      const chunkB64 = btoa(bin)
                      setInfo(`Uploading ${ci + 1}/${totalChunks}`)
                      const cr = await fetch(`${getApiBase()}/extensions/${EXTENSION_ID}/command`, {
                        method:'POST',
                        headers:{'Content-Type':'application/json', ...authHeader()},
                        body: JSON.stringify({ command:'upload_chunk', args:{ tmp: tmpPath, data: chunkB64 } })
                      }).then(r => r.json())
                      if (!cr.success) throw new Error(cr.error?.message || 'chunk failed')
                    }
                    const endR = await fetch(`${getApiBase()}/extensions/${EXTENSION_ID}/command`, {
                      method:'POST',
                      headers:{'Content-Type':'application/json', ...authHeader()},
                      body: JSON.stringify({ command:'upload_end', args:{ tmp: tmpPath, filename: f.name } })
                    }).then(r => r.json())
                    if (endR.success) {
                      setSourceUrl(endR.data?.path || '')
                      setInfo(null)
                      setError(null)
                    } else {
                      setError(`Upload failed: ${endR.error?.message || '?'}`)
                    }
                  } catch(err) {
                    setError(`Upload failed: ${err}`)
                  }
                  resetInput()
                }}
              />
            </label>
          </div>
        </div>

        <div className="vvlm-field">
          <span className="vvlm-label">  {'Prompt'}{' '}
            {isRunning && configApply !== 'idle' && (
              <span style={{
                fontSize: 10, opacity: configApply === 'applied' ? 0.7 : 0.45,
                color: configApply === 'applied' ? 'var(--primary)' : 'var(--muted-foreground)',
              }}>
                {configApply === 'applied' ? '✓ applied' : '· typing…'}
              </span>
            )}
          </span>
          <textarea
            className="vvlm-input"
            value={prompt}
            onChange={e => setPrompt(e.target.value)}
            onCompositionStart={() => { composingRef.current = true }}
            onCompositionEnd={() => { composingRef.current = false }}
            placeholder="Describe the scene... (applies live while running)"
            rows={4}
          />
        </div>

        <div className="vvlm-field">
          <span className="vvlm-label">  {'Interval'}  </span>
          <div className="vvlm-row">
            <input
              className="vvlm-input"
              type="number"
              value={analyzeEvery}
              onChange={e => setAnalyzeEvery(parseInt(e.target.value) || 48)}
              min={1}
              max={120}
            />
            <span className="vvlm-hint" style={{flexShrink:0}}>{'frames'}</span>
          </div>
          <span className="vvlm-hint">{'Analyze every N frames (live)'}</span>
        </div>

        {/* Voice toggle */}
        <button
          className="vvlm-btn"
          style={{
            background: voiceOn ? 'var(--primary)' : 'transparent',
            color: voiceOn ? 'var(--primary-foreground)' : 'var(--muted-foreground)',
            border: voiceOn ? 'none' : '1px solid var(--border)',
            fontSize: 12,
            padding: '7px 0',
          }}
          onClick={() => setVoiceOn(!voiceOn)}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{display:'inline',verticalAlign:'middle',marginRight:6}}>
            {voiceOn
              ? <><polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"/><path d="M15.5 8.5a5 5 0 0 1 0 7"/></>
              : <><polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"/><line x1="22" y1="9" x2="16" y2="15"/><line x1="16" y1="9" x2="22" y2="15"/></>}
          </svg>
          Voice
        </button>

        {/* Loop toggle */}
        <button
          className="vvlm-btn"
          style={{
            background: loopVideo ? 'var(--primary)' : 'transparent',
            color: loopVideo ? 'var(--primary-foreground)' : 'var(--muted-foreground)',
            border: loopVideo ? 'none' : '1px solid var(--border)',
            fontSize: 12,
            padding: '7px 0',
          }}
          onClick={() => setLoopVideo(!loopVideo)}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{display:'inline',verticalAlign:'middle',marginRight:6}}>
            <polyline points="17 1 21 5 17 9"/>
            <path d="M3 11V9a4 4 0 0 1 4-4h14"/>
            <polyline points="7 23 3 19 7 15"/>
            <path d="M21 13v2a4 4 0 0 1-4 4H3"/>
          </svg>
          Loop
        </button>

        <div style={{marginTop: 'auto'}}>
          {isRunning ? (
            <button className="vvlm-btn vvlm-btn-stop" onClick={disconnect}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" style={{display:'inline',verticalAlign:'middle',marginRight:6}}>
                <rect x="5" y="5" width="14" height="14" rx="2"/>
              </svg>
              Stop
            </button>
          ) : (
            <button
              className="vvlm-btn vvlm-btn-start"
              onClick={connect}
              disabled={connecting || !sourceUrl.trim()}
            >
              {connecting ? 'Connecting...' : (
                <>
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" style={{display:'inline',verticalAlign:'middle',marginRight:6}}>
                    <polygon points="5 3 19 12 5 21 5 3"/>
                  </svg>
                  Start
                </>
              )}
            </button>
          )}
        </div>
      </div>

      {/* ===== Center: Video ===== */}
      <div className="vvlm-center">
        {error && <div className="vvlm-error">{error}</div>}
          {info && !error && <div className="vvlm-info">{info}</div>}
        {connecting && <div className="vvlm-connecting">{'Connecting...'}</div>}

        {/* Canvas always mounted — frames draw as they arrive.
            Client-camera mode renders the LOCAL MediaStream at native fps
            instead (the server echo runs at upload cadence, ~5fps). */}
        <canvas ref={canvasRef} className="vvlm-canvas" style={{
          display: hasFrame && !camPreview ? 'block' : 'none'
        }} />
        {camPreview && (
          <video
            ref={camVideoRef}
            autoPlay
            muted
            playsInline
            className="vvlm-canvas"
            style={{display: 'block', width: '100%', height: '100%', objectFit: 'contain', background: '#000'}}
          />
        )}

        {/* Placeholder overlay (hidden once frames arrive) */}
        {!hasFrame && !connecting && (
          <div className="vvlm-placeholder" style={{
            position: 'absolute', inset: 0,
            display: 'flex', alignItems: 'center', justifyContent: 'center'
          }}>
            <div style={{display:'flex',flexDirection:'column',alignItems:'center',gap:8}}>
              <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                <rect x="2" y="4" width="20" height="16" rx="2"/>
                <path d="M10 9l5 3-5 3V9z" fill="currentColor"/>
              </svg>
              <span>{isRunning ? 'Waiting for frames...' : 'Configure on the left, then press Start'}</span>
            </div>
          </div>
        )}
      </div>

      {/* ===== Right: VLM Info ===== */}
      <div className="vvlm-right">
        {/* VLM Timeline — newest at top, scrollable */}
        <div className="vvlm-info-section-vlm">
          <span className="vvlm-info-title">{'VLM Timeline'}</span>
          {vlmHistory.length > 0 ? (
            <div className="vvlm-timeline" style={{flex:1, overflowY:'auto', minHeight:0}}>
              {(() => {
                // Collapse consecutive identical texts: show the LATEST frame/time
                // for a run of repeats so static scenes don't flood the timeline.
                const rev = [...vlmHistory].reverse()
                const collapsed: Array<typeof rev[number] & { repeats?: number }> = []
                for (const e of rev) {
                  const last = collapsed[collapsed.length - 1]
                  if (last && last.text === e.text) {
                    last.frame = e.frame
                    last.timestamp = e.timestamp
                    last.latency_ms = e.latency_ms
                    last.repeats = (last.repeats || 1) + 1
                  } else {
                    collapsed.push({ ...e, repeats: 1 })
                  }
                }
                const fmt = (ts: number) => {
                  const d = new Date(ts)
                  const pad = (n: number) => String(n).padStart(2, '0')
                  return `${pad(d.getMonth()+1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
                }
                return collapsed.map((e, i) => (
                <div key={i} className="vvlm-tl-entry" style={{
                  padding: '5px 0',
                  borderBottom: i < collapsed.length - 1 ? '1px solid rgba(255,255,255,0.06)' : 'none',
                  opacity: i === 0 ? 1 : 0.6,
                  display: 'flex',
                  gap: '8px',
                }}>
                  {e.thumb && (
                    <img src={`data:image/jpeg;base64,${e.thumb}`}
                         style={{maxWidth:56, maxHeight:32, width:'auto', height:'auto', borderRadius:4, objectFit:'contain', flexShrink:0, border:'1px solid rgba(255,255,255,0.1)'}}
                    />
                  )}
                  <div style={{flex:1, minWidth:0}}>
                    <div className="vvlm-vlm-meta">
                      <span>{fmt(e.timestamp)}</span>
                      <span>F#{e.frame}</span>
                      <span>{(e.latency_ms / 1000).toFixed(1)}s</span>
                      {(e.repeats ?? 1) > 1 && <span style={{opacity:0.6}}>×{e.repeats}</span>}
                    </div>
                    <div style={{fontSize:'12px', lineHeight:1.45, marginTop:'2px'}}>{e.text}</div>
                  </div>
                </div>
                ))
              })()}
            </div>
          ) : (
            <span style={{color:'#555',fontSize:12,flex:1}}>
              {isRunning ? 'Waiting...' : 'Idle'}
            </span>
          )}
        </div>

        {/* Runtime Stats */}
        <div className="vvlm-info-section">
          <span className="vvlm-info-title">{'Stats'}</span>
          <div className="vvlm-stat-grid">
            <div className="vvlm-stat">
              <span className="vvlm-stat-val">{stats.fps > 0 ? stats.fps.toFixed(1) : '—'}</span>
              <span className="vvlm-stat-label">FPS</span>
            </div>
            <div className="vvlm-stat">
              <span className="vvlm-stat-val">{stats.totalFrames > 0 ? stats.totalFrames : '—'}</span>
              <span className="vvlm-stat-label">{'Frames'}</span>
            </div>
            <div className="vvlm-stat">
              <span className="vvlm-stat-val">{isRunning ? fmtTime(stats.sessionTime) : '—'}</span>
              <span className="vvlm-stat-label">{'Uptime'}</span>
            </div>
            <div className="vvlm-stat">
              <span className="vvlm-stat-val">{vlm.latencyMs > 0 ? (vlm.latencyMs / 1000).toFixed(1) + 's' : '—'}</span>
              <span className="vvlm-stat-label">{'Infer'}</span>
            </div>
          </div>
        </div>

        {/* Status Bar */}
        <div className="vvlm-status-bar">
          <span className={`vvlm-dot ${isRunning ? 'vvlm-dot-on' : error ? 'vvlm-dot-err' : 'vvlm-dot-off'}`} />
          <span>{isRunning ? 'Running' : error ? 'Error' : 'Standby'}</span>
          {sessionIdRef.current && <span style={{marginLeft:'auto',opacity:.5}}>#{sessionIdRef.current.slice(0,8)}</span>}
        </div>
      </div>
    </div>
  )
})

export default { VideoVlmDisplay }
