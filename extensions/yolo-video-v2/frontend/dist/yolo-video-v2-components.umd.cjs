(function(O,o){typeof exports=="object"&&typeof module<"u"?o(exports,require("react/jsx-runtime"),require("react")):typeof define=="function"&&define.amd?define(["exports","react/jsx-runtime","react"],o):(O=typeof globalThis<"u"?globalThis:O||self,o(O.YoloVideoV2Components={},O.jsxRuntime,O.React))})(this,function(O,o,n){"use strict";const Mo="yolo-video-v2",ro="yolo-styles-v2",Do=`
.yolo {
  --yolo-fg: var(--foreground);
  --yolo-muted: var(--muted-foreground);
  --yolo-accent: var(--primary);
  --yolo-success: var(--color-success);
  --yolo-warning: var(--color-warning);
  --yolo-error: var(--color-error, #ef4444);
  --yolo-card: var(--card);
  --yolo-border: var(--border);
  --yolo-on-primary: var(--primary-foreground, #ffffff);
  width: 100%;
  height: 100%;
  font-size: 12px;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
}
.dark .yolo {
  --yolo-on-primary: var(--primary-foreground, #17172a);
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
  gap: 4px;
  flex-wrap: wrap;
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
.yolo-status-dot.yolo-status-error { background: var(--yolo-error); animation: none; }
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
  color: var(--yolo-on-primary);
  background: var(--yolo-accent);
  border: none;
  border-radius: 4px;
  cursor: pointer;
  transition: opacity 0.2s;
}
.yolo-btn:hover { opacity: 0.9; }
.yolo-btn-stop {
  background: var(--yolo-error);
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
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  display: block;
}
.yolo-video-placeholder {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  color: rgba(255,255,255,0.4);
  gap: 8px;
  padding: 20px;
  text-align: center;
  z-index: 2;
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
  z-index: 3;
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
  color: var(--yolo-error);
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

/* Drawing Toolbar */
.yolo-draw-toolbar {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px 10px;
  border-top: 1px solid var(--yolo-border);
  border-bottom: 1px solid var(--yolo-border);
  background: var(--yolo-card);
}
.yolo-draw-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 3px;
  width: 26px;
  height: 26px;
  padding: 0;
  font-size: 10px;
  font-weight: 500;
  color: var(--yolo-muted);
  background: var(--yolo-card);
  border: 1px solid var(--yolo-border);
  border-radius: 4px;
  cursor: pointer;
  transition: all 0.15s;
  white-space: nowrap;
}
.yolo-draw-btn:hover {
  color: var(--yolo-fg);
  border-color: var(--yolo-accent);
}
.yolo-draw-btn.yolo-draw-active {
  color: var(--yolo-on-primary);
  background: var(--yolo-accent);
  border-color: var(--yolo-accent);
}
.yolo-draw-btn.yolo-draw-danger {
  color: var(--yolo-error);
  border-color: rgba(239,68,68,0.3);
}
.yolo-draw-btn.yolo-draw-danger:hover {
  background: var(--yolo-error);
  color: white;
}

/* ROI / Line List */
/* Regions & Lines Panel — grid card layout */
.yolo-regions {
  padding: 6px 8px;
  border-top: 1px solid var(--yolo-border);
  display: flex;
  flex-direction: column;
  gap: 8px;
  max-height: 160px;
  overflow-y: auto;
}
.yolo-regions::-webkit-scrollbar { width: 3px; }
.yolo-regions::-webkit-scrollbar-thumb { background: var(--yolo-border); border-radius: 2px; }

/* Grid for cards */
.yolo-section-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
  gap: 6px;
}

/* Individual card */
.yolo-card {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 1px;
  padding: 3px 5px;
  background: var(--yolo-card);
  border: 1px solid var(--yolo-border);
  border-radius: 6px;
  transition: box-shadow 0.15s;
}
.yolo-card:hover {
  box-shadow: 0 1px 4px rgba(0,0,0,0.06);
}
.yolo-card-row {
  display: flex;
  align-items: center;
  gap: 6px;
}
.yolo-card-name {
  flex: 1;
  font-size: 11px;
  font-weight: 600;
  color: var(--yolo-fg);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  line-height: 1.2;
}
.yolo-card-data {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px;
}
.yolo-card-badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 18px;
  height: 16px;
  padding: 0 4px;
  font-size: 9px;
  font-weight: 700;
  border-radius: 8px;
  line-height: 1;
}
.yolo-card-actions {
  display: inline-flex;
  align-items: center;
  gap: 0;
  margin-left: auto;
  flex-shrink: 0;
}
.yolo-card-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 12px;
  height: 12px;
  padding: 0;
  background: none;
  border: none;
  cursor: pointer;
  color: var(--yolo-muted);
  opacity: 0;
  transition: opacity 0.15s, color 0.15s;
  flex-shrink: 0;
  border-radius: 3px;
}
.yolo-card:hover .yolo-card-btn { opacity: 0.7; }
.yolo-card-btn:hover { opacity: 1 !important; color: var(--yolo-error); background: rgba(0,0,0,0.04); }
.yolo-card-btn-edit:hover { color: var(--yolo-accent) !important; }

/* Rules inside card */
.yolo-card-rules {
  display: flex;
  flex-wrap: wrap;
  gap: 3px;
}
.yolo-rule-pill {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  padding: 0 4px 0 5px;
  font-size: 9px;
  background: rgba(59,130,246,0.08);
  border-radius: 6px;
  color: var(--yolo-muted);
  line-height: 15px;
}
.yolo-rule-pill-btn {
  display: inline-flex;
  align-items: center;
  width: 10px;
  height: 10px;
  background: none;
  border: none;
  cursor: pointer;
  color: var(--yolo-muted);
  padding: 0;
  opacity: 0.6;
}
.yolo-rule-pill-btn:hover { opacity: 1; color: var(--yolo-error); }

/* Line direction chips */
.yolo-line-dir {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  padding: 0 4px;
  font-size: 9px;
  font-weight: 700;
  border-radius: 3px;
  line-height: 14px;
}

/* Captures strip — no title */
.yolo-captures {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 3px 8px;
  border-top: 1px solid var(--yolo-border);
  overflow-x: auto;
  overflow-y: hidden;
  flex-shrink: 0;
  height: 50px;
  min-height: 50px;
  max-height: 50px;
}
.yolo-captures::-webkit-scrollbar { height: 3px; }
.yolo-captures::-webkit-scrollbar-thumb { background: var(--yolo-border); border-radius: 2px; }
.yolo-capture-item {
  position: relative;
  flex-shrink: 0;
  width: 44px;
  height: 44px;
  border-radius: 4px;
  overflow: hidden;
  border: 1px solid var(--yolo-border);
  cursor: pointer;
  transition: opacity 0.15s;
}
.yolo-capture-item:hover { opacity: 0.85; }
.yolo-capture-item img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.yolo-capture-label {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  background: rgba(0,0,0,0.65);
  color: #fff;
  font-size: 7px;
  padding: 1px 3px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* Rule Editor Popup — self-contained, no CSS variable dependency */
.yolo-rule-popup-overlay {
  position: fixed;
  inset: 0;
  z-index: 1000;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0,0,0,0.4);
  backdrop-filter: blur(4px);
  -webkit-backdrop-filter: blur(4px);
}
.yolo-rule-popup {
  background: hsl(0 0% 100%);
  border: 1px solid hsl(0 0% 90%);
  border-radius: 12px;
  padding: 20px;
  min-width: 280px;
  max-width: 340px;
  box-shadow: 0 20px 60px rgba(0,0,0,0.15), 0 0 0 1px rgba(0,0,0,0.05);
  font-size: 13px;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  color: hsl(0 0% 9%);
  animation: yolo-popup-in 0.15s ease-out;
}
.dark .yolo-rule-popup {
  background: hsl(0 0% 14%);
  border-color: hsl(0 0% 22%);
  color: hsl(0 0% 95%);
  box-shadow: 0 20px 60px rgba(0,0,0,0.4), 0 0 0 1px rgba(255,255,255,0.06);
}
@keyframes yolo-popup-in {
  from { opacity: 0; transform: scale(0.95) translateY(4px); }
  to { opacity: 1; transform: scale(1) translateY(0); }
}
.yolo-rule-popup-title {
  font-size: 14px;
  font-weight: 600;
  margin-bottom: 16px;
  padding-bottom: 12px;
  border-bottom: 1px solid hsl(0 0% 90%);
  color: inherit;
}
.dark .yolo-rule-popup-title { border-bottom-color: hsl(0 0% 22%); }
.yolo-rule-field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin-bottom: 12px;
}
.yolo-rule-field > span {
  font-size: 11px;
  font-weight: 500;
  color: hsl(0 0% 45%);
}
.dark .yolo-rule-field > span { color: hsl(0 0% 60%); }
.yolo-rule-field select,
.yolo-rule-field input[type="number"],
.yolo-rule-field input[type="text"] {
  width: 100%;
  font-size: 13px;
  padding: 8px 10px;
  border: 1px solid hsl(0 0% 82%);
  border-radius: 6px;
  background: hsl(0 0% 100%);
  color: hsl(0 0% 9%);
  outline: none;
  min-height: 36px;
  box-sizing: border-box;
  font-family: inherit;
  transition: border-color 0.15s;
  appearance: auto;
}
.dark .yolo-rule-field select,
.dark .yolo-rule-field input {
  background: hsl(0 0% 18%);
  border-color: hsl(0 0% 28%);
  color: hsl(0 0% 95%);
}
.yolo-rule-field select:focus,
.yolo-rule-field input:focus {
  border-color: hsl(221 83% 53%);
  box-shadow: 0 0 0 3px rgba(59,130,246,0.12);
}
.yolo-rule-popup-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 16px;
  padding-top: 12px;
  border-top: 1px solid hsl(0 0% 90%);
}
.dark .yolo-rule-popup-actions { border-top-color: hsl(0 0% 22%); }
.yolo-rule-popup-cancel {
  padding: 7px 16px;
  font-size: 13px;
  font-weight: 500;
  border: 1px solid hsl(0 0% 82%);
  border-radius: 6px;
  background: hsl(0 0% 100%);
  color: hsl(0 0% 40%);
  cursor: pointer;
  font-family: inherit;
  transition: background 0.15s;
}
.dark .yolo-rule-popup-cancel {
  background: hsl(0 0% 18%);
  border-color: hsl(0 0% 28%);
  color: hsl(0 0% 65%);
}
.yolo-rule-popup-cancel:hover { background: hsl(0 0% 96%); }
.dark .yolo-rule-popup-cancel:hover { background: hsl(0 0% 22%); }
.yolo-rule-popup-save {
  padding: 7px 16px;
  font-size: 13px;
  font-weight: 500;
  border: none;
  border-radius: 6px;
  background: hsl(221 83% 53%);
  color: var(--yolo-on-primary);
  cursor: pointer;
  font-family: inherit;
  transition: opacity 0.15s;
}
.yolo-rule-popup-save:hover { opacity: 0.9; }

`;function zo(){if(typeof document>"u"||document.getElementById(ro))return;const d=document.createElement("style");d.id=ro,d.textContent=Do,document.head.appendChild(d)}const lo={video:'<path d="M23 7l-7 5 7 5V7z"/><rect x="1" y="5" width="15" height="14" rx="2" ry="2"/>',play:'<polygon points="5 3 19 12 5 21 5 3"/>',stop:'<rect x="3" y="3" width="18" height="18" rx="2" ry="2"/>',camera:'<path d="M23 19a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4l2-3h6l2 3h4a2 2 0 0 1 2 2z"/><circle cx="12" cy="13" r="4"/>',activity:'<polyline points="22 12 18 12 15 21 9 3 6 12 2 12"/>',clock:'<circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/>',eye:'<path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/>',layers:'<polygon points="12 2 2 7 12 12 22 7 12 2"/><polyline points="2 17 12 22 22 17"/><polyline points="2 12 12 17 22 12"/>',alert:'<circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/>',polygon:'<polygon points="12 2 22 8.5 18 20 6 20 2 8.5 12 2"/>',line:'<line x1="4" y1="20" x2="20" y2="4"/><polyline points="16 4 20 4 20 8"/>',trash:'<polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>',arrowRight:'<line x1="5" y1="12" x2="19" y2="12"/><polyline points="12 5 19 12 12 19"/>',arrowLeft:'<line x1="19" y1="12" x2="5" y2="12"/><polyline points="12 5 5 12 12 19"/>',zap:'<polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/>',plus:'<line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/>',edit:'<path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/>',x:'<line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>'},b=({name:d,className:m="",style:N})=>o.jsx("svg",{viewBox:"0 0 24 24",fill:"none",stroke:"currentColor",strokeWidth:"2",strokeLinecap:"round",strokeLinejoin:"round",className:m,style:N,dangerouslySetInnerHTML:{__html:lo[d]||lo.video}}),ao=[[38,70,83],[40,116,74],[117,79,12],[115,53,88],[192,41,66],[11,121,175],[232,168,124],[211,212,211],[232,212,77],[32,169,199],[57,94,121],[237,139,0],[133,160,131],[174,30,70],[255,183,59],[197,198,53],[166,207,213],[136,86,82],[119,104,174],[51,159,160],[166,59,111],[197,166,137],[108,118,135],[38,131,116],[233,126,67],[255,179,71],[48,96,106],[197,104,80],[227,105,145],[229,193,175]];function Po(d){const[m,N,M]=ao[d%ao.length],F=(.299*m+.587*N+.114*M)/255;return{bg:`rgba(${m}, ${N}, ${M}, 0.85)`,fg:F>.5?"#000":"#fff",border:`rgb(${m}, ${N}, ${M})`}}const Ne=["#3b82f6","#22c55e","#f59e0b","#ef4444","#8b5cf6","#ec4899","#06b6d4","#f97316"];let jo=0;function Ye(){return`r${Date.now().toString(36)}_${++jo}`}function K(d,m){const N=parseInt(d.slice(1,3),16),M=parseInt(d.slice(3,5),16),F=parseInt(d.slice(5,7),16);return`rgba(${N}, ${M}, ${F}, ${m})`}const de=n.forwardRef(function({title:m="YOLO Detection",dataSource:N,className:M="",confidenceThreshold:F=.5,maxObjects:A=20,sourceUrl:S="camera://0",fps:X=15,drawBoxes:Q=!0,showStats:L=!0,variant:Z="default"},R){n.useEffect(()=>{zo()},[]);const[u,ee]=n.useState(!1),[oe,D]=n.useState(null),[te,ne]=n.useState(0),[g,Te]=n.useState(0),[Ho,_e]=n.useState(0),[Be,pe]=n.useState([]),[Vo,Le]=n.useState(null),[et,He]=n.useState("pending"),[$e,re]=n.useState("idle"),[T,ye]=n.useState("none"),[$,Ve]=n.useState([]),[W,Xe]=n.useState([]),[le,ue]=n.useState([]),[ae,he]=n.useState([]),[w,Oe]=n.useState([]),[j,We]=n.useState(null),[Y,qe]=n.useState(null),[Ie,yo]=n.useState([]),[uo,Je]=n.useState([]),[Me,fe]=n.useState(null),[ho,fo]=n.useState(null),B=S.startsWith("rtsp://")||S.startsWith("rtmp://")||S.startsWith("hls://")||S.includes(".m3u8")||S.startsWith("http://")||S.startsWith("https://")||S.startsWith("file://")?"network":"camera",Ue=n.useRef($);Ue.current=$;const Ge=n.useRef(W);Ge.current=W;const go=n.useRef(Ie);go.current=Ie;const q=n.useRef(null),bo=n.useRef(null),De=n.useRef(null),H=n.useRef(null),ze=n.useRef(!1),J=n.useRef(null),U=n.useRef(null),se=n.useRef({frames:0,lastTime:Date.now()}),Xo=n.useRef(0),G=n.useRef(null),ge=n.useRef(!1),ie=n.useRef(!1),Ke=n.useRef(0),V=n.useRef(null),Qe=n.useRef(null),xo=n.useRef(null),Ze=n.useRef(null),be=n.useRef(null),Re=n.useRef(0),ce=n.useRef(null),Pe=(N==null?void 0:N.extensionId)||Mo,mo=n.useCallback(()=>{const t=!!window.__TAURI_INTERNALS__,e=!t&&window.location.protocol==="https:"?"wss:":"ws:",r=t?"localhost:9375":window.location.host,i=`${e}//${r}/api/extensions/${Pe}/stream`,s=localStorage.getItem("neomind_token")||sessionStorage.getItem("neomind_token_session");return s?`${i}?token=${encodeURIComponent(s)}`:i},[Pe]),vo=n.useCallback(()=>{const t=!!window.__TAURI_INTERNALS__,e=t?"http:":window.location.protocol==="https:"?"https:":"http:",r=t?"localhost:9375":window.location.host;return`${e}//${r}`},[]),wo=n.useCallback(async()=>{const t=G.current;if(t)try{await fetch(`${vo()}/api/extensions/${Pe}/command`,{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({command:"update_stream_config",args:{stream_id:t,rois:Ue.current,lines:Ge.current,capture_rules:go.current}})})}catch(e){console.warn("[YOLO] Config update failed:",e)}},[vo,Pe]),z=n.useCallback(()=>{ce.current&&clearTimeout(ce.current),ce.current=setTimeout(()=>{wo(),ce.current=null},150)},[wo]),ko=n.useCallback(()=>{if(!ge.current||ie.current)return;const t=Date.now();if(t-Ke.current<50)return;const e=q.current,r=bo.current;if(!e||!r||e.paused||e.ended)return;const i=r.getContext("2d");if(!i)return;i.drawImage(e,0,0,r.width,r.height);const s=H.current,c=G.current;(s==null?void 0:s.readyState)===WebSocket.OPEN&&c&&(ie.current=!0,Ke.current=t,V.current&&clearTimeout(V.current),V.current=setTimeout(()=>{ie.current&&(console.warn("[YOLO] Frame lock timeout, auto-releasing"),ie.current=!1)},200),r.toBlob(p=>{var C;V.current&&(clearTimeout(V.current),V.current=null),ie.current=!1,ge.current&&p&&((C=H.current)==null?void 0:C.readyState)===WebSocket.OPEN&&G.current&&p.arrayBuffer().then(a=>{var _;const x=Xo.current++,h=new ArrayBuffer(8);new DataView(h).setBigUint64(0,BigInt(x),!1);const k=new Uint8Array(8+a.byteLength);k.set(new Uint8Array(h),0),k.set(new Uint8Array(a),8),(_=H.current)==null||_.send(k)}).catch(a=>{console.warn("[YOLO] Failed to send frame:",a)})},"image/jpeg",.8))},[]),So=n.useCallback(async()=>{try{He("pending");const t=await navigator.mediaDevices.getUserMedia({video:{width:{ideal:640},height:{ideal:480},facingMode:"user"},audio:!1});return He("granted"),De.current=t,q.current&&(q.current.srcObject=t,await q.current.play()),!0}catch(t){return He("denied"),t instanceof Error&&(t.name==="NotAllowedError"?D("Camera permission denied"):t.name==="NotFoundError"?D("No camera found"):D(`Camera error: ${t.message}`)),!1}},[]),Co=n.useCallback(()=>{ge.current=!1,ie.current=!1,Ke.current=0,V.current&&(clearTimeout(V.current),V.current=null),De.current&&(De.current.getTracks().forEach(t=>t.stop()),De.current=null),q.current&&(q.current.srcObject=null)},[]),No=n.useCallback(()=>{const t=mo(),e=new WebSocket(t);e.binaryType="arraybuffer",e.onopen=()=>{const r={type:"init",config:{source_url:S,confidence_threshold:F,max_objects:A,target_fps:X,draw_boxes:Q,rois:Ue.current,lines:Ge.current}};console.log("[YOLO] Sending init:",JSON.stringify(r)),e.send(JSON.stringify(r))},e.onmessage=r=>{var i,s,c,p,C,a,x,h,k,_,f,y,I;if(r.data instanceof ArrayBuffer){console.debug("[YOLO] Received binary response (no metadata), skipping");return}if(typeof r.data=="string")try{const l=JSON.parse(r.data);switch(l.type){case"session_created":G.current=l.session_id,ee(!0),ne(0),J.current=setInterval(()=>ne(v=>v+1),1e3),B==="camera"?(ge.current=!0,U.current=setInterval(ko,50)):re("connecting");break;case"push_output":if(l.data_type==="application/json"&&l.data){try{const v=typeof l.data=="string"?JSON.parse(l.data):l.data;v.type==="status"&&v.status?re(v.status):v.type==="error"&&(re("error"),D(v.message||"Stream error"))}catch{}break}l.data&&l.data_type==="image/jpeg"&&(re("streaming"),Le(l.data),be.current=l.data,To(),(i=l.metadata)!=null&&i.detections&&pe(l.metadata.detections),(s=l.metadata)!=null&&s.roi_stats&&ue(l.metadata.roi_stats),(c=l.metadata)!=null&&c.line_stats&&he(l.metadata.line_stats),(p=l.metadata)!=null&&p.capture_events&&l.metadata.capture_events.length>0&&Je(v=>[...l.metadata.capture_events,...v].slice(0,10)));break;case"result":if(l.data){const v=l.skipped===!0||typeof l.data=="string"&&(l.data.startsWith("{")||((C=l.metadata)==null?void 0:C.skipped)===!0),P=((a=l.metadata)==null?void 0:a.status)==="waiting";v?(x=l.metadata)!=null&&x.detections&&pe(l.metadata.detections):P||typeof l.data=="string"&&l.data.length>0&&(Le(l.data),be.current=l.data,To(),(h=l.metadata)!=null&&h.frame_count?_e(l.metadata.frame_count):_e(E=>E+1),(k=l.metadata)!=null&&k.fps&&Te(l.metadata.fps),(_=l.metadata)!=null&&_.detections&&pe(l.metadata.detections),(f=l.metadata)!=null&&f.roi_stats&&ue(l.metadata.roi_stats),(y=l.metadata)!=null&&y.line_stats&&he(l.metadata.line_stats),(I=l.metadata)!=null&&I.capture_events&&l.metadata.capture_events.length>0&&Je(E=>[...l.metadata.capture_events,...E].slice(0,10)))}break;case"error":if(l.message&&l.message.includes("Frame rate too high")){console.debug("[YOLO] Frame dropped due to rate limiting (normal)");break}D(`${l.code}: ${l.message}`);break;case"session_closed":ee(!1),G.current=null;break}}catch(l){console.error("[YOLO] Failed to parse message:",l)}},e.onerror=r=>{ze.current||(console.error("[YOLO] WebSocket error:",r),D("WebSocket connection error"))},e.onclose=()=>{const r=ze.current;H.current=null,ee(!1),re("idle"),G.current=null,ge.current=!1,ze.current=!1,r&&D(null),J.current&&(clearInterval(J.current),J.current=null),U.current&&(clearInterval(U.current),U.current=null)},H.current=e},[mo,S,F,A,B,ko,X,Q]),To=()=>{se.current.frames++;const t=Date.now(),e=t-se.current.lastTime;e>=1e3&&(Te(Math.round(se.current.frames*1e3/e)),se.current.frames=0,se.current.lastTime=t)},_o=n.useCallback(()=>{H.current&&(ze.current=!0,H.current.readyState===WebSocket.OPEN&&H.current.send(JSON.stringify({type:"close"})),H.current.close(),H.current=null,J.current&&(clearInterval(J.current),J.current=null),U.current&&(clearInterval(U.current),U.current=null),ee(!1),G.current=null,pe([]))},[]),qo=n.useCallback(async()=>{D(null),Le(null),Te(0),_e(0),se.current={frames:0,lastTime:Date.now()},!(B==="camera"&&!await So())&&No()},[B,So,No]),eo=n.useCallback(()=>{B==="camera"&&Co(),_o(),re("idle"),pe([]),Te(0),_e(0),ne(0),Le(null),be.current=null,Ze.current=null,ue([]),he([]),Je([])},[B,Co,_o]),oo=n.useCallback(()=>{if(w.length<3)return;const t={id:Ye(),name:`ROI ${$.length+1}`,points:w,class_filter:[],color:Ne[($.length+W.length)%Ne.length]};Ve(e=>[...e,t]),Oe([]),ye("none"),u&&z()},[w,$.length,W.length,u,z]),Jo=n.useCallback(()=>{if(!j||!Y)return;const t={id:Ye(),name:`Line ${W.length+1}`,start:j,end:Y,color:Ne[($.length+W.length)%Ne.length]},e=[...W,t];Xe(e),We(null),qe(null),ye("none"),u&&z()},[j,Y,W,$.length,u,z]),Uo=.03,Go=n.useCallback(t=>{if(T==="none")return;const e=Qe.current;if(!e)return;const r=e.getBoundingClientRect(),i=(t.clientX-r.left)/r.width,s=(t.clientY-r.top)/r.height;if(T==="roi"){if(w.length>=3){const c=w[0];if(Math.sqrt((i-c[0])**2+(s-c[1])**2)<Uo){oo();return}}Oe(c=>[...c,[i,s]])}else T==="line"&&(j?Y||qe([i,s]):We([i,s]))},[T,j,Y,w,oo]),to=n.useCallback(()=>{Oe([]),We(null),qe(null),ye("none")},[]),Lo=n.useCallback(t=>{Ve(e=>e.filter(r=>r.id!==t)),ue(e=>e.filter(r=>r.id!==t)),u&&z()},[u,z]),$o=n.useCallback(t=>{Xe(e=>e.filter(r=>r.id!==t)),he(e=>e.filter(r=>r.id!==t)),u&&z()},[u,z]),Ko=n.useCallback((t,e,r)=>{const i=$.find(p=>p.id===t),s=e.type==="threshold"?`${e.class_name}≥${e.threshold}`:e.type==="presence"?`${e.class_name} appears`:`${e.class_name} gone`,c={id:Ye(),name:i?`${i.name}: ${s}`:s,roi_id:t,condition:e,cooldown_seconds:r,quality:80};yo(p=>[...p,c]),fe(null),u&&setTimeout(()=>z(),50)},[$,u,z]),Oo=n.useCallback(t=>{yo(e=>e.filter(r=>r.id!==t)),u&&setTimeout(()=>z(),50)},[u,z]),je=n.useCallback(()=>{const t=Qe.current;if(!t)return;const e=t.getContext("2d");if(!e)return;const r=xo.current;if(!r)return;const i=r.clientWidth,s=r.clientHeight;t.width=i,t.height=s,e.clearRect(0,0,i,s);const c=i,p=s,C=Ze.current;if(C&&C.complete&&C.naturalWidth>0){const a=C.naturalWidth/C.naturalHeight,x=c/p;let h=0,k=0,_=C.naturalWidth,f=C.naturalHeight;a>x?(_=C.naturalHeight*x,h=(C.naturalWidth-_)/2):(f=C.naturalWidth/x,k=(C.naturalHeight-f)/2),e.drawImage(C,h,k,_,f,0,0,c,p)}for(const a of $){if(a.points.length<3)continue;e.beginPath(),e.moveTo(a.points[0][0]*c,a.points[0][1]*p);for(let f=1;f<a.points.length;f++)e.lineTo(a.points[f][0]*c,a.points[f][1]*p);e.closePath(),e.fillStyle=K(a.color,.15),e.fill(),e.strokeStyle=a.color,e.lineWidth=2,e.stroke();const x=a.points.reduce((f,y)=>f+y[0],0)/a.points.length*c,h=a.points.reduce((f,y)=>f+y[1],0)/a.points.length*p;e.font="bold 12px -apple-system, sans-serif";const k=le.find(f=>f.id===a.id),_=e.measureText(a.name);if(k){const f=String(k.count),y=e.measureText(f),I=6,l=_.width+I+y.width+16,v=x-l/2,P=h-9,E=18;e.fillStyle=K(a.color,.9),e.beginPath(),e.roundRect(v,P,l,E,3),e.fill(),e.fillStyle="rgba(255,255,255,0.8)",e.textAlign="left",e.textBaseline="middle",e.fillText(a.name,v+6,h),e.fillStyle="#fff",e.font="bold 12px -apple-system, sans-serif",e.fillText(f,v+_.width+I+6,h),e.font="bold 12px -apple-system, sans-serif"}else{const f=_.width+12,y=x-f/2,I=h-9;e.fillStyle=K(a.color,.9),e.beginPath(),e.roundRect(y,I,f,18,3),e.fill(),e.fillStyle="#fff",e.textAlign="center",e.textBaseline="middle",e.fillText(a.name,x,h)}}for(const a of W){const x=a.start[0]*c,h=a.start[1]*p,k=a.end[0]*c,_=a.end[1]*p;e.beginPath(),e.moveTo(x,h),e.lineTo(k,_),e.strokeStyle=a.color,e.lineWidth=2,e.setLineDash([6,3]),e.stroke(),e.setLineDash([]);const y=Math.atan2(_-h,k-x)+Math.PI/2,I=(x+k)/2,l=(h+_)/2,v=12,P=5,E=I+Math.cos(y)*v,xe=l+Math.sin(y)*v;e.beginPath(),e.moveTo(I+Math.cos(y)*4,l+Math.sin(y)*4),e.lineTo(E,xe),e.strokeStyle="#4ade80",e.lineWidth=2,e.stroke(),e.beginPath(),e.moveTo(E,xe),e.lineTo(E-P*Math.cos(y-.5),xe-P*Math.sin(y-.5)),e.moveTo(E,xe),e.lineTo(E-P*Math.cos(y+.5),xe-P*Math.sin(y+.5)),e.stroke();const me=I-Math.cos(y)*v,ve=l-Math.sin(y)*v;e.beginPath(),e.moveTo(I-Math.cos(y)*4,l-Math.sin(y)*4),e.lineTo(me,ve),e.strokeStyle="#60a5fa",e.lineWidth=2,e.stroke(),e.beginPath(),e.moveTo(me,ve),e.lineTo(me+P*Math.cos(y-.5),ve+P*Math.sin(y-.5)),e.moveTo(me,ve),e.lineTo(me+P*Math.cos(y+.5),ve+P*Math.sin(y+.5)),e.stroke(),e.strokeStyle=a.color,e.lineWidth=2;const no=ae.find(we=>we.id===a.id);if(e.font="bold 11px -apple-system, sans-serif",no){const we=e.measureText(a.name),ke=`→${no.forward_count}`,Ee=`←${no.backward_count}`,Se=e.measureText(ke),Ro=e.measureText(Ee),Fe=5,Wo=we.width+Fe+Se.width+Fe+Ro.width+14,Io=I-Wo/2,Ae=l-18;e.fillStyle=K(a.color,.9),e.beginPath(),e.roundRect(Io,Ae,Wo,18,3),e.fill();let Ce=Io+7;e.fillStyle="rgba(255,255,255,0.8)",e.textAlign="left",e.textBaseline="middle",e.fillText(a.name,Ce,Ae+9),Ce+=we.width+Fe,e.fillStyle="#4ade80",e.fillText(ke,Ce,Ae+9),Ce+=Se.width+Fe,e.fillStyle="#60a5fa",e.fillText(Ee,Ce,Ae+9)}else{const ke=e.measureText(a.name).width+12,Ee=I-ke/2,Se=l-18;e.fillStyle=K(a.color,.9),e.beginPath(),e.roundRect(Ee,Se,ke,18,3),e.fill(),e.fillStyle="#fff",e.textAlign="center",e.textBaseline="middle",e.fillText(a.name,I,Se+9)}}if(T==="roi"&&w.length>0){e.beginPath(),e.moveTo(w[0][0]*c,w[0][1]*p);for(let a=1;a<w.length;a++)e.lineTo(w[a][0]*c,w[a][1]*p);e.strokeStyle="#3b82f6",e.lineWidth=2,e.setLineDash([4,4]),e.stroke(),e.setLineDash([]);for(let a=0;a<w.length;a++){const x=w[a],h=a===0;e.beginPath(),e.arc(x[0]*c,x[1]*p,h?6:4,0,Math.PI*2),e.fillStyle=h?"#22c55e":"#3b82f6",e.fill(),e.strokeStyle="#fff",e.lineWidth=1,e.stroke()}if(w.length>=3){const a=w[0];e.beginPath(),e.arc(a[0]*c,a[1]*p,12,0,Math.PI*2),e.strokeStyle="rgba(34,197,94,0.5)",e.lineWidth=2,e.setLineDash([3,3]),e.stroke(),e.setLineDash([]),e.fillStyle="rgba(0,0,0,0.6)",e.font="10px -apple-system, sans-serif",e.textAlign="center",e.fillText("Click green point to close",c/2,p-10)}}if(T==="line"&&j){const a=j[0]*c,x=j[1]*p;if(e.beginPath(),e.arc(a,x,4,0,Math.PI*2),e.fillStyle="#22c55e",e.fill(),e.strokeStyle="#fff",e.lineWidth=1,e.stroke(),Y){const h=Y[0]*c,k=Y[1]*p;e.beginPath(),e.moveTo(a,x),e.lineTo(h,k),e.strokeStyle="#22c55e",e.lineWidth=2,e.setLineDash([6,3]),e.stroke(),e.setLineDash([]),e.beginPath(),e.arc(h,k,4,0,Math.PI*2),e.fillStyle="#22c55e",e.fill(),e.strokeStyle="#fff",e.lineWidth=1,e.stroke(),e.fillStyle="rgba(0,0,0,0.6)",e.font="10px -apple-system, sans-serif",e.textAlign="center",e.fillText("Click Save to confirm",c/2,p-10)}else e.fillStyle="rgba(0,0,0,0.6)",e.font="10px -apple-system, sans-serif",e.textAlign="center",e.fillText("Click to set end point",c/2,p-10)}},[$,W,le,ae,w,j,Y,T]);n.useEffect(()=>{let t=!0;const e=()=>{if(!t)return;const r=be.current;if(r){be.current=null;const i=new Image;i.onload=()=>{t&&(Ze.current=i,je())},i.src=`data:image/jpeg;base64,${r}`}Re.current=requestAnimationFrame(e)};return Re.current=requestAnimationFrame(e),()=>{t=!1,cancelAnimationFrame(Re.current)}},[je]),n.useEffect(()=>{je()},[$,W,le,ae,w,j,T,je]),n.useEffect(()=>()=>{eo(),ce.current&&clearTimeout(ce.current)},[eo]);const Qo=t=>{const e=Math.floor(t/60),r=t%60;return`${e.toString().padStart(2,"0")}:${r.toString().padStart(2,"0")}`},Zo=()=>B==="network"?u&&$e==="reconnecting"?"Reconnecting...":u&&$e==="error"?"Error":S.startsWith("rtsp://")?"RTSP":S.startsWith("rtmp://")?"RTMP":S.startsWith("hls://")||S.includes(".m3u8")?"HLS":"Network":"CAM";return o.jsx("div",{ref:R,className:`yolo ${M}`,children:o.jsxs("div",{className:"yolo-card",children:[o.jsxs("div",{className:"yolo-header",children:[o.jsxs("div",{className:"yolo-title",children:[o.jsx(b,{name:"camera",className:"yolo-title-icon"}),m]}),o.jsxs("div",{className:"yolo-controls",children:[o.jsx("button",{className:`yolo-draw-btn${T==="roi"?" yolo-draw-active":""}`,onClick:()=>{T==="roi"?to():(ye("roi"),We(null))},title:"Draw ROI polygon",children:o.jsx(b,{name:"polygon",style:{width:12,height:12}})}),o.jsx("button",{className:`yolo-draw-btn${T==="line"?" yolo-draw-active":""}`,onClick:()=>{T==="line"?to():(ye("line"),Oe([]))},title:"Draw crossing line",children:o.jsx(b,{name:"line",style:{width:12,height:12}})}),T==="roi"&&w.length>=3&&o.jsx("button",{className:"yolo-draw-btn",onClick:oo,title:"Finish ROI",children:o.jsx(b,{name:"play",style:{width:10,height:10}})}),T==="line"&&j&&Y&&o.jsx("button",{className:"yolo-draw-btn",style:{background:"var(--color-success, #22c55e)",borderColor:"var(--color-success, #22c55e)",color:"var(--yolo-on-primary)"},onClick:Jo,title:"Save line",children:o.jsx(b,{name:"play",style:{width:10,height:10}})}),T!=="none"&&o.jsx("button",{className:"yolo-draw-btn yolo-draw-danger",onClick:to,title:"Cancel",children:"×"}),$.length+W.length>0&&T==="none"&&o.jsx("button",{className:"yolo-draw-btn yolo-draw-danger",onClick:()=>{Ve([]),Xe([]),ue([]),he([]),u&&z()},title:"Clear all",children:o.jsx(b,{name:"trash",style:{width:10,height:10}})}),o.jsx("span",{style:{width:1,height:12,background:"var(--yolo-border)",margin:"0 2px"}}),u&&o.jsxs("div",{className:"yolo-status",children:[o.jsx("span",{className:`yolo-status-dot${$e==="reconnecting"?" yolo-status-warning":$e==="error"?" yolo-status-error":""}`}),Zo()]}),u?o.jsxs("button",{onClick:eo,className:"yolo-btn yolo-btn-stop",children:[o.jsx(b,{name:"stop",style:{width:12,height:12,display:"inline",verticalAlign:"middle",marginRight:2}}),"Stop"]}):o.jsxs("button",{onClick:qo,className:"yolo-btn",children:[o.jsx(b,{name:"play",style:{width:12,height:12,display:"inline",verticalAlign:"middle",marginRight:2}}),"Start"]})]})]}),o.jsxs("div",{className:"yolo-video-wrap",ref:xo,children:[B==="camera"&&o.jsxs(o.Fragment,{children:[o.jsx("video",{ref:q,style:{display:"none"},playsInline:!0,muted:!0}),o.jsx("canvas",{ref:bo,width:640,height:480,style:{display:"none"}})]}),o.jsx("canvas",{ref:Qe,className:"yolo-video-frame",style:{cursor:T!=="none"?"crosshair":"default"},onClick:Go}),oe&&o.jsxs("div",{className:"yolo-error",children:[o.jsx(b,{name:"alert",className:"yolo-error-icon"}),o.jsx("div",{className:"yolo-error-text",children:oe})]}),!u&&!oe&&o.jsxs("div",{className:"yolo-video-placeholder",children:[o.jsx(b,{name:"video",className:"yolo-video-icon"}),o.jsx("div",{className:"yolo-video-text",children:B==="camera"?"Click Start to begin detection":`Click Start to connect to ${S}`})]}),u&&!Vo&&!oe&&o.jsxs("div",{className:"yolo-video-loading",children:[o.jsx("div",{className:"yolo-spinner"}),o.jsx("div",{className:"yolo-video-text",children:B==="camera"?"Starting camera...":"Connecting..."})]})]}),u&&L&&o.jsxs("div",{className:"yolo-stats",children:[o.jsxs("div",{className:"yolo-stat-group",children:[o.jsxs("div",{className:"yolo-stat",children:[o.jsx(b,{name:"clock",className:"yolo-stat-icon"}),o.jsx("span",{className:"yolo-stat-val",children:Qo(te)})]}),o.jsxs("div",{className:"yolo-stat",children:[o.jsx(b,{name:"activity",className:"yolo-stat-icon"}),o.jsx("span",{className:"yolo-stat-val",children:g}),o.jsx("span",{children:"FPS"})]}),o.jsxs("div",{className:"yolo-stat",children:[o.jsx(b,{name:"layers",className:"yolo-stat-icon"}),o.jsx("span",{className:"yolo-stat-val",children:Ho}),o.jsx("span",{children:"frames"})]})]}),o.jsxs("div",{className:"yolo-stat",children:[o.jsx(b,{name:"eye",className:"yolo-stat-icon"}),o.jsx("span",{className:"yolo-stat-val",children:Be.length}),o.jsx("span",{children:"objects"})]})]}),u&&Be.length>0&&o.jsxs("div",{className:"yolo-detections",children:[o.jsx("div",{className:"yolo-detections-title",children:"Detected Objects"}),o.jsx("div",{className:"yolo-detections-list",children:(()=>{const t=new Map;for(const r of Be){const i=r.class_id||0,s=t.get(i);s?s.count++:t.set(i,{label:r.label,count:1})}return[...t.entries()].sort((r,i)=>i[1].count-r[1].count).map(([r,{label:i,count:s}])=>{const c=Po(r);return o.jsxs("span",{className:"yolo-detection-tag",style:{backgroundColor:c.bg,color:c.fg,border:`1px solid ${c.border}`},children:[i,o.jsxs("span",{style:{opacity:.8,fontWeight:700},children:["x",s]})]},r)})})()})]}),(le.length>0||ae.length>0||$.length>0||W.length>0)&&o.jsx("div",{className:"yolo-regions",children:o.jsxs("div",{className:"yolo-section-grid",children:[le.map(t=>{const e=$.find(s=>s.id===t.id),r=(e==null?void 0:e.color)||"#3b82f6",i=Ie.filter(s=>s.roi_id===t.id);return o.jsxs("div",{className:"yolo-card",children:[o.jsxs("div",{className:"yolo-card-row",children:[o.jsx("span",{className:"yolo-card-name",children:t.name}),o.jsxs("span",{className:"yolo-card-actions",children:[o.jsx("button",{className:"yolo-card-btn yolo-card-btn-edit",onClick:()=>fe(Me===t.id?null:t.id),title:"Edit capture rules",children:o.jsx(b,{name:"edit",style:{width:10,height:10}})}),o.jsx("button",{className:"yolo-card-btn",onClick:()=>Lo(t.id),title:"Delete",children:o.jsx(b,{name:"x",style:{width:10,height:10}})})]})]}),(t.count>0||i.length>0)&&o.jsxs("div",{className:"yolo-card-data",children:[o.jsx("span",{className:"yolo-card-badge",style:{background:K(r,.15),color:r},children:t.count}),i.map(s=>o.jsxs("span",{className:"yolo-rule-pill",children:[s.condition.type==="threshold"?`${s.condition.class_name}≥${s.condition.threshold}`:s.condition.type==="presence"?`${s.condition.class_name}↑`:`${s.condition.class_name}↓`,o.jsx("button",{className:"yolo-rule-pill-btn",onClick:()=>Oo(s.id),children:o.jsx(b,{name:"x",style:{width:8,height:8}})})]},s.id))]})]},t.id)}),$.filter(t=>!le.some(e=>e.id===t.id)).map(t=>{const e=Ie.filter(r=>r.roi_id===t.id);return o.jsxs("div",{className:"yolo-card",children:[o.jsxs("div",{className:"yolo-card-row",children:[o.jsx("span",{className:"yolo-card-name",children:t.name}),o.jsxs("span",{className:"yolo-card-actions",children:[o.jsx("button",{className:"yolo-card-btn yolo-card-btn-edit",onClick:()=>fe(Me===t.id?null:t.id),title:"Edit capture rules",children:o.jsx(b,{name:"edit",style:{width:10,height:10}})}),o.jsx("button",{className:"yolo-card-btn",onClick:()=>Lo(t.id),title:"Delete",children:o.jsx(b,{name:"x",style:{width:10,height:10}})})]})]}),e.length>0&&o.jsx("div",{className:"yolo-card-data",children:e.map(r=>o.jsxs("span",{className:"yolo-rule-pill",children:[r.condition.type==="threshold"?`${r.condition.class_name}≥${r.condition.threshold}`:r.condition.type==="presence"?`${r.condition.class_name}↑`:`${r.condition.class_name}↓`,o.jsx("button",{className:"yolo-rule-pill-btn",onClick:()=>Oo(r.id),children:o.jsx(b,{name:"x",style:{width:8,height:8}})})]},r.id))})]},t.id)}),ae.map(t=>{const e=W.find(r=>r.id===t.id);return e!=null&&e.color,o.jsxs("div",{className:"yolo-card",children:[o.jsxs("div",{className:"yolo-card-row",children:[o.jsx("span",{className:"yolo-card-name",children:t.name}),o.jsx("span",{className:"yolo-card-actions",children:o.jsx("button",{className:"yolo-card-btn",onClick:()=>$o(t.id),title:"Delete",children:o.jsx(b,{name:"x",style:{width:10,height:10}})})})]}),o.jsxs("div",{className:"yolo-card-data",children:[o.jsxs("span",{className:"yolo-line-dir",style:{background:"rgba(34,197,94,0.12)",color:"#22c55e"},children:["→",t.forward_count]}),o.jsxs("span",{className:"yolo-line-dir",style:{background:"rgba(59,130,246,0.12)",color:"#3b82f6"},children:["←",t.backward_count]})]})]},t.id)}),W.filter(t=>!ae.some(e=>e.id===t.id)).map(t=>o.jsx("div",{className:"yolo-card",children:o.jsxs("div",{className:"yolo-card-row",children:[o.jsx("span",{className:"yolo-card-name",children:t.name}),o.jsx("span",{className:"yolo-card-actions",children:o.jsx("button",{className:"yolo-card-btn",onClick:()=>$o(t.id),title:"Delete",children:o.jsx(b,{name:"x",style:{width:10,height:10}})})})]})},t.id))]})}),Me&&o.jsx("div",{style:{position:"fixed",inset:0,zIndex:1e4,display:"flex",alignItems:"center",justifyContent:"center",background:"rgba(0,0,0,0.4)",backdropFilter:"blur(4px)",WebkitBackdropFilter:"blur(4px)"},onClick:()=>fe(null),children:o.jsx(Eo,{roiId:Me,onAdd:Ko,onCancel:()=>fe(null)})}),uo.length>0&&o.jsx("div",{className:"yolo-captures",children:uo.map((t,e)=>o.jsxs("div",{className:"yolo-capture-item",title:`${t.rule_name}
${t.condition}
${new Date(t.timestamp).toLocaleTimeString()}`,onClick:()=>fo(`data:image/jpeg;base64,${t.image_base64}`),children:[o.jsx("img",{src:`data:image/jpeg;base64,${t.image_base64}`,alt:t.rule_name}),o.jsx("span",{className:"yolo-capture-label",children:t.rule_name})]},`${t.rule_id}-${t.timestamp}-${e}`))}),ho&&o.jsx("div",{style:{position:"fixed",inset:0,zIndex:2e4,display:"flex",alignItems:"center",justifyContent:"center",background:"rgba(0,0,0,0.7)",backdropFilter:"blur(6px)",WebkitBackdropFilter:"blur(6px)",cursor:"zoom-out"},onClick:()=>fo(null),children:o.jsx("img",{src:ho,alt:"capture",style:{maxWidth:"90vw",maxHeight:"85vh",borderRadius:"8px",boxShadow:"0 8px 40px rgba(0,0,0,0.4)"},onClick:t=>t.stopPropagation()})})]})})});function so({value:d,options:m,open:N,onToggle:M,onChange:F}){const A=m.find(L=>L.value===d),S={width:"100%",height:"36px",fontSize:"13px",padding:"0 10px",border:N?"1px solid var(--yolo-accent)":"1px solid var(--yolo-border)",borderRadius:"6px",background:"var(--yolo-card)",color:"var(--yolo-fg)",boxSizing:"border-box",fontFamily:"inherit",cursor:"pointer",display:"flex",alignItems:"center",justifyContent:"space-between",outline:"none",transition:"border-color 0.15s",boxShadow:N?"0 0 0 3px rgba(59,130,246,0.12)":"none"},X={position:"absolute",left:0,right:0,top:"100%",marginTop:"4px",background:"var(--yolo-card)",border:"1px solid var(--yolo-border)",borderRadius:"6px",boxShadow:"0 4px 20px rgba(0,0,0,0.1)",maxHeight:"180px",overflowY:"auto",zIndex:100,padding:"4px"},Q=L=>({padding:"6px 10px",fontSize:"13px",cursor:"pointer",borderRadius:"4px",background:L?"var(--yolo-accent)":"transparent",color:L?"var(--yolo-on-primary)":"var(--yolo-fg)",transition:"background 0.1s"});return o.jsxs("div",{style:{position:"relative"},children:[o.jsxs("button",{type:"button",style:S,onClick:M,children:[o.jsx("span",{children:(A==null?void 0:A.label)||d}),o.jsx("svg",{width:"12",height:"12",viewBox:"0 0 12 12",fill:"none",style:{opacity:.5,flexShrink:0},children:o.jsx("path",{d:"M3 4.5L6 7.5L9 4.5",stroke:"currentColor",strokeWidth:"1.5",strokeLinecap:"round",strokeLinejoin:"round"})})]}),N&&o.jsx("div",{style:X,children:m.map(L=>o.jsx("div",{style:Q(L.value===d),onClick:()=>F(L.value),onMouseEnter:Z=>{L.value!==d&&(Z.currentTarget.style.background="var(--yolo-hover)")},onMouseLeave:Z=>{L.value!==d&&(Z.currentTarget.style.background="transparent")},children:L.label},L.value))})]})}function Eo({roiId:d,onAdd:m,onCancel:N}){const[M,F]=n.useState("threshold"),[A,S]=n.useState("person"),[X,Q]=n.useState(3),[L,Z]=n.useState(5),[R,u]=n.useState(null),ee=[{value:"threshold",label:"Threshold (count ≥ N)"},{value:"presence",label:"Presence (appears)"},{value:"absence",label:"Absence (disappears)"}],oe=["person","car","truck","bus","bicycle","motorcycle","dog","cat","bird","chair","bottle","cell phone","backpack","umbrella","handbag","suitcase"],D={fontSize:"12px",fontWeight:500,color:"var(--yolo-muted)"},te={display:"flex",flexDirection:"column",gap:"6px",marginBottom:"14px"},ne={width:"100%",height:"36px",fontSize:"13px",padding:"0 10px",border:"1px solid var(--yolo-border)",borderRadius:"6px",background:"var(--yolo-card)",color:"var(--yolo-fg)",outline:"none",boxSizing:"border-box",fontFamily:"inherit"};return o.jsxs("div",{style:{background:"var(--yolo-card)",border:"1px solid var(--yolo-border)",borderRadius:"12px",padding:"20px",minWidth:"300px",maxWidth:"360px",boxShadow:"0 20px 60px rgba(0,0,0,0.15), 0 0 0 1px rgba(0,0,0,0.05)",fontSize:"13px",fontFamily:'-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif',color:"var(--yolo-fg)"},onClick:g=>g.stopPropagation(),children:[o.jsx("div",{style:{fontSize:"15px",fontWeight:600,marginBottom:"16px",paddingBottom:"12px",borderBottom:"1px solid var(--yolo-border)"},children:"Add Capture Rule"}),o.jsxs("label",{style:te,children:[o.jsx("span",{style:D,children:"Condition"}),o.jsx(so,{value:M,options:ee,open:R==="cond",onToggle:()=>u(R==="cond"?null:"cond"),onChange:g=>{F(g),u(null)}})]}),o.jsxs("label",{style:te,children:[o.jsx("span",{style:D,children:"Class"}),o.jsx(so,{value:A,options:oe.map(g=>({value:g,label:g})),open:R==="class",onToggle:()=>u(R==="class"?null:"class"),onChange:g=>{S(g),u(null)}})]}),M==="threshold"&&o.jsxs("label",{style:te,children:[o.jsx("span",{style:D,children:"Threshold"}),o.jsx("input",{style:ne,type:"number",min:1,max:100,value:X,onChange:g=>Q(Number(g.target.value))})]}),o.jsxs("label",{style:te,children:[o.jsx("span",{style:D,children:"Cooldown (s)"}),o.jsx("input",{style:ne,type:"number",min:1,max:300,value:L,onChange:g=>Z(Number(g.target.value))})]}),o.jsxs("div",{style:{display:"flex",justifyContent:"flex-end",gap:"8px",marginTop:"18px",paddingTop:"14px",borderTop:"1px solid var(--yolo-border)"},children:[o.jsx("button",{style:{height:"34px",padding:"0 16px",fontSize:"13px",fontWeight:500,border:"1px solid var(--yolo-border)",borderRadius:"6px",background:"var(--yolo-card)",color:"var(--yolo-muted)",cursor:"pointer",fontFamily:"inherit"},onClick:N,onMouseEnter:g=>g.currentTarget.style.background="var(--yolo-hover)",onMouseLeave:g=>g.currentTarget.style.background="var(--yolo-card)",children:"Cancel"}),o.jsx("button",{style:{height:"34px",padding:"0 16px",fontSize:"13px",fontWeight:500,border:"none",borderRadius:"6px",background:"var(--yolo-accent)",color:"var(--yolo-on-primary)",cursor:"pointer",fontFamily:"inherit"},onClick:()=>{m(d,M==="threshold"?{type:"threshold",class_name:A,threshold:X}:{type:M,class_name:A},L)},onMouseEnter:g=>g.currentTarget.style.opacity="0.9",onMouseLeave:g=>g.currentTarget.style.opacity="1",children:"Add Rule"})]})]})}const io=n.forwardRef((d,m)=>o.jsx("div",{ref:m,style:{height:"100%",minHeight:300},children:o.jsx(de,{...d,title:d.title||"YOLO Detection"})})),co=n.forwardRef((d,m)=>o.jsx("div",{ref:m,style:{height:280},children:o.jsx(de,{...d,title:d.title||"YOLO"})})),po=n.forwardRef((d,m)=>o.jsx("div",{ref:m,style:{height:"100%",minHeight:500},children:o.jsx(de,{...d,title:d.title||"YOLO Video Detection"})})),Fo={YoloVideoDisplay:de},Ao=io,Yo=co,Bo=po;O.Card=Ao,O.Panel=Bo,O.Widget=Yo,O.YoloVideoCard=io,O.YoloVideoDisplay=de,O.YoloVideoPanel=po,O.YoloVideoWidget=co,O.default=Fo,Object.defineProperties(O,{__esModule:{value:!0},[Symbol.toStringTag]:{value:"Module"}})});
