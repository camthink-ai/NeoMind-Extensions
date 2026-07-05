/**
 * VoiceAssistantCard — NeoMind Extension Frontend Component
 *
 * Features:
 *   - WebGL orb shader (dark theme) / CSS gradient cloud (light theme)
 *   - WebSocket streaming: mic → VAD → ASR → LLM → TTS → speaker
 *   - 5-state FSM: idle → listening → thinking → speaking → idle
 *   - Barge-in support, live transcripts, latency metrics
 *   - NeoMind CSS variable theming (auto light/dark)
 */

import {
  forwardRef, useEffect, useState, useCallback, useRef, useMemo
} from 'react'

// ─── Types ───────────────────────────────────────────────────────────

export interface ExtensionComponentProps {
  title?: string
  dataSource?: { type: string; [key: string]: any }
  className?: string
  config?: Record<string, any>
  wsUrl?: string
  showTranscripts?: boolean
  showMetrics?: boolean
}

type OrbState = 'idle' | 'listening' | 'thinking' | 'speaking' | 'error'

interface TranscriptEntry {
  id: number
  text: string
  role: 'user' | 'assistant'
}

interface VoiceMetrics {
  asr_ms?: number
  llm_first_sentence_ms?: number
  tts_first_chunk_ms?: number
  total_ms?: number
}

interface WsFrame {
  type: string
  [key: string]: any
}

// ─── Constants ───────────────────────────────────────────────────────

const CSS_ID = 'voice-assistant-styles-v2'
const WORKLET_NAME = 'va-pcm-recorder'

// Barge-in fade-out duration in milliseconds. ~8ms is short enough to feel
// instantaneous (well below the ~30ms frame the audio pipeline operates on)
// but long enough to prevent the click/pop from a hard sample-boundary cut
// when src.stop() yanks scheduled audio mid-waveform.
const BARGE_IN_FADE_OUT_MS = 8

// ─── WebGL Shader (dark theme) ──────────────────────────────────────

const VERT_SHADER = `
precision highp float;
attribute vec2 position;
attribute vec2 uv;
varying vec2 vUv;
void main(){ vUv=uv; gl_Position=vec4(position,0.0,1.0); }
`

const FRAG_SHADER = `
precision highp float;
uniform float iTime;
uniform vec3 iResolution;
uniform float uHue;
uniform float uHover;
uniform float uRot;
uniform float uWaveAmp;
uniform float uIntensity;
uniform vec3 uColor1;
uniform vec3 uColor2;
uniform vec3 uColor3;
uniform vec3 uBgColor;
varying vec2 vUv;
vec3 rgb2yiq(vec3 c){float y=dot(c,vec3(.299,.587,.114));float i=dot(c,vec3(.596,-.274,-.322));float q=dot(c,vec3(.211,-.523,.312));return vec3(y,i,q);}
vec3 yiq2rgb(vec3 c){return vec3(c.x+.956*c.y+.621*c.z,c.x-.272*c.y-.647*c.z,c.x-1.106*c.y+1.703*c.z);}
vec3 adjustHue(vec3 color,float hueDeg){float h=hueDeg*3.14159265/180.0;vec3 yiq=rgb2yiq(color);float cosA=cos(h);float sinA=sin(h);float i2=yiq.y*cosA-yiq.z*sinA;float q2=yiq.y*sinA+yiq.z*cosA;yiq.y=i2;yiq.z=q2;return yiq2rgb(yiq);}
vec3 hash33(vec3 p3){p3=fract(p3*vec3(.1031,.11369,.13787));p3+=dot(p3,p3.yxz+19.19);return -1.0+2.0*fract(vec3(p3.x+p3.y,p3.x+p3.z,p3.y+p3.z)*p3.zyx);}
float snoise3(vec3 p){const float K1=.333333333;const float K2=.166666167;vec3 i=floor(p+(p.x+p.y+p.z)*K1);vec3 d0=p-(i-(i.x+i.y+i.z)*K2);vec3 e=step(vec3(0.0),d0-d0.yzx);vec3 i1=e*(1.0-e.zxy);vec3 i2=1.0-e.zxy*(1.0-e);vec3 d1=d0-(i1-K2);vec3 d2=d0-(i2-K1);vec3 d3=d0-0.5;vec4 h=max(0.6-vec4(dot(d0,d0),dot(d1,d1),dot(d2,d2),dot(d3,d3)),0.0);vec4 n=h*h*h*h*vec4(dot(d0,hash33(i)),dot(d1,hash33(i+i1)),dot(d2,hash33(i+i2)),dot(d3,hash33(i+1.0)));return dot(vec4(31.316),n);}
vec4 extractAlpha(vec3 c){float a=max(max(c.r,c.g),c.b);return vec4(c/(a+1e-5),a);}
const float innerRadius=0.55;
const float noiseScale=0.60;
float light1(float i,float a,float d){return i/(1.0+d*a);}
float light2(float i,float a,float d){return i/(1.0+d*d*a);}
vec4 draw(vec2 uv){
  vec3 c1=adjustHue(uColor1,uHue);vec3 c2=adjustHue(uColor2,uHue);vec3 c3=adjustHue(uColor3,uHue);
  float ang=atan(uv.y,uv.x);float len=length(uv);float invLen=len>0.0?1.0/len:0.0;
  float bgLum=dot(uBgColor,vec3(.299,.587,.114));
  float n0=snoise3(vec3(uv*noiseScale,iTime*0.45))*0.5+0.5;
  float r0=mix(mix(innerRadius,1.0,0.4),mix(innerRadius,1.0,0.65),n0);
  float d0=distance(uv,(r0*invLen)*uv);
  float v0=light1(1.0,9.0,d0);v0*=smoothstep(r0*1.05,r0,len);
  float innerFade=smoothstep(r0*0.75,r0*0.95,len);v0*=mix(innerFade,1.0,bgLum*0.7);
  float cl=cos(ang+iTime*1.6)*0.5+0.5;
  float a2=iTime*-0.8;vec2 pos=vec2(cos(a2),sin(a2))*r0;float d=distance(uv,pos);
  float v1=light2(1.4,5.0,d);v1*=light1(1.0,45.0,d0);
  float v2=smoothstep(1.0,mix(innerRadius,1.0,n0*0.5),len);
  float v3=smoothstep(innerRadius,mix(innerRadius,1.0,0.5),len);
  vec3 colBase=mix(c1,c2,cl);
  vec3 fc=mix(c3,colBase,v0);fc=(fc+v1)*v2*v3;fc=clamp(fc,0.0,1.0);fc*=uIntensity;
  return extractAlpha(fc);
}
vec4 mainImage(vec2 fragCoord){
  vec2 center=iResolution.xy*0.5;float sz=min(iResolution.x,iResolution.y);
  vec2 uv=(fragCoord-center)/sz*2.0;
  float s2=sin(uRot),c2=cos(uRot);uv=vec2(c2*uv.x-s2*uv.y,s2*uv.x+c2*uv.y);
  uv.x+=uHover*uWaveAmp*0.10*sin(uv.y*9.0+iTime);
  uv.y+=uHover*uWaveAmp*0.10*sin(uv.x*9.0+iTime);
  return draw(uv);
}
void main(){vec2 fc=vUv*iResolution.xy;vec4 col=mainImage(fc);gl_FragColor=vec4(col.rgb*col.a,col.a);}
`

// ─── State Config ────────────────────────────────────────────────────

