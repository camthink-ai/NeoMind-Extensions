/**
 * YOLO Video V2 - Dashboard Edition
 * Matches NeoMind dashboard design system with compact, elegant layout
 */

import React, { useState, useEffect, useRef, useCallback } from 'react'

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
  id: number
  label: string
  confidence: number
  bbox: { x: number; y: number; width: number; height: number }
  class_id: number
}

interface RoiRegion {
  id: string
  name: string
  points: [number, number][]  // normalized 0.0-1.0
  class_filter: string[]
  color: string
}

interface CrossLine {
  id: string
  name: string
  start: [number, number]  // normalized 0.0-1.0
  end: [number, number]
  color: string
}

interface RoiStat {
  id: string
  name: string
  count: number
}

interface LineStat {
  id: string
  name: string
  forward_count: number
  backward_count: number
}

interface CaptureCondition {
  type: 'threshold' | 'presence' | 'absence'
  class_name: string
  threshold?: number
}

interface CaptureRule {
  id: string
  name: string
  roi_id: string
  condition: CaptureCondition
  cooldown_seconds: number
  quality: number
}

interface CaptureEvent {
  rule_id: string
  rule_name: string
  roi_id: string
  condition: string
  roi_counts: Record<string, number>
  image_base64: string
  timestamp: number
}

type StreamMode = 'camera' | 'network'
type DrawingTool = 'none' | 'roi' | 'line'

// ============================================================================
// Constants & Styles
// ============================================================================

const EXTENSION_ID = 'yolo-video-v2'
const CSS_ID = 'yolo-styles-v2'

const STYLES = `
.yolo {
  --yolo-fg: hsl(240 10% 10%);
  --yolo-muted: hsl(240 5% 45%);
  --yolo-accent: hsl(221 83% 53%);
  --yolo-success: #22c55e;
  --yolo-warning: #f59e0b;
  --yolo-card: rgba(255,255,255,0.5);
  --yolo-border: rgba(0,0,0,0.06);
  width: 100%;
  height: 100%;
  font-size: 12px;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
}
.dark .yolo {
  --yolo-fg: hsl(0 0% 95%);
  --yolo-muted: hsl(0 0% 60%);
  --yolo-card: rgba(30,30,30,0.5);
  --yolo-border: rgba(255,255,255,0.08);
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
.yolo-status-dot.yolo-status-error { background: #ef4444; animation: none; }
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
  color: white;
  background: var(--yolo-accent);
  border: none;
  border-radius: 4px;
  cursor: pointer;
  transition: opacity 0.2s;
}
.yolo-btn:hover { opacity: 0.9; }
.yolo-btn-stop {
  background: #ef4444;
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
  color: #ef4444;
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
  color: white;
  background: var(--yolo-accent);
  border-color: var(--yolo-accent);
}
.yolo-draw-btn.yolo-draw-danger {
  color: #ef4444;
  border-color: rgba(239,68,68,0.3);
}
.yolo-draw-btn.yolo-draw-danger:hover {
  background: #ef4444;
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
.yolo-card-btn:hover { opacity: 1 !important; color: #ef4444; background: rgba(0,0,0,0.04); }
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
.yolo-rule-pill-btn:hover { opacity: 1; color: #ef4444; }

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
  color: #fff;
  cursor: pointer;
  font-family: inherit;
  transition: opacity 0.15s;
}
.yolo-rule-popup-save:hover { opacity: 0.9; }

`

function injectStyles() {
  if (typeof document === 'undefined' || document.getElementById(CSS_ID)) return
  const style = document.createElement('style')
  style.id = CSS_ID
  style.textContent = STYLES
  document.head.appendChild(style)
}

// ============================================================================
// Icons
// ============================================================================

const ICONS: Record<string, string> = {
  video: '<path d="M23 7l-7 5 7 5V7z"/><rect x="1" y="5" width="15" height="14" rx="2" ry="2"/>',
  play: '<polygon points="5 3 19 12 5 21 5 3"/>',
  stop: '<rect x="3" y="3" width="18" height="18" rx="2" ry="2"/>',
  camera: '<path d="M23 19a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4l2-3h6l2 3h4a2 2 0 0 1 2 2z"/><circle cx="12" cy="13" r="4"/>',
  activity: '<polyline points="22 12 18 12 15 21 9 3 6 12 2 12"/>',
  clock: '<circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/>',
  eye: '<path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/>',
  layers: '<polygon points="12 2 2 7 12 12 22 7 12 2"/><polyline points="2 17 12 22 22 17"/><polyline points="2 12 12 17 22 12"/>',
  alert: '<circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/>',
  polygon: '<polygon points="12 2 22 8.5 18 20 6 20 2 8.5 12 2"/>',
  line: '<line x1="4" y1="20" x2="20" y2="4"/><polyline points="16 4 20 4 20 8"/>',
  trash: '<polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>',
  arrowRight: '<line x1="5" y1="12" x2="19" y2="12"/><polyline points="12 5 19 12 12 19"/>',
  arrowLeft: '<line x1="19" y1="12" x2="5" y2="12"/><polyline points="12 5 5 12 12 19"/>',
  zap: '<polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/>',
  plus: '<line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/>',
  edit: '<path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/>',
  x: '<line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>',
}

const Icon = ({ name, className = '', style }: { name: string; className?: string; style?: React.CSSProperties }) => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"
    strokeLinecap="round" strokeLinejoin="round" className={className} style={style}
    dangerouslySetInnerHTML={{ __html: ICONS[name] || ICONS.video }} />
)

// ============================================================================
// Detection Colors (COCO palette - matches backend, keyed by class_id)
// ============================================================================

const COCO_COLORS_RGB: [number, number, number][] = [
  [38, 70, 83],   [40, 116, 74],  [117, 79, 12],  [115, 53, 88],  [192, 41, 66],
  [11, 121, 175], [232, 168, 124],[211, 212, 211],[232, 212, 77], [32, 169, 199],
  [57, 94, 121],  [237, 139, 0],  [133, 160, 131],[174, 30, 70],  [255, 183, 59],
  [197, 198, 53], [166, 207, 213],[136, 86, 82],  [119, 104, 174],[51, 159, 160],
  [166, 59, 111], [197, 166, 137],[108, 118, 135],[38, 131, 116], [233, 126, 67],
  [255, 179, 71], [48, 96, 106],  [197, 104, 80], [227, 105, 145],[229, 193, 175],
]

function getClassColor(classId: number) {
  const [r, g, b] = COCO_COLORS_RGB[classId % COCO_COLORS_RGB.length]
  // Use high contrast: dark background with white text
  const luminance = (0.299 * r + 0.587 * g + 0.114 * b) / 255
  return {
    bg: `rgba(${r}, ${g}, ${b}, 0.85)`,
    fg: luminance > 0.5 ? '#000' : '#fff',
    border: `rgb(${r}, ${g}, ${b})`,
  }
}

const ROI_COLORS = ['#3b82f6', '#22c55e', '#f59e0b', '#ef4444', '#8b5cf6', '#ec4899', '#06b6d4', '#f97316']
let _idCounter = 0
function uid() { return `r${Date.now().toString(36)}_${(++_idCounter)}` }

function hexToRgba(hex: string, alpha: number): string {
  const r = parseInt(hex.slice(1, 3), 16)
  const g = parseInt(hex.slice(3, 5), 16)
  const b = parseInt(hex.slice(5, 7), 16)
  return `rgba(${r}, ${g}, ${b}, ${alpha})`
}

// ============================================================================
// Component
// ============================================================================

