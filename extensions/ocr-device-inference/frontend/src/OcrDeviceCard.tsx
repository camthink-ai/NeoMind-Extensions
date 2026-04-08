/**
 * OCR Device Inference Extension Card
 *
 * Provides dual-tab interface for manual OCR testing and device binding management.
 * Matches the styling pattern of yolo-device-inference.
 */

import React, { useState, useEffect, useRef, useCallback } from 'react'

// ============================================================================
// Types
// ============================================================================

export interface OcrDeviceCardProps {
  executeCommand?: (command: string, args: Record<string, unknown>) => Promise<{ success: boolean; data?: any; error?: string }>
  config?: Record<string, unknown>
}

export interface TextBlock {
  text: string
  confidence: number
  bbox: { x: number; y: number; width: number; height: number }
}

export interface OcrResult {
  device_id: string
  text_blocks: TextBlock[]
  full_text: string
  total_blocks: number
  avg_confidence: number
  inference_time_ms: number
  image_width: number
  image_height: number
  timestamp: number
  annotated_image_base64: string | null
}

export interface RoiPolygon {
  label?: string
  points: [number, number][]  // [[x1,y1], [x2,y2], ...] normalized 0-1
}

export interface DeviceBinding {
  device_id: string
  device_name?: string
  image_metric: string
  result_metric_prefix: string
  draw_boxes: boolean
  active: boolean
  roi_regions?: RoiPolygon[]
  roi_overlap_threshold?: number
}

export interface BindingStatus {
  binding: DeviceBinding
  last_inference: number | null
  total_inferences: number
  total_text_blocks: number
  last_error: string | null
  last_image?: string
  last_text_blocks?: TextBlock[]
  last_annotated_image?: string
  last_full_text?: string
}

export interface ExtensionStatus {
  model_loaded: boolean
  model_version: string
  total_bindings: number
  total_inferences: number
  total_text_blocks: number
  total_errors: number
}

interface Device {
  id: string
  name: string
  type?: string
  metrics?: Metric[]
}

interface Metric {
  id: string
  name: string
  display_name?: string
  type?: string
  data_type?: string
  value?: any
}

// ============================================================================
// Styles
// ============================================================================

const CSS_ID = 'ocr-styles-v2'