const STATE_CFG: Record<OrbState, {
  hover: number; intensity: number; rotSpeed: number; waveAmp: number
  hueShift: number
}> = {
  idle:      { hover: 0.06, intensity: 0.62, rotSpeed: 0.10, waveAmp: 0.10, hueShift: 0 },
  listening: { hover: 0.45, intensity: 0.92, rotSpeed: 0.25, waveAmp: 0.32, hueShift: -45 },
  thinking:  { hover: 0.30, intensity: 1.08, rotSpeed: 0.58, waveAmp: 0.22, hueShift: 28 },
  speaking:  { hover: 0.88, intensity: 1.12, rotSpeed: 0.42, waveAmp: 0.55, hueShift: -12 },
  error:     { hover: 0.60, intensity: 0.95, rotSpeed: 0.15, waveAmp: 0.45, hueShift: 85 },
}

// Color palettes
const COOL = { c1: [0.42, 0.46, 0.99], c2: [0.35, 0.78, 0.93], c3: [0.10, 0.12, 0.55] }
const BG_DARK = [0.027, 0.035, 0.058]

// ─── OrbRenderer (WebGL) ────────────────────────────────────────────

class OrbRenderer {
  canvas: HTMLCanvasElement
  gl: WebGLRenderingContext | null
  pgm: WebGLProgram | null = null
  u: Record<string, WebGLUniformLocation | null> = {}
  hue = 0
  waveAmp = 0.25
  colors = COOL
  bgColor: number[] = BG_DARK
  targetHover = 0.05
  currentHover = 0.05
  currentRot = 0
  lastTs = 0
  targetIntensity = 0.62
  currentIntensity = 0.62
  targetRotSpeed = 0.10
  currentRotSpeed = 0.10
  currentState: OrbState = 'idle'
  baseWaveAmp = 0.25
  flashTimer = 0
  // Mic RMS level (0..~0.5), exponentially smoothed. Drives the
  // audio-reactive boost in 'listening' state. Decays toward 0 when
  // no fresh RMS samples arrive so the orb doesn't "stick" at a level
  // after the user stops speaking.
  rmsLevel = 0
  rmsTarget = 0
  private rafId = 0
  private onResize: () => void

  constructor(canvas: HTMLCanvasElement) {
    this.canvas = canvas
    this.gl = canvas.getContext('webgl', { alpha: true, premultipliedAlpha: false, antialias: false })
    this.onResize = () => this._resize()
    if (!this.gl) return
    this._build()
    this._resize()
    window.addEventListener('resize', this.onResize)
    // Only start the rAF loop if the program actually linked.
    // If shaders failed (WK WebGL blocklist / lost context), spinning rAF
    // with a null program thrashes WKWebView's RemoteLayerTree compositor
    // ("scheduleDisplayLink(): page has no displayID") and can freeze Tauri.
    if (this.pgm) {
      this.rafId = requestAnimationFrame(this._loop.bind(this))
    }
  }

  private _compile(type: number, src: string): WebGLShader | null {
    const gl = this.gl!
    // createShader can return null (GPU blocklist, lost context, sandbox).
    // Guard explicitly — passing null to shaderSource throws a TypeError
    // that would crash the entire React subtree via ErrorBoundary.
    const s = gl.createShader(type)
    if (!s) {
      console.warn('VA: createShader returned null — WebGL may be unavailable')
      return null
    }
    gl.shaderSource(s, src)
    gl.compileShader(s)
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) {
      console.error('VA shader:', gl.getShaderInfoLog(s))
      gl.deleteShader(s)
      return null
    }
    return s
  }

  private _build() {
    const gl = this.gl!
    const vs = this._compile(gl.VERTEX_SHADER, VERT_SHADER)
    const fs = this._compile(gl.FRAGMENT_SHADER, FRAG_SHADER)
    if (!vs || !fs) return
    this.pgm = gl.createProgram()!
    gl.attachShader(this.pgm, vs)
    gl.attachShader(this.pgm, fs)
    gl.linkProgram(this.pgm)
    if (!gl.getProgramParameter(this.pgm, gl.LINK_STATUS)) return
    gl.useProgram(this.pgm)

    const posLoc = gl.getAttribLocation(this.pgm, 'position')
    const uvLoc = gl.getAttribLocation(this.pgm, 'uv')
    const posBuf = gl.createBuffer()!
    gl.bindBuffer(gl.ARRAY_BUFFER, posBuf)
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW)
    gl.enableVertexAttribArray(posLoc)
    gl.vertexAttribPointer(posLoc, 2, gl.FLOAT, false, 0, 0)
    const uvBuf = gl.createBuffer()!
    gl.bindBuffer(gl.ARRAY_BUFFER, uvBuf)
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([0, 0, 2, 0, 0, 2]), gl.STATIC_DRAW)
    gl.enableVertexAttribArray(uvLoc)
    gl.vertexAttribPointer(uvLoc, 2, gl.FLOAT, false, 0, 0)

    const names = ['iTime', 'iResolution', 'uHue', 'uHover', 'uRot', 'uWaveAmp',
      'uIntensity', 'uColor1', 'uColor2', 'uColor3', 'uBgColor']
    const pgm = this.pgm as WebGLProgram
    names.forEach(n => { this.u[n] = gl.getUniformLocation(pgm, n) })

    gl.enable(gl.BLEND)
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA)
    gl.clearColor(0, 0, 0, 0)
  }

  private _resize() {
    const dpr = Math.min(window.devicePixelRatio || 1, 2)
    const w = this.canvas.clientWidth
    const h = this.canvas.clientHeight
    this.canvas.width = w * dpr
    this.canvas.height = h * dpr
    if (this.gl) this.gl.viewport(0, 0, this.canvas.width, this.canvas.height)
  }

  private _loop(ts: number) {
    // Bail BEFORE scheduling the next frame — otherwise a null pgm spins
    // rAF forever doing no rendering, thrashing the WK GPU compositor.
    if (!this.pgm || !this.gl) return
    this.rafId = requestAnimationFrame(this._loop.bind(this))
    const gl = this.gl
    const t = ts * 0.001
    const dt = this.lastTs ? Math.min(t - this.lastTs, 0.05) : 0.016
    this.lastTs = t

    this.currentHover += (this.targetHover - this.currentHover) * Math.min(dt * 3.5, 1)
    this.currentIntensity += (this.targetIntensity - this.currentIntensity) * Math.min(dt * 3, 1)
    this.currentRotSpeed += (this.targetRotSpeed - this.currentRotSpeed) * Math.min(dt * 2, 1)
    this.currentRot += dt * this.currentRotSpeed

    // Audio-reactive modulation
    let waveAmp = this.baseWaveAmp
    let intensityBoost = 0
    if (this.currentState === 'speaking') {
      const pulse = 0.5 + 0.5 * Math.sin(t * 7.5) * Math.sin(t * 2.3)
      waveAmp = this.baseWaveAmp * (0.55 + 0.7 * pulse)
      intensityBoost = 0.12 * pulse
    } else if (this.currentState === 'listening') {
      // RMS-driven modulation: when the user actually speaks, the orb
      // intensifies and deforms; when silent, it stays at a calm
      // baseline (a tiny breathing sine keeps it from looking dead).
      //
      // rmsTarget ~0.0 = silence, ~0.05 = quiet speech, ~0.15+ = loud.
      // We normalize by 0.18 (loud speech RMS at 16kHz mono) and clamp
      // to [0,1.2] so unexpected spikes don't blow out the shader.
      const norm = Math.max(0, Math.min(1.2, this.rmsLevel / 0.18))
      // Slow-decay smoothing so the orb doesn't twitch on every glottal
      // closure — fast attack (toward target) + slow release.
      const attack = Math.min(dt * 18, 1)
      const release = Math.min(dt * 4, 1)
      const a = this.rmsTarget > this.rmsLevel ? attack : release
      this.rmsLevel += (this.rmsTarget - this.rmsLevel) * a
      // Tiny baseline pulse so silence isn't dead.
      const breathe = 0.5 + 0.5 * Math.sin(t * 2.5)
      const drive = 0.4 + 1.6 * norm + 0.1 * breathe * (1 - norm)
      waveAmp = this.baseWaveAmp * drive
      intensityBoost = 0.05 * breathe * (1 - norm) + 0.35 * norm
    } else if (this.currentState === 'thinking') {
      intensityBoost = 0.08 * (0.5 + 0.5 * Math.sin(t * 5.0))
    } else if (this.currentState === 'error') {
      intensityBoost = 0.15 * (0.5 + 0.5 * Math.sin(t * 12.0))
    }
    if (this.flashTimer > 0) {
      this.flashTimer -= dt
      const f = Math.max(0, this.flashTimer / 0.35)
      intensityBoost += 0.25 * f * f
    }
    this.waveAmp = waveAmp
    const finalIntensity = this.currentIntensity + intensityBoost

    gl!.clear(gl!.COLOR_BUFFER_BIT)
    gl!.useProgram(this.pgm)
    gl!.uniform1f(this.u.iTime, t)
    gl!.uniform3f(this.u.iResolution, this.canvas.width, this.canvas.height,
      this.canvas.width / this.canvas.height)
    gl!.uniform1f(this.u.uHue, this.hue)
    gl!.uniform1f(this.u.uHover, this.currentHover)
    gl!.uniform1f(this.u.uRot, this.currentRot)
    gl!.uniform1f(this.u.uWaveAmp, this.waveAmp)
    gl!.uniform1f(this.u.uIntensity, finalIntensity)
    gl!.uniform3f(this.u.uColor1, this.colors.c1[0], this.colors.c1[1], this.colors.c1[2])
    gl!.uniform3f(this.u.uColor2, this.colors.c2[0], this.colors.c2[1], this.colors.c2[2])
    gl!.uniform3f(this.u.uColor3, this.colors.c3[0], this.colors.c3[1], this.colors.c3[2])
    gl!.uniform3f(this.u.uBgColor, this.bgColor[0], this.bgColor[1], this.bgColor[2])
    gl!.drawArrays(gl!.TRIANGLES, 0, 3)
  }

  setState(state: OrbState) {
    const cfg = STATE_CFG[state] || STATE_CFG.idle
    this.currentState = state
    this.targetHover = cfg.hover
    this.targetIntensity = cfg.intensity
    this.targetRotSpeed = cfg.rotSpeed
    this.baseWaveAmp = cfg.waveAmp
    this.hue = cfg.hueShift
    this.flashTimer = 0.35
    // Reset mic-driven level on state change so the orb doesn't enter
    // 'thinking'/'speaking' still glowing from the user's last word.
    this.rmsTarget = 0
    this.rmsLevel = 0
  }

  /// Push a fresh RMS level (0..~0.5) from the AudioWorklet. The actual
  /// smoothing happens in `_loop` so callers can just fire-and-forget.
  setLevel(rms: number) {
    // Clamp negatives (shouldn't happen, but a bad worklet build could
    // emit NaNs and we don't want NaN to leak into the shader uniforms).
    this.rmsTarget = isFinite(rms) && rms > 0 ? Math.min(rms, 0.5) : 0
  }

  destroy() {
    cancelAnimationFrame(this.rafId)
    window.removeEventListener('resize', this.onResize)
    if (this.gl) {
      const ext = this.gl.getExtension('WEBGL_lose_context')
      if (ext) ext.loseContext()
    }
  }
}