export const YoloVideoDisplay = function YoloVideoDisplay({
  title = 'YOLO Detection',
  dataSource,
  className = '',
  confidenceThreshold = 0.5,
  maxObjects = 20,
  sourceUrl = 'camera://0',
  fps: fpsProp = 15,
  drawBoxes = true,
}: ExtensionComponentProps & {
  sourceUrl?: string
  confidenceThreshold?: number
  maxObjects?: number
  fps?: number
  drawBoxes?: boolean
}) {
  // Setup
  useEffect(() => { injectStyles() }, [])

  // State
  const [isRunning, setIsRunning] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [sessionTime, setSessionTime] = useState(0)
  const [fps, setFps] = useState(0)
  const [frameCount, setFrameCount] = useState(0)
  const [detections, setDetections] = useState<Detection[]>([])
  const [frameData, setFrameData] = useState<string | null>(null)
  const [cameraPermission, setCameraPermission] = useState<'pending' | 'granted' | 'denied'>('pending')
  const [streamStatus, setStreamStatus] = useState<'idle' | 'streaming' | 'reconnecting' | 'error' | 'connecting'>('idle')

  // ROI / Line state
  const [drawingTool, setDrawingTool] = useState<DrawingTool>('none')
  const [rois, setRois] = useState<RoiRegion[]>([])
  const [lines, setLines] = useState<CrossLine[]>([])
  const [roiStats, setRoiStats] = useState<RoiStat[]>([])
  const [lineStats, setLineStats] = useState<LineStat[]>([])
  const [drawingPoints, setDrawingPoints] = useState<[number, number][]>([])  // in-progress polygon
  const [lineStart, setLineStart] = useState<[number, number] | null>(null)  // in-progress line start
  const [lineEnd, setLineEnd] = useState<[number, number] | null>(null)      // in-progress line end (preview)

  // Capture rules state
  const [captureRules, setCaptureRules] = useState<CaptureRule[]>([])
  const [captureEvents, setCaptureEvents] = useState<CaptureEvent[]>([])
  const [editingRuleRoiId, setEditingRuleRoiId] = useState<string | null>(null)  // which ROI is being configured
  const [lightboxSrc, setLightboxSrc] = useState<string | null>(null)  // capture image lightbox

  // Determine mode based on source URL
  const isNetworkStream = sourceUrl.startsWith('rtsp://')
    || sourceUrl.startsWith('rtmp://')
    || sourceUrl.startsWith('hls://')
    || sourceUrl.includes('.m3u8')
    || sourceUrl.startsWith('http://')
    || sourceUrl.startsWith('https://')
    || sourceUrl.startsWith('file://')
  const mode: StreamMode = isNetworkStream ? 'network' : 'camera'

  // Refs for latest rois/lines (avoids stale closure in connectWebSocket restart)
  const roisRef = useRef<RoiRegion[]>(rois)
  roisRef.current = rois
  const linesRef = useRef<CrossLine[]>(lines)
  linesRef.current = lines
  const captureRulesRef = useRef<CaptureRule[]>(captureRules)
  captureRulesRef.current = captureRules

  const videoRef = useRef<HTMLVideoElement>(null)
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const streamRef = useRef<MediaStream | null>(null)
  const wsRef = useRef<WebSocket | null>(null)
  const isManualCloseRef = useRef(false)
  const sessionTimerRef = useRef<ReturnType<typeof setInterval> | null>(null)
  const frameTimerRef = useRef<ReturnType<typeof setInterval> | null>(null)
  const fpsCounterRef = useRef({ frames: 0, lastTime: Date.now() })
  const sequenceRef = useRef(0)
  const sessionIdRef = useRef<string | null>(null)
  const sendingRef = useRef(false)
  const isFrameSendingRef = useRef(false)  // Lock for frame sending
  const lastFrameTimeRef = useRef(0)  // Last frame send time for throttling
  const lockTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)  // Safety timeout for lock
  const displayCanvasRef = useRef<HTMLCanvasElement>(null)  // Single canvas: frame + overlays
  const videoWrapRef = useRef<HTMLDivElement>(null)
  const frameImgRef = useRef<HTMLImageElement | null>(null)  // Cached decoded frame

  // Config update: debounced hot-update via REST API (no stream restart)
  const configUpdateTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  const extensionId = dataSource?.extensionId || EXTENSION_ID

  // WebSocket URL (with auth token)
  const getWebSocketUrl = useCallback(() => {
    const isTauri = !!(window as any).__TAURI_INTERNALS__
    const protocol = (isTauri ? false : window.location.protocol === 'https:') ? 'wss:' : 'ws:'
    const host = isTauri ? 'localhost:9375' : window.location.host
    const baseUrl = `${protocol}//${host}/api/extensions/${extensionId}/stream`
    // Read auth token from NeoMind's tokenManager storage
    const token = localStorage.getItem('neomind_token')
      || sessionStorage.getItem('neomind_token_session')
    if (token) {
      return `${baseUrl}?token=${encodeURIComponent(token)}`
    }
    return baseUrl
  }, [extensionId])

  // REST API base URL for command calls
  const getApiBaseUrl = useCallback(() => {
    const isTauri = !!(window as any).__TAURI_INTERNALS__
    const protocol = isTauri ? 'http:' : window.location.protocol === 'https:' ? 'https:' : 'http:'
    const host = isTauri ? 'localhost:9375' : window.location.host
    return `${protocol}//${host}`
  }, [])

  // Hot-update ROI/Line config on running stream (no restart)
  const sendConfigUpdate = useCallback(async () => {
    const sessionId = sessionIdRef.current
    if (!sessionId) return
    try {
      await fetch(`${getApiBaseUrl()}/api/extensions/${extensionId}/command`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          command: 'update_stream_config',
          args: { stream_id: sessionId, rois: roisRef.current, lines: linesRef.current, capture_rules: captureRulesRef.current },
        }),
      })
    } catch (e) { console.warn('[YOLO] Config update failed:', e) }
  }, [getApiBaseUrl, extensionId])

  // Debounced: coalesce rapid clicks into one REST call
  const debouncedConfigUpdate = useCallback(() => {
    if (configUpdateTimerRef.current) clearTimeout(configUpdateTimerRef.current)
    configUpdateTimerRef.current = setTimeout(() => { sendConfigUpdate(); configUpdateTimerRef.current = null }, 150)
  }, [sendConfigUpdate])

  // Capture and send frame (camera mode)
  const captureAndSendFrame = useCallback(() => {
    if (!sendingRef.current) return

    // Skip if previous frame is still being processed
    if (isFrameSendingRef.current) {
      return
    }

    // Throttle to max 20 FPS (matching backend 50ms threshold)
    const now = Date.now()
    if (now - lastFrameTimeRef.current < 50) {
      return
    }

    const video = videoRef.current
    const canvas = canvasRef.current
    if (!video || !canvas || video.paused || video.ended) return

    const ctx = canvas.getContext('2d')
    if (!ctx) return

    ctx.drawImage(video, 0, 0, canvas.width, canvas.height)

    const ws = wsRef.current
    const sessionId = sessionIdRef.current

    if (ws?.readyState === WebSocket.OPEN && sessionId) {
      isFrameSendingRef.current = true  // Acquire lock
      lastFrameTimeRef.current = now

      // Safety timeout: auto-release lock after 200ms if callback doesn't fire
      if (lockTimeoutRef.current) {
        clearTimeout(lockTimeoutRef.current)
      }
      lockTimeoutRef.current = setTimeout(() => {
        if (isFrameSendingRef.current) {
          console.warn('[YOLO] Frame lock timeout, auto-releasing')
          isFrameSendingRef.current = false
        }
      }, 200)

      canvas.toBlob((blob) => {
        // Clear safety timeout
        if (lockTimeoutRef.current) {
          clearTimeout(lockTimeoutRef.current)
          lockTimeoutRef.current = null
        }
        // Always release lock first
        isFrameSendingRef.current = false

        if (!sendingRef.current) return  // Component may have stopped

        if (blob && wsRef.current?.readyState === WebSocket.OPEN && sessionIdRef.current) {
          blob.arrayBuffer().then(buffer => {
            const sequence = sequenceRef.current++
            const header = new ArrayBuffer(8)
            new DataView(header).setBigUint64(0, BigInt(sequence), false)

            const frame = new Uint8Array(8 + buffer.byteLength)
            frame.set(new Uint8Array(header), 0)
            frame.set(new Uint8Array(buffer), 8)

            wsRef.current?.send(frame)
          }).catch((err) => {
            console.warn('[YOLO] Failed to send frame:', err)
          })
        }
      }, 'image/jpeg', 0.8)
    }
  }, [])

  // Start camera
  const startCamera = useCallback(async () => {
    try {
      setCameraPermission('pending')
      const stream = await navigator.mediaDevices.getUserMedia({
        video: { width: { ideal: 640 }, height: { ideal: 480 }, facingMode: 'user' },
        audio: false
      })

      setCameraPermission('granted')
      streamRef.current = stream

      if (videoRef.current) {
        videoRef.current.srcObject = stream
        await videoRef.current.play()
      }

      return true
    } catch (e) {
      setCameraPermission('denied')
      if (e instanceof Error) {
        if (e.name === 'NotAllowedError') {
          setError('Camera permission denied')
        } else if (e.name === 'NotFoundError') {
          setError('No camera found')
        } else {
          setError(`Camera error: ${e.message}`)
        }
      }
      return false
    }
  }, [])

  // Stop camera
  const stopCamera = useCallback(() => {
    sendingRef.current = false
    isFrameSendingRef.current = false  // Reset frame sending lock
    lastFrameTimeRef.current = 0  // Reset throttle timer

    // Clear safety timeout
    if (lockTimeoutRef.current) {
      clearTimeout(lockTimeoutRef.current)
      lockTimeoutRef.current = null
    }

    if (streamRef.current) {
      streamRef.current.getTracks().forEach(track => track.stop())
      streamRef.current = null
    }

    if (videoRef.current) {
      videoRef.current.srcObject = null
    }
  }, [])

  // Connect WebSocket
  const connectWebSocket = useCallback(() => {
    const url = getWebSocketUrl()
    const ws = new WebSocket(url)
    ws.binaryType = 'arraybuffer'

    ws.onopen = () => {
      const initMsg = {
        type: 'init',
        config: {
          source_url: sourceUrl,
          confidence_threshold: confidenceThreshold,
          max_objects: maxObjects,
          target_fps: fpsProp,
          draw_boxes: drawBoxes,
          rois: roisRef.current,
          lines: linesRef.current,
        }
      }
      console.log('[YOLO] Sending init:', JSON.stringify(initMsg))
      ws.send(JSON.stringify(initMsg))
    }

    ws.onmessage = (event) => {
      // Binary responses are not used - all results come as JSON text messages
      // This is because metadata (including detections) must be sent with the frame
      if (event.data instanceof ArrayBuffer) {
        // Skip binary responses - they don't contain detection metadata
        console.debug('[YOLO] Received binary response (no metadata), skipping')
        return
      }

      // Text message (JSON)
      if (typeof event.data !== 'string') return

      try {
        const msg = JSON.parse(event.data)

        switch (msg.type) {
          case 'session_created':
            sessionIdRef.current = msg.session_id
            setIsRunning(true)
            setSessionTime(0)
            sessionTimerRef.current = setInterval(() => setSessionTime(t => t + 1), 1000)

            if (mode === 'camera') {
              // Camera mode: capture and send frames
              sendingRef.current = true
              frameTimerRef.current = setInterval(captureAndSendFrame, 50)
            } else {
              // Network/Push mode: server pushes frames via push_output messages
              // No polling needed - just wait for push_output messages
              setStreamStatus('connecting')
            }
            break

          case 'push_output':
            // Check for status/error JSON messages (backend stream status)
            if (msg.data_type === 'application/json' && msg.data) {
              try {
                const statusData = typeof msg.data === 'string' ? JSON.parse(msg.data) : msg.data
                if (statusData.type === 'status' && statusData.status) {
                  setStreamStatus(statusData.status as any)
                } else if (statusData.type === 'error') {
                  setStreamStatus('error')
                  setError(statusData.message || 'Stream error')
                }
              } catch { /* ignore parse errors */ }
              break
            }
            // Image frame from backend stream
            if (msg.data && msg.data_type === 'image/jpeg') {
              setStreamStatus('streaming')
              setFrameData(msg.data)
              updateFps()
              if (msg.metadata?.detections) {
                setDetections(msg.metadata.detections)
              }
              if (msg.metadata?.roi_stats) {
                setRoiStats(msg.metadata.roi_stats)
              }
              if (msg.metadata?.line_stats) {
                setLineStats(msg.metadata.line_stats)
              }
              if (msg.metadata?.capture_events && msg.metadata.capture_events.length > 0) {
                setCaptureEvents(prev => [...msg.metadata.capture_events, ...prev].slice(0, 10))
              }
            }
            break

          case 'result':
            // Processing result from server - data is base64 encoded
            if (msg.data) {
              const isSkipped = msg.skipped === true ||
                                (typeof msg.data === 'string' &&
                                 (msg.data.startsWith('{') ||
                                  msg.metadata?.skipped === true));
              const isWaiting = msg.metadata?.status === 'waiting'

              if (isSkipped) {
                if (msg.metadata?.detections) {
                  setDetections(msg.metadata.detections)
                }
              } else if (isWaiting) {
                // No frame yet from FFmpeg, keep waiting
              } else if (typeof msg.data === 'string' && msg.data.length > 0) {
                setFrameData(msg.data)
                updateFps()
                if (msg.metadata?.frame_count) {
                  setFrameCount(msg.metadata.frame_count)
                } else {
                  setFrameCount(prev => prev + 1)
                }
                if (msg.metadata?.fps) {
                  setFps(msg.metadata.fps)
                }
                if (msg.metadata?.detections) {
                  setDetections(msg.metadata.detections)
                }
                if (msg.metadata?.capture_events && msg.metadata.capture_events.length > 0) {
                  setCaptureEvents(prev => [...msg.metadata.capture_events, ...prev].slice(0, 10))
                }
              }
            }
            break

          case 'error':
            // Ignore frame rate throttling errors (these are normal during high load)
            if (msg.message && msg.message.includes('Frame rate too high')) {
              console.debug('[YOLO] Frame dropped due to rate limiting (normal)')
              break
            }
            // Show other errors to user
            setError(`${msg.code}: ${msg.message}`)
            break

          case 'session_closed':
            setIsRunning(false)
            sessionIdRef.current = null
            break
        }
      } catch (e) {
        console.error('[YOLO] Failed to parse message:', e)
      }
    }

    ws.onerror = (e) => {
      // Ignore errors from manual close - onclose will handle cleanup
      if (isManualCloseRef.current) return
      console.error('[YOLO] WebSocket error:', e)
      setError('WebSocket connection error')
    }

    ws.onclose = () => {
      const wasManual = isManualCloseRef.current
      wsRef.current = null
      setIsRunning(false)
      setStreamStatus('idle')
      sessionIdRef.current = null
      sendingRef.current = false
      isManualCloseRef.current = false

      // Clear error on manual close
      if (wasManual) {
        setError(null)
      }

      if (sessionTimerRef.current) {
        clearInterval(sessionTimerRef.current)
        sessionTimerRef.current = null
      }

      if (frameTimerRef.current) {
        clearInterval(frameTimerRef.current)
        frameTimerRef.current = null
      }
    }

    wsRef.current = ws
  }, [getWebSocketUrl, sourceUrl, confidenceThreshold, maxObjects, mode, captureAndSendFrame, fpsProp, drawBoxes])

  // Update FPS
  const updateFps = () => {
    fpsCounterRef.current.frames++
    const now = Date.now()
    const elapsed = now - fpsCounterRef.current.lastTime
    if (elapsed >= 1000) {
      setFps(Math.round(fpsCounterRef.current.frames * 1000 / elapsed))
      fpsCounterRef.current.frames = 0
      fpsCounterRef.current.lastTime = now
    }
  }

  // Disconnect WebSocket (simple synchronous close)
  const disconnectWebSocket = useCallback(() => {
    if (wsRef.current) {
      isManualCloseRef.current = true
      if (wsRef.current.readyState === WebSocket.OPEN) {
        wsRef.current.send(JSON.stringify({ type: 'close' }))
      }
      wsRef.current.close()
      wsRef.current = null

      if (sessionTimerRef.current) {
        clearInterval(sessionTimerRef.current)
        sessionTimerRef.current = null
      }

      if (frameTimerRef.current) {
        clearInterval(frameTimerRef.current)
        frameTimerRef.current = null
      }

      setIsRunning(false)
      sessionIdRef.current = null
      setDetections([])
    }
  }, [])

  // Start stream
  const startStream = useCallback(async () => {
    setError(null)
    setFrameData(null)
    setFps(0)
    setFrameCount(0)
    fpsCounterRef.current = { frames: 0, lastTime: Date.now() }

    if (mode === 'camera') {
      const cameraOk = await startCamera()
      if (!cameraOk) return
    }

    connectWebSocket()
  }, [mode, startCamera, connectWebSocket])

  // Stop stream
  const stopStream = useCallback(() => {
    if (mode === 'camera') {
      stopCamera()
    }
    disconnectWebSocket()
    setStreamStatus('idle')
    setDetections([])
    setFps(0)
    setFrameCount(0)
    setSessionTime(0)
    setFrameData(null)
    setRoiStats([])
    setLineStats([])
    setCaptureEvents([])
  }, [mode, stopCamera, disconnectWebSocket])

  // Drawing tool handlers

  const finishRoi = useCallback(() => {
    if (drawingPoints.length < 3) return
    const newRoi: RoiRegion = {
      id: uid(),
      name: `ROI ${rois.length + 1}`,
      points: drawingPoints as [number, number][],
      class_filter: [],
      color: ROI_COLORS[(rois.length + lines.length) % ROI_COLORS.length],
    }
    setRois(prev => [...prev, newRoi])
    setDrawingPoints([])
    setDrawingTool('none')
    if (isRunning) {
      debouncedConfigUpdate()
    }
  }, [drawingPoints, rois.length, lines.length, isRunning, debouncedConfigUpdate])

  const saveLine = useCallback(() => {
    if (!lineStart || !lineEnd) return
    const newLine: CrossLine = {
      id: uid(),
      name: `Line ${lines.length + 1}`,
      start: lineStart,
      end: lineEnd,
      color: ROI_COLORS[(rois.length + lines.length) % ROI_COLORS.length],
    }
    const newLines = [...lines, newLine]
    setLines(newLines)
    setLineStart(null)
    setLineEnd(null)
    setDrawingTool('none')
    // Hot-update config without restarting stream
    if (isRunning) {
      debouncedConfigUpdate()
    }
  }, [lineStart, lineEnd, lines, rois.length, isRunning, debouncedConfigUpdate])

  // Click-to-close threshold (distance from first point, in normalized coords)
  const CLOSE_THRESHOLD = 0.03

  const handleDrawCanvasClick = useCallback((e: React.MouseEvent<HTMLCanvasElement>) => {
    if (drawingTool === 'none') return
    const canvas = displayCanvasRef.current
    if (!canvas) return
    const rect = canvas.getBoundingClientRect()
    const nx = (e.clientX - rect.left) / rect.width
    const ny = (e.clientY - rect.top) / rect.height

    if (drawingTool === 'roi') {
      // If >= 3 points and click is near the first point → close polygon
      if (drawingPoints.length >= 3) {
        const first = drawingPoints[0]
        const dist = Math.sqrt((nx - first[0]) ** 2 + (ny - first[1]) ** 2)
        if (dist < CLOSE_THRESHOLD) {
          finishRoi()
          return
        }
      }
      setDrawingPoints(prev => [...prev, [nx, ny]])
    } else if (drawingTool === 'line') {
      if (!lineStart) {
        setLineStart([nx, ny])
      } else if (!lineEnd) {
        // Second click: show preview, wait for save
        setLineEnd([nx, ny])
      }
      // If both start and end are set, do nothing (waiting for Save)
    }
  }, [drawingTool, lineStart, lineEnd, drawingPoints, finishRoi])

  const cancelDrawing = useCallback(() => {
    setDrawingPoints([])
    setLineStart(null)
    setLineEnd(null)
    setDrawingTool('none')
  }, [])

  const removeRoi = useCallback((id: string) => {
    setRois(prev => prev.filter(r => r.id !== id))
    setRoiStats(prev => prev.filter(s => s.id !== id))
    if (isRunning) {
      debouncedConfigUpdate()
    }
  }, [isRunning, debouncedConfigUpdate])

  const removeLine = useCallback((id: string) => {
    setLines(prev => prev.filter(l => l.id !== id))
    setLineStats(prev => prev.filter(s => s.id !== id))
    if (isRunning) {
      debouncedConfigUpdate()
    }
  }, [isRunning, debouncedConfigUpdate])

  // Capture rule management
  const addCaptureRule = useCallback((roiId: string, condition: CaptureCondition, cooldown: number) => {
    const roi = rois.find(r => r.id === roiId)
    const condLabel = condition.type === 'threshold'
      ? `${condition.class_name}≥${condition.threshold}`
      : condition.type === 'presence' ? `${condition.class_name} appears` : `${condition.class_name} gone`
    const rule: CaptureRule = {
      id: uid(),
      name: roi ? `${roi.name}: ${condLabel}` : condLabel,
      roi_id: roiId,
      condition,
      cooldown_seconds: cooldown,
      quality: 80,
    }
    setCaptureRules(prev => [...prev, rule])
    setEditingRuleRoiId(null)
    if (isRunning) {
      // Delay to let state flush to ref
      setTimeout(() => debouncedConfigUpdate(), 50)
    }
  }, [rois, isRunning, debouncedConfigUpdate])

  const removeCaptureRule = useCallback((id: string) => {
    setCaptureRules(prev => prev.filter(r => r.id !== id))
    if (isRunning) {
      setTimeout(() => debouncedConfigUpdate(), 50)
    }
  }, [isRunning, debouncedConfigUpdate])

  // Render frame + overlays to the single display canvas
  const renderCanvas = useCallback(() => {
    const canvas = displayCanvasRef.current
    if (!canvas) return
    const ctx = canvas.getContext('2d')
    if (!ctx) return

    const wrap = videoWrapRef.current
    if (!wrap) return
    const cw = wrap.clientWidth
    const ch = wrap.clientHeight
    canvas.width = cw
    canvas.height = ch

    ctx.clearRect(0, 0, cw, ch)

    const w = cw
    const h = ch

    // Draw the video frame as background
    const img = frameImgRef.current
    if (img && img.complete && img.naturalWidth > 0) {
      // Cover-fit: scale to fill while maintaining aspect ratio
      const imgAspect = img.naturalWidth / img.naturalHeight
      const canvasAspect = w / h
      let sx = 0, sy = 0, sw = img.naturalWidth, sh = img.naturalHeight
      if (imgAspect > canvasAspect) {
        sw = img.naturalHeight * canvasAspect
        sx = (img.naturalWidth - sw) / 2
      } else {
        sh = img.naturalWidth / canvasAspect
        sy = (img.naturalHeight - sh) / 2
      }
      ctx.drawImage(img, sx, sy, sw, sh, 0, 0, w, h)
    }

    // Draw existing ROIs
    for (const roi of rois) {
      if (roi.points.length < 3) continue
      ctx.beginPath()
      ctx.moveTo(roi.points[0][0] * w, roi.points[0][1] * h)
      for (let i = 1; i < roi.points.length; i++) {
        ctx.lineTo(roi.points[i][0] * w, roi.points[i][1] * h)
      }
      ctx.closePath()
      ctx.fillStyle = hexToRgba(roi.color, 0.15)
      ctx.fill()
      ctx.strokeStyle = roi.color
      ctx.lineWidth = 2
      ctx.stroke()
      // Label
      const cx = roi.points.reduce((s, p) => s + p[0], 0) / roi.points.length * w
      const cy = roi.points.reduce((s, p) => s + p[1], 0) / roi.points.length * h
      ctx.font = 'bold 12px -apple-system, sans-serif'
      const stat = roiStats.find(s => s.id === roi.id)
      const nameTm = ctx.measureText(roi.name)
      if (stat) {
        const countLabel = String(stat.count)
        const countTm = ctx.measureText(countLabel)
        const gap = 6
        const totalW = nameTm.width + gap + countTm.width + 16
        const bgX = cx - totalW / 2
        const bgY = cy - 9
        const bgH = 18
        // Background pill
        ctx.fillStyle = hexToRgba(roi.color, 0.9)
        ctx.beginPath()
        ctx.roundRect(bgX, bgY, totalW, bgH, 3)
        ctx.fill()
        // Name text
        ctx.fillStyle = 'rgba(255,255,255,0.8)'
        ctx.textAlign = 'left'
        ctx.textBaseline = 'middle'
        ctx.fillText(roi.name, bgX + 6, cy)
        // Count text with emphasis
        ctx.fillStyle = '#fff'
        ctx.font = 'bold 12px -apple-system, sans-serif'
        ctx.fillText(countLabel, bgX + nameTm.width + gap + 6, cy)
        ctx.font = 'bold 12px -apple-system, sans-serif'
      } else {
        const totalW = nameTm.width + 12
        const bgX = cx - totalW / 2
        const bgY = cy - 9
        ctx.fillStyle = hexToRgba(roi.color, 0.9)
        ctx.beginPath()
        ctx.roundRect(bgX, bgY, totalW, 18, 3)
        ctx.fill()
        ctx.fillStyle = '#fff'
        ctx.textAlign = 'center'
        ctx.textBaseline = 'middle'
        ctx.fillText(roi.name, cx, cy)
      }
    }

    // Draw existing lines
    for (const line of lines) {
      const sx = line.start[0] * w, sy = line.start[1] * h
      const ex = line.end[0] * w, ey = line.end[1] * h
      ctx.beginPath()
      ctx.moveTo(sx, sy)
      ctx.lineTo(ex, ey)
      ctx.strokeStyle = line.color
      ctx.lineWidth = 2
      ctx.setLineDash([6, 3])
      ctx.stroke()
      ctx.setLineDash([])
      // Perpendicular arrows (crossing direction indicators)
      const lineAngle = Math.atan2(ey - sy, ex - sx)
      const perpAngle = lineAngle + Math.PI / 2  // perpendicular to line
      const mx = (sx + ex) / 2, my = (sy + ey) / 2
      const aOffset = 12  // distance from line midpoint
      const aHeadLen = 5
      // Forward arrow (one side of line)
      const fwdX = mx + Math.cos(perpAngle) * aOffset
      const fwdY = my + Math.sin(perpAngle) * aOffset
      ctx.beginPath()
      ctx.moveTo(mx + Math.cos(perpAngle) * 4, my + Math.sin(perpAngle) * 4)
      ctx.lineTo(fwdX, fwdY)
      ctx.strokeStyle = '#4ade80'
      ctx.lineWidth = 2
      ctx.stroke()
      ctx.beginPath()
      ctx.moveTo(fwdX, fwdY)
      ctx.lineTo(fwdX - aHeadLen * Math.cos(perpAngle - 0.5), fwdY - aHeadLen * Math.sin(perpAngle - 0.5))
      ctx.moveTo(fwdX, fwdY)
      ctx.lineTo(fwdX - aHeadLen * Math.cos(perpAngle + 0.5), fwdY - aHeadLen * Math.sin(perpAngle + 0.5))
      ctx.stroke()
      // Backward arrow (other side of line)
      const bwdX = mx - Math.cos(perpAngle) * aOffset
      const bwdY = my - Math.sin(perpAngle) * aOffset
      ctx.beginPath()
      ctx.moveTo(mx - Math.cos(perpAngle) * 4, my - Math.sin(perpAngle) * 4)
      ctx.lineTo(bwdX, bwdY)
      ctx.strokeStyle = '#60a5fa'
      ctx.lineWidth = 2
      ctx.stroke()
      ctx.beginPath()
      ctx.moveTo(bwdX, bwdY)
      ctx.lineTo(bwdX + aHeadLen * Math.cos(perpAngle - 0.5), bwdY + aHeadLen * Math.sin(perpAngle - 0.5))
      ctx.moveTo(bwdX, bwdY)
      ctx.lineTo(bwdX + aHeadLen * Math.cos(perpAngle + 0.5), bwdY + aHeadLen * Math.sin(perpAngle + 0.5))
      ctx.stroke()
      // Restore line color
      ctx.strokeStyle = line.color
      ctx.lineWidth = 2
      // Label (mx/my already defined above)
      const stat = lineStats.find(s => s.id === line.id)
      ctx.font = 'bold 11px -apple-system, sans-serif'
      if (stat) {
        const nameTm = ctx.measureText(line.name)
        const fwdLabel = `→${stat.forward_count}`
        const bwdLabel = `←${stat.backward_count}`
        const fwdTm = ctx.measureText(fwdLabel)
        const bwdTm = ctx.measureText(bwdLabel)
        const gap = 5
        const totalW = nameTm.width + gap + fwdTm.width + gap + bwdTm.width + 14
        const bgX = mx - totalW / 2
        const bgY = my - 18
        // Background pill
        ctx.fillStyle = hexToRgba(line.color, 0.9)
        ctx.beginPath()
        ctx.roundRect(bgX, bgY, totalW, 18, 3)
        ctx.fill()
        let textX = bgX + 7
        // Name
        ctx.fillStyle = 'rgba(255,255,255,0.8)'
        ctx.textAlign = 'left'
        ctx.textBaseline = 'middle'
        ctx.fillText(line.name, textX, bgY + 9)
        textX += nameTm.width + gap
        // Forward
        ctx.fillStyle = '#4ade80'
        ctx.fillText(fwdLabel, textX, bgY + 9)
        textX += fwdTm.width + gap
        // Backward
        ctx.fillStyle = '#60a5fa'
        ctx.fillText(bwdLabel, textX, bgY + 9)
      } else {
        const nameTm = ctx.measureText(line.name)
        const totalW = nameTm.width + 12
        const bgX = mx - totalW / 2
        const bgY = my - 18
        ctx.fillStyle = hexToRgba(line.color, 0.9)
        ctx.beginPath()
        ctx.roundRect(bgX, bgY, totalW, 18, 3)
        ctx.fill()
        ctx.fillStyle = '#fff'
        ctx.textAlign = 'center'
        ctx.textBaseline = 'middle'
        ctx.fillText(line.name, mx, bgY + 9)
      }
    }

    // Draw in-progress polygon
    if (drawingTool === 'roi' && drawingPoints.length > 0) {
      ctx.beginPath()
      ctx.moveTo(drawingPoints[0][0] * w, drawingPoints[0][1] * h)
      for (let i = 1; i < drawingPoints.length; i++) {
        ctx.lineTo(drawingPoints[i][0] * w, drawingPoints[i][1] * h)
      }
      ctx.strokeStyle = '#3b82f6'
      ctx.lineWidth = 2
      ctx.setLineDash([4, 4])
      ctx.stroke()
      ctx.setLineDash([])
      // Draw vertices
      for (let i = 0; i < drawingPoints.length; i++) {
        const p = drawingPoints[i]
        const isFirst = i === 0
        ctx.beginPath()
        ctx.arc(p[0] * w, p[1] * h, isFirst ? 6 : 4, 0, Math.PI * 2)
        ctx.fillStyle = isFirst ? '#22c55e' : '#3b82f6'
        ctx.fill()
        ctx.strokeStyle = '#fff'
        ctx.lineWidth = 1
        ctx.stroke()
      }
      // When >= 3 points, highlight first point as close target
      if (drawingPoints.length >= 3) {
        const fp = drawingPoints[0]
        // Pulsing ring around first point
        ctx.beginPath()
        ctx.arc(fp[0] * w, fp[1] * h, 12, 0, Math.PI * 2)
        ctx.strokeStyle = 'rgba(34,197,94,0.5)'
        ctx.lineWidth = 2
        ctx.setLineDash([3, 3])
        ctx.stroke()
        ctx.setLineDash([])
        // Hint text
        ctx.fillStyle = 'rgba(0,0,0,0.6)'
        ctx.font = '10px -apple-system, sans-serif'
        ctx.textAlign = 'center'
        ctx.fillText('Click green point to close', w / 2, h - 10)
      }
    }

    // Draw in-progress line
    if (drawingTool === 'line' && lineStart) {
      const sx = lineStart[0] * w, sy = lineStart[1] * h
      // Start point
      ctx.beginPath()
      ctx.arc(sx, sy, 4, 0, Math.PI * 2)
      ctx.fillStyle = '#22c55e'
      ctx.fill()
      ctx.strokeStyle = '#fff'
      ctx.lineWidth = 1
      ctx.stroke()

      if (lineEnd) {
        // Preview line
        const ex = lineEnd[0] * w, ey = lineEnd[1] * h
        ctx.beginPath()
        ctx.moveTo(sx, sy)
        ctx.lineTo(ex, ey)
        ctx.strokeStyle = '#22c55e'
        ctx.lineWidth = 2
        ctx.setLineDash([6, 3])
        ctx.stroke()
        ctx.setLineDash([])
        // End point
        ctx.beginPath()
        ctx.arc(ex, ey, 4, 0, Math.PI * 2)
        ctx.fillStyle = '#22c55e'
        ctx.fill()
        ctx.strokeStyle = '#fff'
        ctx.lineWidth = 1
        ctx.stroke()
        // Hint
        ctx.fillStyle = 'rgba(0,0,0,0.6)'
        ctx.font = '10px -apple-system, sans-serif'
        ctx.textAlign = 'center'
        ctx.fillText('Click Save to confirm', w / 2, h - 10)
      } else {
        ctx.fillStyle = 'rgba(0,0,0,0.6)'
        ctx.font = '10px -apple-system, sans-serif'
        ctx.textAlign = 'center'
        ctx.fillText('Click to set end point', w / 2, h - 10)
      }
    }
  }, [rois, lines, roiStats, lineStats, drawingPoints, lineStart, lineEnd, drawingTool])

  // Decode frame data into Image and trigger canvas redraw
  useEffect(() => {
    if (!frameData) {
      frameImgRef.current = null
      renderCanvas()
      return
    }
    const img = new Image()
    img.onload = () => {
      frameImgRef.current = img
      renderCanvas()
    }
    img.src = `data:image/jpeg;base64,${frameData}`
  }, [frameData, renderCanvas])

  // Redraw when overlays change (without new frame)
  useEffect(() => {
    renderCanvas()
  }, [rois, lines, roiStats, lineStats, drawingPoints, lineStart, drawingTool, renderCanvas])

  // Double-click removed: click-to-close on first point is the primary close mechanism

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      stopStream()
      if (configUpdateTimerRef.current) clearTimeout(configUpdateTimerRef.current)
    }
  }, [stopStream])

  // Format time
  const formatTime = (seconds: number) => {
    const mins = Math.floor(seconds / 60)
    const secs = seconds % 60
    return `${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`
  }

  // Get mode label
  const getModeLabel = () => {
    if (mode === 'network') {
      if (isRunning && streamStatus === 'reconnecting') return 'Reconnecting...'
      if (isRunning && streamStatus === 'error') return 'Error'
      if (sourceUrl.startsWith('rtsp://')) return 'RTSP'
      if (sourceUrl.startsWith('rtmp://')) return 'RTMP'
      if (sourceUrl.startsWith('hls://') || sourceUrl.includes('.m3u8')) return 'HLS'
      return 'Network'
    }
    return 'CAM'
  }

  // Render
  return (
    <div className={`yolo ${className}`}>
      <div className="yolo-card">
        {/* Header */}
        <div className="yolo-header">
          <div className="yolo-title">
            <Icon name="camera" className="yolo-title-icon" />
            {title}
          </div>
          <div className="yolo-controls">
            {/* Drawing tools - icon only */}
            <button
              className={`yolo-draw-btn${drawingTool === 'roi' ? ' yolo-draw-active' : ''}`}
              onClick={() => {
                if (drawingTool === 'roi') { cancelDrawing() }
                else { setDrawingTool('roi'); setLineStart(null) }
              }}
              title="Draw ROI polygon"
            >
              <Icon name="polygon" style={{ width: 12, height: 12 }} />
            </button>
            <button
              className={`yolo-draw-btn${drawingTool === 'line' ? ' yolo-draw-active' : ''}`}
              onClick={() => {
                if (drawingTool === 'line') { cancelDrawing() }
                else { setDrawingTool('line'); setDrawingPoints([]) }
              }}
              title="Draw crossing line"
            >
              <Icon name="line" style={{ width: 12, height: 12 }} />
            </button>
            {drawingTool === 'roi' && drawingPoints.length >= 3 && (
              <button className="yolo-draw-btn" onClick={finishRoi} title="Finish ROI">
                <Icon name="play" style={{ width: 10, height: 10 }} />
              </button>
            )}
            {drawingTool === 'line' && lineStart && lineEnd && (
              <button className="yolo-draw-btn" style={{ background: '#22c55e', borderColor: '#22c55e', color: '#fff' }} onClick={saveLine} title="Save line">
                <Icon name="play" style={{ width: 10, height: 10 }} />
              </button>
            )}
            {drawingTool !== 'none' && (
              <button className="yolo-draw-btn yolo-draw-danger" onClick={cancelDrawing} title="Cancel">
                &times;
              </button>
            )}
            {rois.length + lines.length > 0 && drawingTool === 'none' && (
              <button className="yolo-draw-btn yolo-draw-danger"
                onClick={() => { setRois([]); setLines([]); setRoiStats([]); setLineStats([]); if (isRunning) debouncedConfigUpdate() }}
                title="Clear all"
              >
                <Icon name="trash" style={{ width: 10, height: 10 }} />
              </button>
            )}
            <span style={{ width: 1, height: 12, background: 'var(--yolo-border)', margin: '0 2px' }} />
            {isRunning && (
              <div className="yolo-status">
                <span className={`yolo-status-dot${streamStatus === 'reconnecting' ? ' yolo-status-warning' : streamStatus === 'error' ? ' yolo-status-error' : ''}`} />
                {getModeLabel()}
              </div>
            )}
            {!isRunning ? (
              <button onClick={startStream} className="yolo-btn">
                <Icon name="play" style={{ width: 12, height: 12, display: 'inline', verticalAlign: 'middle', marginRight: 2 }} />
                Start
              </button>
            ) : (
              <button onClick={stopStream} className="yolo-btn yolo-btn-stop">
                <Icon name="stop" style={{ width: 12, height: 12, display: 'inline', verticalAlign: 'middle', marginRight: 2 }} />
                Stop
              </button>
            )}
          </div>
        </div>

        {/* Video Display */}
        <div className="yolo-video-wrap" ref={videoWrapRef}>
          {/* Hidden elements for camera capture */}
          {mode === 'camera' && (
            <>
              <video ref={videoRef} className="hidden" playsInline muted />
              <canvas ref={canvasRef} width={640} height={480} className="hidden" />
            </>
          )}

          {/* Single canvas: frame + ROI/Line overlays */}
          <canvas
            ref={displayCanvasRef}
            className="yolo-video-frame"
            style={{ cursor: drawingTool !== 'none' ? 'crosshair' : 'default' }}
            onClick={handleDrawCanvasClick}
          />

          {/* Error overlay */}
          {error && (
            <div className="yolo-error">
              <Icon name="alert" className="yolo-error-icon" />
              <div className="yolo-error-text">{error}</div>
            </div>
          )}

          {/* Placeholder */}
          {!isRunning && !error && (
            <div className="yolo-video-placeholder">
              <Icon name="video" className="yolo-video-icon" />
              <div className="yolo-video-text">
                {mode === 'camera'
                  ? 'Click Start to begin detection'
                  : `Click Start to connect to ${sourceUrl}`}
              </div>
            </div>
          )}

          {/* Loading - show when waiting for first frame */}
          {isRunning && !frameData && !error && (
            <div className="yolo-video-loading">
              <div className="yolo-spinner" />
              <div className="yolo-video-text">
                {mode === 'camera' ? 'Starting camera...' : 'Connecting...'}
              </div>
            </div>
          )}
        </div>

        {/* Stats Bar */}
        {isRunning && (
          <div className="yolo-stats">
            <div className="yolo-stat-group">
              <div className="yolo-stat">
                <Icon name="clock" className="yolo-stat-icon" />
                <span className="yolo-stat-val">{formatTime(sessionTime)}</span>
              </div>
              <div className="yolo-stat">
                <Icon name="activity" className="yolo-stat-icon" />
                <span className="yolo-stat-val">{fps}</span>
                <span>FPS</span>
              </div>
              <div className="yolo-stat">
                <Icon name="layers" className="yolo-stat-icon" />
                <span className="yolo-stat-val">{frameCount}</span>
                <span>frames</span>
              </div>
            </div>
            <div className="yolo-stat">
              <Icon name="eye" className="yolo-stat-icon" />
              <span className="yolo-stat-val">{detections.length}</span>
              <span>objects</span>
            </div>
          </div>
        )}

        {/* Detections - aggregated by class */}
        {isRunning && detections.length > 0 && (
          <div className="yolo-detections">
            <div className="yolo-detections-title">Detected Objects</div>
            <div className="yolo-detections-list">
              {(() => {
                const classMap = new Map<number, { label: string; count: number }>()
                for (const det of detections) {
                  const cid = det.class_id || 0
                  const entry = classMap.get(cid)
                  if (entry) { entry.count++ }
                  else { classMap.set(cid, { label: det.label, count: 1 }) }
                }
                const sorted = [...classMap.entries()].sort((a, b) => b[1].count - a[1].count)
                return sorted.map(([classId, { label, count }]) => {
                  const color = getClassColor(classId)
                  return (
                    <span key={classId} className="yolo-detection-tag"
                      style={{ backgroundColor: color.bg, color: color.fg, border: `1px solid ${color.border}` }}>
                      {label}
                      <span style={{ opacity: 0.8, fontWeight: 700 }}>x{count}</span>
                    </span>
                  )
                })
              })()}
            </div>
          </div>
        )}

        {/* ROI & Line Cards */}
        {(roiStats.length > 0 || lineStats.length > 0 || rois.length > 0 || lines.length > 0) && (
          <div className="yolo-regions">
            <div className="yolo-section-grid">
              {/* ROI with stats */}
              {roiStats.map(stat => {
                const roi = rois.find(r => r.id === stat.id)
                const color = roi?.color || '#3b82f6'
                const rulesForRoi = captureRules.filter(r => r.roi_id === stat.id)
                return (
                  <div key={stat.id} className="yolo-card">
                    <div className="yolo-card-row">
                      <span className="yolo-card-name">{stat.name}</span>
                      <span className="yolo-card-actions">
                        <button className="yolo-card-btn yolo-card-btn-edit" onClick={() => setEditingRuleRoiId(editingRuleRoiId === stat.id ? null : stat.id)} title="Edit capture rules"><Icon name="edit" style={{ width: 10, height: 10 }} /></button>
                        <button className="yolo-card-btn" onClick={() => removeRoi(stat.id)} title="Delete"><Icon name="x" style={{ width: 10, height: 10 }} /></button>
                      </span>
                    </div>
                    {(stat.count > 0 || rulesForRoi.length > 0) && (
                      <div className="yolo-card-data">
                        <span className="yolo-card-badge" style={{ background: hexToRgba(color, 0.15), color }}>{stat.count}</span>
                        {rulesForRoi.map(rule => (
                          <span key={rule.id} className="yolo-rule-pill">
                            {rule.condition.type === 'threshold' ? `${rule.condition.class_name}≥${rule.condition.threshold}` : rule.condition.type === 'presence' ? `${rule.condition.class_name}↑` : `${rule.condition.class_name}↓`}
                            <button className="yolo-rule-pill-btn" onClick={() => removeCaptureRule(rule.id)}><Icon name="x" style={{ width: 8, height: 8 }} /></button>
                          </span>
                        ))}
                      </div>
                    )}
                  </div>
                )
              })}
              {/* ROI without stats */}
              {rois.filter(r => !roiStats.some(s => s.id === r.id)).map(roi => {
                const rulesForRoi = captureRules.filter(r => r.roi_id === roi.id)
                return (
                  <div key={roi.id} className="yolo-card">
                    <div className="yolo-card-row">
                      <span className="yolo-card-name">{roi.name}</span>
                      <span className="yolo-card-actions">
                        <button className="yolo-card-btn yolo-card-btn-edit" onClick={() => setEditingRuleRoiId(editingRuleRoiId === roi.id ? null : roi.id)} title="Edit capture rules"><Icon name="edit" style={{ width: 10, height: 10 }} /></button>
                        <button className="yolo-card-btn" onClick={() => removeRoi(roi.id)} title="Delete"><Icon name="x" style={{ width: 10, height: 10 }} /></button>
                      </span>
                    </div>
                    {rulesForRoi.length > 0 && (
                      <div className="yolo-card-data">
                        {rulesForRoi.map(rule => (
                          <span key={rule.id} className="yolo-rule-pill">
                            {rule.condition.type === 'threshold' ? `${rule.condition.class_name}≥${rule.condition.threshold}` : rule.condition.type === 'presence' ? `${rule.condition.class_name}↑` : `${rule.condition.class_name}↓`}
                            <button className="yolo-rule-pill-btn" onClick={() => removeCaptureRule(rule.id)}><Icon name="x" style={{ width: 8, height: 8 }} /></button>
                          </span>
                        ))}
                      </div>
                    )}
                  </div>
                )
              })}
              {/* Lines with stats */}
              {lineStats.map(stat => {
                const line = lines.find(l => l.id === stat.id)
                const color = line?.color || '#22c55e'
                return (
                  <div key={stat.id} className="yolo-card">
                    <div className="yolo-card-row">
                      <span className="yolo-card-name">{stat.name}</span>
                      <span className="yolo-card-actions">
                        <button className="yolo-card-btn" onClick={() => removeLine(stat.id)} title="Delete"><Icon name="x" style={{ width: 10, height: 10 }} /></button>
                      </span>
                    </div>
                    <div className="yolo-card-data">
                      <span className="yolo-line-dir" style={{ background: 'rgba(34,197,94,0.12)', color: '#22c55e' }}>→{stat.forward_count}</span>
                      <span className="yolo-line-dir" style={{ background: 'rgba(59,130,246,0.12)', color: '#3b82f6' }}>←{stat.backward_count}</span>
                    </div>
                  </div>
                )
              })}
              {/* Lines without stats */}
              {lines.filter(l => !lineStats.some(s => s.id === l.id)).map(line => (
                <div key={line.id} className="yolo-card">
                  <div className="yolo-card-row">
                    <span className="yolo-card-name">{line.name}</span>
                    <span className="yolo-card-actions">
                      <button className="yolo-card-btn" onClick={() => removeLine(line.id)} title="Delete"><Icon name="x" style={{ width: 10, height: 10 }} /></button>
                    </span>
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Capture Rule Editor Popup */}
        {editingRuleRoiId && (
          <div style={{
            position: 'fixed', inset: 0, zIndex: 10000,
            display: 'flex', alignItems: 'center', justifyContent: 'center',
            background: 'rgba(0,0,0,0.4)', backdropFilter: 'blur(4px)', WebkitBackdropFilter: 'blur(4px)',
          }} onClick={() => setEditingRuleRoiId(null)}>
            <CaptureRuleEditor roiId={editingRuleRoiId} onAdd={addCaptureRule} onCancel={() => setEditingRuleRoiId(null)} />
          </div>
        )}

        {/* Capture Events Strip — no title */}
        {captureEvents.length > 0 && (
          <div className="yolo-captures">
            {captureEvents.map((evt, i) => (
              <div key={`${evt.rule_id}-${evt.timestamp}-${i}`} className="yolo-capture-item"
                title={`${evt.rule_name}\n${evt.condition}\n${new Date(evt.timestamp).toLocaleTimeString()}`}
                onClick={() => setLightboxSrc(`data:image/jpeg;base64,${evt.image_base64}`)}>
                <img src={`data:image/jpeg;base64,${evt.image_base64}`} alt={evt.rule_name} />
                <span className="yolo-capture-label">{evt.rule_name}</span>
              </div>
            ))}
          </div>
        )}

        {/* Lightbox */}
        {lightboxSrc && (
          <div style={{
            position: 'fixed', inset: 0, zIndex: 20000,
            display: 'flex', alignItems: 'center', justifyContent: 'center',
            background: 'rgba(0,0,0,0.7)', backdropFilter: 'blur(6px)', WebkitBackdropFilter: 'blur(6px)',
            cursor: 'zoom-out',
          }} onClick={() => setLightboxSrc(null)}>
            <img src={lightboxSrc} alt="capture" style={{
              maxWidth: '90vw', maxHeight: '85vh', borderRadius: '8px',
              boxShadow: '0 8px 40px rgba(0,0,0,0.4)',
            }} onClick={e => e.stopPropagation()} />
          </div>
        )}
      </div>
    </div>
  )
}

// ============================================================================
// Custom Dropdown (shadcn/ui-inspired)
// ============================================================================

function CustomDropdown({ value, options, open, onToggle, onChange }: {
  value: string
  options: { value: string; label: string }[]
  open: boolean
  onToggle: () => void
  onChange: (value: string) => void
}) {
  const selected = options.find(o => o.value === value)
  const triggerStyle: React.CSSProperties = {
    width: '100%', height: '36px', fontSize: '13px', padding: '0 10px',
    border: open ? '1px solid hsl(221 83% 53%)' : '1px solid hsl(0 0% 82%)',
    borderRadius: '6px', background: '#fff', color: 'hsl(0 0% 9%)',
    boxSizing: 'border-box', fontFamily: 'inherit', cursor: 'pointer',
    display: 'flex', alignItems: 'center', justifyContent: 'space-between',
    outline: 'none', transition: 'border-color 0.15s',
    boxShadow: open ? '0 0 0 3px rgba(59,130,246,0.12)' : 'none',
  }
  const panelStyle: React.CSSProperties = {
    position: 'absolute', left: 0, right: 0, top: '100%', marginTop: '4px',
    background: '#fff', border: '1px solid hsl(0 0% 90%)', borderRadius: '6px',
    boxShadow: '0 4px 20px rgba(0,0,0,0.1)', maxHeight: '180px', overflowY: 'auto',
    zIndex: 100, padding: '4px',
  }
  const optionStyle = (active: boolean): React.CSSProperties => ({
    padding: '6px 10px', fontSize: '13px', cursor: 'pointer', borderRadius: '4px',
    background: active ? 'hsl(221 83% 53%)' : 'transparent',
    color: active ? '#fff' : 'hsl(0 0% 9%)',
    transition: 'background 0.1s',
  })

  return (
    <div style={{ position: 'relative' }}>
      <button type="button" style={triggerStyle} onClick={onToggle}>
        <span>{selected?.label || value}</span>
        <svg width="12" height="12" viewBox="0 0 12 12" fill="none" style={{ opacity: 0.5, flexShrink: 0 }}>
          <path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/>
        </svg>
      </button>
      {open && (
        <div style={panelStyle}>
          {options.map(opt => (
            <div key={opt.value}
              style={optionStyle(opt.value === value)}
              onClick={() => onChange(opt.value)}
              onMouseEnter={e => { if (opt.value !== value) (e.currentTarget.style.background = 'hsl(0 0% 95%)') }}
              onMouseLeave={e => { if (opt.value !== value) (e.currentTarget.style.background = 'transparent') }}>
              {opt.label}
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

// ============================================================================
// Capture Rule Editor (inline component)
// ============================================================================

function CaptureRuleEditor({ roiId, onAdd, onCancel }: {
  roiId: string
  onAdd: (roiId: string, condition: CaptureCondition, cooldown: number) => void
  onCancel: () => void
}) {
  const [condType, setCondType] = useState<'threshold' | 'presence' | 'absence'>('threshold')
  const [className, setClassName] = useState('person')
  const [threshold, setThreshold] = useState(3)
  const [cooldown, setCooldown] = useState(5)
  const [openDropdown, setOpenDropdown] = useState<string | null>(null)

  const condOptions = [
    { value: 'threshold' as const, label: 'Threshold (count ≥ N)' },
    { value: 'presence' as const, label: 'Presence (appears)' },
    { value: 'absence' as const, label: 'Absence (disappears)' },
  ]
  const classOptions = ['person','car','truck','bus','bicycle','motorcycle','dog','cat','bird','chair','bottle','cell phone','backpack','umbrella','handbag','suitcase']

  const labelStyle: React.CSSProperties = { fontSize: '12px', fontWeight: 500, color: 'hsl(0 0% 45%)' }
  const fieldStyle: React.CSSProperties = { display: 'flex', flexDirection: 'column', gap: '6px', marginBottom: '14px' }
  const inputStyle: React.CSSProperties = {
    width: '100%', height: '36px', fontSize: '13px', padding: '0 10px',
    border: '1px solid hsl(0 0% 82%)', borderRadius: '6px',
    background: '#fff', color: 'hsl(0 0% 9%)', outline: 'none',
    boxSizing: 'border-box', fontFamily: 'inherit',
  }

  return (
    <div style={{
      background: '#fff', border: '1px solid hsl(0 0% 90%)', borderRadius: '12px',
      padding: '20px', minWidth: '300px', maxWidth: '360px',
      boxShadow: '0 20px 60px rgba(0,0,0,0.15), 0 0 0 1px rgba(0,0,0,0.05)',
      fontSize: '13px', fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif',
      color: 'hsl(0 0% 9%)',
    }} onClick={e => e.stopPropagation()}>
      <div style={{ fontSize: '15px', fontWeight: 600, marginBottom: '16px', paddingBottom: '12px', borderBottom: '1px solid hsl(0 0% 90%)' }}>Add Capture Rule</div>

      {/* Condition Type */}
      <label style={fieldStyle}>
        <span style={labelStyle}>Condition</span>
        <CustomDropdown
          value={condType} options={condOptions}
          open={openDropdown === 'cond'} onToggle={() => setOpenDropdown(openDropdown === 'cond' ? null : 'cond')}
          onChange={v => { setCondType(v as any); setOpenDropdown(null) }}
        />
      </label>

      {/* Class */}
      <label style={fieldStyle}>
        <span style={labelStyle}>Class</span>
        <CustomDropdown
          value={className} options={classOptions.map(c => ({ value: c, label: c }))}
          open={openDropdown === 'class'} onToggle={() => setOpenDropdown(openDropdown === 'class' ? null : 'class')}
          onChange={v => { setClassName(v); setOpenDropdown(null) }}
        />
      </label>

      {condType === 'threshold' && (
        <label style={fieldStyle}>
          <span style={labelStyle}>Threshold</span>
          <input style={inputStyle} type="number" min={1} max={100} value={threshold}
            onChange={e => setThreshold(Number(e.target.value))} />
        </label>
      )}
      <label style={fieldStyle}>
        <span style={labelStyle}>Cooldown (s)</span>
        <input style={inputStyle} type="number" min={1} max={300} value={cooldown}
          onChange={e => setCooldown(Number(e.target.value))} />
      </label>
      <div style={{ display: 'flex', justifyContent: 'flex-end', gap: '8px', marginTop: '18px', paddingTop: '14px', borderTop: '1px solid hsl(0 0% 90%)' }}>
        <button style={{ height: '34px', padding: '0 16px', fontSize: '13px', fontWeight: 500, border: '1px solid hsl(0 0% 82%)', borderRadius: '6px', background: '#fff', color: 'hsl(0 0% 40%)', cursor: 'pointer', fontFamily: 'inherit' }}
          onClick={onCancel}
          onMouseEnter={e => (e.currentTarget.style.background = 'hsl(0 0% 96%)')}
          onMouseLeave={e => (e.currentTarget.style.background = '#fff')}>Cancel</button>
        <button style={{ height: '34px', padding: '0 16px', fontSize: '13px', fontWeight: 500, border: 'none', borderRadius: '6px', background: 'hsl(221 83% 53%)', color: '#fff', cursor: 'pointer', fontFamily: 'inherit' }}
          onClick={() => {
            const condition: CaptureCondition = condType === 'threshold'
              ? { type: 'threshold', class_name: className, threshold }
              : { type: condType, class_name: className }
            onAdd(roiId, condition, cooldown)
          }}
          onMouseEnter={e => (e.currentTarget.style.opacity = '0.9')}
          onMouseLeave={e => (e.currentTarget.style.opacity = '1')}>Add Rule</button>
      </div>
    </div>
  )
}

// ============================================================================
// Export variants
// ============================================================================

export const YoloVideoCard = (props: ExtensionComponentProps) => (
  <div style={{ height: '100%', minHeight: 300 }}>
    <YoloVideoDisplay {...props} title={props.title || 'YOLO Detection'} />
  </div>
)

export const YoloVideoWidget = (props: ExtensionComponentProps) => (
  <div style={{ height: 280 }}>
    <YoloVideoDisplay {...props} title={props.title || 'YOLO'} />
  </div>
)

export const YoloVideoPanel = (props: ExtensionComponentProps) => (
  <div style={{ height: '100%', minHeight: 500 }}>
    <YoloVideoDisplay {...props} title={props.title || 'YOLO Video Detection'} />
  </div>
)

export default YoloVideoDisplay
export const Component = YoloVideoDisplay
export const Card = YoloVideoCard
export const Widget = YoloVideoWidget
export const Panel = YoloVideoPanel
