/**
 * YOLO Device Inference Extension
 * Image-focused design matching Image Analyzer V2 style
 */

import { forwardRef, useEffect, useState, useRef, useCallback, useMemo } from 'react'

// ============================================================================
// Types
// ============================================================================

export interface ExtensionComponentProps {
  title?: string
  dataSource?: DataSource
  className?: string
  config?: Record<string, any>
  getDevices?: () => Promise<Device[]>
  getDeviceMetrics?: (deviceId: string) => Promise<Metric[]>
  onDataSourceChange?: (dataSource: DataSource) => void
  onConfigChange?: (config: Record<string, any>) => void
}

export interface DataSource {
  type: string
  extensionId?: string
  deviceId?: string
  metricId?: string
  deviceName?: string
  [key: string]: any
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

interface Detection {
  label: string
  confidence: number
  bbox: { x: number; y: number; width: number; height: number } | null
  class_id?: number
}

interface BindingStatus {
  binding: {
    device_id: string
    device_name?: string
    image_metric: string
    result_metric_prefix: string
    confidence_threshold: number
    draw_boxes: boolean
    active: boolean
  }
  last_inference: number | null
  total_inferences: number
  total_detections: number
  last_error: string | null
  last_image?: string
  last_detections?: Detection[]
  last_annotated_image?: string
}

interface ExtensionStatus {
  model_loaded: boolean
  model_version: string
  total_bindings: number
  total_inferences: number
  total_detections: number
  total_errors: number
}

// ============================================================================
// API
// ============================================================================

const EXTENSION_ID = 'yolo-device-inference'

const getApiHeaders = () => {
  const token = localStorage.getItem('neomind_token') || sessionStorage.getItem('neomind_token_session')
  const headers: Record<string, string> = { 'Content-Type': 'application/json' }
  if (token) headers['Authorization'] = `Bearer ${token}`
  return headers
}

const getApiBase = () => (window as any).__TAURI__ ? 'http://localhost:9375/api' : '/api'

async function executeCommand(
  extensionId: string,
  command: string,
  args: Record<string, unknown> = {}
): Promise<{ success: boolean; data?: any; error?: string }> {
  try {
    const res = await fetch(`${getApiBase()}/extensions/${extensionId}/command`, {
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

async function getStatus(extensionId: string): Promise<ExtensionStatus | null> {
  const result = await executeCommand(extensionId, 'get_status', {})
  return result.success && result.data ? result.data : null
}

async function getBindings(extensionId: string): Promise<BindingStatus[]> {
  const result = await executeCommand(extensionId, 'get_bindings', {})
  return result.success && result.data?.bindings ? result.data.bindings : []
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
// Styles
// ============================================================================

const CSS_ID = 'ydi-styles-v3'

const STYLES = `
.ydi {
  --ydi-fg: hsl(240 10% 10%);
  --ydi-muted: hsl(240 5% 45%);
  --ydi-accent: hsl(142 70% 55%);
  --ydi-card: rgba(255,255,255,0.5);
  --ydi-border: rgba(0,0,0,0.06);
  --ydi-hover: rgba(0,0,0,0.03);
  width: 100%;
  height: 100%;
  font-size: 12px;
}
.dark .ydi {
  --ydi-fg: hsl(0 0% 95%);
  --ydi-muted: hsl(0 0% 60%);
  --ydi-card: rgba(30,30,30,0.5);
  --ydi-border: rgba(255,255,255,0.08);
  --ydi-hover: rgba(255,255,255,0.03);
}

.ydi-card {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: 10px;
  background: var(--ydi-card);
  backdrop-filter: blur(12px);
  border: 1px solid var(--ydi-border);
  border-radius: 8px;
  box-sizing: border-box;
}

.ydi-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-shrink: 0;
  margin-bottom: 8px;
}

.ydi-title {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--ydi-fg);
  font-size: 13px;
  font-weight: 600;
}

.ydi-badge {
  padding: 2px 6px;
  background: rgba(142, 70, 65, 0.1);
  color: var(--ydi-accent);
  border-radius: 4px;
  font-size: 9px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.3px;
}
.ydi-badge-active { background: hsl(142 70% 90%); color: hsl(142 70% 30%); }
.dark .ydi-badge-active { background: hsl(142 70% 20%); color: hsl(142 70% 70%); }

.ydi-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 0;
  overflow: hidden;
}

/* Image preview - main focus */
.ydi-preview-wrapper {
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
.dark .ydi-preview-wrapper {
  background: rgba(0,0,0,0.2);
}

.ydi-preview {
  position: relative;
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
}

.ydi-canvas {
  max-width: 100%;
  max-height: 100%;
  width: auto;
  height: auto;
  object-fit: contain;
}

.ydi-placeholder {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  color: var(--ydi-muted);
  font-size: 11px;
  padding: 20px;
  text-align: center;
}

.ydi-placeholder-icon {
  width: 40px;
  height: 40px;
  opacity: 0.4;
}

/* Results overlay - floating on top of image */
.ydi-overlay {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 8px;
  background: linear-gradient(to top, rgba(0,0,0,0.75) 0%, rgba(0,0,0,0.5) 70%, transparent 100%);
  border-radius: 0 0 6px 6px;
  max-height: 50%;
  overflow-y: auto;
}
.ydi-overlay::-webkit-scrollbar { width: 4px; }
.ydi-overlay::-webkit-scrollbar-track { background: transparent; }
.ydi-overlay::-webkit-scrollbar-thumb { background: rgba(255,255,255,0.3); border-radius: 2px; }

/* Stats in overlay */
.ydi-stats-overlay {
  display: flex;
  gap: 12px;
}

.ydi-stat-overlay {
  display: flex;
  align-items: baseline;
  gap: 4px;
}

.ydi-stat-label-overlay {
  font-size: 10px;
  color: rgba(255,255,255,0.7);
  text-transform: uppercase;
  letter-spacing: 0.3px;
}

.ydi-stat-value-overlay {
  font-size: 14px;
  font-weight: 700;
  color: #fff;
}

/* Object tags in overlay */
.ydi-objects-overlay {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}

.ydi-object-tag-overlay {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 2px 6px;
  background: rgba(255,255,255,0.15);
  backdrop-filter: blur(4px);
  border-radius: 4px;
  font-size: 10px;
  color: #fff;
  border: 1px solid rgba(255,255,255,0.2);
}

/* Error in overlay */
.ydi-error-overlay {
  padding: 6px 8px;
  background: rgba(239, 68, 68, 0.3);
  backdrop-filter: blur(4px);
  border-radius: 4px;
  color: #fff;
  font-size: 10px;
}

/* Styled dropdown */
.ydi-dropdown-container {
  position: relative;
}

.ydi-dropdown-trigger {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 4px;
  width: 100%;
  padding: 6px 10px;
  border: 1px solid var(--ydi-border);
  border-radius: 6px;
  background: var(--ydi-card);
  color: var(--ydi-fg);
  font-size: 11px;
  cursor: pointer;
  transition: all 0.15s;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ydi-dropdown-trigger:hover {
  background: var(--ydi-hover);
  border-color: var(--ydi-accent);
}
.ydi-dropdown-trigger:focus {
  outline: none;
  border-color: var(--ydi-accent);
  box-shadow: 0 0 0 2px rgba(142, 70, 65, 0.1);
}
.ydi-dropdown-trigger-placeholder {
  color: var(--ydi-muted);
}

.ydi-dropdown-menu {
  position: absolute;
  top: 100%;
  left: 0;
  right: 0;
  margin-top: 4px;
  background: var(--ydi-card);
  border: 1px solid var(--ydi-border);
  border-radius: 6px;
  box-shadow: 0 4px 12px rgba(0,0,0,0.15);
  z-index: 100;
  max-height: 150px;
  overflow-y: auto;
  backdrop-filter: blur(12px);
}
.ydi-dropdown-menu::-webkit-scrollbar { width: 4px; }
.ydi-dropdown-menu::-webkit-scrollbar-thumb { background: var(--ydi-border); border-radius: 2px; }

.ydi-dropdown-item {
  padding: 6px 10px;
  font-size: 11px;
  color: var(--ydi-fg);
  cursor: pointer;
  transition: background 0.1s;
}
.ydi-dropdown-item:hover {
  background: var(--ydi-hover);
}
.ydi-dropdown-item-selected {
  background: rgba(142, 70, 65, 0.1);
  color: var(--ydi-accent);
  font-weight: 500;
}

.ydi-dropdown-empty {
  padding: 8px 10px;
  font-size: 10px;
  color: var(--ydi-muted);
  text-align: center;
}

/* Control bar - compact selectors */
.ydi-control-bar {
  display: flex;
  gap: 6px;
  flex-shrink: 0;
}

.ydi-selector {
  flex: 1;
  min-width: 0;
}

.ydi-selector-label {
  font-size: 9px;
  color: var(--ydi-muted);
  text-transform: uppercase;
  letter-spacing: 0.3px;
  margin-bottom: 2px;
}

/* Actions */
.ydi-actions {
  display: flex;
  gap: 6px;
  flex-shrink: 0;
  margin-top: auto;
  padding-top: 4px;
  border-top: 1px solid var(--ydi-border);
}

.ydi-btn {
  flex: 1;
  padding: 6px 12px;
  border: 1px solid var(--ydi-border);
  border-radius: 6px;
  font-size: 11px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.2s;
  background: transparent;
  color: var(--ydi-fg);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
}
.ydi-btn:hover {
  background: var(--ydi-hover);
}
.ydi-btn-primary {
  background: var(--ydi-accent);
  border-color: var(--ydi-accent);
  color: #000;
}
.ydi-btn-primary:hover {
  opacity: 0.9;
  background: var(--ydi-accent);
}
.ydi-btn-danger {
  color: hsl(0 72% 51%);
  border-color: hsl(0 72% 51% 0.3);
}
.ydi-btn-danger:hover {
  background: hsl(0 72% 51% 0.1);
}
.ydi-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.ydi-icon { width: 14px; height: 14px; flex-shrink: 0; }
.ydi-icon-sm { width: 12px; height: 12px; }

.ydi-spinner {
  width: 20px;
  height: 20px;
  border: 2px solid var(--ydi-border);
  border-top-color: var(--ydi-accent);
  border-radius: 50%;
  animation: ydi-spin 0.7s linear infinite;
}
@keyframes ydi-spin {
  to { transform: rotate(360deg); }
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
  camera: '<path d="M23 19a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4l2-3h6l2 3h4a2 2 0 0 1 2 2z"/><circle cx="12" cy="13" r="4"/>',
  box: '<path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z"/>',
  play: '<polygon points="5 3 19 12 5 21 5 3"/>',
  pause: '<rect x="6" y="4" width="4" height="16"/><rect x="14" y="4" width="4" height="16"/>',
  trash: '<polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>',
  link: '<path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/>',
  image: '<rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/>',
  chevron: '<polyline points="6 9 12 15 18 9"/>',
  x: '<line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>',
}

const Icon = ({ name, className = '', style }: { name: string; className?: string; style?: React.CSSProperties }) => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={className} style={style}
    dangerouslySetInnerHTML={{ __html: ICONS[name] || ICONS.camera }} />
)

// ============================================================================
// Styled Dropdown Component
// ============================================================================

interface DropdownProps {
  value: string
  options: { value: string; label: string }[]
  placeholder?: string
  onChange: (value: string) => void
  disabled?: boolean
}

const Dropdown = ({ value, options, placeholder, onChange, disabled }: DropdownProps) => {
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)

  const selected = options.find(o => o.value === value)

  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false)
      }
    }
    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [])

  return (
    <div className="ydi-dropdown-container" ref={ref}>
      <button
        className="ydi-dropdown-trigger"
        onClick={() => !disabled && setOpen(!open)}
        disabled={disabled}
      >
        <span className={!selected ? 'ydi-dropdown-trigger-placeholder' : ''}>
          {selected?.label || placeholder || 'Select...'}
        </span>
        <Icon name="chevron" style={{ width: '12px', height: '12px', opacity: 0.5 }} />
      </button>
      {open && (
        <div className="ydi-dropdown-menu">
          {options.length === 0 ? (
            <div className="ydi-dropdown-empty">No options</div>
          ) : (
            options.map(opt => (
              <div
                key={opt.value}
                className={`ydi-dropdown-item ${opt.value === value ? 'ydi-dropdown-item-selected' : ''}`}
                onClick={() => {
                  onChange(opt.value)
                  setOpen(false)
                }}
              >
                {opt.label}
              </div>
            ))
          )}
        </div>
      )}
    </div>
  )
}

// ============================================================================
// Color Helper
// ============================================================================

const COLORS = ['#ef4444', '#22c55e', '#3b82f6', '#f97316', '#a855f7', '#06b6d4', '#ec4899', '#eab308']
const getColor = (index: number) => COLORS[index % COLORS.length]

// ============================================================================
// Draw Detections on Canvas
// ============================================================================

function drawDetections(
  canvas: HTMLCanvasElement,
  imageBase64: string,
  detections: Detection[]
): Promise<void> {
  return new Promise((resolve) => {
    const ctx = canvas.getContext('2d')
    if (!ctx) { resolve(); return }

    const img = new Image()
    img.onload = () => {
      const parent = canvas.parentElement
      const maxW = parent?.clientWidth || 400
      const maxH = parent?.clientHeight || 300

      const scale = Math.min(maxW / img.width, maxH / img.height)
      const canvasW = img.width * scale
      const canvasH = img.height * scale

      canvas.width = canvasW
      canvas.height = canvasH

      const offsetX = (canvasW - img.width * scale) / 2
      const offsetY = (canvasH - img.height * scale) / 2

      ctx.fillStyle = 'transparent'
      ctx.fillRect(0, 0, canvasW, canvasH)
      ctx.drawImage(img, offsetX, offsetY, img.width * scale, img.height * scale)

      // Draw detections
      detections.forEach((det, i) => {
        if (!det.bbox) return
        const x = det.bbox.x * scale + offsetX
        const y = det.bbox.y * scale + offsetY
        const w = det.bbox.width * scale
        const h = det.bbox.height * scale
        const color = getColor(det.class_id ?? i)

        ctx.strokeStyle = color
        ctx.lineWidth = 2
        ctx.strokeRect(x, y, w, h)

        const label = `${det.label} ${(det.confidence * 100).toFixed(0)}%`
        ctx.font = 'bold 11px sans-serif'
        const textW = ctx.measureText(label).width
        const textH = 16

        ctx.fillStyle = color
        ctx.fillRect(x, y >= textH ? y - textH : y, textW + 8, textH)
        ctx.fillStyle = '#fff'
        ctx.fillText(label, x + 4, (y >= textH ? y - textH : y) + 12)
      })

      resolve()
    }
    img.onerror = () => resolve()
    img.src = imageBase64.startsWith("data:") ? imageBase64 : `data:image/jpeg;base64,${imageBase64}`
  })
}

// ============================================================================
// Main Component
// ============================================================================

export const DeviceInferenceCard = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function DeviceInferenceCard(props, ref) {
    const {
      title,
      dataSource,
      config = {},
      className = '',
      getDevices,
      getDeviceMetrics,
      onDataSourceChange,
      onConfigChange: _onConfigChange
    } = props

    useEffect(() => injectStyles(), [])

    const extensionId = dataSource?.extensionId || EXTENSION_ID

    // Device selection state
    const [devices, setDevices] = useState<Device[]>([])
    const [selectedDevice, setSelectedDevice] = useState<string>(dataSource?.deviceId || '')
    const [metrics, setMetrics] = useState<Metric[]>([])
    const [selectedMetric, setSelectedMetric] = useState<string>(dataSource?.metricId || '')

    // Binding state
    const [status, setStatus] = useState<ExtensionStatus | null>(null)
    const [binding, setBinding] = useState<BindingStatus | null>(null)
    const [loading, setLoading] = useState(false)
    const [error, setError] = useState<string | null>(null)

    const canvasRef = useRef<HTMLCanvasElement>(null)

    // Load devices
    useEffect(() => {
      const loadDevices = async () => {
        const deviceList = getDevices ? await getDevices() : await fetchDevices()
        setDevices(Array.isArray(deviceList) ? deviceList : [])
      }
      loadDevices()
    }, [getDevices])

    // Load metrics when device changes
    useEffect(() => {
      const loadMetrics = async () => {
        if (!selectedDevice) {
          setMetrics([])
          return
        }
        const metricList = getDeviceMetrics ? await getDeviceMetrics(selectedDevice) : await fetchDeviceMetrics(selectedDevice)
        setMetrics(Array.isArray(metricList) ? metricList : [])

        // Auto-select image metric
        if (metricList.length > 0 && !selectedMetric) {
          const imageMetrics = metricList.filter(m =>
            m.type === 'image' || m.name.toLowerCase().includes('image') || m.id.toLowerCase().includes('image')
          )
          if (imageMetrics.length > 0) {
            setSelectedMetric(imageMetrics[0].id)
          }
        }
      }
      loadMetrics()
    }, [selectedDevice, getDeviceMetrics, selectedMetric])

    // Fetch status and bindings
    const refresh = useCallback(async () => {
      const [s, b] = await Promise.all([
        getStatus(extensionId),
        getBindings(extensionId)
      ])
      setStatus(s)

      const found = b.find(x => x.binding.device_id === selectedDevice)
      setBinding(found || null)
    }, [extensionId, selectedDevice])

    useEffect(() => {
      refresh()
      const interval = setInterval(refresh, 3000)
      return () => clearInterval(interval)
    }, [refresh])

    // Draw detections when annotated image is available
    useEffect(() => {
      if (binding?.last_annotated_image && binding?.last_detections && canvasRef.current) {
        drawDetections(canvasRef.current, binding.last_annotated_image, binding.last_detections)
      }
    }, [binding?.last_annotated_image, binding?.last_detections])

    // Draw from raw image if no annotated version
    useEffect(() => {
      if (binding?.last_image && !binding?.last_annotated_image && binding?.last_detections && canvasRef.current) {
        drawDetections(canvasRef.current, binding.last_image, binding.last_detections)
      }
    }, [binding?.last_image, binding?.last_annotated_image, binding?.last_detections])

    // Bind device
    const handleBind = async () => {
      if (!selectedDevice) return
      setLoading(true)
      setError(null)

      const result = await executeCommand(extensionId, 'bind_device', {
        device_id: selectedDevice,
        device_name: devices.find(d => d.id === selectedDevice)?.name,
        image_metric: selectedMetric || 'image',
        confidence_threshold: config.confidence ?? 0.25,
        draw_boxes: config.drawBoxes ?? true
      })

      if (result.success) {
        await refresh()
      } else {
        setError(result.error || 'Failed to bind device')
      }
      setLoading(false)
    }

    // Unbind device
    const handleUnbind = async () => {
      if (!selectedDevice) return
      setLoading(true)
      await executeCommand(extensionId, 'unbind_device', { device_id: selectedDevice })
      setBinding(null)
      await refresh()
      setLoading(false)
    }

    // Toggle binding
    const handleToggle = async () => {
      if (!selectedDevice || !binding) return
      await executeCommand(extensionId, 'toggle_binding', {
        device_id: selectedDevice,
        active: !binding.binding.active
      })
      await refresh()
    }

    const displayTitle = title || 'YOLO Device Inference'
    const isBound = !!binding

    // Filter image metrics
    const imageMetrics = useMemo(() => metrics.filter(m =>
      m.type === 'image' || m.name.toLowerCase().includes('image') || m.id.toLowerCase().includes('image')
    ), [metrics])

    // Object counts
    const objectCounts = useMemo(() => {
      if (!binding?.last_detections) return {}
      return binding.last_detections.reduce((acc, obj) => {
        acc[obj.label] = (acc[obj.label] || 0) + 1
        return acc
      }, {} as Record<string, number>)
    }, [binding?.last_detections])

    const deviceOptions = useMemo(() =>
      devices.map(d => ({ value: d.id, label: d.name || d.id })),
    [devices])

    const metricOptions = useMemo(() =>
      (imageMetrics.length > 0 ? imageMetrics : metrics).map(m => ({
        value: m.id,
        label: m.display_name || m.name
      })),
    [imageMetrics, metrics])

    return (
      <div ref={ref} className={`ydi ${className}`}>
        <div className="ydi-card">
          {/* Header */}
          <div className="ydi-header">
            <div className="ydi-title">
              <Icon name="camera" style={{ width: '16px', height: '16px' }} />
              <span>{displayTitle}</span>
            </div>
            <div className={`ydi-badge ${status?.model_loaded && isBound ? 'ydi-badge-active' : ''}`}>
              {status?.model_loaded ? (isBound ? 'Active' : 'Ready') : 'No Model'}
            </div>
          </div>

          {/* Content */}
          <div className="ydi-content">
            {/* Preview with overlay */}
            <div className="ydi-preview-wrapper">
              <div className="ydi-preview">
                {binding?.last_image ? (
                  <canvas ref={canvasRef} className="ydi-canvas" />
                ) : (
                  <div className="ydi-placeholder">
                    <Icon name="image" className="ydi-placeholder-icon" />
                    <div>{isBound ? 'Waiting for image...' : 'Bind a device to start'}</div>
                  </div>
                )}
              </div>

              {/* Results overlay */}
              {binding?.last_detections && binding.last_detections.length > 0 && (
                <div className="ydi-overlay">
                  {/* Stats */}
                  <div className="ydi-stats-overlay">
                    <div className="ydi-stat-overlay">
                      <span className="ydi-stat-label-overlay">Objects</span>
                      <span className="ydi-stat-value-overlay">{binding.total_detections}</span>
                    </div>
                    <div className="ydi-stat-overlay">
                      <span className="ydi-stat-label-overlay">Inferences</span>
                      <span className="ydi-stat-value-overlay">{binding.total_inferences}</span>
                    </div>
                  </div>

                  {/* Object tags */}
                  {Object.keys(objectCounts).length > 0 && (
                    <div className="ydi-objects-overlay">
                      {Object.entries(objectCounts).map(([label, count]) => (
                        <div key={label} className="ydi-object-tag-overlay">
                          <Icon name="box" style={{ width: '10px', height: '10px' }} />
                          <span>{label} ×{count}</span>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              )}

              {/* Error overlay */}
              {error && (
                <div className="ydi-overlay">
                  <div className="ydi-error-overlay">{error}</div>
                </div>
              )}
            </div>

            {/* Control bar - compact selectors */}
            <div className="ydi-control-bar">
              <div className="ydi-selector">
                <div className="ydi-selector-label">Device</div>
                <Dropdown
                  value={selectedDevice}
                  options={deviceOptions}
                  placeholder="Select device..."
                  onChange={(val) => {
                    setSelectedDevice(val)
                    setBinding(null)
                    if (onDataSourceChange) {
                      onDataSourceChange({
                        type: 'device',
                        extensionId,
                        deviceId: val,
                        metricId: selectedMetric,
                        deviceName: devices.find(d => d.id === val)?.name,
                      })
                    }
                  }}
                />
              </div>
              <div className="ydi-selector">
                <div className="ydi-selector-label">Image Source</div>
                <Dropdown
                  value={selectedMetric}
                  options={metricOptions}
                  placeholder="Auto"
                  onChange={(val) => {
                    setSelectedMetric(val)
                    if (onDataSourceChange && selectedDevice) {
                      onDataSourceChange({
                        type: 'device',
                        extensionId,
                        deviceId: selectedDevice,
                        metricId: val,
                        deviceName: devices.find(d => d.id === selectedDevice)?.name,
                      })
                    }
                  }}
                  disabled={!selectedDevice}
                />
              </div>
            </div>

            {/* Actions */}
            <div className="ydi-actions">
              {isBound ? (
                <>
                  <button className="ydi-btn" onClick={handleToggle} disabled={loading}>
                    <Icon name={binding?.binding.active ? 'pause' : 'play'} style={{ width: '14px', height: '14px' }} />
                    {binding?.binding.active ? 'Pause' : 'Resume'}
                  </button>
                  <button className="ydi-btn ydi-btn-danger" onClick={handleUnbind} disabled={loading}>
                    <Icon name="trash" style={{ width: '14px', height: '14px' }} />
                    Unbind
                  </button>
                </>
              ) : (
                <button
                  className="ydi-btn ydi-btn-primary"
                  onClick={handleBind}
                  disabled={loading || !selectedDevice}
                >
                  {loading ? (
                    <>
                      <div className="ydi-spinner" style={{ width: '14px', height: '14px' }} />
                      Binding...
                    </>
                  ) : (
                    <>
                      <Icon name="link" style={{ width: '14px', height: '14px' }} />
                      Bind Device
                    </>
                  )}
                </button>
              )}
            </div>
          </div>
        </div>
      </div>
    )
  }
)

DeviceInferenceCard.displayName = 'DeviceInferenceCard'
export default { DeviceInferenceCard }