// ─── Theme Detection ────────────────────────────────────────────────

function detectTheme(): 'light' | 'dark' {
  try {
    const bg = getComputedStyle(document.documentElement)
      .getPropertyValue('--background').trim()
    if (bg) {
      const m = bg.match(/#([0-9a-f]{3,8})/i)
      if (m) {
        const hex = m[1].length === 3
          ? m[1].split('').map(c => c + c).join('')
          : m[1].substring(0, 6)
        const r = parseInt(hex.substring(0, 2), 16)
        const g = parseInt(hex.substring(2, 4), 16)
        const b = parseInt(hex.substring(4, 6), 16)
        const lum = (0.299 * r + 0.587 * g + 0.114 * b) / 255
        return lum > 0.5 ? 'light' : 'dark'
      }
    }
  } catch {}
  return document.documentElement.classList.contains('dark') ? 'dark' : 'light'
}

function useTheme(): 'light' | 'dark' {
  const [theme, setTheme] = useState<'light' | 'dark'>(detectTheme)
  useEffect(() => {
    const observer = new MutationObserver(() => setTheme(detectTheme()))
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['class', 'data-theme']
    })
    return () => observer.disconnect()
  }, [])
  return theme
}

// ─── Style Injection ────────────────────────────────────────────────

function injectStyles() {
  if (document.getElementById(CSS_ID)) return
  const style = document.createElement('style')
  style.id = CSS_ID
  style.textContent = `
/* @property for smooth cloud color transitions */
@property --va-cloud-1 { syntax: '<color>'; inherits: true; initial-value: #b8c4ff; }
@property --va-cloud-2 { syntax: '<color>'; inherits: true; initial-value: #c4dcff; }
@property --va-cloud-3 { syntax: '<color>'; inherits: true; initial-value: #d4c0ff; }
@property --va-cloud-blur { syntax: '<length>'; inherits: true; initial-value: 22px; }
@property --va-cloud-scale { syntax: '<number>'; inherits: true; initial-value: 1; }

.va-root {
  --va-fg: var(--foreground, #e8eaf0);
  --va-fg-dim: var(--muted-foreground, #c0c4d2);
  --va-muted: var(--muted-foreground, #6c7388);
  --va-accent: var(--primary, #8b9aff);
  --va-on-primary: var(--primary-foreground, #ffffff);
  --va-card: var(--card, rgba(20,24,36,0.72));
  --va-border: var(--border, rgba(255,255,255,0.08));
  --va-success: var(--color-success, #4ade80);
  --va-warning: var(--color-warning, #a78bfa);
  --va-info: var(--color-info, #60a5fa);
  --va-error: var(--color-error, #f87171);
  --va-radius: var(--radius-xl, 12px);
  --va-radius-lg: var(--radius-2xl, 16px);
  --va-gap: var(--space-3, 12px);

  display: flex;
  flex-direction: column;
  height: 100%;
  font-family: -apple-system, BlinkMacSystemFont, "SF Pro Display", "Inter", sans-serif;
  color: var(--va-fg);
}

.va-card-inner {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: var(--va-gap);
  min-height: 0;
  padding: 16px;
  border: 1px solid var(--va-border);
  border-radius: var(--va-radius-lg);
  background: var(--va-card);
  backdrop-filter: blur(8px);
}

/* Header */
.va-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 8px;
  padding: 0 4px;
  /* Allow wrap on narrow cards so metrics don't crowd the pill on
     ~280px-wide dashboard tiles. Wrapped metrics drop to a second
     row, still top-aligned. */
  flex-wrap: wrap;
}
.va-header-left {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
}
.va-title {
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 1.5px;
  text-transform: uppercase;
  color: var(--va-muted);
}
.va-pill {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 10px;
  letter-spacing: 1px;
  text-transform: uppercase;
  color: var(--va-muted);
}
.va-pill-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--va-muted);
  transition: background 0.3s;
}
.va-pill-dot.listening { background: var(--va-success); box-shadow: 0 0 8px var(--va-success); }
.va-pill-dot.thinking  { background: var(--va-warning); box-shadow: 0 0 8px var(--va-warning); animation: va-blink 1s infinite; }
.va-pill-dot.speaking  { background: var(--va-info); box-shadow: 0 0 8px var(--va-info); animation: va-blink 0.6s infinite; }
.va-pill-dot.error     { background: var(--va-error); box-shadow: 0 0 8px var(--va-error); }
@keyframes va-blink { 0%,100%{opacity:1} 50%{opacity:0.3} }

/* Orb */
.va-orb-wrap {
  position: relative;
  width: 100%;
  aspect-ratio: 1;
  max-width: 200px;
  margin: 4px auto;
  display: flex;
  align-items: center;
  justify-content: center;
}
.va-orb-wrap canvas {
  position: relative;
  z-index: 1;
  width: 100%;
  height: 100%;
  display: block;
}
.va-orb-wrap::before {
  content: "";
  position: absolute;
  inset: -15%;
  border-radius: 50%;
  background: radial-gradient(circle at 50% 50%,
    rgba(120,140,255,0.18) 0%,
    rgba(120,140,255,0.08) 35%,
    rgba(120,140,255,0) 70%);
  pointer-events: none;
  z-index: 0;
}

/* Light theme: CSS cloud (hide canvas, show cloud) */
.va-root[data-theme="light"] .va-orb-wrap canvas { display: none; }
.va-root[data-theme="light"] .va-orb-wrap::before { content: none; }
.va-root[data-theme="light"] .va-orb-wrap::after {
  content: "";
  position: absolute;
  inset: 6%;
  border-radius: 50%;
  filter: blur(var(--va-cloud-blur));
  z-index: 1;
  transform: scale(var(--va-cloud-scale));
  background:
    radial-gradient(circle at 35% 35%, var(--va-cloud-1) 0%, transparent 55%),
    radial-gradient(circle at 65% 60%, var(--va-cloud-2) 0%, transparent 55%),
    radial-gradient(circle at 50% 50%, var(--va-cloud-3) 0%, transparent 70%);
  transition:
    --va-cloud-1 0.7s ease, --va-cloud-2 0.7s ease, --va-cloud-3 0.7s ease,
    --va-cloud-blur 0.5s ease,
    --va-cloud-scale 0.5s cubic-bezier(0.34,1.56,0.64,1);
  animation: va-breathe var(--va-breathe-dur, 6s) ease-in-out infinite;
}

/* Per-state cloud palettes + params (light) */
.va-root[data-theme="light"][data-state="idle"] .va-orb-wrap {
  --va-cloud-1: #aabaff; --va-cloud-2: #88ccff; --va-cloud-3: #d4c8ff;
  --va-cloud-blur: 26px; --va-cloud-scale: 0.95; --va-breathe-dur: 7s; }
.va-root[data-theme="light"][data-state="listening"] .va-orb-wrap {
  --va-cloud-1: #a0e8b0; --va-cloud-2: #80d8c0; --va-cloud-3: #c8f0c0;
  --va-cloud-blur: 20px; --va-cloud-scale: 1.0; --va-breathe-dur: 2.8s; }
.va-root[data-theme="light"][data-state="thinking"] .va-orb-wrap {
  --va-cloud-1: #c4a0ff; --va-cloud-2: #b088ff; --va-cloud-3: #e0c0ff;
  --va-cloud-blur: 18px; --va-cloud-scale: 1.04; --va-breathe-dur: 1.8s; }
.va-root[data-theme="light"][data-state="speaking"] .va-orb-wrap {
  --va-cloud-1: #80b0ff; --va-cloud-2: #60a0ff; --va-cloud-3: #a0d0ff;
  --va-cloud-blur: 14px; --va-cloud-scale: 1.1; --va-breathe-dur: 1.2s; }
.va-root[data-theme="light"][data-state="error"] .va-orb-wrap {
  --va-cloud-1: #ff9a8a; --va-cloud-2: #ff8888; --va-cloud-3: #ffb0a0;
  --va-cloud-blur: 18px; --va-cloud-scale: 1.02; --va-breathe-dur: 0.9s; }

@keyframes va-breathe {
  0%,100% { transform: scale(var(--va-cloud-scale)) rotate(0deg); }
  25%     { transform: scale(calc(var(--va-cloud-scale) * 1.05)) rotate(2deg); }
  50%     { transform: scale(calc(var(--va-cloud-scale) * 0.98)) rotate(0deg); }
  75%     { transform: scale(calc(var(--va-cloud-scale) * 1.05)) rotate(-2deg); }
}
.va-root[data-state="speaking"] .va-orb-wrap::after {
  animation: va-breathe 1.2s ease-in-out infinite, va-pulse 0.45s ease-in-out infinite alternate;
}
@keyframes va-pulse {
  from { filter: blur(var(--va-cloud-blur)) brightness(1.0); }
  to   { filter: blur(calc(var(--va-cloud-blur) - 3px)) brightness(1.15); }
}

/* Subtitle */
.va-subtitle {
  font-size: 14px;
  font-weight: 500;
  text-align: center;
  color: var(--va-fg);
  min-height: 20px;
  transition: opacity 0.2s;
}
.va-subtitle.fading { opacity: 0; }
.va-meta {
  font-size: 11px;
  text-align: center;
  color: var(--va-muted);
  min-height: 14px;
  letter-spacing: 0.4px;
}

/* Mic button */
.va-mic-btn {
  width: 48px;
  height: 48px;
  border-radius: 50%;
  border: 1px solid var(--va-border);
  background: var(--va-card);
  color: var(--va-fg-dim);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  margin: 0 auto;
  transition: all 0.2s;
}
.va-mic-btn:hover {
  transform: translateY(-1px);
  color: var(--va-fg);
}
.va-mic-btn.active {
  background: var(--va-accent);
  color: var(--va-on-primary);
  border-color: var(--va-accent);
  box-shadow: 0 0 16px color-mix(in srgb, var(--va-accent) 40%, transparent);
}

/* Transcripts — only the latest turn is rendered now (last user +
   last assistant), so this block no longer needs flex:1. Reserve a
   small footprint; if a long assistant answer wraps, it expands
   naturally up to a soft cap before the card itself grows. */
.va-transcripts {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 4px 4px 0;
  max-height: 35%;
  overflow-y: auto;
}
.va-transcripts::-webkit-scrollbar { width: 4px; }
.va-transcripts::-webkit-scrollbar-thumb {
  background: var(--va-border);
  border-radius: 2px;
}
.va-msg {
  font-size: 13px;
  line-height: 1.4;
  padding: 6px 12px;
  border-radius: var(--va-radius);
  max-width: 85%;
  word-break: break-word;
}
.va-msg.user {
  align-self: flex-end;
  background: var(--va-accent);
  color: var(--va-on-primary);
}
.va-msg.assistant {
  align-self: flex-start;
  background: color-mix(in srgb, var(--va-fg) 8%, transparent);
  color: var(--va-fg-dim);
}
/* Sentence lines within a single assistant message block. Tight margin
   so multi-sentence replies read as one cohesive answer. */
.va-msg.assistant .va-msg-line {
  margin: 0;
}
.va-msg.assistant .va-msg-line + .va-msg-line {
  margin-top: 4px;
}

/* Metrics — inline next to the state pill in the header. Compact
   spacing, smaller font than the old bottom-row variant. */
.va-metrics {
  display: flex;
  gap: 8px;
  align-items: center;
  font-size: 10px;
  color: var(--va-muted);
  letter-spacing: 0.3px;
  flex-wrap: wrap;
  justify-content: flex-end;
}
.va-metric {
  white-space: nowrap;
}
.va-metric strong {
  color: var(--va-fg-dim);
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}

.va-error {
  text-align: center;
  font-size: 12px;
  color: var(--va-error);
  padding: 4px;
}
  `
  document.head.appendChild(style)
}

