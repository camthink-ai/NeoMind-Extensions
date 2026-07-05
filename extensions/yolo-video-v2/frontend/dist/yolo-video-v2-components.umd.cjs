(function(W,o){typeof exports=="object"&&typeof module<"u"?o(exports,require("react/jsx-runtime"),require("react")):typeof define=="function"&&define.amd?define(["exports","react/jsx-runtime","react"],o):(W=typeof globalThis<"u"?globalThis:W||self,o(W.YoloVideoV2Components={},W.jsxRuntime,W.React))})(this,function(W,o,n){"use strict";const qo="yolo-video-v2",xo="yolo-styles-v2",Jo=`
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
  padding: 3px 8px;
  border-bottom: 1px solid var(--yolo-border);
}
.yolo-title {
  display: flex;
  align-items: center;
  gap: 4px;
  color: var(--yolo-fg);
  font-size: 11px;
  font-weight: 600;
  line-height: 1;
}
.yolo-title-cluster {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}
.yolo-title-icon {
  width: 12px;
  height: 12px;
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
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
  height: 22px;
  padding: 0 10px;
  font-size: 11px;
  font-weight: 500;
  color: var(--yolo-on-primary);
  background: var(--yolo-accent);
  border: none;
  border-radius: 5px;
  cursor: pointer;
  transition: filter .15s ease, transform .05s ease;
  -webkit-tap-highlight-color: transparent;
  user-select: none;
}
.yolo-btn:hover { filter: brightness(1.08); }
.yolo-btn:active { transform: scale(0.97); }
.yolo-btn:focus-visible { outline: 2px solid var(--yolo-accent); outline-offset: 2px; }
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

/* Floating video overlays — glass pills sitting on the video, freeing bottom layout space */
.yolo-overlay-stats {
  position: absolute;
  top: 8px;
  left: 8px;
  z-index: 4;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 4px 8px;
  font-size: 10px;
  color: rgba(255,255,255,0.85);
  background: rgba(0,0,0,0.55);
  backdrop-filter: blur(8px);
  -webkit-backdrop-filter: blur(8px);
  border: 1px solid rgba(255,255,255,0.08);
  border-radius: 999px;
  pointer-events: none;
}
.yolo-overlay-stat-icon { width: 11px; height: 11px; opacity: 0.7; flex-shrink: 0; }
.yolo-overlay-stat-val { font-weight: 600; color: #fff; }
.yolo-overlay-sep { width: 1px; height: 9px; background: rgba(255,255,255,0.18); display: inline-block; }

.yolo-overlay-detections {
  position: absolute;
  top: 8px;
  right: 8px;
  z-index: 4;
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  justify-content: flex-end;
  max-width: 65%;
  pointer-events: none;
}
.yolo-overlay-detections .yolo-detection-tag {
  font-size: 9px;
  padding: 1px 5px;
  backdrop-filter: blur(6px);
  -webkit-backdrop-filter: blur(6px);
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

/* Drawing Toolbar — aligned with NeoMind button system */
.yolo-draw-toolbar {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 10px;
  border-top: 1px solid var(--yolo-border);
  border-bottom: 1px solid var(--yolo-border);
  background: var(--yolo-card);
}
.yolo-draw-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
  width: 22px;
  height: 22px;
  padding: 0;
  font-size: 11px;
  font-weight: 500;
  color: var(--yolo-muted);
  background: transparent;
  border: 1px solid var(--yolo-border);
  border-radius: 5px;
  cursor: pointer;
  transition: color .15s ease, background-color .15s ease, border-color .15s ease, transform .05s ease;
  white-space: nowrap;
  -webkit-tap-highlight-color: transparent;
  user-select: none;
}
.yolo-draw-btn:hover {
  color: var(--yolo-fg);
  background: var(--yolo-accent-soft, rgba(59,130,246,0.08));
  border-color: var(--yolo-accent);
}
.yolo-draw-btn:active {
  transform: scale(0.96);
}
.yolo-draw-btn:focus-visible {
  outline: 2px solid var(--yolo-accent);
  outline-offset: 1px;
}
.yolo-draw-btn.yolo-draw-active {
  color: var(--yolo-on-primary);
  background: var(--yolo-accent);
  border-color: var(--yolo-accent);
}
.yolo-draw-btn.yolo-draw-active:hover {
  background: var(--yolo-accent-hover, var(--yolo-accent));
  filter: brightness(1.08);
}
.yolo-draw-btn.yolo-draw-success {
  color: var(--yolo-on-primary);
  background: var(--yolo-success, #22c55e);
  border-color: var(--yolo-success, #22c55e);
}
.yolo-draw-btn.yolo-draw-success:hover {
  filter: brightness(1.08);
}
.yolo-draw-btn.yolo-draw-danger {
  color: var(--yolo-error);
  border-color: var(--yolo-error-border, rgba(239,68,68,0.3));
  background: transparent;
}
.yolo-draw-btn.yolo-draw-danger:hover {
  background: var(--yolo-error);
  color: var(--yolo-on-primary);
  border-color: var(--yolo-error);
}
.yolo-draw-divider {
  width: 1px;
  height: 14px;
  background: var(--yolo-border);
  margin: 0 3px;
  flex-shrink: 0;
}

/* Floating regions widget — chip (collapsed) / popover panel (expanded).
   Sits over the video bottom-left so it consumes zero layout height. */
.yolo-regions-float {
  position: absolute;
  left: 8px;
  bottom: 8px;
  z-index: 5;
  max-width: calc(100% - 16px);
}
.yolo-regions-chip {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 4px 9px;
  font-size: 10px;
  color: rgba(255,255,255,0.9);
  background: rgba(0,0,0,0.6);
  backdrop-filter: blur(8px);
  -webkit-backdrop-filter: blur(8px);
  border: 1px solid rgba(255,255,255,0.1);
  border-radius: 999px;
  cursor: pointer;
  transition: background-color .15s ease, transform .05s ease;
  -webkit-tap-highlight-color: transparent;
}
.yolo-regions-chip:hover { background: rgba(0,0,0,0.75); }
.yolo-regions-chip:active { transform: scale(0.97); }
.yolo-regions-chip b { color: #fff; font-weight: 700; }
.yolo-regions-chip-dot {
  width: 6px; height: 6px; border-radius: 50%;
  background: var(--yolo-success, #22c55e);
}
.yolo-regions-chip-sep { opacity: 0.4; }
.yolo-regions-chip-arrow { opacity: 0.5; font-size: 9px; margin-left: 1px; }

.yolo-regions-panel {
  background: var(--yolo-card);
  border: 1px solid var(--yolo-border);
  border-radius: 8px;
  box-shadow: 0 8px 24px rgba(0,0,0,0.25);
  overflow: hidden;
  min-width: 220px;
  max-width: 360px;
  animation: yolo-panel-in .12s ease-out;
}
@keyframes yolo-panel-in {
  from { opacity: 0; transform: translateY(4px); }
  to   { opacity: 1; transform: translateY(0); }
}
.yolo-regions-panel::-webkit-scrollbar { width: 4px; }
.yolo-regions-panel::-webkit-scrollbar-thumb { background: var(--yolo-border); border-radius: 2px; }
.yolo-regions-panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
  padding: 6px 8px;
  background: none;
  border: none;
  border-bottom: 1px solid var(--yolo-border);
  cursor: pointer;
  font-size: 10px;
  color: var(--yolo-muted);
  text-transform: uppercase;
  letter-spacing: 0.3px;
  transition: color .15s ease;
}
.yolo-regions-panel-header:hover { color: var(--yolo-fg); }
.yolo-regions-summary { display: inline-flex; align-items: center; gap: 4px; }
.yolo-regions-count { color: var(--yolo-fg); font-weight: 600; }
.yolo-regions-dot { opacity: 0.5; }
.yolo-regions-toggle { font-size: 9px; opacity: 0.6; }

/* Compact region pill strip — horizontal wrap, minimal height */
.yolo-region-pills {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  padding: 5px 6px;
  max-height: 96px;   /* ~3 rows max, then scroll */
  overflow-y: auto;
}
.yolo-region-pills::-webkit-scrollbar { width: 3px; }
.yolo-region-pills::-webkit-scrollbar-thumb { background: var(--yolo-border); border-radius: 2px; }

.yolo-region-pill {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 1px 4px 1px 6px;
  font-size: 10px;
  line-height: 1;
  color: var(--yolo-fg);
  background: var(--yolo-card);
  border: 1px solid var(--yolo-border);
  border-radius: 999px;
  white-space: nowrap;
}
.yolo-region-pill-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  flex-shrink: 0;
}
.yolo-region-pill-name {
  font-weight: 600;
  max-width: 80px;
  overflow: hidden;
  text-overflow: ellipsis;
}
.yolo-region-pill-name-editable {
  cursor: text;
  border-radius: 3px;
  padding: 0 1px;
  transition: background-color .12s ease;
}
.yolo-region-pill-name-editable:hover {
  background: rgba(0,0,0,0.06);
  outline: 1px dashed var(--yolo-border);
}
.yolo-region-pill-input {
  font: inherit;
  font-weight: 600;
  font-size: 10px;
  color: var(--yolo-fg);
  background: var(--yolo-bg, #fff);
  border: 1px solid var(--yolo-accent);
  border-radius: 3px;
  padding: 0 3px;
  outline: none;
  width: 90px;
  height: 16px;
  line-height: 1;
}
.yolo-region-pill-count {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 14px;
  height: 13px;
  padding: 0 3px;
  font-size: 9px;
  font-weight: 700;
  border-radius: 7px;
  line-height: 1;
}
.yolo-region-pill-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 14px;
  height: 14px;
  padding: 0;
  background: none;
  border: none;
  cursor: pointer;
  color: var(--yolo-muted);
  border-radius: 50%;
  transition: color .15s, background-color .15s, opacity .12s;
  /* Hidden by default; revealed on pill hover/focus to avoid covering name/counts. */
  opacity: 0;
  pointer-events: none;
}
.yolo-region-pill:hover .yolo-region-pill-btn,
.yolo-region-pill:focus-within .yolo-region-pill-btn {
  opacity: 1;
  pointer-events: auto;
}
.yolo-region-pill-btn:hover { color: var(--yolo-error); background: rgba(239,68,68,0.1); }
.yolo-region-pill-edit:hover { color: var(--yolo-accent) !important; background: rgba(59,130,246,0.1) !important; }
/* When rules are being edited, keep buttons visible so toggle state is clear. */
.yolo-region-pill[data-rules-open="true"] .yolo-region-pill-btn {
  opacity: 1;
  pointer-events: auto;
}
.yolo-region-pill-rule {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  padding: 0 3px 0 5px;
  font-size: 9px;
  color: var(--yolo-muted);
  background: var(--yolo-bg, rgba(0,0,0,0.04));
  border-radius: 7px;
  margin-left: 2px;
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

`;function Ko(){if(typeof document>"u"||document.getElementById(xo))return;const d=document.createElement("style");d.id=xo,d.textContent=Jo,document.head.appendChild(d)}const mo={video:'<path d="M23 7l-7 5 7 5V7z"/><rect x="1" y="5" width="15" height="14" rx="2" ry="2"/>',play:'<polygon points="5 3 19 12 5 21 5 3"/>',stop:'<rect x="3" y="3" width="18" height="18" rx="2" ry="2"/>',camera:'<path d="M23 19a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4l2-3h6l2 3h4a2 2 0 0 1 2 2z"/><circle cx="12" cy="13" r="4"/>',activity:'<polyline points="22 12 18 12 15 21 9 3 6 12 2 12"/>',clock:'<circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/>',eye:'<path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/>',layers:'<polygon points="12 2 2 7 12 12 22 7 12 2"/><polyline points="2 17 12 22 22 17"/><polyline points="2 12 12 17 22 12"/>',alert:'<circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/>',polygon:'<polygon points="12 2 22 8.5 18 20 6 20 2 8.5 12 2"/>',line:'<line x1="4" y1="20" x2="20" y2="4"/><polyline points="16 4 20 4 20 8"/>',trash:'<polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>',arrowRight:'<line x1="5" y1="12" x2="19" y2="12"/><polyline points="12 5 19 12 12 19"/>',arrowLeft:'<line x1="19" y1="12" x2="5" y2="12"/><polyline points="12 5 5 12 12 19"/>',zap:'<polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/>',plus:'<line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/>',edit:'<path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/>',x:'<line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>',close:'<line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>'},w=({name:d,className:k="",style:O})=>o.jsx("svg",{viewBox:"0 0 24 24",fill:"none",stroke:"currentColor",strokeWidth:"2",strokeLinecap:"round",strokeLinejoin:"round",className:k,style:O,dangerouslySetInnerHTML:{__html:mo[d]||mo.video}}),vo=[[38,70,83],[40,116,74],[117,79,12],[115,53,88],[192,41,66],[11,121,175],[232,168,124],[211,212,211],[232,212,77],[32,169,199],[57,94,121],[237,139,0],[133,160,131],[174,30,70],[255,183,59],[197,198,53],[166,207,213],[136,86,82],[119,104,174],[51,159,160],[166,59,111],[197,166,137],[108,118,135],[38,131,116],[233,126,67],[255,179,71],[48,96,106],[197,104,80],[227,105,145],[229,193,175]];function Uo(d){const[k,O,M]=vo[d%vo.length],H=(.299*k+.587*O+.114*M)/255;return{bg:`rgba(${k}, ${O}, ${M}, 0.85)`,fg:H>.5?"#000":"#fff",border:`rgb(${k}, ${O}, ${M})`}}const De=["#3b82f6","#22c55e","#f59e0b","#ef4444","#8b5cf6","#ec4899","#06b6d4","#f97316"];let Go=0;function to(){return`r${Date.now().toString(36)}_${++Go}`}function G(d,k){const O=parseInt(d.slice(1,3),16),M=parseInt(d.slice(3,5),16),H=parseInt(d.slice(5,7),16);return`rgba(${O}, ${M}, ${H}, ${k})`}const ke=n.forwardRef(function({title:k="YOLO Detection",dataSource:O,className:M="",confidenceThreshold:H=.5,maxObjects:V=20,sourceUrl:T="camera://0",fps:R=15,drawBoxes:ne=!0,showStats:I=!0,variant:re="default"},le){n.useEffect(()=>{Ko()},[]);const[p,ee]=n.useState(!1),[ae,E]=n.useState(null),[ie,se]=n.useState(0),[b,Me]=n.useState(0),[gt,Ee]=n.useState(0),[no,ce]=n.useState([]),[tt,Pe]=n.useState(null),[yt,ro]=n.useState("pending"),[Fe,A]=n.useState("idle"),[L,Se]=n.useState("none"),[y,Ye]=n.useState([]),[m,Ae]=n.useState([]),[de,pe]=n.useState([]),[ue,ge]=n.useState([]),[C,je]=n.useState([]),[j,Be]=n.useState(null),[X,lo]=n.useState(null),[He,No]=n.useState([]),[To,ao]=n.useState([]),[ye,Ce]=n.useState(null),[_o,Oo]=n.useState(null),[nt,Lo]=n.useState(!0),[Ve,io]=n.useState(null),[fe,Ne]=n.useState(""),q=T.startsWith("rtsp://")||T.startsWith("rtmp://")||T.startsWith("hls://")||T.includes(".m3u8")||T.startsWith("http://")||T.startsWith("https://")||T.startsWith("file://")?"network":"camera",Xe=n.useRef(y);Xe.current=y;const qe=n.useRef(m);qe.current=m;const $o=n.useRef(He);$o.current=He;const oe=n.useRef(null),Io=n.useRef(null),Je=n.useRef(null),x=n.useRef(null),Ke=n.useRef(!1),J=n.useRef(null),K=n.useRef(null),he=n.useRef({frames:0,lastTime:Date.now()}),rt=n.useRef(0),Q=n.useRef(null),be=n.useRef(!1),xe=n.useRef(!1),so=n.useRef(0),U=n.useRef(null),co=n.useRef(null),Wo=n.useRef(null),po=n.useRef(null),Te=n.useRef(null),uo=n.useRef(0),me=n.useRef(!1),te=n.useRef(!1),go=n.useRef(!1),P=n.useRef(null),F=n.useRef(0),ve=n.useRef(null),Ue=(O==null?void 0:O.extensionId)||qo,zo=n.useCallback(()=>{const t=!!window.__TAURI_INTERNALS__,e=!t&&window.location.protocol==="https:"?"wss:":"ws:",r=t?"localhost:9375":window.location.host,s=`${e}//${r}/api/extensions/${Ue}/stream`,l=localStorage.getItem("neomind_token")||sessionStorage.getItem("neomind_token_session");return l?`${s}?token=${encodeURIComponent(l)}`:s},[Ue]),Do=n.useCallback(()=>{const t=!!window.__TAURI_INTERNALS__,e=t?"http:":window.location.protocol==="https:"?"https:":"http:",r=t?"localhost:9375":window.location.host;return`${e}//${r}`},[]),Mo=n.useCallback(async()=>{const t=Q.current;if(!t){console.warn("[YOLO] Cannot update config: no active session (start the stream first)");return}const e=localStorage.getItem("neomind_token")||sessionStorage.getItem("neomind_token_session"),r={"Content-Type":"application/json"};e&&(r.Authorization=`Bearer ${e}`);try{const s=await fetch(`${Do()}/api/extensions/${Ue}/command`,{method:"POST",headers:r,body:JSON.stringify({command:"update_stream_config",args:{stream_id:t,rois:Xe.current,lines:qe.current,capture_rules:$o.current}})});if(s.ok)console.log(`[YOLO] Config updated: ${Xe.current.length} ROI(s), ${qe.current.length} line(s)`);else{const l=await s.text().catch(()=>"");console.warn(`[YOLO] Config update failed: HTTP ${s.status}`,l)}}catch(s){console.warn("[YOLO] Config update error:",s)}},[Do,Ue]),z=n.useCallback(()=>{ve.current&&clearTimeout(ve.current),ve.current=setTimeout(()=>{Mo(),ve.current=null},150)},[Mo]),Eo=n.useCallback(()=>{if(!be.current||xe.current)return;const t=Date.now();if(t-so.current<50)return;const e=oe.current,r=Io.current;if(!e||!r||e.paused||e.ended)return;const s=r.getContext("2d");if(!s)return;s.drawImage(e,0,0,r.width,r.height);const l=x.current,c=Q.current;(l==null?void 0:l.readyState)===WebSocket.OPEN&&c&&(xe.current=!0,so.current=t,U.current&&clearTimeout(U.current),U.current=setTimeout(()=>{xe.current&&(console.warn("[YOLO] Frame lock timeout, auto-releasing"),xe.current=!1)},200),r.toBlob(u=>{var _;U.current&&(clearTimeout(U.current),U.current=null),xe.current=!1,be.current&&u&&((_=x.current)==null?void 0:_.readyState)===WebSocket.OPEN&&Q.current&&u.arrayBuffer().then(i=>{var $;const v=rt.current++,f=new ArrayBuffer(8);new DataView(f).setBigUint64(0,BigInt(v),!1);const N=new Uint8Array(8+i.byteLength);N.set(new Uint8Array(f),0),N.set(new Uint8Array(i),8),($=x.current)==null||$.send(N)}).catch(i=>{console.warn("[YOLO] Failed to send frame:",i)})},"image/jpeg",.8))},[]),Po=n.useCallback(async()=>{try{ro("pending");const t=await navigator.mediaDevices.getUserMedia({video:{width:{ideal:640},height:{ideal:480},facingMode:"user"},audio:!1});return ro("granted"),Je.current=t,oe.current&&(oe.current.srcObject=t,await oe.current.play()),!0}catch(t){return ro("denied"),t instanceof Error&&(t.name==="NotAllowedError"?E("Camera permission denied"):t.name==="NotFoundError"?E("No camera found"):E(`Camera error: ${t.message}`)),!1}},[]),Fo=n.useCallback(()=>{be.current=!1,xe.current=!1,so.current=0,U.current&&(clearTimeout(U.current),U.current=null),Je.current&&(Je.current.getTracks().forEach(t=>t.stop()),Je.current=null),oe.current&&(oe.current.srcObject=null)},[]),we=n.useCallback(()=>{const t=zo(),e=new WebSocket(t);e.binaryType="arraybuffer",e.onopen=()=>{F.current=0;const r={type:"init",config:{source_url:T,confidence_threshold:H,max_objects:V,target_fps:R,draw_boxes:ne,rois:Xe.current,lines:qe.current}};console.log("[YOLO] Sending init:",JSON.stringify(r)),e.send(JSON.stringify(r))},e.onmessage=r=>{var s,l,c,u,_,i,v,f,N,$,h,g,D;if(r.data instanceof ArrayBuffer){console.debug("[YOLO] Received binary response (no metadata), skipping");return}if(typeof r.data=="string")try{const a=JSON.parse(r.data);switch(a.type){case"session_created":Q.current=a.session_id,ee(!0),se(0),J.current=setInterval(()=>se(S=>S+1),1e3),q==="camera"?(be.current=!0,K.current=setInterval(Eo,50)):A("connecting");break;case"push_output":if(a.data_type==="application/json"&&a.data){try{const S=typeof a.data=="string"?JSON.parse(a.data):a.data;S.type==="status"&&S.status?A(S.status):S.type==="error"&&(A("error"),E(S.message||"Stream error"))}catch{}break}a.data&&a.data_type==="image/jpeg"&&(A("streaming"),me.current||(me.current=!0,Pe(a.data)),Te.current=a.data,Yo(),(s=a.metadata)!=null&&s.detections&&ce(a.metadata.detections),(l=a.metadata)!=null&&l.roi_stats&&pe(a.metadata.roi_stats),(c=a.metadata)!=null&&c.line_stats&&ge(a.metadata.line_stats),(u=a.metadata)!=null&&u.capture_events&&a.metadata.capture_events.length>0&&ao(S=>[...a.metadata.capture_events,...S].slice(0,10)));break;case"result":if(a.data){const S=a.skipped===!0||typeof a.data=="string"&&(a.data.startsWith("{")||((_=a.metadata)==null?void 0:_.skipped)===!0),Y=((i=a.metadata)==null?void 0:i.status)==="waiting";S?(v=a.metadata)!=null&&v.detections&&ce(a.metadata.detections):Y||typeof a.data=="string"&&a.data.length>0&&(me.current||(me.current=!0,Pe(a.data)),Te.current=a.data,Yo(),(f=a.metadata)!=null&&f.frame_count?Ee(a.metadata.frame_count):Ee(B=>B+1),(N=a.metadata)!=null&&N.fps&&Me(a.metadata.fps),($=a.metadata)!=null&&$.detections&&ce(a.metadata.detections),(h=a.metadata)!=null&&h.roi_stats&&pe(a.metadata.roi_stats),(g=a.metadata)!=null&&g.line_stats&&ge(a.metadata.line_stats),(D=a.metadata)!=null&&D.capture_events&&a.metadata.capture_events.length>0&&ao(B=>[...a.metadata.capture_events,...B].slice(0,10)))}break;case"error":if(a.message&&a.message.includes("Frame rate too high")){console.debug("[YOLO] Frame dropped due to rate limiting (normal)");break}E(`${a.code}: ${a.message}`);break;case"session_closed":ee(!1),Q.current=null;break}}catch(a){console.error("[YOLO] Failed to parse message:",a)}},e.onerror=r=>{if(Ke.current)return;console.error("[YOLO] WebSocket error:",r);const s=x.current;if(s){const l=F.current;setTimeout(()=>{if(x.current===s&&x.current&&(x.current.readyState===WebSocket.CLOSING||x.current.readyState===WebSocket.CLOSED)&&(console.warn("[YOLO] onclose not fired after onerror — forcing reconnect"),x.current=null,Q.current=null,be.current=!1,J.current&&(clearInterval(J.current),J.current=null),K.current&&(clearInterval(K.current),K.current=null),l===F.current&&te.current)){A("reconnecting");const c=Math.min(1e3*2**F.current,3e4);P.current=setTimeout(()=>{F.current++,P.current=null,we()},c)}},2e3)}},e.onclose=()=>{const r=Ke.current;if(x.current=null,Q.current=null,be.current=!1,Ke.current=!1,J.current&&(clearInterval(J.current),J.current=null),K.current&&(clearInterval(K.current),K.current=null),r||!te.current){E(null),A("idle"),ee(!1),ce([]),F.current=0;return}if(go.current){A("reconnecting");return}const s=10,l=F.current;if(l>=s){E(`连接断开，自动重连失败（已尝试 ${s} 次），请手动重新开始`),A("error"),ee(!1),te.current=!1,F.current=0;return}const c=Math.min(1e3*2**l,3e4);console.log(`[YOLO] 连接断开，${c}ms 后自动重连 (${l+1}/${s})`),A("reconnecting"),P.current=setTimeout(()=>{F.current++,P.current=null,we()},c)},x.current=e},[zo,T,H,V,q,Eo,R,ne]),Yo=()=>{he.current.frames++;const t=Date.now(),e=t-he.current.lastTime;e>=1e3&&(Me(Math.round(he.current.frames*1e3/e)),he.current.frames=0,he.current.lastTime=t)},Ao=n.useCallback(()=>{te.current=!1,P.current&&(clearTimeout(P.current),P.current=null),F.current=0,x.current&&(Ke.current=!0,x.current.readyState===WebSocket.OPEN&&x.current.send(JSON.stringify({type:"close"})),x.current.close(),x.current=null,J.current&&(clearInterval(J.current),J.current=null),K.current&&(clearInterval(K.current),K.current=null),ee(!1),Q.current=null,ce([]))},[]),lt=n.useCallback(async()=>{E(null),Pe(null),Me(0),Ee(0),me.current=!1,te.current=!0,F.current=0,he.current={frames:0,lastTime:Date.now()},!(q==="camera"&&!await Po())&&we()},[q,Po,we]),yo=n.useCallback(()=>{q==="camera"&&Fo(),Ao(),A("idle"),ce([]),Me(0),Ee(0),se(0),Pe(null),me.current=!1,Te.current=null,po.current=null,pe([]),ge([]),ao([])},[q,Fo,Ao]);n.useEffect(()=>{const t=()=>{if(document.hidden){if(x.current){if(go.current=!0,x.current.readyState===WebSocket.OPEN)try{x.current.send(JSON.stringify({type:"close"}))}catch{}x.current.close(),x.current=null,A("reconnecting")}}else go.current=!1,te.current&&(!x.current||x.current.readyState!==WebSocket.OPEN)&&(P.current&&(clearTimeout(P.current),P.current=null),F.current=0,we())};return document.addEventListener("visibilitychange",t),()=>document.removeEventListener("visibilitychange",t)},[we]),n.useEffect(()=>()=>{te.current=!1,P.current&&(clearTimeout(P.current),P.current=null)},[]);const fo=n.useCallback(()=>{if(C.length<3)return;const t={id:to(),name:`ROI ${y.length+1}`,points:C,class_filter:[],color:De[(y.length+m.length)%De.length]};Ye(e=>[...e,t]),je([]),Se("none"),p&&z()},[C,y.length,m.length,p,z]),at=n.useCallback(()=>{if(!j||!X)return;const t={id:to(),name:`Line ${m.length+1}`,start:j,end:X,color:De[(y.length+m.length)%De.length]},e=[...m,t];Ae(e),Be(null),lo(null),Se("none"),p&&z()},[j,X,m,y.length,p,z]),it=.03,st=n.useCallback(t=>{if(L==="none")return;const e=co.current;if(!e)return;const r=e.getBoundingClientRect(),s=(t.clientX-r.left)/r.width,l=(t.clientY-r.top)/r.height;if(L==="roi"){if(C.length>=3){const c=C[0];if(Math.sqrt((s-c[0])**2+(l-c[1])**2)<it){fo();return}}je(c=>[...c,[s,l]])}else L==="line"&&(j?X||lo([s,l]):Be([s,l]))},[L,j,X,C,fo]),ho=n.useCallback(()=>{je([]),Be(null),lo(null),Se("none")},[]),jo=n.useCallback(t=>{Ye(e=>e.filter(r=>r.id!==t)),pe(e=>e.filter(r=>r.id!==t)),p&&z()},[p,z]),Bo=n.useCallback(t=>{Ae(e=>e.filter(r=>r.id!==t)),ge(e=>e.filter(r=>r.id!==t)),p&&z()},[p,z]),Ge=n.useCallback((t,e)=>{io(t),Ne(e)},[]),Z=n.useCallback(t=>{const e=fe.trim();if(io(null),!e)return;let r=!1;Ye(s=>s.map(l=>l.id===t&&l.name!==e?(r=!0,{...l,name:e}):l)),Ae(s=>s.map(l=>l.id===t&&l.name!==e?(r=!0,{...l,name:e}):l)),pe(s=>s.map(l=>l.id===t?{...l,name:e}:l)),ge(s=>s.map(l=>l.id===t?{...l,name:e}:l)),r&&p&&z()},[fe,p,z]),Qe=n.useCallback(()=>io(null),[]),ct=n.useCallback((t,e,r)=>{const s=y.find(u=>u.id===t),l=e.type==="threshold"?`${e.class_name}≥${e.threshold}`:e.type==="presence"?`${e.class_name} appears`:`${e.class_name} gone`,c={id:to(),name:s?`${s.name}: ${l}`:l,roi_id:t,condition:e,cooldown_seconds:r,quality:80};No(u=>[...u,c]),Ce(null),p&&setTimeout(()=>z(),50)},[y,p,z]),Ho=n.useCallback(t=>{No(e=>e.filter(r=>r.id!==t)),p&&setTimeout(()=>z(),50)},[p,z]),Ze=n.useCallback(()=>{const t=co.current;if(!t)return;const e=t.getContext("2d");if(!e)return;const r=Wo.current;if(!r)return;const s=r.clientWidth,l=r.clientHeight;t.width=s,t.height=l,e.clearRect(0,0,s,l);const c=s,u=l,_=po.current;if(_&&_.complete&&_.naturalWidth>0){const i=_.naturalWidth/_.naturalHeight,v=c/u;let f=0,N=0,$=_.naturalWidth,h=_.naturalHeight;i>v?($=_.naturalHeight*v,f=(_.naturalWidth-$)/2):(h=_.naturalWidth/v,N=(_.naturalHeight-h)/2),e.drawImage(_,f,N,$,h,0,0,c,u)}for(const i of y){if(i.points.length<3)continue;e.beginPath(),e.moveTo(i.points[0][0]*c,i.points[0][1]*u);for(let h=1;h<i.points.length;h++)e.lineTo(i.points[h][0]*c,i.points[h][1]*u);e.closePath(),e.fillStyle=G(i.color,.15),e.fill(),e.strokeStyle=i.color,e.lineWidth=2,e.stroke();const v=i.points.reduce((h,g)=>h+g[0],0)/i.points.length*c,f=i.points.reduce((h,g)=>h+g[1],0)/i.points.length*u;e.font="bold 12px -apple-system, sans-serif";const N=de.find(h=>h.id===i.id),$=e.measureText(i.name);if(N){const h=String(N.count),g=e.measureText(h),D=6,a=$.width+D+g.width+16,S=v-a/2,Y=f-9,B=18;e.fillStyle=G(i.color,.9),e.beginPath(),e.roundRect(S,Y,a,B,3),e.fill(),e.fillStyle="rgba(255,255,255,0.8)",e.textAlign="left",e.textBaseline="middle",e.fillText(i.name,S+6,f),e.fillStyle="#fff",e.font="bold 12px -apple-system, sans-serif",e.fillText(h,S+$.width+D+6,f),e.font="bold 12px -apple-system, sans-serif"}else{const h=$.width+12,g=v-h/2,D=f-9;e.fillStyle=G(i.color,.9),e.beginPath(),e.roundRect(g,D,h,18,3),e.fill(),e.fillStyle="#fff",e.textAlign="center",e.textBaseline="middle",e.fillText(i.name,v,f)}}for(const i of m){const v=i.start[0]*c,f=i.start[1]*u,N=i.end[0]*c,$=i.end[1]*u;e.beginPath(),e.moveTo(v,f),e.lineTo(N,$),e.strokeStyle=i.color,e.lineWidth=2,e.setLineDash([6,3]),e.stroke(),e.setLineDash([]);const g=Math.atan2($-f,N-v)+Math.PI/2,D=(v+N)/2,a=(f+$)/2,S=12,Y=5,B=D+Math.cos(g)*S,_e=a+Math.sin(g)*S;e.beginPath(),e.moveTo(D+Math.cos(g)*4,a+Math.sin(g)*4),e.lineTo(B,_e),e.strokeStyle="#4ade80",e.lineWidth=2,e.stroke(),e.beginPath(),e.moveTo(B,_e),e.lineTo(B-Y*Math.cos(g-.5),_e-Y*Math.sin(g-.5)),e.moveTo(B,_e),e.lineTo(B-Y*Math.cos(g+.5),_e-Y*Math.sin(g+.5)),e.stroke();const Oe=D-Math.cos(g)*S,Le=a-Math.sin(g)*S;e.beginPath(),e.moveTo(D-Math.cos(g)*4,a-Math.sin(g)*4),e.lineTo(Oe,Le),e.strokeStyle="#60a5fa",e.lineWidth=2,e.stroke(),e.beginPath(),e.moveTo(Oe,Le),e.lineTo(Oe+Y*Math.cos(g-.5),Le+Y*Math.sin(g-.5)),e.moveTo(Oe,Le),e.lineTo(Oe+Y*Math.cos(g+.5),Le+Y*Math.sin(g+.5)),e.stroke(),e.strokeStyle=i.color,e.lineWidth=2;const bo=ue.find($e=>$e.id===i.id);if(e.font="bold 11px -apple-system, sans-serif",bo){const $e=e.measureText(i.name),Ie=`→${bo.forward_count}`,Re=`←${bo.backward_count}`,We=e.measureText(Ie),ut=e.measureText(Re),eo=5,Vo=$e.width+eo+We.width+eo+ut.width+14,Xo=D-Vo/2,oo=a-18;e.fillStyle=G(i.color,.9),e.beginPath(),e.roundRect(Xo,oo,Vo,18,3),e.fill();let ze=Xo+7;e.fillStyle="rgba(255,255,255,0.8)",e.textAlign="left",e.textBaseline="middle",e.fillText(i.name,ze,oo+9),ze+=$e.width+eo,e.fillStyle="#4ade80",e.fillText(Ie,ze,oo+9),ze+=We.width+eo,e.fillStyle="#60a5fa",e.fillText(Re,ze,oo+9)}else{const Ie=e.measureText(i.name).width+12,Re=D-Ie/2,We=a-18;e.fillStyle=G(i.color,.9),e.beginPath(),e.roundRect(Re,We,Ie,18,3),e.fill(),e.fillStyle="#fff",e.textAlign="center",e.textBaseline="middle",e.fillText(i.name,D,We+9)}}if(L==="roi"&&C.length>0){e.beginPath(),e.moveTo(C[0][0]*c,C[0][1]*u);for(let i=1;i<C.length;i++)e.lineTo(C[i][0]*c,C[i][1]*u);e.strokeStyle="#3b82f6",e.lineWidth=2,e.setLineDash([4,4]),e.stroke(),e.setLineDash([]);for(let i=0;i<C.length;i++){const v=C[i],f=i===0;e.beginPath(),e.arc(v[0]*c,v[1]*u,f?6:4,0,Math.PI*2),e.fillStyle=f?"#22c55e":"#3b82f6",e.fill(),e.strokeStyle="#fff",e.lineWidth=1,e.stroke()}if(C.length>=3){const i=C[0];e.beginPath(),e.arc(i[0]*c,i[1]*u,12,0,Math.PI*2),e.strokeStyle="rgba(34,197,94,0.5)",e.lineWidth=2,e.setLineDash([3,3]),e.stroke(),e.setLineDash([]),e.fillStyle="rgba(0,0,0,0.6)",e.font="10px -apple-system, sans-serif",e.textAlign="center",e.fillText("Click green point to close",c/2,u-10)}}if(L==="line"&&j){const i=j[0]*c,v=j[1]*u;if(e.beginPath(),e.arc(i,v,4,0,Math.PI*2),e.fillStyle="#22c55e",e.fill(),e.strokeStyle="#fff",e.lineWidth=1,e.stroke(),X){const f=X[0]*c,N=X[1]*u;e.beginPath(),e.moveTo(i,v),e.lineTo(f,N),e.strokeStyle="#22c55e",e.lineWidth=2,e.setLineDash([6,3]),e.stroke(),e.setLineDash([]),e.beginPath(),e.arc(f,N,4,0,Math.PI*2),e.fillStyle="#22c55e",e.fill(),e.strokeStyle="#fff",e.lineWidth=1,e.stroke(),e.fillStyle="rgba(0,0,0,0.6)",e.font="10px -apple-system, sans-serif",e.textAlign="center",e.fillText("Click Save to confirm",c/2,u-10)}else e.fillStyle="rgba(0,0,0,0.6)",e.font="10px -apple-system, sans-serif",e.textAlign="center",e.fillText("Click to set end point",c/2,u-10)}},[y,m,de,ue,C,j,X,L]);n.useEffect(()=>{let t=!0;const e=()=>{if(!t)return;const r=Te.current;if(r){Te.current=null;const s=new Image;s.onload=()=>{t&&(po.current=s,Ze())},s.src=`data:image/jpeg;base64,${r}`}uo.current=requestAnimationFrame(e)};return uo.current=requestAnimationFrame(e),()=>{t=!1,cancelAnimationFrame(uo.current)}},[Ze]),n.useEffect(()=>{Ze()},[y,m,de,ue,C,j,L,Ze]),n.useEffect(()=>()=>{yo(),ve.current&&clearTimeout(ve.current)},[yo]);const dt=t=>{const e=Math.floor(t/60),r=t%60;return`${e.toString().padStart(2,"0")}:${r.toString().padStart(2,"0")}`},pt=()=>q==="network"?p&&Fe==="reconnecting"?"Reconnecting...":p&&Fe==="error"?"Error":T.startsWith("rtsp://")?"RTSP":T.startsWith("rtmp://")?"RTMP":T.startsWith("hls://")||T.includes(".m3u8")?"HLS":"Network":"CAM";return o.jsx("div",{ref:le,className:`yolo ${M}`,children:o.jsxs("div",{className:"yolo-card",children:[o.jsxs("div",{className:"yolo-header",children:[o.jsxs("div",{className:"yolo-title-cluster",children:[o.jsxs("div",{className:"yolo-title",children:[o.jsx(w,{name:"camera",className:"yolo-title-icon"}),k]}),p&&o.jsxs("div",{className:"yolo-status",children:[o.jsx("span",{className:`yolo-status-dot${Fe==="reconnecting"?" yolo-status-warning":Fe==="error"?" yolo-status-error":""}`}),pt()]})]}),o.jsxs("div",{className:"yolo-controls",children:[o.jsx("button",{className:`yolo-draw-btn${L==="roi"?" yolo-draw-active":""}`,onClick:()=>{L==="roi"?ho():(Se("roi"),Be(null))},title:"Draw ROI polygon",children:o.jsx(w,{name:"polygon",style:{width:12,height:12}})}),o.jsx("button",{className:`yolo-draw-btn${L==="line"?" yolo-draw-active":""}`,onClick:()=>{L==="line"?ho():(Se("line"),je([]))},title:"Draw crossing line",children:o.jsx(w,{name:"line",style:{width:12,height:12}})}),L==="roi"&&C.length>=3&&o.jsx("button",{className:"yolo-draw-btn",onClick:fo,title:"Finish ROI",children:o.jsx(w,{name:"play",style:{width:11,height:11}})}),L==="line"&&j&&X&&o.jsx("button",{className:"yolo-draw-btn yolo-draw-success",onClick:at,title:"Save line",children:o.jsx(w,{name:"play",style:{width:11,height:11}})}),L!=="none"&&o.jsx("button",{className:"yolo-draw-btn yolo-draw-danger",onClick:ho,title:"Cancel",children:o.jsx(w,{name:"close",style:{width:12,height:12}})}),y.length+m.length>0&&L==="none"&&o.jsx("button",{className:"yolo-draw-btn yolo-draw-danger",onClick:()=>{Ye([]),Ae([]),pe([]),ge([]),p&&z()},title:"Clear all",children:o.jsx(w,{name:"trash",style:{width:12,height:12}})}),o.jsx("span",{className:"yolo-draw-divider"}),p?o.jsxs("button",{onClick:yo,className:"yolo-btn yolo-btn-stop",children:[o.jsx(w,{name:"stop",style:{width:12,height:12}}),"Stop"]}):o.jsxs("button",{onClick:lt,className:"yolo-btn",children:[o.jsx(w,{name:"play",style:{width:12,height:12}}),"Start"]})]})]}),o.jsxs("div",{className:"yolo-video-wrap",ref:Wo,children:[q==="camera"&&o.jsxs(o.Fragment,{children:[o.jsx("video",{ref:oe,style:{display:"none"},playsInline:!0,muted:!0}),o.jsx("canvas",{ref:Io,width:640,height:480,style:{display:"none"}})]}),o.jsx("canvas",{ref:co,className:"yolo-video-frame",style:{cursor:L!=="none"?"crosshair":"default"},onClick:st}),ae&&o.jsxs("div",{className:"yolo-error",children:[o.jsx(w,{name:"alert",className:"yolo-error-icon"}),o.jsx("div",{className:"yolo-error-text",children:ae})]}),!p&&!ae&&o.jsxs("div",{className:"yolo-video-placeholder",children:[o.jsx(w,{name:"video",className:"yolo-video-icon"}),o.jsx("div",{className:"yolo-video-text",children:q==="camera"?"Click Start to begin detection":`Click Start to connect to ${T}`})]}),p&&!tt&&!ae&&o.jsxs("div",{className:"yolo-video-loading",children:[o.jsx("div",{className:"yolo-spinner"}),o.jsx("div",{className:"yolo-video-text",children:q==="camera"?"Starting camera...":"Connecting..."})]}),p&&I&&o.jsxs("div",{className:"yolo-overlay-stats",children:[o.jsx(w,{name:"clock",className:"yolo-overlay-stat-icon"}),o.jsx("span",{className:"yolo-overlay-stat-val",children:dt(ie)}),o.jsx("span",{className:"yolo-overlay-sep"}),o.jsx("span",{className:"yolo-overlay-stat-val",children:b}),o.jsx("span",{children:"fps"}),o.jsx("span",{className:"yolo-overlay-sep"}),o.jsx("span",{className:"yolo-overlay-stat-val",children:no.length}),o.jsx("span",{children:"obj"})]}),p&&no.length>0&&o.jsx("div",{className:"yolo-overlay-detections",children:(()=>{const t=new Map;for(const r of no){const s=r.class_id||0,l=t.get(s);l?l.count++:t.set(s,{label:r.label,count:1})}return[...t.entries()].sort((r,s)=>s[1].count-r[1].count).map(([r,{label:s,count:l}])=>{const c=Uo(r);return o.jsxs("span",{className:"yolo-detection-tag",style:{backgroundColor:c.bg,color:c.fg,border:`1px solid ${c.border}`},children:[s,o.jsxs("span",{style:{opacity:.8,fontWeight:700},children:["×",l]})]},r)})})()}),(de.length>0||ue.length>0||y.length>0||m.length>0)&&o.jsx("div",{className:"yolo-regions-float",children:nt?o.jsxs("button",{className:"yolo-regions-chip",onClick:()=>Lo(!1),title:"Show regions & lines",children:[o.jsx("span",{className:"yolo-regions-chip-dot"}),y.length>0&&o.jsxs(o.Fragment,{children:[o.jsx("b",{children:y.length})," ROI",y.length>1?"s":""]}),y.length>0&&m.length>0&&o.jsx("span",{className:"yolo-regions-chip-sep",children:"·"}),m.length>0&&o.jsxs(o.Fragment,{children:[o.jsx("b",{children:m.length})," Line",m.length>1?"s":""]}),o.jsx("span",{className:"yolo-regions-chip-arrow",children:"▸"})]}):o.jsxs("div",{className:"yolo-regions-panel",children:[o.jsxs("button",{className:"yolo-regions-panel-header",onClick:()=>Lo(!0),title:"Collapse",children:[o.jsxs("span",{className:"yolo-regions-summary",children:[y.length>0&&o.jsxs(o.Fragment,{children:[o.jsx("span",{className:"yolo-regions-count",children:y.length})," ROI",y.length>1?"s":""]}),y.length>0&&m.length>0&&o.jsx("span",{className:"yolo-regions-dot",children:"·"}),m.length>0&&o.jsxs(o.Fragment,{children:[o.jsx("span",{className:"yolo-regions-count",children:m.length})," Line",m.length>1?"s":""]})]}),o.jsx("span",{className:"yolo-regions-toggle",children:"▾"})]}),o.jsxs("div",{className:"yolo-region-pills",children:[de.map(t=>{const e=y.find(l=>l.id===t.id),r=(e==null?void 0:e.color)||"#3b82f6",s=He.filter(l=>l.roi_id===t.id);return o.jsxs("span",{className:"yolo-region-pill","data-rules-open":ye===t.id,style:{borderColor:G(r,.4)},children:[o.jsx("span",{className:"yolo-region-pill-dot",style:{background:r}}),Ve===t.id?o.jsx("input",{className:"yolo-region-pill-input",value:fe,autoFocus:!0,onChange:l=>Ne(l.target.value),onBlur:()=>Z(t.id),onClick:l=>l.stopPropagation(),onKeyDown:l=>{l.key==="Enter"?Z(t.id):l.key==="Escape"&&Qe()}}):o.jsx("span",{className:"yolo-region-pill-name yolo-region-pill-name-editable",onDoubleClick:()=>Ge(t.id,t.name),title:"Double-click to rename",children:t.name}),t.count>0&&o.jsx("span",{className:"yolo-region-pill-count",style:{background:G(r,.15),color:r},children:t.count}),o.jsx("button",{className:"yolo-region-pill-btn yolo-region-pill-edit",onClick:()=>Ce(ye===t.id?null:t.id),title:"Edit capture rules",children:o.jsx(w,{name:"edit",style:{width:9,height:9}})}),o.jsx("button",{className:"yolo-region-pill-btn",onClick:()=>jo(t.id),title:"Delete",children:o.jsx(w,{name:"x",style:{width:9,height:9}})}),s.map(l=>o.jsxs("span",{className:"yolo-region-pill-rule",children:[l.condition.type==="threshold"?`${l.condition.class_name}≥${l.condition.threshold}`:l.condition.type==="presence"?`${l.condition.class_name}↑`:`${l.condition.class_name}↓`,o.jsx("button",{className:"yolo-rule-pill-btn",onClick:()=>Ho(l.id),children:o.jsx(w,{name:"x",style:{width:7,height:7}})})]},l.id))]},t.id)}),y.filter(t=>!de.some(e=>e.id===t.id)).map(t=>{const e=He.filter(r=>r.roi_id===t.id);return o.jsxs("span",{className:"yolo-region-pill","data-rules-open":ye===t.id,style:{borderColor:G(t.color,.4)},children:[o.jsx("span",{className:"yolo-region-pill-dot",style:{background:t.color}}),Ve===t.id?o.jsx("input",{className:"yolo-region-pill-input",value:fe,autoFocus:!0,onChange:r=>Ne(r.target.value),onBlur:()=>Z(t.id),onClick:r=>r.stopPropagation(),onKeyDown:r=>{r.key==="Enter"?Z(t.id):r.key==="Escape"&&Qe()}}):o.jsx("span",{className:"yolo-region-pill-name yolo-region-pill-name-editable",onDoubleClick:()=>Ge(t.id,t.name),title:"Double-click to rename",children:t.name}),o.jsx("button",{className:"yolo-region-pill-btn yolo-region-pill-edit",onClick:()=>Ce(ye===t.id?null:t.id),title:"Edit capture rules",children:o.jsx(w,{name:"edit",style:{width:9,height:9}})}),o.jsx("button",{className:"yolo-region-pill-btn",onClick:()=>jo(t.id),title:"Delete",children:o.jsx(w,{name:"x",style:{width:9,height:9}})}),e.map(r=>o.jsxs("span",{className:"yolo-region-pill-rule",children:[r.condition.type==="threshold"?`${r.condition.class_name}≥${r.condition.threshold}`:r.condition.type==="presence"?`${r.condition.class_name}↑`:`${r.condition.class_name}↓`,o.jsx("button",{className:"yolo-rule-pill-btn",onClick:()=>Ho(r.id),children:o.jsx(w,{name:"x",style:{width:7,height:7}})})]},r.id))]},t.id)}),ue.map(t=>o.jsxs("span",{className:"yolo-region-pill yolo-region-pill-line",children:[Ve===t.id?o.jsx("input",{className:"yolo-region-pill-input",value:fe,autoFocus:!0,onChange:e=>Ne(e.target.value),onBlur:()=>Z(t.id),onClick:e=>e.stopPropagation(),onKeyDown:e=>{e.key==="Enter"?Z(t.id):e.key==="Escape"&&Qe()}}):o.jsx("span",{className:"yolo-region-pill-name yolo-region-pill-name-editable",onDoubleClick:()=>Ge(t.id,t.name),title:"Double-click to rename",children:t.name}),o.jsxs("span",{className:"yolo-region-pill-count",style:{background:"rgba(34,197,94,0.15)",color:"#22c55e"},children:["→",t.forward_count]}),o.jsxs("span",{className:"yolo-region-pill-count",style:{background:"rgba(59,130,246,0.15)",color:"#3b82f6"},children:["←",t.backward_count]}),o.jsx("button",{className:"yolo-region-pill-btn",onClick:()=>Bo(t.id),title:"Delete",children:o.jsx(w,{name:"x",style:{width:9,height:9}})})]},t.id)),m.filter(t=>!ue.some(e=>e.id===t.id)).map(t=>o.jsxs("span",{className:"yolo-region-pill yolo-region-pill-line",children:[Ve===t.id?o.jsx("input",{className:"yolo-region-pill-input",value:fe,autoFocus:!0,onChange:e=>Ne(e.target.value),onBlur:()=>Z(t.id),onClick:e=>e.stopPropagation(),onKeyDown:e=>{e.key==="Enter"?Z(t.id):e.key==="Escape"&&Qe()}}):o.jsx("span",{className:"yolo-region-pill-name yolo-region-pill-name-editable",onDoubleClick:()=>Ge(t.id,t.name),title:"Double-click to rename",children:t.name}),o.jsx("button",{className:"yolo-region-pill-btn",onClick:()=>Bo(t.id),title:"Delete",children:o.jsx(w,{name:"x",style:{width:9,height:9}})})]},t.id))]})]})})]}),ye&&o.jsx("div",{style:{position:"fixed",inset:0,zIndex:1e4,display:"flex",alignItems:"center",justifyContent:"center",background:"rgba(0,0,0,0.4)",backdropFilter:"blur(4px)",WebkitBackdropFilter:"blur(4px)"},onClick:()=>Ce(null),children:o.jsx(Qo,{roiId:ye,onAdd:ct,onCancel:()=>Ce(null)})}),To.length>0&&o.jsx("div",{className:"yolo-captures",children:To.map((t,e)=>o.jsxs("div",{className:"yolo-capture-item",title:`${t.rule_name}
${t.condition}
${new Date(t.timestamp).toLocaleTimeString()}`,onClick:()=>Oo(`data:image/jpeg;base64,${t.image_base64}`),children:[o.jsx("img",{src:`data:image/jpeg;base64,${t.image_base64}`,alt:t.rule_name}),o.jsx("span",{className:"yolo-capture-label",children:t.rule_name})]},`${t.rule_id}-${t.timestamp}-${e}`))}),_o&&o.jsx("div",{style:{position:"fixed",inset:0,zIndex:2e4,display:"flex",alignItems:"center",justifyContent:"center",background:"rgba(0,0,0,0.7)",backdropFilter:"blur(6px)",WebkitBackdropFilter:"blur(6px)",cursor:"zoom-out"},onClick:()=>Oo(null),children:o.jsx("img",{src:_o,alt:"capture",style:{maxWidth:"90vw",maxHeight:"85vh",borderRadius:"8px",boxShadow:"0 8px 40px rgba(0,0,0,0.4)"},onClick:t=>t.stopPropagation()})})]})})});function wo({value:d,options:k,open:O,onToggle:M,onChange:H}){const V=k.find(I=>I.value===d),T={width:"100%",height:"36px",fontSize:"13px",padding:"0 10px",border:O?"1px solid var(--yolo-accent)":"1px solid var(--yolo-border)",borderRadius:"6px",background:"var(--yolo-card)",color:"var(--yolo-fg)",boxSizing:"border-box",fontFamily:"inherit",cursor:"pointer",display:"flex",alignItems:"center",justifyContent:"space-between",outline:"none",transition:"border-color 0.15s",boxShadow:O?"0 0 0 3px rgba(59,130,246,0.12)":"none"},R={position:"absolute",left:0,right:0,top:"100%",marginTop:"4px",background:"var(--yolo-card)",border:"1px solid var(--yolo-border)",borderRadius:"6px",boxShadow:"0 4px 20px rgba(0,0,0,0.1)",maxHeight:"180px",overflowY:"auto",zIndex:100,padding:"4px"},ne=I=>({padding:"6px 10px",fontSize:"13px",cursor:"pointer",borderRadius:"4px",background:I?"var(--yolo-accent)":"transparent",color:I?"var(--yolo-on-primary)":"var(--yolo-fg)",transition:"background 0.1s"});return o.jsxs("div",{style:{position:"relative"},children:[o.jsxs("button",{type:"button",style:T,onClick:M,children:[o.jsx("span",{children:(V==null?void 0:V.label)||d}),o.jsx("svg",{width:"12",height:"12",viewBox:"0 0 12 12",fill:"none",style:{opacity:.5,flexShrink:0},children:o.jsx("path",{d:"M3 4.5L6 7.5L9 4.5",stroke:"currentColor",strokeWidth:"1.5",strokeLinecap:"round",strokeLinejoin:"round"})})]}),O&&o.jsx("div",{style:R,children:k.map(I=>o.jsx("div",{style:ne(I.value===d),onClick:()=>H(I.value),onMouseEnter:re=>{I.value!==d&&(re.currentTarget.style.background="var(--yolo-hover)")},onMouseLeave:re=>{I.value!==d&&(re.currentTarget.style.background="transparent")},children:I.label},I.value))})]})}function Qo({roiId:d,onAdd:k,onCancel:O}){const[M,H]=n.useState("threshold"),[V,T]=n.useState("person"),[R,ne]=n.useState(3),[I,re]=n.useState(5),[le,p]=n.useState(null),ee=[{value:"threshold",label:"Threshold (count ≥ N)"},{value:"presence",label:"Presence (appears)"},{value:"absence",label:"Absence (disappears)"}],ae=["person","car","truck","bus","bicycle","motorcycle","dog","cat","bird","chair","bottle","cell phone","backpack","umbrella","handbag","suitcase"],E={fontSize:"12px",fontWeight:500,color:"var(--yolo-muted)"},ie={display:"flex",flexDirection:"column",gap:"6px",marginBottom:"14px"},se={width:"100%",height:"36px",fontSize:"13px",padding:"0 10px",border:"1px solid var(--yolo-border)",borderRadius:"6px",background:"var(--yolo-card)",color:"var(--yolo-fg)",outline:"none",boxSizing:"border-box",fontFamily:"inherit"};return o.jsxs("div",{style:{background:"var(--yolo-card)",border:"1px solid var(--yolo-border)",borderRadius:"12px",padding:"20px",minWidth:"300px",maxWidth:"360px",boxShadow:"0 20px 60px rgba(0,0,0,0.15), 0 0 0 1px rgba(0,0,0,0.05)",fontSize:"13px",fontFamily:'-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif',color:"var(--yolo-fg)"},onClick:b=>b.stopPropagation(),children:[o.jsx("div",{style:{fontSize:"15px",fontWeight:600,marginBottom:"16px",paddingBottom:"12px",borderBottom:"1px solid var(--yolo-border)"},children:"Add Capture Rule"}),o.jsxs("label",{style:ie,children:[o.jsx("span",{style:E,children:"Condition"}),o.jsx(wo,{value:M,options:ee,open:le==="cond",onToggle:()=>p(le==="cond"?null:"cond"),onChange:b=>{H(b),p(null)}})]}),o.jsxs("label",{style:ie,children:[o.jsx("span",{style:E,children:"Class"}),o.jsx(wo,{value:V,options:ae.map(b=>({value:b,label:b})),open:le==="class",onToggle:()=>p(le==="class"?null:"class"),onChange:b=>{T(b),p(null)}})]}),M==="threshold"&&o.jsxs("label",{style:ie,children:[o.jsx("span",{style:E,children:"Threshold"}),o.jsx("input",{style:se,type:"number",min:1,max:100,value:R,onChange:b=>ne(Number(b.target.value))})]}),o.jsxs("label",{style:ie,children:[o.jsx("span",{style:E,children:"Cooldown (s)"}),o.jsx("input",{style:se,type:"number",min:1,max:300,value:I,onChange:b=>re(Number(b.target.value))})]}),o.jsxs("div",{style:{display:"flex",justifyContent:"flex-end",gap:"8px",marginTop:"18px",paddingTop:"14px",borderTop:"1px solid var(--yolo-border)"},children:[o.jsx("button",{style:{height:"34px",padding:"0 16px",fontSize:"13px",fontWeight:500,border:"1px solid var(--yolo-border)",borderRadius:"6px",background:"var(--yolo-card)",color:"var(--yolo-muted)",cursor:"pointer",fontFamily:"inherit"},onClick:O,onMouseEnter:b=>b.currentTarget.style.background="var(--yolo-hover)",onMouseLeave:b=>b.currentTarget.style.background="var(--yolo-card)",children:"Cancel"}),o.jsx("button",{style:{height:"34px",padding:"0 16px",fontSize:"13px",fontWeight:500,border:"none",borderRadius:"6px",background:"var(--yolo-accent)",color:"var(--yolo-on-primary)",cursor:"pointer",fontFamily:"inherit"},onClick:()=>{k(d,M==="threshold"?{type:"threshold",class_name:V,threshold:R}:{type:M,class_name:V},I)},onMouseEnter:b=>b.currentTarget.style.opacity="0.9",onMouseLeave:b=>b.currentTarget.style.opacity="1",children:"Add Rule"})]})]})}const ko=n.forwardRef((d,k)=>o.jsx("div",{ref:k,style:{height:"100%",minHeight:300},children:o.jsx(ke,{...d,title:d.title||"YOLO Detection"})})),So=n.forwardRef((d,k)=>o.jsx("div",{ref:k,style:{height:280},children:o.jsx(ke,{...d,title:d.title||"YOLO"})})),Co=n.forwardRef((d,k)=>o.jsx("div",{ref:k,style:{height:"100%",minHeight:500},children:o.jsx(ke,{...d,title:d.title||"YOLO Video Detection"})})),Zo={YoloVideoDisplay:ke},Ro=ko,et=So,ot=Co;W.Card=Ro,W.Panel=ot,W.Widget=et,W.YoloVideoCard=ko,W.YoloVideoDisplay=ke,W.YoloVideoPanel=Co,W.YoloVideoWidget=So,W.default=Zo,Object.defineProperties(W,{__esModule:{value:!0},[Symbol.toStringTag]:{value:"Module"}})});