const STYLES = `
.ocr {
  --ocr-fg: hsl(240 10% 10%);
  --ocr-muted: hsl(240 5% 45%);
  --ocr-accent: hsl(200 70% 55%);
  --ocr-card: rgba(255,255,255,0.5);
  --ocr-border: rgba(0,0,0,0.06);
  --ocr-hover: rgba(0,0,0,0.03);
  --ocr-danger: hsl(0 72% 51%);
  --ocr-success: hsl(200 70% 45%);
  width: 100%;
  height: 100%;
  font-size: 12px;
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
}

.dark .ocr {
  --ocr-fg: hsl(0 0% 95%);
  --ocr-muted: hsl(0 0% 60%);
  --ocr-card: rgba(30,30,30,0.5);
  --ocr-border: rgba(255,255,255,0.08);
  --ocr-hover: rgba(255,255,255,0.03);
}

.ocr-card {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: 12px;
  background: var(--ocr-card);
  backdrop-filter: blur(12px);
  border: 1px solid var(--ocr-border);
  border-radius: 8px;
  box-sizing: border-box;
}

.ocr-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-shrink: 0;
  margin-bottom: 10px;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--ocr-border);
}

.ocr-title {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--ocr-fg);
  font-size: 14px;
  font-weight: 600;
}

.ocr-badge {
  padding: 3px 8px;
  background: rgba(142, 70, 65, 0.1);
  color: var(--ocr-accent);
  border-radius: 4px;
  font-size: 9px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.3px;
}

.ocr-badge-active {
  background: hsl(142 70% 90%);
  color: hsl(142 70% 30%);
}

.dark .ocr-badge-active {
  background: hsl(142 70% 20%);
  color: hsl(142 70% 70%);
}

/* Tabs */
.ocr-tabs {
  display: flex;
  gap: 4px;
  flex-shrink: 0;
  margin-bottom: 10px;
  background: var(--ocr-hover);
  padding: 4px;
  border-radius: 6px;
}

.ocr-tab {
  flex: 1;
  padding: 6px 12px;
  border: none;
  border-radius: 4px;
  font-size: 11px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.2s;
  background: transparent;
  color: var(--ocr-muted);
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
}

.ocr-tab:hover {
  background: var(--ocr-hover);
  color: var(--ocr-fg);
}

.ocr-tab-active {
  background: var(--ocr-card);
  color: var(--ocr-fg);
  box-shadow: 0 1px 3px rgba(0,0,0,0.1);
}

/* Content area */
.ocr-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-height: 0;
  overflow: auto;
}

.ocr-content-hidden {
  display: none;
}

/* Upload area */
.ocr-upload-area {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  border: 2px dashed var(--ocr-border);
  border-radius: 8px;
  padding: 20px;
  text-align: center;
  cursor: pointer;
  transition: all 0.2s;
  background: rgba(0,0,0,0.02);
  min-height: 150px;
}

.ocr-upload-area:hover {
  border-color: var(--ocr-accent);
  background: rgba(142, 70, 65, 0.05);
}

.ocr-upload-area-dragging {
  border-color: var(--ocr-accent);
  background: rgba(142, 70, 65, 0.1);
}

.ocr-upload-placeholder {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  color: var(--ocr-muted);
}

.ocr-upload-icon {
  width: 32px;
  height: 32px;
  opacity: 0.5;
}

.ocr-upload-text {
  font-size: 11px;
}

.ocr-upload-hint {
  font-size: 9px;
  opacity: 0.7;
}

/* Preview area - adaptive for image display */
.ocr-preview-area {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-height: 0;
  position: relative;
  border-radius: 6px;
  overflow: hidden;
  background: rgba(0,0,0,0.04);
}

.dark .ocr-preview-area {
  background: rgba(255,255,255,0.04);
}

.ocr-image-preview {
  flex: 1;
  border-radius: 6px;
  overflow: hidden;
  background: rgba(0,0,0,0.03);
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 120px;
}

.dark .ocr-image-preview {
  background: rgba(255,255,255,0.02);
}

.ocr-image-preview img {
  max-width: 100%;
  max-height: 100%;
  object-fit: contain;
}

/* Floating action buttons */
.ocr-actions-floating {
  position: absolute;
  bottom: 8px;
  left: 50%;
  transform: translateX(-50%);
  display: flex;
  gap: 8px;
  background: rgba(255,255,255,0.92);
  backdrop-filter: blur(12px);
  padding: 6px 10px;
  border-radius: 8px;
  border: 1px solid var(--ocr-border);
  box-shadow: 0 2px 12px rgba(0,0,0,0.12);
  z-index: 10;
}

.dark .ocr-actions-floating {
  background: rgba(40,40,40,0.92);
  border-color: rgba(255,255,255,0.1);
}

.ocr-actions-bottom {
  display: flex;
  gap: 8px;
  padding: 10px;
  background: var(--ocr-bg);
  border-top: 1px solid var(--ocr-border);
  margin-top: auto;
}

/* Annotated image preview */
.ocr-annotated-preview {
  margin-bottom: 8px;
  border-radius: 6px;
  overflow: hidden;
  border: 1px solid var(--ocr-border);
}

.ocr-annotated-preview img {
  width: 100%;
  height: auto;
  display: block;
}

.ocr-annotated-label {
  padding: 6px 8px;
  font-size: 10px;
  font-weight: 600;
  color: var(--ocr-muted);
  background: var(--ocr-hover);
}

.ocr-annotated-preview img {
  width: 100%;
  height: auto;
  display: block;
}

/* Text results */
.ocr-text-results {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.ocr-text-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 8px;
  background: var(--ocr-hover);
  border-radius: 4px;
}

.ocr-text-label {
  font-size: 10px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.3px;
  color: var(--ocr-muted);
}

.ocr-copy-btn {
  padding: 3px 6px;
  border: 1px solid var(--ocr-border);
  border-radius: 4px;
  font-size: 9px;
  cursor: pointer;
  background: transparent;
  color: var(--ocr-muted);
  transition: all 0.15s;
  display: inline-flex;
  align-items: center;
}

.ocr-copy-btn:hover {
  background: var(--ocr-hover);
  color: var(--ocr-fg);
  border-color: rgba(0,0,0,0.12);
}

.ocr-text-content {
  flex: 1;
  padding: 8px;
  background: var(--ocr-hover);
  border-radius: 4px;
  font-size: 11px;
  line-height: 1.5;
  color: var(--ocr-fg);
  white-space: pre-wrap;
  word-break: break-word;
  max-height: 100px;
  overflow-y: auto;
}

.ocr-text-placeholder {
  color: var(--ocr-muted);
  font-style: italic;
}

/* Text blocks */
.ocr-text-blocks {
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 80px;
  overflow-y: auto;
}

.ocr-text-block {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 4px 6px;
  background: var(--ocr-hover);
  border-radius: 3px;
  font-size: 10px;
}

.ocr-text-block-text {
  flex: 1;
  color: var(--ocr-fg);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.ocr-text-block-conf {
  padding: 2px 4px;
  border-radius: 3px;
  font-size: 9px;
  font-weight: 600;
  background: hsl(142 70% 90%);
  color: hsl(142 70% 30%);
}

.dark .ocr-text-block-conf {
  background: hsl(142 70% 20%);
  color: hsl(142 70% 70%);
}

.ocr-text-block-conf-low {
  background: hsl(45 90% 90%);
  color: hsl(45 90% 30%);
}

.dark .ocr-text-block-conf-low {
  background: hsl(45 90% 20%);
  color: hsl(45 90% 70%);
}

/* Buttons */
.ocr-btn {
  padding: 7px 14px;
  border: 1px solid var(--ocr-border);
  border-radius: 5px;
  font-size: 11px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.15s;
  background: var(--ocr-card);
  color: var(--ocr-fg);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 5px;
  white-space: nowrap;
}

.ocr-btn:hover {
  background: var(--ocr-hover);
  border-color: rgba(0,0,0,0.12);
}

.dark .ocr-btn:hover {
  border-color: rgba(255,255,255,0.15);
}

.ocr-btn-primary {
  background: var(--ocr-accent);
  border-color: var(--ocr-accent);
  color: #fff;
}

.ocr-btn-primary:hover {
  opacity: 0.85;
  background: var(--ocr-accent);
}

.ocr-btn-accent {
  border-color: var(--ocr-accent);
  color: var(--ocr-accent);
}

.ocr-btn-danger {
  color: var(--ocr-danger);
  border-color: hsl(0 72% 51% 0.3);
}

.ocr-btn-danger:hover {
  background: hsl(0 72% 51% 0.1);
}

.ocr-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.ocr-btn-sm {
  padding: 4px 10px;
  font-size: 10px;
  border-radius: 4px;
  gap: 4px;
}

.ocr-actions {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
  padding: 8px 0 0 0;
  border-top: 1px solid var(--ocr-border);
  background: var(--ocr-card);
}

.ocr-actions-sticky {
  position: sticky;
  bottom: 0;
  margin: 0 -12px -12px -12px;
  padding: 10px 12px;
  background: var(--ocr-card);
  backdrop-filter: blur(8px);
}

.ocr-spinner {
  width: 16px;
  height: 16px;
  border: 2px solid var(--ocr-border);
  border-top-color: var(--ocr-accent);
  border-radius: 50%;
  animation: ocr-spin 0.7s linear infinite;
}

@keyframes ocr-spin {
  to { transform: rotate(360deg); }
}

/* Device bindings list */
.ocr-bindings-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.ocr-binding-item {
  padding: 10px;
  background: var(--ocr-hover);
  border-radius: 6px;
  border: 1px solid var(--ocr-border);
}

.ocr-binding-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 6px;
}

.ocr-binding-name {
  font-weight: 600;
  color: var(--ocr-fg);
  font-size: 11px;
}

.ocr-binding-status {
  padding: 2px 6px;
  border-radius: 3px;
  font-size: 9px;
  font-weight: 600;
  text-transform: uppercase;
}

.ocr-binding-status-active {
  background: hsl(142 70% 90%);
  color: hsl(142 70% 30%);
}

.dark .ocr-binding-status-active {
  background: hsl(142 70% 20%);
  color: hsl(142 70% 70%);
}

.ocr-binding-status-paused {
  background: hsl(45 90% 90%);
  color: hsl(45 90% 30%);
}

.dark .ocr-binding-status-paused {
  background: hsl(45 90% 20%);
  color: hsl(45 90% 70%);
}

.ocr-binding-info {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-bottom: 6px;
  font-size: 10px;
  color: var(--ocr-muted);
}

.ocr-binding-stat {
  display: flex;
  align-items: center;
  gap: 3px;
}

.ocr-binding-actions {
  display: flex;
  gap: 6px;
  margin-top: 8px;
  padding-top: 8px;
  border-top: 1px solid var(--ocr-border);
}

/* Form */
.ocr-form {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 10px;
  background: var(--ocr-hover);
  border-radius: 6px;
}

.ocr-form-group {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.ocr-form-label {
  font-size: 10px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.3px;
  color: var(--ocr-muted);
}

.ocr-form-input {
  padding: 6px 8px;
  border: 1px solid var(--ocr-border);
  border-radius: 4px;
  font-size: 11px;
  background: var(--ocr-card);
  color: var(--ocr-fg);
  transition: border-color 0.15s;
}

.ocr-form-input:focus {
  outline: none;
  border-color: var(--ocr-accent);
}

.ocr-form-select {
  padding: 6px 8px;
  border: 1px solid var(--ocr-border);
  border-radius: 4px;
  font-size: 11px;
  background: var(--ocr-card);
  color: var(--ocr-fg);
  cursor: pointer;
}

/* Web-style dropdown */
.ocr-dropdown {
  position: relative;
}

.ocr-dropdown-trigger {
  width: 100%;
  padding: 8px 10px;
  border: 1px solid var(--ocr-border);
  border-radius: 4px;
  font-size: 11px;
  background: var(--ocr-card);
  color: var(--ocr-fg);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  transition: border-color 0.15s;
}

.ocr-dropdown-trigger:hover {
  border-color: var(--ocr-accent);
}

.ocr-dropdown-trigger-placeholder {
  color: var(--ocr-muted);
}

.ocr-dropdown-arrow {
  width: 12px;
  height: 12px;
  transition: transform 0.2s;
}

.ocr-dropdown-open .ocr-dropdown-arrow {
  transform: rotate(180deg);
}

.ocr-dropdown-menu {
  position: absolute;
  top: 100%;
  left: 0;
  right: 0;
  margin-top: 4px;
  background: var(--ocr-card);
  border: 1px solid var(--ocr-border);
  border-radius: 4px;
  box-shadow: 0 4px 12px rgba(0,0,0,0.15);
  max-height: 200px;
  overflow-y: auto;
  z-index: 100;
  display: none;
}

.ocr-dropdown-open .ocr-dropdown-menu {
  display: block;
}

.ocr-dropdown-item {
  padding: 8px 10px;
  font-size: 11px;
  cursor: pointer;
  transition: background 0.15s;
  display: flex;
  align-items: center;
  gap: 8px;
}

.ocr-dropdown-item:hover {
  background: var(--ocr-hover);
}

.ocr-dropdown-item-selected {
  background: rgba(142, 70, 65, 0.1);
  color: var(--ocr-accent);
}

.ocr-dropdown-item-empty {
  padding: 12px;
  font-size: 11px;
  color: var(--ocr-muted);
  text-align: center;
}

.ocr-form-checkbox {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  color: var(--ocr-fg);
}

.ocr-empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 30px;
  color: var(--ocr-muted);
  font-size: 11px;
  text-align: center;
}

.ocr-empty-icon {
  width: 40px;
  height: 40px;
  opacity: 0.3;
  margin-bottom: 10px;
}

/* Error message */
.ocr-error {
  padding: 8px 10px;
  background: hsl(0 72% 51% 0.1);
  border: 1px solid hsl(0 72% 51% 0.3);
  border-radius: 4px;
  color: var(--ocr-danger);
  font-size: 10px;
  display: flex;
  align-items: center;
  gap: 6px;
}

/* Success message */
.ocr-success {
  padding: 8px 10px;
  background: hsl(142 70% 45% 0.1);
  border: 1px solid hsl(142 70% 45% 0.3);
  border-radius: 4px;
  color: var(--ocr-success);
  font-size: 10px;
  display: flex;
  align-items: center;
  gap: 6px;
}

/* Binding preview styles */
.ocr-binding-preview {
  margin-top: 8px;
  border-radius: 6px;
  overflow: hidden;
  background: rgba(0,0,0,0.03);
  position: relative;
  border: 1px solid var(--ocr-border);
}
.dark .ocr-binding-preview {
  background: rgba(255,255,255,0.03);
}
.ocr-binding-preview img {
  width: 100%;
  height: auto;
  max-height: 200px;
  object-fit: contain;
  display: block;
}
.ocr-binding-text-container {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  max-height: 80px;
  overflow-y: auto;
  padding: 8px 10px;
  background: linear-gradient(to bottom, rgba(255,255,255,0), rgba(255,255,255,0.9) 20%);
  backdrop-filter: blur(6px);
  border-top: 1px solid rgba(0,0,0,0.05);
  font-size: 11px;
  line-height: 1.5;
  color: var(--ocr-fg);
  white-space: pre-wrap;
  word-break: break-word;
}
.dark .ocr-binding-text-container {
  background: linear-gradient(to bottom, rgba(30,30,30,0), rgba(30,30,30,0.9) 20%);
  border-top-color: rgba(255,255,255,0.06);
}
.ocr-binding-text-label {
  font-size: 10px;
  font-weight: 500;
  color: var(--ocr-muted);
  margin-bottom: 4px;
}
.ocr-binding-text {
  font-size: 11px;
  line-height: 1.5;
  color: var(--ocr-fg);
  white-space: pre-wrap;
  word-break: break-word;
}
.ocr-binding-empty-text {
  color: var(--ocr-muted);
  font-style: italic;
  font-size: 10px;
  text-align: center;
  padding: 20px;
}

/* Floating results overlay for manual test */
.ocr-results-overlay {
  position: absolute;
  bottom: 48px;
  right: 8px;
  left: 8px;
  max-height: 45%;
  background: rgba(255,255,255,0.94);
  backdrop-filter: blur(12px);
  border-radius: 8px;
  border: 1px solid var(--ocr-border);
  box-shadow: 0 4px 16px rgba(0,0,0,0.15);
  display: flex;
  flex-direction: column;
  overflow: hidden;
  z-index: 10;
  transition: opacity 0.2s, transform 0.2s;
}
.dark .ocr-results-overlay {
  background: rgba(30,30,30,0.94);
  border-color: rgba(255,255,255,0.1);
}
.ocr-results-overlay-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 10px;
  border-bottom: 1px solid var(--ocr-border);
  flex-shrink: 0;
  background: rgba(0,0,0,0.02);
}
.dark .ocr-results-overlay-header {
  background: rgba(255,255,255,0.03);
}
.ocr-results-overlay-title {
  font-size: 10px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.3px;
  color: var(--ocr-muted);
}
.ocr-results-overlay-body {
  flex: 1;
  overflow-y: auto;
  padding: 10px 12px;
  font-size: 11px;
  line-height: 1.6;
  color: var(--ocr-fg);
  white-space: pre-wrap;
  word-break: break-word;
}

/* ROI Editor overlay */
.ocr-roi-editor {
  position: absolute;
  inset: 0;
  z-index: 20;
  background: rgba(0,0,0,0.3);
  border-radius: 6px;
  display: flex;
  flex-direction: column;
}
.ocr-roi-editor-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 10px;
  background: var(--ocr-card);
  border-bottom: 1px solid var(--ocr-border);
  flex-shrink: 0;
}
.ocr-roi-editor-title {
  font-size: 11px;
  font-weight: 600;
  color: var(--ocr-fg);
}
.ocr-roi-editor-canvas-wrap {
  flex: 1;
  position: relative;
  overflow: hidden;
}
.ocr-roi-editor-canvas-wrap img {
  width: 100%;
  height: 100%;
  object-fit: contain;
}
.ocr-roi-editor-canvas-wrap canvas {
  position: absolute;
  inset: 0;
  cursor: crosshair;
}
.ocr-roi-editor-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 10px;
  background: var(--ocr-card);
  border-top: 1px solid var(--ocr-border);
  flex-shrink: 0;
  gap: 8px;
}
.ocr-roi-threshold {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 10px;
  color: var(--ocr-muted);
}
.ocr-roi-threshold input[type=range] {
  width: 80px;
  height: 4px;
  accent-color: var(--ocr-accent);
}
.ocr-roi-polygon-item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 3px 6px;
  background: var(--ocr-hover);
  border-radius: 3px;
  font-size: 10px;
}
.ocr-roi-polygon-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
}
.ocr-roi-hint {
  font-size: 9px;
  color: var(--ocr-muted);
  padding: 4px 10px;
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
// Icons
// ============================================================================

const ICONS: Record<string, string> = {
  type: '<polyline points="4 7 4 4 20 4 20 7"/><line x1="9" y1="20" x2="15" y2="20"/><line x1="12" y1="4" x2="12" y2="20"/>',
  link: '<path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/>',
  play: '<polygon points="5 3 19 12 5 21 5 3"/>',
  pause: '<rect x="6" y="4" width="4" height="16"/><rect x="14" y="4" width="4" height="16"/>',
  trash: '<polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>',
  image: '<rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/>',
  upload: '<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/>',
  copy: '<rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>',
  check: '<polyline points="20 6 9 17 4 12"/>',
  x: '<line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>',
  alert: '<circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/>',
  target: '<circle cx="12" cy="12" r="10"/><circle cx="12" cy="12" r="6"/><circle cx="12" cy="12" r="2"/>',
  eye: '<path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/>',
  eyeOff: '<path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24"/><line x1="1" y1="1" x2="23" y2="23"/>',
}

const Icon = ({ name, className = '', style }: { name: string; className?: string; style?: React.CSSProperties }) => (
  <svg
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
    className={className}
    style={style}
    dangerouslySetInnerHTML={{ __html: ICONS[name] || ICONS.type }}
  />
)

// ============================================================================
// API Helpers
// ============================================================================

const EXTENSION_ID = 'ocr-device-inference'

// Image compression to avoid HTTP 413 errors
const compressImage = (dataUrl: string, maxSizeKB = 500): Promise<string> => {
  return new Promise((resolve, reject) => {
    const img = new Image()
    img.onload = () => {
      let { width, height } = img
      const maxDimension = 1920

      // Scale down if needed
      if (width > maxDimension || height > maxDimension) {
        const ratio = Math.min(maxDimension / width, maxDimension / height)
        width = Math.round(width * ratio)
        height = Math.round(height * ratio)
      }

      const canvas = document.createElement('canvas')
      canvas.width = width
      canvas.height = height

      const ctx = canvas.getContext('2d')
      if (!ctx) {
        reject(new Error('Failed to get canvas context'))
        return
      }

      ctx.drawImage(img, 0, 0, width, height)

      // Try different quality levels until size is acceptable
      let quality = 0.9
      const tryCompress = (): string | null => {
        const data = canvas.toDataURL('image/jpeg', quality)
        // Estimate base64 size (actual bytes = base64 length * 3/4)
        const sizeKB = (data.length * 0.75) / 1024
        if (sizeKB <= maxSizeKB || quality <= 0.1) {
          return data
        }
        return null
      }

      let result = tryCompress()
      while (!result && quality > 0.1) {
        quality -= 0.1
        result = tryCompress()
      }

      if (result) {
        resolve(result)
      } else {
        // Fallback to PNG if JPEG compression fails
        resolve(canvas.toDataURL('image/png'))
      }
    }
    img.onerror = () => reject(new Error('Failed to load image'))
    img.src = dataUrl
  })
}

const getApiHeaders = () => {
  const token = localStorage.getItem('neomind_token') || sessionStorage.getItem('neomind_token_session')
  const headers: Record<string, string> = { 'Content-Type': 'application/json' }
  if (token) headers['Authorization'] = `Bearer ${token}`
  return headers
}

const getApiBase = () => (window as any).__TAURI__ ? 'http://localhost:9375/api' : '/api'

async function executeCommandApi(
  command: string,
  args: Record<string, unknown> = {}
): Promise<{ success: boolean; data?: any; error?: string }> {
  try {
    const res = await fetch(`${getApiBase()}/extensions/${EXTENSION_ID}/command`, {
      method: 'POST',
      headers: getApiHeaders(),
      body: JSON.stringify({ command, args })
    })
    if (!res.ok) return { success: false, error: `HTTP ${res.status}` }
    return res.json()
  } catch (e) {
    return { success: false, error: e instanceof Error ? e.message : 'Network error' }
  }
}

async function fetchDevices(): Promise<Device[]> {
  try {
    const res = await fetch(`${getApiBase()}/devices`, { headers: getApiHeaders() })
    if (!res.ok) return []
    const data = await res.json()
    const devices = data.data?.devices || data.devices || data.data || []
    console.log('[OCR Frontend] Fetched devices:', devices.length, devices.map((d: Device) => ({ id: d.id, name: d.name, metricsCount: d.metrics?.length })))
    return devices
  } catch {
    return []
  }
}


// ============================================================================
// ROI Editor Component
// ============================================================================

const ROI_COLORS = ['#3b82f6', '#ef4444', '#22c55e', '#f59e0b', '#8b5cf6', '#ec4899']

interface RoiEditorProps {
  binding: DeviceBinding
  imageUrl?: string
  onSave: (regions: RoiPolygon[], threshold: number) => Promise<void>
  onCancel: () => void
}

const RoiEditor: React.FC<RoiEditorProps> = ({ binding, imageUrl, onSave, onCancel }) => {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const imgRef = useRef<HTMLImageElement | null>(null)
  const [regions, setRegions] = useState<RoiPolygon[]>(binding.roi_regions || [])
  const [threshold, setThreshold] = useState(binding.roi_overlap_threshold || 0.5)
  const [currentPoints, setCurrentPoints] = useState<[number, number][]>([])
  const [saving, setSaving] = useState(false)

  // Redraw canvas
  const redraw = useCallback(() => {
    const canvas = canvasRef.current
    if (!canvas || !imgRef.current) return
    const ctx = canvas.getContext('2d')
    if (!ctx) return

    const rect = canvas.parentElement?.getBoundingClientRect()
    if (!rect) return
    canvas.width = rect.width
    canvas.height = rect.height
    ctx.clearRect(0, 0, canvas.width, canvas.height)

    // Draw existing regions
    regions.forEach((region, idx) => {
      const color = ROI_COLORS[idx % ROI_COLORS.length]
      drawPolygon(ctx, region.points, color, canvas.width, canvas.height)
    })

    // Draw current drawing
    if (currentPoints.length > 0) {
      const color = ROI_COLORS[regions.length % ROI_COLORS.length]
      drawPolygon(ctx, currentPoints, color, canvas.width, canvas.height, true)
    }
  }, [regions, currentPoints])

  const drawPolygon = (
    ctx: CanvasRenderingContext2D,
    points: [number, number][],
    color: string,
    w: number,
    h: number,
    isOpen = false
  ) => {
    if (points.length === 0) return
    ctx.save()
    ctx.strokeStyle = color
    ctx.fillStyle = color + '33'
    ctx.lineWidth = 2

    ctx.beginPath()
    ctx.moveTo(points[0][0] * w, points[0][1] * h)
    for (let i = 1; i < points.length; i++) {
      ctx.lineTo(points[i][0] * w, points[i][1] * h)
    }
    if (!isOpen && points.length > 2) {
      ctx.closePath()
      ctx.fill()
    }
    ctx.stroke()

    // Draw vertex dots
    points.forEach((p) => {
      ctx.beginPath()
      ctx.arc(p[0] * w, p[1] * h, 4, 0, Math.PI * 2)
      ctx.fillStyle = color
      ctx.fill()
      ctx.strokeStyle = '#fff'
      ctx.lineWidth = 1
      ctx.stroke()
    })

    ctx.restore()
  }

  useEffect(() => {
    redraw()
    const handleResize = () => redraw()
    window.addEventListener('resize', handleResize)
    return () => window.removeEventListener('resize', handleResize)
  }, [redraw])

  const handleCanvasClick = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const canvas = canvasRef.current
    if (!canvas) return
    const rect = canvas.getBoundingClientRect()
    const x = (e.clientX - rect.left) / rect.width
    const y = (e.clientY - rect.top) / rect.height

    // Check if clicking near first point to close polygon
    if (currentPoints.length >= 3) {
      const first = currentPoints[0]
      const dist = Math.sqrt((x - first[0]) ** 2 + (y - first[1]) ** 2)
      if (dist < 0.03) {
        // Close the polygon
        setRegions([...regions, { points: [...currentPoints] as [number, number][] }])
        setCurrentPoints([])
        return
      }
    }

    setCurrentPoints([...currentPoints, [x, y] as [number, number]])
  }

  const handleDoubleClick = () => {
    if (currentPoints.length >= 3) {
      setRegions([...regions, { points: [...currentPoints] as [number, number][] }])
      setCurrentPoints([])
    }
  }

  const removeRegion = (idx: number) => {
    setRegions(regions.filter((_, i) => i !== idx))
  }

  const handleSave = async () => {
    setSaving(true)
    try {
      await onSave(regions, threshold)
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="ocr-roi-editor">
      <div className="ocr-roi-editor-header">
        <span className="ocr-roi-editor-title">
          ROI Editor — {binding.device_name || binding.device_id}
        </span>
        <button className="ocr-btn ocr-btn-sm" onClick={onCancel}>
          <Icon name="x" style={{ width: '12px', height: '12px' }} />
        </button>
      </div>
      <div className="ocr-roi-editor-canvas-wrap">
        {imageUrl && (
          <img
            ref={(el) => { if (el) { imgRef.current = el } }}
            src={imageUrl}
            alt="ROI base"
            onLoad={() => { setTimeout(redraw, 50) }}
          />
        )}
        <canvas
          ref={canvasRef}
          onClick={handleCanvasClick}
          onDoubleClick={handleDoubleClick}
        />
      </div>
      <div className="ocr-roi-hint">
        Click to add vertices. Double-click or click near first point to close polygon.
      </div>
      <div className="ocr-roi-editor-footer">
        <div className="ocr-roi-threshold">
          <span>Threshold:</span>
          <input
            type="range"
            min={0.1}
            max={1}
            step={0.05}
            value={threshold}
            onChange={(e) => setThreshold(parseFloat(e.target.value))}
          />
          <span>{Math.round(threshold * 100)}%</span>
        </div>
        <div style={{ display: 'flex', gap: '4px', flexWrap: 'wrap', flex: 1, justifyContent: 'center' }}>
          {regions.map((r, idx) => (
            <div key={idx} className="ocr-roi-polygon-item">
              <span className="ocr-roi-polygon-dot" style={{ background: ROI_COLORS[idx % ROI_COLORS.length] }} />
              <span>{r.label || `Region ${idx + 1}`}</span>
              <button
                style={{ background: 'none', border: 'none', cursor: 'pointer', padding: '0 2px', color: 'var(--ocr-danger)' }}
                onClick={() => removeRegion(idx)}
              >
                <Icon name="x" style={{ width: '10px', height: '10px' }} />
              </button>
            </div>
          ))}
          {currentPoints.length > 0 && (
            <div className="ocr-roi-polygon-item" style={{ opacity: 0.6 }}>
              <span className="ocr-roi-polygon-dot" style={{ background: ROI_COLORS[regions.length % ROI_COLORS.length] }} />
              Drawing ({currentPoints.length} pts)
            </div>
          )}
        </div>
        <div style={{ display: 'flex', gap: '6px' }}>
          <button className="ocr-btn ocr-btn-sm" onClick={onCancel}>Cancel</button>
          <button className="ocr-btn ocr-btn-sm ocr-btn-primary" onClick={handleSave} disabled={saving}>
            {saving ? <div className="ocr-spinner" /> : 'Save'}
          </button>
        </div>
      </div>
    </div>
  )
}

// ============================================================================
// Main Component
// ============================================================================

export const OcrDeviceCard: React.FC<OcrDeviceCardProps> = ({
  executeCommand = executeCommandApi
}) => {
  useEffect(() => injectStyles(), [])

  // Tab state
  const [activeTab, setActiveTab] = useState<'manual' | 'bindings'>('manual')

  // Manual test state
  const [selectedImage, setSelectedImage] = useState<string | null>(null)
  const [isDragging, setIsDragging] = useState(false)
  const [ocrResult, setOcrResult] = useState<OcrResult | null>(null)
  const [recognizing, setRecognizing] = useState(false)
  const [showResults, setShowResults] = useState(true)

  // ROI editor state
  const [roiEditorDeviceId, setRoiEditorDeviceId] = useState<string | null>(null)

  // Device bindings state
  const [devices, setDevices] = useState<Device[]>([])
  const [bindings, setBindings] = useState<BindingStatus[]>([])
  const [status, setStatus] = useState<ExtensionStatus | null>(null)

  // Form state
  const [formDevice, setFormDevice] = useState('')
  const [formImageMetric, setFormImageMetric] = useState('')
  const [deviceDropdownOpen, setDeviceDropdownOpen] = useState(false)
  const [metricDropdownOpen, setMetricDropdownOpen] = useState(false)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [success, setSuccess] = useState<string | null>(null)
  const [deviceMetrics, setDeviceMetrics] = useState<Metric[]>([])

  // Available metrics from selected device
  const availableMetrics = deviceMetrics

  // Fetch device metrics when device changes - using /devices/{id}/current endpoint
  useEffect(() => {
    const fetchDeviceMetrics = async () => {
      if (!formDevice) {
        setDeviceMetrics([])
        setFormImageMetric('')
        return
      }

      try {
        // Use /devices/{id}/current to get metrics (same as yolo-device)
        const res = await fetch(`${getApiBase()}/devices/${formDevice}/current`, { headers: getApiHeaders() })
        if (res.ok) {
          const data = await res.json()
          const metricsObj = data.data?.metrics || data.metrics || {}
          // Convert metrics object to array
          const metrics: Metric[] = Object.entries(metricsObj).map(([id, m]: [string, any]) => ({
            id,
            name: m.name || id,
            display_name: m.display_name || m.name || id,
            type: m.data_type || 'string',
            data_type: m.data_type || 'string'
          }))
          console.log('[OCR Frontend] Fetched device metrics from /current:', formDevice, metrics.length, metrics.map(m => m.id))
          setDeviceMetrics(metrics)

          // Auto-select image metric
          if (metrics.length > 0) {
            const imageMetric = metrics.find(m =>
              m.id === 'image' || m.name === 'image' ||
              m.data_type === 'image' || m.id.toLowerCase().includes('image')
            )
            setFormImageMetric(imageMetric?.id || metrics[0].id)
          } else {
            setFormImageMetric('')
          }
        } else {
          console.log('[OCR Frontend] /current endpoint failed, status:', res.status)
          setDeviceMetrics([])
          setFormImageMetric('')
        }
      } catch (e) {
        console.error('[OCR Frontend] Failed to fetch device metrics:', e)
        setDeviceMetrics([])
        setFormImageMetric('')
      }
    }

    fetchDeviceMetrics()
  }, [formDevice])

  const fileInputRef = useRef<HTMLInputElement>(null)

  // Load devices
  useEffect(() => {
    const loadDevices = async () => {
      const deviceList = await fetchDevices()
      setDevices(Array.isArray(deviceList) ? deviceList : [])
    }
    loadDevices()
  }, [])

  // Close dropdowns on outside click
  useEffect(() => {
    const handleClickOutside = () => {
      setDeviceDropdownOpen(false)
    }
    if (deviceDropdownOpen) {
      document.addEventListener('click', handleClickOutside)
      return () => document.removeEventListener('click', handleClickOutside)
    }
  }, [deviceDropdownOpen])

  // Refresh bindings and status
  const refresh = useCallback(async () => {
    const statusResult = await executeCommand('get_status', {})
    if (statusResult.success && statusResult.data) {
      setStatus(statusResult.data)
    }

    const bindingsResult = await executeCommand('get_bindings', {})
    if (bindingsResult.success && bindingsResult.data?.bindings) {
      setBindings(bindingsResult.data.bindings)
    }
  }, [executeCommand])

  useEffect(() => {
    refresh()
    const interval = setInterval(refresh, 3000)
    return () => clearInterval(interval)
  }, [refresh])

  // Image upload handlers
  const handleFileSelect = async (file: File) => {
    if (!file.type.startsWith('image/')) {
      setError('Please select an image file')
      return
    }

    const reader = new FileReader()
    reader.onload = async (e) => {
      const result = e.target?.result as string
      try {
        // Compress image to avoid HTTP 413
        const compressed = await compressImage(result, 500)
        setSelectedImage(compressed)
        setOcrResult(null)
        setError(null)
        setSuccess(null)
      } catch {
        setError('Failed to process image')
      }
    }
    reader.readAsDataURL(file)
  }

  const handleDragOver = (e: React.DragEvent) => {
    e.preventDefault()
    setIsDragging(true)
  }

  const handleDragLeave = () => {
    setIsDragging(false)
  }

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault()
    setIsDragging(false)

    const file = e.dataTransfer.files[0]
    if (file) {
      handleFileSelect(file)
    }
  }

  const handleInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (file) {
      handleFileSelect(file)
    }
  }

  // OCR recognition
  const handleRecognize = async () => {
    if (!selectedImage) return

    setRecognizing(true)
    setError(null)

    // Convert data URI to base64
    const base64Data = selectedImage.split(',')[1]

    console.log('[OCR Frontend] Sending recognize_image command, base64 length:', base64Data?.length)

    const result = await executeCommand('recognize_image', { image: base64Data })

    console.log('[OCR Frontend] Result:', result)
    console.log('[OCR Frontend] text_blocks:', result.data?.data?.text_blocks)
    console.log('[OCR Frontend] full_text:', result.data?.data?.full_text)
    console.log('[OCR Frontend] annotated_image_base64 length:', result.data?.data?.annotated_image_base64?.length)

    if (result.success && result.data?.data) {
      // The actual OCR data is nested in result.data.data
      const ocrData = {
        ...result.data.data,
        text_blocks: result.data.data.text_blocks || [],
        full_text: result.data.data.full_text || ''
      }
      setOcrResult(ocrData)
      setSuccess('OCR completed')
      setTimeout(() => setSuccess(null), 3000)
    } else {
      setError(result.error || 'Recognition failed')
    }

    setRecognizing(false)
  }

  // Bind device
  const handleBind = async () => {
    if (!formDevice) {
      setError('Please select a device')
      return
    }

    if (!formImageMetric) {
      setError('Please select an image metric')
      return
    }

    setLoading(true)
    setError(null)
    setSuccess(null)

    const device = devices.find(d => d.id === formDevice)

    const result = await executeCommand('bind_device', {
      device_id: formDevice,
      device_name: device?.name,
      image_metric: formImageMetric,
      result_metric_prefix: 'ocr_',
      draw_boxes: true,
      active: true
    })

    if (result.success) {
      setSuccess('Device bound')
      setFormDevice('')
      await refresh()
      setTimeout(() => setSuccess(null), 3000)
    } else {
      setError(result.error || 'Bind failed')
    }

    setLoading(false)
  }

  // Unbind device
  const handleUnbind = async (deviceId: string) => {
    setLoading(true)
    setError(null)

    const result = await executeCommand('unbind_device', { device_id: deviceId })

    if (result.success) {
      setSuccess('Device unbound')
      await refresh()
      setTimeout(() => setSuccess(null), 3000)
    } else {
      setError(result.error || 'Failed to unbind')
    }

    setLoading(false)
  }

  // Toggle binding
  const handleToggle = async (deviceId: string, active: boolean) => {
    const result = await executeCommand('toggle_binding', { device_id: deviceId, active: !active })
    if (result.success) {
      await refresh()
    }
  }

  // Render manual test tab
  const renderManualTest = () => (
    <div className="ocr-content">
      {error && (
        <div className="ocr-error">
          <Icon name="alert" style={{ width: '14px', height: '14px' }} />
          {error}
        </div>
      )}

      {success && (
        <div className="ocr-success">
          <Icon name="check" style={{ width: '14px', height: '14px' }} />
          {success}
        </div>
      )}

      {!selectedImage ? (
        <div
          className={`ocr-upload-area ${isDragging ? 'ocr-upload-area-dragging' : ''}`}
          onDragOver={handleDragOver}
          onDragLeave={handleDragLeave}
          onDrop={handleDrop}
          onClick={() => fileInputRef.current?.click()}
        >
          <div className="ocr-upload-placeholder">
            <Icon name="upload" className="ocr-upload-icon" />
            <div className="ocr-upload-text">Click or drag to upload image</div>
            <div className="ocr-upload-hint">Supports JPG, PNG formats</div>
          </div>
          <input
            ref={fileInputRef}
            type="file"
            accept="image/*"
            onChange={handleInputChange}
            style={{ display: 'none' }}
          />
        </div>
      ) : (
        <>
          <div className="ocr-preview-area">
            {/* Show annotated image or original image — fills entire area */}
            {ocrResult?.annotated_image_base64 ? (
              <div className="ocr-image-preview">
                <img src={`data:image/jpeg;base64,${ocrResult.annotated_image_base64}`} alt="OCR Result" style={{ width: '100%', height: '100%', objectFit: 'contain' }} />
              </div>
            ) : (
              <div className="ocr-image-preview">
                <img src={selectedImage} alt="Preview" style={{ width: '100%', height: '100%', objectFit: 'contain' }} />
              </div>
            )}

            {/* Floating results overlay */}
            {ocrResult && showResults && (
              <div className="ocr-results-overlay">
                <div className="ocr-results-overlay-header">
                  <span className="ocr-results-overlay-title">
                    Detected Text ({ocrResult.total_blocks} blocks, {(ocrResult.avg_confidence * 100).toFixed(0)}%)
                  </span>
                  <button className="ocr-copy-btn" onClick={() => setShowResults(false)} title="Hide results">
                    <Icon name="eyeOff" style={{ width: '12px', height: '12px' }} />
                  </button>
                </div>
                <div className="ocr-results-overlay-body">
                  {ocrResult.full_text || <span className="ocr-text-placeholder">No text detected</span>}
                </div>
              </div>
            )}

            {/* Floating action bar: Clear + OCR + Show/Hide Results */}
            <div className="ocr-actions-floating">
              <button className="ocr-btn ocr-btn-sm" onClick={() => { setSelectedImage(null); setOcrResult(null); setError(null); setShowResults(true); }}>
                Clear
              </button>
              <button
                className="ocr-btn ocr-btn-sm ocr-btn-primary"
                onClick={handleRecognize}
                disabled={recognizing}
              >
                {recognizing ? (
                  <>
                    <div className="ocr-spinner" />
                    OCR...
                  </>
                ) : (
                  <>
                    <Icon name="type" style={{ width: '12px', height: '12px' }} />
                    OCR
                  </>
                )}
              </button>
              {ocrResult && (
                <button
                  className={`ocr-btn ocr-btn-sm ${showResults ? '' : 'ocr-btn-accent'}`}
                  onClick={() => setShowResults(!showResults)}
                >
                  <Icon name={showResults ? 'eyeOff' : 'eye'} style={{ width: '12px', height: '12px' }} />
                  {showResults ? 'Hide' : 'Show'} ({ocrResult.total_blocks})
                </button>
              )}
            </div>
          </div>
        </>
      )}
    </div>
  )

  // Render device bindings tab
  const renderBindings = () => (
    <div className="ocr-content">
      {error && (
        <div className="ocr-error">
          <Icon name="alert" style={{ width: '14px', height: '14px' }} />
          {error}
        </div>
      )}

      {success && (
        <div className="ocr-success">
          <Icon name="check" style={{ width: '14px', height: '14px' }} />
          {success}
        </div>
      )}

      {/* Add binding form */}
      <div className="ocr-form">
        <div className="ocr-form-group">
          <label className="ocr-form-label">Device</label>
          <div className={`ocr-dropdown ${deviceDropdownOpen ? 'ocr-dropdown-open' : ''}`}>
            <div
              className="ocr-dropdown-trigger"
              onClick={(e) => { e.stopPropagation(); setDeviceDropdownOpen(!deviceDropdownOpen) }}
            >
              {formDevice ? (
                <span>{devices.find(d => d.id === formDevice)?.name || formDevice}</span>
              ) : (
                <span className="ocr-dropdown-trigger-placeholder">Select device...</span>
              )}
              <svg className="ocr-dropdown-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <polyline points="6 9 12 15 18 9" />
              </svg>
            </div>
            {deviceDropdownOpen && (
              <div className="ocr-dropdown-menu">
                {devices.length === 0 ? (
                  <div className="ocr-dropdown-item-empty">No devices available</div>
                ) : (
                  devices.map(d => (
                    <div
                      key={d.id}
                      className={`ocr-dropdown-item ${formDevice === d.id ? 'ocr-dropdown-item-selected' : ''}`}
                      onClick={(e) => { e.stopPropagation(); setFormDevice(d.id); setDeviceDropdownOpen(false) }}
                    >
                      <Icon name="link" style={{ width: '12px', height: '12px' }} />
                      {d.name || d.id}
                    </div>
                  ))
                )}
              </div>
            )}
          </div>
        </div>

        <div className="ocr-form-group">
          <label className="ocr-form-label">Image Metric</label>
          <div className={`ocr-dropdown ${metricDropdownOpen ? 'ocr-dropdown-open' : ''}`}>
            <div
              className="ocr-dropdown-trigger"
              onClick={(e) => { e.stopPropagation(); setMetricDropdownOpen(!metricDropdownOpen); setDeviceDropdownOpen(false) }}
            >
              {formImageMetric ? (
                <span>{availableMetrics.find(m => m.id === formImageMetric)?.display_name || formImageMetric}</span>
              ) : (
                <span className="ocr-dropdown-trigger-placeholder">{formDevice ? 'Select metric...' : 'Select device first'}</span>
              )}
              <svg className="ocr-dropdown-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <polyline points="6 9 12 15 18 9" />
              </svg>
            </div>
            {metricDropdownOpen && (
              <div className="ocr-dropdown-menu">
                {availableMetrics.length === 0 ? (
                  <div className="ocr-dropdown-item-empty">{formDevice ? 'No metrics found' : 'Select device first'}</div>
                ) : (
                  availableMetrics.map(m => (
                    <div
                      key={m.id}
                      className={`ocr-dropdown-item ${formImageMetric === m.id ? 'ocr-dropdown-item-selected' : ''}`}
                      onClick={(e) => { e.stopPropagation(); setFormImageMetric(m.id); setMetricDropdownOpen(false) }}
                    >
                      <Icon name="image" style={{ width: '12px', height: '12px' }} />
                      {m.display_name || m.name || m.id}
                    </div>
                  ))
                )}
              </div>
            )}
          </div>
        </div>

        <button
          className="ocr-btn ocr-btn-primary"
          onClick={handleBind}
          disabled={loading || !formDevice || !formImageMetric}
        >
          {loading ? (
            <>
              <div className="ocr-spinner" />
              Binding...
            </>
          ) : (
            <>
              <Icon name="link" style={{ width: '14px', height: '14px' }} />
              Bind Device
            </>
          )}
        </button>
      </div>

      {/* Bindings list */}
      <div className="ocr-bindings-list">
        {bindings.length === 0 ? (
          <div className="ocr-empty-state">
            <Icon name="link" className="ocr-empty-icon" />
            <div>No device bindings</div>
          </div>
        ) : (
          bindings.map((bindingStatus, idx) => (
            <div key={idx} className="ocr-binding-item">
              <div className="ocr-binding-header">
                <span className="ocr-binding-name">
                  {bindingStatus.binding.device_name || bindingStatus.binding.device_id}
                </span>
                <span className={`ocr-binding-status ${bindingStatus.binding.active ? 'ocr-binding-status-active' : 'ocr-binding-status-paused'}`}>
                  {bindingStatus.binding.active ? 'Running' : 'Paused'}
                </span>
              </div>

              <div className="ocr-binding-info">
                <div className="ocr-binding-stat">
                  Inferences: {bindingStatus.total_inferences}
                </div>
                <div className="ocr-binding-stat">
                  Text blocks: {bindingStatus.total_text_blocks}
                </div>
                {bindingStatus.last_inference && (
                  <div className="ocr-binding-stat">
                    Last: {new Date(bindingStatus.last_inference).toLocaleTimeString()}
                  </div>
                )}
              </div>

              {bindingStatus.last_error && (
                <div style={{ fontSize: '10px', color: 'var(--ocr-danger)', marginBottom: '6px' }}>
                  Error: {bindingStatus.last_error}
                </div>
              )}

              {/* Preview image with floating text overlay */}
              {(bindingStatus.last_annotated_image || bindingStatus.last_image) && (
                <div className="ocr-binding-preview">
                  <img src={bindingStatus.last_annotated_image || bindingStatus.last_image} alt="OCR Result" />
                  {bindingStatus.last_full_text && (
                    <div className="ocr-binding-text-container">
                      <div className="ocr-binding-text">
                        {bindingStatus.last_full_text}
                      </div>
                    </div>
                  )}
                </div>
              )}

              <div className="ocr-binding-actions">
                <button
                  className="ocr-btn ocr-btn-sm"
                  onClick={() => setRoiEditorDeviceId(bindingStatus.binding.device_id)}
                  disabled={loading}
                  title="Edit ROI regions"
                >
                  <Icon name="target" style={{ width: '12px', height: '12px' }} />
                  ROI{bindingStatus.binding.roi_regions && bindingStatus.binding.roi_regions.length > 0 ? ` (${bindingStatus.binding.roi_regions.length})` : ''}
                </button>
                <button
                  className="ocr-btn ocr-btn-sm"
                  onClick={() => handleToggle(bindingStatus.binding.device_id, bindingStatus.binding.active)}
                  disabled={loading}
                >
                  <Icon name={bindingStatus.binding.active ? 'pause' : 'play'} style={{ width: '12px', height: '12px' }} />
                  {bindingStatus.binding.active ? 'Pause' : 'Resume'}
                </button>
                <button
                  className="ocr-btn ocr-btn-sm ocr-btn-danger"
                  onClick={() => handleUnbind(bindingStatus.binding.device_id)}
                  disabled={loading}
                >
                  <Icon name="trash" style={{ width: '12px', height: '12px' }} />
                  Unbind
                </button>
              </div>

              {/* ROI Editor for this binding */}
              {roiEditorDeviceId === bindingStatus.binding.device_id && (
                <RoiEditor
                  binding={bindingStatus.binding}
                  imageUrl={bindingStatus.last_annotated_image || bindingStatus.last_image || undefined}
                  onSave={async (regions, threshold) => {
                    await executeCommand('update_roi', {
                      device_id: bindingStatus.binding.device_id,
                      roi_regions: regions,
                      roi_overlap_threshold: threshold,
                    })
                    setRoiEditorDeviceId(null)
                    refresh()
                  }}
                  onCancel={() => setRoiEditorDeviceId(null)}
                />
              )}
            </div>
          ))
        )}
      </div>
    </div>
  )

  return (
    <div className="ocr">
      <div className="ocr-card">
        {/* Header */}
        <div className="ocr-header">
          <div className="ocr-title">
            <Icon name="type" style={{ width: '16px', height: '16px' }} />
            <span>OCR Device Inference</span>
          </div>
          <div className={`ocr-badge ${status?.model_loaded ? 'ocr-badge-active' : ''}`}>
            {status?.model_loaded ? 'Loaded' : 'Not Loaded'}
          </div>
        </div>

        {/* Tabs */}
        <div className="ocr-tabs">
          <button
            className={`ocr-tab ${activeTab === 'manual' ? 'ocr-tab-active' : ''}`}
            onClick={() => setActiveTab('manual')}
          >
            <Icon name="image" style={{ width: '14px', height: '14px' }} />
            Test
          </button>
          <button
            className={`ocr-tab ${activeTab === 'bindings' ? 'ocr-tab-active' : ''}`}
            onClick={() => setActiveTab('bindings')}
          >
            <Icon name="link" style={{ width: '14px', height: '14px' }} />
            Bindings
          </button>
        </div>

        {/* Content */}
        {activeTab === 'manual' ? renderManualTest() : renderBindings()}
      </div>
    </div>
  )
}

export default { OcrDeviceCard }