// ─── AudioWorklet Code ──────────────────────────────────────────────

const WORKLET_CODE = `
class PcmRecorderProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this._frameSize = 3200;   // 200ms @ 16kHz
    this._buf = new Float32Array(this._frameSize);
    this._off = 0;
    this._ratio = sampleRate / 16000;  // input→16k downsample ratio (e.g. 3 at 48kHz)
    this._rmsBlocksPerSec = 30;        // emit ~30 level updates/sec when active
    this._rmsBlock = 0;
    // Online RMS via exponential smoothing over downsampled samples.
    // alpha chosen so a 200ms frame at 16kHz ≈ 3200 samples yields a
    // stable-but-responsive estimate (≈ 80ms time constant).
    this._rmsAlpha = 0.04;
    this._rmsSq = 0.0;
    this._rmsActive = false;
  }
  process(inputs) {
    const input = inputs[0];
    if (!input || !input[0]) return true;
    const ch = input[0];
    let frameRms = 0.0;
    // Step through input by _ratio, picking samples to downsample to 16kHz.
    for (let i = 0; i < ch.length; i += this._ratio) {
      const s = ch[i | 0];
      this._buf[this._off++] = s;
      // Exponential smoothing of squared amplitude.
      this._rmsSq = this._rmsSq * (1 - this._rmsAlpha) + s * s * this._rmsAlpha;
      if (this._off >= this._frameSize) {
        const pcm = new Int16Array(this._frameSize);
        for (let j = 0; j < this._frameSize; j++) {
          let p = Math.max(-1, Math.min(1, this._buf[j]));
          pcm[j] = p < 0 ? p * 0x8000 : p * 0x7FFF;
        }
        // Final RMS for this 200ms frame, derived from the smoothed
        // mean-square estimate (avoid a second pass over _buf).
        frameRms = Math.sqrt(this._rmsSq);
        this.port.postMessage(pcm.buffer);
        // Tag along a level update on the same port. Consumers dispatch
        // by message type: ArrayBuffer = PCM, object = level telemetry.
        this.port.postMessage({ rms: frameRms });
        this._off = 0;
        this._rmsActive = true;
      }
    }
    return true;
  }
}
registerProcessor('${WORKLET_NAME}', PcmRecorderProcessor);
`

