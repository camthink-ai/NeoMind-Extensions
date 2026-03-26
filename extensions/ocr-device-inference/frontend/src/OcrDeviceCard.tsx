/**
 * OCR Device Inference Extension Card
 *
 * Provides dual-tab interface for manual OCR testing and device binding management.
 * Matches the styling pattern of yolo-device-inference.
 */

import React, { useState, useEffect, useRef, useCallback, useMemo } from 'react'

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

export interface DeviceBinding {
  device_id: string
  device_name?: string
  image_metric: string
  result_metric_prefix: string
  draw_boxes: boolean
  active: boolean
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

const CSS_ID = 'ocr-styles-v1'

const STYLES = `
.ocr {
  --ocr-fg: hsl(240 10% 10%);
  --ocr-muted: hsl(240 5% 45%);
  --ocr-accent: hsl(142 70% 55%);
  --ocr-card: rgba(255,255,255,0.5);
  --ocr-border: rgba(0,0,0,0.06);
  --ocr-hover: rgba(0,0,0,0.03);
  --ocr-danger: hsl(0 72% 51%);
  --ocr-success: hsl(142 70% 45%);
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
  border: 2px dashed var(--ocr-border);
  border-radius: 8px;
  padding: 20px;
  text-align: center;
  cursor: pointer;
  transition: all 0.2s;
  background: rgba(0,0,0,0.02);
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

/* Preview area */
.ocr-preview-area {
  display: flex;
  gap: 10px;
  min-height: 120px;
}

.ocr-image-preview {
  flex: 1;
  border-radius: 6px;
  overflow: hidden;
  background: rgba(0,0,0,0.05);
  display: flex;
  align-items: center;
  justify-content: center;
}

.ocr-image-preview img {
  max-width: 100%;
  max-height: 120px;
  object-fit: contain;
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
  border-radius: 3px;
  font-size: 9px;
  cursor: pointer;
  background: transparent;
  color: var(--ocr-fg);
  transition: all 0.15s;
}

.ocr-copy-btn:hover {
  background: var(--ocr-hover);
  border-color: var(--ocr-accent);
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
  padding: 8px 16px;
  border: 1px solid var(--ocr-border);
  border-radius: 6px;
  font-size: 11px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.2s;
  background: transparent;
  color: var(--ocr-fg);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
}

.ocr-btn:hover {
  background: var(--ocr-hover);
}

.ocr-btn-primary {
  background: var(--ocr-accent);
  border-color: var(--ocr-accent);
  color: #000;
}

.ocr-btn-primary:hover {
  opacity: 0.9;
  background: var(--ocr-accent);
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
  padding: 4px 8px;
  font-size: 10px;
}

.ocr-actions {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
  margin-top: auto;
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
    return data.data?.devices || data.devices || data.data || []
  } catch {
    return []
  }
}

async function fetchDeviceMetrics(deviceId: string): Promise<Metric[]> {
  try {
    const res = await fetch(`${getApiBase()}/devices/${deviceId}/current`, { headers: getApiHeaders() })
    if (!res.ok) return []
    const data = await res.json()
    const metrics = data.data?.metrics || data.metrics || {}
    return Object.entries(metrics).map(([id, m]: [string, any]) => ({
      id,
      name: m.name || id,
      display_name: m.display_name || m.name || id,
      type: m.data_type || 'string',
      data_type: m.data_type || 'string'
    }))
  } catch {
    return []
  }
}

// ============================================================================
// Main Component
// ============================================================================

export const OcrDeviceCard: React.FC<OcrDeviceCardProps> = ({
  executeCommand = executeCommandApi,
  config = {}
}) => {
  useEffect(() => injectStyles(), [])

  // Tab state
  const [activeTab, setActiveTab] = useState<'manual' | 'bindings'>('manual')

  // Manual test state
  const [selectedImage, setSelectedImage] = useState<string | null>(null)
  const [isDragging, setIsDragging] = useState(false)
  const [ocrResult, setOcrResult] = useState<OcrResult | null>(null)
  const [recognizing, setRecognizing] = useState(false)
  const [copySuccess, setCopySuccess] = useState(false)

  // Device bindings state
  const [devices, setDevices] = useState<Device[]>([])
  const [bindings, setBindings] = useState<BindingStatus[]>([])
  const [status, setStatus] = useState<ExtensionStatus | null>(null)

  // Form state
  const [formDevice, setFormDevice] = useState('')
  const [formImageMetric, setFormImageMetric] = useState('image')
  const [formResultPrefix, setFormResultPrefix] = useState('ocr_')
  const [formDrawBoxes, setFormDrawBoxes] = useState(true)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [success, setSuccess] = useState<string | null>(null)

  const fileInputRef = useRef<HTMLInputElement>(null)

  // Load devices
  useEffect(() => {
    const loadDevices = async () => {
      const deviceList = await fetchDevices()
      setDevices(Array.isArray(deviceList) ? deviceList : [])
    }
    loadDevices()
  }, [])

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
  const handleFileSelect = (file: File) => {
    if (!file.type.startsWith('image/')) {
      setError('Please select an image file')
      return
    }

    const reader = new FileReader()
    reader.onload = (e) => {
      const result = e.target?.result as string
      setSelectedImage(result)
      setOcrResult(null)
      setError(null)
      setSuccess(null)
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

    const result = await executeCommand('recognize_image', { image: base64Data })

    if (result.success && result.data) {
      setOcrResult(result.data)
      setSuccess('Recognition completed successfully')
      setTimeout(() => setSuccess(null), 3000)
    } else {
      setError(result.error || 'Recognition failed')
    }

    setRecognizing(false)
  }

  // Copy text to clipboard
  const handleCopyText = async () => {
    if (!ocrResult?.full_text) return

    try {
      await navigator.clipboard.writeText(ocrResult.full_text)
      setCopySuccess(true)
      setTimeout(() => setCopySuccess(false), 2000)
    } catch (err) {
      setError('Failed to copy text')
    }
  }

  // Bind device
  const handleBind = async () => {
    if (!formDevice) {
      setError('Please select a device')
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
      result_metric_prefix: formResultPrefix,
      draw_boxes: formDrawBoxes,
      active: true
    })

    if (result.success) {
      setSuccess('Device bound successfully')
      setFormDevice('')
      await refresh()
      setTimeout(() => setSuccess(null), 3000)
    } else {
      setError(result.error || 'Failed to bind device')
    }

    setLoading(false)
  }

  // Unbind device
  const handleUnbind = async (deviceId: string) => {
    setLoading(true)
    setError(null)

    const result = await executeCommand('unbind_device', { device_id: deviceId })

    if (result.success) {
      setSuccess('Device unbound successfully')
      await refresh()
      setTimeout(() => setSuccess(null), 3000)
    } else {
      setError(result.error || 'Failed to unbind device')
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
            <div className="ocr-upload-text">点击或拖拽上传图片</div>
            <div className="ocr-upload-hint">支持 JPG、PNG 格式</div>
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
            <div className="ocr-image-preview">
              <img src={selectedImage} alt="Preview" />
            </div>

            {ocrResult && (
              <div className="ocr-text-results">
                <div className="ocr-text-header">
                  <span className="ocr-text-label">识别结果</span>
                  <button className="ocr-copy-btn" onClick={handleCopyText}>
                    {copySuccess ? (
                      <>
                        <Icon name="check" style={{ width: '10px', height: '10px' }} />
                        已复制
                      </>
                    ) : (
                      <>
                        <Icon name="copy" style={{ width: '10px', height: '10px' }} />
                        复制
                      </>
                    )}
                  </button>
                </div>
                <div className="ocr-text-content">
                  {ocrResult.full_text || <span className="ocr-text-placeholder">未识别到文字</span>}
                </div>
                <div className="ocr-text-blocks">
                  {ocrResult.text_blocks.map((block, idx) => (
                    <div key={idx} className="ocr-text-block">
                      <span className="ocr-text-block-text">{block.text}</span>
                      <span className={`ocr-text-block-conf ${block.confidence < 0.8 ? 'ocr-text-block-conf-low' : ''}`}>
                        {(block.confidence * 100).toFixed(0)}%
                      </span>
                    </div>
                  ))}
                </div>
              </div>
            )}
          </div>

          <div className="ocr-actions">
            <button className="ocr-btn" onClick={() => { setSelectedImage(null); setOcrResult(null); setError(null); }}>
              清除
            </button>
            <button
              className="ocr-btn ocr-btn-primary"
              onClick={handleRecognize}
              disabled={recognizing}
            >
              {recognizing ? (
                <>
                  <div className="ocr-spinner" />
                  识别中...
                </>
              ) : (
                <>
                  <Icon name="type" style={{ width: '14px', height: '14px' }} />
                  开始识别
                </>
              )}
            </button>
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
          <label className="ocr-form-label">设备</label>
          <select
            className="ocr-form-select"
            value={formDevice}
            onChange={(e) => setFormDevice(e.target.value)}
          >
            <option value="">选择设备...</option>
            {devices.map(d => (
              <option key={d.id} value={d.id}>{d.name || d.id}</option>
            ))}
          </select>
        </div>

        <div className="ocr-form-group">
          <label className="ocr-form-label">图像指标</label>
          <input
            className="ocr-form-input"
            type="text"
            value={formImageMetric}
            onChange={(e) => setFormImageMetric(e.target.value)}
            placeholder="image"
          />
        </div>

        <div className="ocr-form-group">
          <label className="ocr-form-label">结果前缀</label>
          <input
            className="ocr-form-input"
            type="text"
            value={formResultPrefix}
            onChange={(e) => setFormResultPrefix(e.target.value)}
            placeholder="ocr_"
          />
        </div>

        <label className="ocr-form-checkbox">
          <input
            type="checkbox"
            checked={formDrawBoxes}
            onChange={(e) => setFormDrawBoxes(e.target.checked)}
          />
          绘制文字框
        </label>

        <button
          className="ocr-btn ocr-btn-primary"
          onClick={handleBind}
          disabled={loading || !formDevice}
        >
          {loading ? (
            <>
              <div className="ocr-spinner" />
              绑定中...
            </>
          ) : (
            <>
              <Icon name="link" style={{ width: '14px', height: '14px' }} />
              绑定设备
            </>
          )}
        </button>
      </div>

      {/* Bindings list */}
      <div className="ocr-bindings-list">
        {bindings.length === 0 ? (
          <div className="ocr-empty-state">
            <Icon name="link" className="ocr-empty-icon" />
            <div>暂无设备绑定</div>
          </div>
        ) : (
          bindings.map((bindingStatus, idx) => (
            <div key={idx} className="ocr-binding-item">
              <div className="ocr-binding-header">
                <span className="ocr-binding-name">
                  {bindingStatus.binding.device_name || bindingStatus.binding.device_id}
                </span>
                <span className={`ocr-binding-status ${bindingStatus.binding.active ? 'ocr-binding-status-active' : 'ocr-binding-status-paused'}`}>
                  {bindingStatus.binding.active ? '运行中' : '已暂停'}
                </span>
              </div>

              <div className="ocr-binding-info">
                <div className="ocr-binding-stat">
                  推断次数: {bindingStatus.total_inferences}
                </div>
                <div className="ocr-binding-stat">
                  文字块: {bindingStatus.total_text_blocks}
                </div>
                {bindingStatus.last_inference && (
                  <div className="ocr-binding-stat">
                    最后: {new Date(bindingStatus.last_inference).toLocaleTimeString()}
                  </div>
                )}
              </div>

              {bindingStatus.last_error && (
                <div style={{ fontSize: '10px', color: 'var(--ocr-danger)', marginBottom: '6px' }}>
                  错误: {bindingStatus.last_error}
                </div>
              )}

              <div className="ocr-binding-actions">
                <button
                  className="ocr-btn ocr-btn-sm"
                  onClick={() => handleToggle(bindingStatus.binding.device_id, bindingStatus.binding.active)}
                  disabled={loading}
                >
                  <Icon name={bindingStatus.binding.active ? 'pause' : 'play'} style={{ width: '12px', height: '12px' }} />
                  {bindingStatus.binding.active ? '暂停' : '恢复'}
                </button>
                <button
                  className="ocr-btn ocr-btn-sm ocr-btn-danger"
                  onClick={() => handleUnbind(bindingStatus.binding.device_id)}
                  disabled={loading}
                >
                  <Icon name="trash" style={{ width: '12px', height: '12px' }} />
                  解绑
                </button>
              </div>
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
            <span>OCR 设备推断</span>
          </div>
          <div className={`ocr-badge ${status?.model_loaded ? 'ocr-badge-active' : ''}`}>
            {status?.model_loaded ? '已加载' : '未加载'}
          </div>
        </div>

        {/* Tabs */}
        <div className="ocr-tabs">
          <button
            className={`ocr-tab ${activeTab === 'manual' ? 'ocr-tab-active' : ''}`}
            onClick={() => setActiveTab('manual')}
          >
            <Icon name="image" style={{ width: '14px', height: '14px' }} />
            手动测试
          </button>
          <button
            className={`ocr-tab ${activeTab === 'bindings' ? 'ocr-tab-active' : ''}`}
            onClick={() => setActiveTab('bindings')}
          >
            <Icon name="link" style={{ width: '14px', height: '14px' }} />
            设备绑定
          </button>
        </div>

        {/* Content */}
        {activeTab === 'manual' ? renderManualTest() : renderBindings()}
      </div>
    </div>
  )
}

export default { OcrDeviceCard }