// ─── MinimalStreamClient ───────────────────────────────────────────
// Connects to NeoMind's extension stream endpoint
// (/api/extensions/voice-assistant/stream?token=<jwt>) and routes the
// host's push_output frames back into the existing audio/event pipeline.
// Used when config.directMode === false (the default). directMode=true
// preserves the legacy raw-WS-to-Python path as a debug escape hatch.

interface PushOutputFrame {
  type: 'push_output'
  session_id: string
  sequence: number
  data: string  // base64-encoded
  data_type: string  // "audio/pcm" | "application/json" | ...
  timestamp: number
  metadata: any | null
}

class MinimalStreamClient {
  private ws: WebSocket | null = null
  private seq = 0
  onPcm: ((bytes: Uint8Array) => void) | null = null
  onEvent: ((msg: any) => void) | null = null
  onError: ((msg: string) => void) | null = null
  onOpen: (() => void) | null = null
  onClose: (() => void) | null = null

  get ready(): boolean {
    return this.ws?.readyState === WebSocket.OPEN
  }

  async connect(): Promise<void> {
    const helper = (window as any).NeoMindStream
    const url: string | null = helper?.urlFor?.('voice-assistant') ?? null
    if (!url) {
      throw new Error('Stream URL unavailable — host did not expose window.NeoMindStream.urlFor')
    }
    const ws = new WebSocket(url)
    ws.binaryType = 'arraybuffer'
    this.ws = ws

    ws.onopen = () => {
      // Handshake: hello, then init with empty config. The Rust extension's
      // run_session_pump treats init as "start a session" and wires the
      // browser↔Python orchestrator bridge.
      ws.send(JSON.stringify({ type: 'hello' }))
      ws.send(JSON.stringify({ type: 'init', config: {} }))
      this.onOpen?.()
    }

    ws.onmessage = (ev) => {
      // Push mode uses text frames only (push_output JSON with base64 data);
      // binary frames are not expected from server in this direction.
      if (ev.data instanceof ArrayBuffer) return
      let frame: any
      try { frame = JSON.parse(ev.data) } catch { return }
      if (frame.type === 'push_output') {
        this.handlePushOutput(frame as PushOutputFrame)
      } else {
        // capability, session_created, session_closed, error, heartbeat
        this.onEvent?.(frame)
      }
    }

    ws.onerror = () => {
      this.onError?.('Stream connection failed')
    }

    ws.onclose = () => {
      this.onClose?.()
    }
  }

  private handlePushOutput(frame: PushOutputFrame) {
    let bytes: Uint8Array
    try {
      const bin = atob(frame.data)
      bytes = new Uint8Array(bin.length)
      for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i)
    } catch {
      return
    }
    if (frame.data_type === 'audio/pcm') {
      this.onPcm?.(bytes)
    } else if (frame.data_type === 'application/json') {
      try {
        const msg = JSON.parse(new TextDecoder().decode(bytes))
        this.onEvent?.(msg)
      } catch {}
    }
    // other data_types ignored — voice-assistant only emits pcm + json
  }

  sendChunk(pcm: ArrayBuffer): void {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) return
    // 8-byte big-endian u64 sequence + raw PCM. Matches platform
    // parse_binary_frame in neomind-api/handlers/extension_stream.rs.
    this.seq = (this.seq + 1) >>> 0
    const out = new Uint8Array(8 + pcm.byteLength)
    const dv = new DataView(out.buffer)
    dv.setUint32(0, Math.floor(this.seq / 0x100000000), false) // high 32 bits
    dv.setUint32(4, this.seq >>> 0, false)                    // low 32 bits
    out.set(new Uint8Array(pcm), 8)
    this.ws.send(out.buffer)
  }

  close(): void {
    if (this.ws) {
      try { this.ws.close() } catch {}
      this.ws = null
    }
  }
}

// ─── Component ──────────────────────────────────────────────────────

export const VoiceAssistantCard = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function VoiceAssistantCard(props, ref) {
    const {
      title = 'Voice Assistant',
      className = '',
      config,
      wsUrl: propWsUrl,
      showTranscripts = true,
      showMetrics = true
    } = props

    // Orchestrator base URL — accepts either http:// or ws:// scheme and a
    // with-or-without-trailing-/ws path. Derived wsUrl is always canonical.
    // Old default was 'ws://127.0.0.1:9384/ws' (WS only); new default is
    // 'http://127.0.0.1:9384' since we now also POST /config.
    const rawBase = propWsUrl || config?.wsUrl || 'http://127.0.0.1:9384'
    const httpBase = rawBase
      .replace(/^ws(s?):\/\//, 'http$1://')
      .replace(/\/ws\/?$/, '')
      .replace(/\/$/, '')
    const wsUrl = httpBase.replace(/^http/, 'ws') + '/ws'
    const showTx = config?.showTranscripts ?? showTranscripts
    const showMt = config?.showMetrics ?? showMetrics
    const directMode = config?.directMode ?? false
    const theme = useTheme()

    // State
    const [connected, setConnected] = useState(false)
    const [orbState, setOrbState] = useState<OrbState>('idle')
    const [micActive, setMicActive] = useState(false)
    const [transcripts, setTranscripts] = useState<TranscriptEntry[]>([])
    const [subtitle, setSubtitle] = useState('Tap to start')
    const [subtitleFading, setSubtitleFading] = useState(false)
    const [metrics, setMetrics] = useState<VoiceMetrics>({})
    const [error, setError] = useState<string | null>(null)
    const [subtitleBlink, setSubtitleBlink] = useState('')

    // Refs
    const canvasRef = useRef<HTMLCanvasElement>(null)
    const orbRendererRef = useRef<OrbRenderer | null>(null)
    const wsRef = useRef<WebSocket | null>(null)
    const streamClientRef = useRef<MinimalStreamClient | null>(null)
    const audioCtxRef = useRef<AudioContext | null>(null)
    // Master gain inserted between every playback source and ctx.destination.
    // Used to apply a short fade-out (~8ms) on barge_in instead of a hard
    // src.stop() — prevents the audible click/pop from a discontinuity at
    // sample boundaries and makes the interrupt feel more natural.
    const masterGainRef = useRef<GainNode | null>(null)
    const micStreamRef = useRef<MediaStream | null>(null)
    const workletNodeRef = useRef<AudioWorkletNode | null>(null)
    const workletLoadedRef = useRef(false)
    const playbackQueueRef = useRef<AudioBuffer[]>([])
    const nextPlayTimeRef = useRef(0)
    // Active AudioBufferSourceNodes scheduled but not yet finished. Tracked
    // so we can stop them synchronously on barge_in — otherwise the Web
    // Audio API keeps playing up to ~300ms of already-scheduled audio.
    const activeSourcesRef = useRef<Set<AudioBufferSourceNode>>(new Set())
    const txIdRef = useRef(0)
    const stateRef = useRef<OrbState>('idle')
    const subtitleTimerRef = useRef<number>(0)

    // Inject styles once
    useEffect(() => { injectStyles() }, [])

    // Orb renderer lifecycle
    useEffect(() => {
      if (theme !== 'dark' || !canvasRef.current) return
      const renderer = new OrbRenderer(canvasRef.current)
      orbRendererRef.current = renderer
      renderer.setState(stateRef.current)
      return () => {
        renderer.destroy()
        orbRendererRef.current = null
      }
    }, [theme])

    // State transition helper
    const transitionState = useCallback((newState: OrbState) => {
      if (stateRef.current === newState) return
      stateRef.current = newState
      setOrbState(newState)
      orbRendererRef.current?.setState(newState)

      const subtitles: Record<OrbState, string> = {
        idle: 'Tap to start',
        listening: 'Listening…',
        thinking: 'Thinking…',
        speaking: 'Speaking…',
        error: 'Connection issue',
      }
      setSubtitleFading(true)
      window.clearTimeout(subtitleTimerRef.current)
      subtitleTimerRef.current = window.setTimeout(() => {
        setSubtitle(subtitles[newState])
        setSubtitleFading(false)
      }, 180)
    }, [])

    // ─── WebSocket ──────────────────────────────────────────────────

    // Push user-supplied config (profile, token, language, voice) to the
    // orchestrator's /config endpoint before opening a session. Server-side
    // changes apply instantly for language/voice and trigger a backend
    // reload for profile/numThreads. Best-effort: failures fall through to
    // WS connect so the user still gets an error message from the WS path.
    const pushConfig = useCallback(async (): Promise<boolean> => {
      const c = config || {}
      const body: Record<string, any> = {}
      if (c.profile) body.profile = c.profile
      if (c.neoMindToken) body.neoMindToken = c.neoMindToken
      if (c.language) body.language = c.language
      if (c.voice) body.voice = c.voice
      if (Object.keys(body).length === 0) return true
      try {
        const resp = await fetch(httpBase + '/config', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(body),
        })
        if (resp.status === 503) {
          // Reload in progress — wait briefly and retry once.
          await new Promise((r) => setTimeout(r, 500))
          try {
            const r2 = await fetch(httpBase + '/config', {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify(body),
            })
            return r2.ok
          } catch { return false }
        }
        return resp.ok
      } catch {
        // Orchestrator may not be up yet, or /config may be absent in
        // older versions. Proceed to WS connect; user will see WS error.
        return false
      }
    }, [httpBase, config?.profile, config?.neoMindToken, config?.language, config?.voice])

    const connectWs = useCallback(async () => {
      setError(null)
      await pushConfig()

      // ── Direct mode: legacy raw-WS-to-Python (debug escape hatch) ──
      if (directMode) {
        if (wsRef.current?.readyState === WebSocket.OPEN) return
        try {
          const ws = new WebSocket(wsUrl)
          ws.binaryType = 'arraybuffer'
          wsRef.current = ws

          ws.onopen = () => {
            setConnected(true)
            setError(null)
            ws.send(JSON.stringify({
              type: 'start',
              session_id: 'va-' + Date.now() + '-' + Math.random().toString(36).slice(2, 8),
              sample_rate: 16000,
            }))
          }

          ws.onmessage = (ev) => {
            if (ev.data instanceof ArrayBuffer) {
              handleAudioPcm(new Uint8Array(ev.data))
            } else {
              try {
                const msg = JSON.parse(ev.data) as WsFrame
                handleWsEvent(msg)
              } catch {}
            }
          }

          ws.onerror = () => {
            setError('WebSocket connection failed')
            transitionState('error')
          }

          ws.onclose = () => {
            setConnected(false)
            setMicActive(false)
            // See comment in stream-mode onClose above: keep micStreamRef
            // consistent with micActive, otherwise the listening/idle
            // fallback in tts_end/stop lies.
            workletNodeRef.current?.disconnect()
            workletNodeRef.current = null
            micStreamRef.current?.getTracks().forEach(t => t.stop())
            micStreamRef.current = null
            transitionState('idle')
          }
        } catch (e: any) {
          setError(e.message || 'Connection failed')
          transitionState('error')
        }
        return
      }

      // ── Stream mode: host stream endpoint → Rust pump → Python ──
      if (streamClientRef.current?.ready) return
      try {
        const client = new MinimalStreamClient()
        client.onPcm = (bytes) => handleAudioPcm(bytes)
        client.onEvent = (msg) => {
          if (msg && typeof msg === 'object' && typeof msg.type === 'string') {
            handleWsEvent(msg as WsFrame)
          }
        }
        client.onOpen = () => { setConnected(true); setError(null) }
        client.onError = (m) => { setError(m); transitionState('error') }
        client.onClose = () => {
          setConnected(false)
          setMicActive(false)
          // Tear down the mic on WS close so micStreamRef doesn't lie about
          // a live mic. Without this, the tts_end/stop "still listening?"
          // check below would say yes even though the pipeline is gone,
          // and the orb would stick on listening instead of falling back
          // to idle. (Also matches directMode's ws.onclose further down.)
          workletNodeRef.current?.disconnect()
          workletNodeRef.current = null
          micStreamRef.current?.getTracks().forEach(t => t.stop())
          micStreamRef.current = null
          transitionState('idle')
        }
        streamClientRef.current = client
        await client.connect()
      } catch (e: any) {
        setError(e.message || 'Stream connection failed')
        transitionState('error')
      }
    }, [directMode, wsUrl, transitionState, pushConfig])

    // Handle WS events
    const handleWsEvent = useCallback((msg: WsFrame) => {
      switch (msg.type) {
        case 'ready':
          // Connection-ready signal from the backend. Do NOT transition
          // unconditionally to idle — startMic() may have already moved us
          // to 'listening' (when getUserMedia resolves faster than the
          // pushConfig + WS handshake), and forcing idle here would
          // silently revert the orb to "Tap to start" right after the user
          // clicked it. Only initialize state if we're still in the
          // pre-connect idle (the typical first-load case).
          if (stateRef.current === 'idle') transitionState('idle')
          break
        case 'asr_start':
          // VAD endpoint fired — we stopped detecting speech and ASR is now
          // processing. Transition to 'thinking' immediately so the user
          // sees the processing state the moment they finish speaking,
          // instead of waiting ~500ms for the transcript event.
          transitionState('thinking')
          break
        case 'transcript':
          if (msg.text) {
            setTranscripts(prev => [...prev, {
              id: ++txIdRef.current,
              text: msg.text,
              role: 'user'
            }])
          }
          // Clear any partial-transcript subtitle now that the final
          // transcript has been appended to history.
          setSubtitle('')
          transitionState('thinking')
          break
        case 'partial_transcript':
          // Overwrite the subtitle with the live partial. The final
          // transcript frame (above) clears it once transcription finishes.
          setSubtitle(msg.text || '')
          break
        case 'llm_sentence':
        case 'reply_sentence':
          if (msg.text) {
            setTranscripts(prev => [...prev, {
              id: ++txIdRef.current,
              text: msg.text,
              role: 'assistant'
            }])
          }
          break
        case 'greeting':
          if (msg.text) {
            setTranscripts(prev => [...prev, {
              id: ++txIdRef.current,
              text: msg.text,
              role: 'assistant'
            }])
          }
          break
        case 'tts_start':
          transitionState('speaking')
          break
        case 'tts_end':
          if (msg.llm_first_sentence_ms !== undefined ||
              msg.tts_first_chunk_ms !== undefined) {
            setMetrics({
              asr_ms: msg.asr_ms,
              llm_first_sentence_ms: msg.llm_first_sentence_ms,
              tts_first_chunk_ms: msg.tts_first_chunk_ms,
              total_ms: msg.total_ms,
            })
          }
          // Hands-free: when mic is still streaming and the connection is
          // live, show LISTENING (not IDLE) so the orb stays green between
          // turns. The backend VAD keeps detecting speech regardless of UI
          // state; this only fixes the visual cue. Falls back to IDLE when
          // the user has explicitly stopped the mic or the connection
          // dropped. NOTE: must check BOTH wsRef (directMode raw-WS path)
          // AND streamClientRef (default stream-mode path) — in stream mode
          // wsRef is never assigned, so checking only wsRef would always
          // fall through to 'idle' there.
          transitionState(
            micStreamRef.current &&
            (wsRef.current?.readyState === WebSocket.OPEN ||
             streamClientRef.current?.ready === true)
              ? 'listening'
              : 'idle'
          )
          break
        case 'stop':
          transitionState(
            micStreamRef.current &&
            (wsRef.current?.readyState === WebSocket.OPEN ||
             streamClientRef.current?.ready === true)
              ? 'listening'
              : 'idle'
          )
          break
        case 'barge_in': {
          // Fade out the master gain over BARGE_IN_FADE_OUT_MS before
          // stopping sources. A hard src.stop() yanks scheduled audio at
          // an arbitrary sample boundary, producing an audible click/pop;
          // the short ramp prevents the discontinuity and makes the
          // interrupt feel like a natural trailing-off instead of a hard
          // cut. Falls back to hard stop if the AudioContext or gain is
          // missing (e.g. barge-in arrived before any audio was ever
          // played).
          const ctx = audioCtxRef.current
          const gain = masterGainRef.current
          if (ctx && gain) {
            const now = ctx.currentTime
            const fadeEnd = now + BARGE_IN_FADE_OUT_MS / 1000
            const stopAt = fadeEnd + 0.001 // small epsilon so ramp fully completes
            gain.gain.cancelScheduledValues(now)
            gain.gain.setValueAtTime(gain.gain.value, now)
            gain.gain.linearRampToValueAtTime(0, fadeEnd)
            // Schedule source stops after the fade so any already-playing
            // scheduled audio is silenced; clear the queue so nothing new
            // starts. src.stop(time) throws if the source already ended —
            // swallow.
            for (const src of activeSourcesRef.current) {
              try { src.stop(stopAt) } catch {}
            }
            // Restore gain to 1.0 for the next TTS playback. Scheduled
            // slightly after the stops land so we never get a blip of the
            // tail end of the cancelled audio at full volume.
            gain.gain.setValueAtTime(1.0, stopAt + 0.001)
          } else {
            for (const src of activeSourcesRef.current) {
              try { src.stop() } catch {}
            }
          }
          activeSourcesRef.current.clear()
          playbackQueueRef.current = []
          nextPlayTimeRef.current = 0
          transitionState('listening')
          break
        }
        case 'error':
          setError(msg.message || 'Unknown error')
          transitionState('error')
          break
      }
    }, [transitionState])

    // ─── Audio Playback ─────────────────────────────────────────────

    const handleAudioPcm = useCallback((bytes: Uint8Array) => {
      const ctx = audioCtxRef.current
      if (!ctx) return
      const i16 = new Int16Array(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength))
      const f32 = new Float32Array(i16.length)
      for (let i = 0; i < i16.length; i++) f32[i] = i16[i] / 32768.0
      const buf = ctx.createBuffer(1, f32.length, 16000)
      buf.copyToChannel(f32, 0)
      playbackQueueRef.current.push(buf)
      pumpPlayback()
    }, [])

    const pumpPlayback = useCallback(() => {
      const ctx = audioCtxRef.current
      if (!ctx || playbackQueueRef.current.length === 0) return
      // Lazily create the master gain the first time we actually play. Done
      // here (not at ctx creation) because some hosts (WKWebView) tear down
      // the AudioContext if mic permission is denied — we don't want a
      // dangling gain referring to a dead context.
      if (!masterGainRef.current || masterGainRef.current.context !== ctx) {
        const g = ctx.createGain()
        g.gain.value = 1.0
        g.connect(ctx.destination)
        masterGainRef.current = g
      }
      const gain = masterGainRef.current
      const now = ctx.currentTime
      if (nextPlayTimeRef.current < now) nextPlayTimeRef.current = now + 0.02
      while (playbackQueueRef.current.length > 0 &&
             nextPlayTimeRef.current < now + 0.3) {
        const buf = playbackQueueRef.current.shift()!
        const src = ctx.createBufferSource()
        src.buffer = buf
        src.connect(gain)
        src.start(nextPlayTimeRef.current)
        nextPlayTimeRef.current += buf.duration
        // Track for synchronous stop on barge_in.
        activeSourcesRef.current.add(src)
        // Re-pump on end so queued buffers beyond the 0.3s schedule-ahead
        // window get scheduled. Without this, multi-sentence TTS (where all
        // frames arrive within ~600ms) strands sentence 2+ in the queue
        // forever — only the first sentence ever plays.
        src.onended = () => {
          activeSourcesRef.current.delete(src)
          pumpPlayback()
        }
      }
    }, [])

    // ─── Mic Control ────────────────────────────────────────────────

    const startMic = useCallback(async () => {
      try {
        if (!audioCtxRef.current) {
          audioCtxRef.current = new AudioContext()
        }
        const ctx = audioCtxRef.current
        if (ctx.state === 'suspended') await ctx.resume()

        // Load worklet (only once per AudioContext — WKWebView throws if
        // addModule is called again with an already-registered processor)
        if (!workletLoadedRef.current) {
          const blob = new Blob([WORKLET_CODE], { type: 'application/javascript' })
          const url = URL.createObjectURL(blob)
          await ctx.audioWorklet.addModule(url)
          URL.revokeObjectURL(url)
          workletLoadedRef.current = true
        }

        const stream = await navigator.mediaDevices.getUserMedia({
          audio: {
            echoCancellation: true,
            noiseSuppression: true,
            autoGainControl: true,
          }
        })
        micStreamRef.current = stream
        const src = ctx.createMediaStreamSource(stream)
        const node = new AudioWorkletNode(ctx, WORKLET_NAME)
        node.port.onmessage = (e) => {
          // Worklet emits two message kinds on the same port:
          //   ArrayBuffer → PCM frame, forward to backend unchanged
          //   { rms: n }  → level telemetry, drive the orb's audio-
          //                 reactive visualization (never send to WS)
          if (e.data instanceof ArrayBuffer) {
            if (directMode) {
              if (wsRef.current?.readyState === WebSocket.OPEN) {
                wsRef.current.send(e.data)
              }
            } else {
              streamClientRef.current?.sendChunk(e.data)
            }
          } else if (e.data && typeof e.data === 'object' && typeof e.data.rms === 'number') {
            orbRendererRef.current?.setLevel(e.data.rms)
          }
        }
        src.connect(node)
        workletNodeRef.current = node
        setMicActive(true)
        transitionState('listening')
      } catch (e: any) {
        setError(e.message || 'Microphone access denied')
        transitionState('error')
      }
    }, [transitionState, directMode])

    const stopMic = useCallback(() => {
      workletNodeRef.current?.disconnect()
      workletNodeRef.current = null
      micStreamRef.current?.getTracks().forEach(t => t.stop())
      micStreamRef.current = null
      setMicActive(false)
      // User explicitly stopped the mic. In hands-free mode (the default),
      // clicking the button to stop means "I'm done talking for now" — go
      // straight to idle. The previous push-to-talk behavior (transition to
      // 'thinking' + 5s fallback timer) was misleading: if the user wanted
      // to abort, they'd see 'Thinking…' for 5s; if they actually spoke,
      // the backend's asr_start frame would re-advance the state from idle
      // anyway, so going via 'thinking' buys nothing.
      transitionState('idle')
    }, [transitionState])

    const toggleMic = useCallback(() => {
      if (micActive) {
        stopMic()
      } else {
        if (!connected) {
          // connectWs is async (POSTs /config first); startMic runs on the
          // next click once WS is open.
          connectWs()
        }
        startMic()
      }
    }, [micActive, connected, connectWs, startMic, stopMic])

    // Cleanup
    useEffect(() => {
      return () => {
        stopMic()
        wsRef.current?.close()
        streamClientRef.current?.close()
        audioCtxRef.current?.close()
        window.clearTimeout(subtitleTimerRef.current)
      }
    }, [stopMic])

    // ─── Render ─────────────────────────────────────────────────────

    const pillText: Record<OrbState, string> = {
      idle: 'STANDBY', listening: 'LISTENING', thinking: 'THINKING',
      speaking: 'SPEAKING', error: 'ERROR'
    }

    // Format latency: <1s shows raw ms (rounded, no decimals), ≥1s shows
    // seconds with one decimal. Backend sends raw floats like 1834.27ms
    // (perf_counter delta × 1000) — without rounding the UI shows "1834.27ms"
    // which is both noisy and inaccurate (sub-ms precision is meaningless
    // across an async WS boundary).
    const fmtMs = (ms: number | undefined): string => {
      if (ms === undefined) return ''
      const v = Math.round(ms)
      return v >= 1000 ? `${(v / 1000).toFixed(1)}s` : `${v}ms`
    }

    // Latest turn = last user question + every assistant sentence that
    // arrived AFTER it. The orchestrator streams the assistant reply as
    // one `llm_sentence` frame per sentence, so a 3-sentence answer is 3
    // separate TranscriptEntry records — collapsing to "last assistant"
    // would silently drop the first two. We instead find the last user
    // turn and show every entry with index ≥ that user message.
    //
    // Edge case: a greeting may arrive before any user turn. In that
    // case fall back to showing the last assistant entry so the card
    // isn't blank.
    const { lastUser, lastAssistantLines } = useMemo(() => {
      let userIdx = -1
      for (let i = transcripts.length - 1; i >= 0; i--) {
        if (transcripts[i].role === 'user') { userIdx = i; break }
      }
      if (userIdx === -1) {
        // No user turn yet — show the latest assistant entry (greeting).
        let last: TranscriptEntry | undefined
        for (let i = transcripts.length - 1; i >= 0; i--) {
          if (transcripts[i].role === 'assistant') { last = transcripts[i]; break }
        }
        return { lastUser: undefined, lastAssistantLines: last ? [last] : [] }
      }
      return {
        lastUser: transcripts[userIdx],
        lastAssistantLines: transcripts.slice(userIdx + 1)
          .filter(tx => tx.role === 'assistant'),
      }
    }, [transcripts])

    return (
      <div
        ref={ref}
        className={`va-root ${className}`}
        data-theme={theme}
        data-state={orbState}
      >
        <div className="va-card-inner">
          {/* Header — title + state pill + inline metrics.
              Metrics moved from the bottom so they're always visible
              without scrolling, and the orb stays the visual anchor
              of the lower portion of the card. */}
          <div className="va-header">
            <div className="va-header-left">
              <div className="va-title">{title}</div>
              <div className="va-pill">
                <span className={`va-pill-dot ${orbState}`} />
                <span>{pillText[orbState]}</span>
              </div>
            </div>
            {showMt && metrics.total_ms !== undefined && (
              <div className="va-metrics">
                {metrics.asr_ms !== undefined && (
                  <span className="va-metric">ASR <strong>{fmtMs(metrics.asr_ms)}</strong></span>
                )}
                {metrics.llm_first_sentence_ms !== undefined && (
                  <span className="va-metric">LLM <strong>{fmtMs(metrics.llm_first_sentence_ms)}</strong></span>
                )}
                {metrics.tts_first_chunk_ms !== undefined && (
                  <span className="va-metric">TTS <strong>{fmtMs(metrics.tts_first_chunk_ms)}</strong></span>
                )}
                {metrics.total_ms !== undefined && (
                  <span className="va-metric">Total <strong>{fmtMs(metrics.total_ms)}</strong></span>
                )}
              </div>
            )}
          </div>

          {/* Orb */}
          <div className="va-orb-wrap">
            <canvas ref={canvasRef} />
          </div>

          {/* Subtitle */}
          <div className={`va-subtitle ${subtitleFading ? 'fading' : ''}`}>
            {subtitle}
          </div>
          <div className="va-meta">
            {connected ? 'Connected' : 'Disconnected'}
          </div>

          {/* Mic button */}
          <button
            className={`va-mic-btn ${micActive ? 'active' : ''}`}
            onClick={toggleMic}
            aria-label={micActive ? 'Stop microphone' : 'Start microphone'}
          >
            {micActive ? (
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none"
                stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor" />
              </svg>
            ) : (
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none"
                stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z" />
                <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
                <line x1="12" y1="19" x2="12" y2="23" />
                <line x1="8" y1="23" x2="16" y2="23" />
              </svg>
            )}
          </button>

          {/* Error */}
          {error && <div className="va-error">{error}</div>}

          {/* Latest turn only — last user question + every assistant
              sentence in this turn's reply. Eliminates the long-screen
              scrolling list. The full transcript stays in `transcripts`
              state for future expansion (e.g. click to expand). */}
          {showTx && (lastUser || lastAssistantLines.length > 0) && (
            <div className="va-transcripts">
              {lastUser && (
                <div key={lastUser.id} className="va-msg user">{lastUser.text}</div>
              )}
              {lastAssistantLines.length > 0 && (
                // Group all assistant sentences in this turn into ONE message
                // block. The orchestrator sentence-splits for TTS, so each
                // sentence arrives as a separate transcript entry — but
                // visually they belong together as the assistant's reply.
                <div className="va-msg assistant">
                  {lastAssistantLines.map((tx, i) => (
                    <p key={tx.id} className="va-msg-line">{tx.text}</p>
                  ))}
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    )
  }
)

VoiceAssistantCard.displayName = 'VoiceAssistantCard'

export default { VoiceAssistantCard }
