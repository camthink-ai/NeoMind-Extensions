/**
 * YOLO Device Inference Extension
 * Matches NeoMind dashboard design system
 */

import { forwardRef, useEffect, useState, useRef, useCallback } from 'react'

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
  // Callbacks for persisting configuration
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

const CSS_ID = 'ydi-styles-v2'

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
  overflow: hidden;
}

.ydi-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 10px;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--ydi-border);
}

.ydi-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-weight: 600;
  font-size: 13px;
  color: var(--ydi-fg);
}

.ydi-badge {
  font-size: 10px;
  padding: 2px 6px;
  border-radius: 4px;
  background: var(--ydi-hover);
  color: var(--ydi-muted);
}
.ydi-badge-success { background: hsl(142 70% 90%); color: hsl(142 70% 30%); }
.dark .ydi-badge-success { background: hsl(142 70% 20%); color: hsl(142 70% 70%); }

.ydi-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 10px;
  overflow: hidden;
}

.ydi-selector {
  display: flex;
  gap: 8px;
}

.ydi-select-wrap {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.ydi-select-label {
  font-size: 10px;
  color: var(--ydi-muted);
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.ydi-select {
  width: 100%;
  padding: 6px 8px;
  border: 1px solid var(--ydi-border);
  border-radius: 6px;
  background: var(--ydi-card);
  color: var(--ydi-fg);
  font-size: 12px;
  cursor: pointer;
}

.ydi-preview {
  flex: 1;
  min-height: 100px;
  background: var(--ydi-hover);
  border-radius: 6px;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  position: relative;
}

.ydi-preview-fill .ydi-preview-canvas {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.ydi-preview-contain .ydi-preview-canvas {
  max-width: 100%;
  max-height: 100%;
  object-fit: contain;
}

.ydi-preview-stretch .ydi-preview-canvas {
  width: 100% !important;
  height: 100% !important;
  object-fit: fill;
}

.ydi-preview-placeholder {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  color: var(--ydi-muted);
  font-size: 11px;
}

.ydi-preview-icon {
  width: 32px;
  height: 32px;
  opacity: 0.5;
}

.ydi-stats {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 8px;
}

.ydi-stat {
  text-align: center;
  padding: 8px;
  background: var(--ydi-hover);
  border-radius: 6px;
}

.ydi-stat-label {
  font-size: 10px;
  color: var(--ydi-muted);
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.ydi-stat-value {
  font-size: 16px;
  font-weight: 600;
  color: var(--ydi-fg);
  margin-top: 2px;
}

.ydi-detections {
  max-height: 60px;
  overflow-y: auto;
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}

.ydi-detection-chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 2px 6px;
  background: var(--ydi-hover);
  border-radius: 4px;
  font-size: 10px;
}

.ydi-detection-chip-conf {
  opacity: 0.7;
}

.ydi-actions {
  display: flex;
  gap: 8px;
  padding-top: 8px;
  border-top: 1px solid var(--ydi-border);
}

.ydi-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 6px 12px;
  border: 1px solid var(--ydi-border);
  border-radius: 6px;
  background: var(--ydi-card);
  color: var(--ydi-fg);
  font-size: 11px;
  cursor: pointer;
  transition: all 0.15s;
}
.ydi-btn:hover { background: var(--ydi-hover); }
.ydi-btn:disabled { opacity: 0.5; cursor: not-allowed; }
.ydi-btn-primary { background: var(--ydi-accent); color: white; border-color: var(--ydi-accent); }
.ydi-btn-primary:hover { filter: brightness(1.1); }
.ydi-btn-danger { color: hsl(0 72% 51%); border-color: hsl(0 72% 51% 0.3); }
.ydi-btn-danger:hover { background: hsl(0 72% 51% 0.1); }

.ydi-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  color: var(--ydi-muted);
  font-size: 11px;
}

.ydi-error {
  padding: 8px;
  background: hsl(0 72% 51% 0.1);
  border: 1px solid hsl(0 72% 51% 0.3);
  border-radius: 6px;
  color: hsl(0 72% 51%);
  font-size: 11px;
}

.ydi-icon { width: 14px; height: 14px; flex-shrink: 0; }
.ydi-icon-sm { width: 12px; height: 12px; }
.ydi-icon-lg { width: 24px; height: 24px; }
`

// ============================================================================
// Icon Component
// ============================================================================

const ICONS: Record<string, string> = {
  camera: '<path d="M23 19a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4l2-3h6l2 3h4a2 2 0 0 1 2 2z"/><circle cx="12" cy="13" r="4"/>',
  box: '<path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z"/>',
  play: '<polygon points="5 3 19 12 5 21 5 3"/>',
  pause: '<rect x="6" y="4" width="4" height="16"/><rect x="14" y="4" width="4" height="16"/>',
  trash: '<polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>',
  link: '<path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/>',
  image: '<rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/>'
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
    dangerouslySetInnerHTML={{ __html: ICONS[name] || ICONS.camera }}
  />
)

// ============================================================================
// Color Helper
// ============================================================================

const COLORS = ['#ef4444', '#22c55e', '#3b82f6', '#f97316', '#a855f7', '#06b6d4', '#ec4899', '#eab308']
const getColor = (index: number) => COLORS[index % COLORS.length]

// ============================================================================
// Draw Detections on Canvas
// ============================================================================

type DisplayMode = 'fill' | 'contain' | 'stretch'

function drawDetections(
  canvas: HTMLCanvasElement,
  imageBase64: string,
  detections: Detection[],
  displayMode: DisplayMode = 'contain'
): Promise<void> {
  return new Promise((resolve) => {
    const ctx = canvas.getContext('2d')
    if (!ctx) { resolve(); return }

    const img = new Image()
    img.onload = () => {
      const parent = canvas.parentElement
      const maxW = parent?.clientWidth || 400
      const maxH = parent?.clientHeight || 300

      let canvasW: number, canvasH: number, scale: number

      if (displayMode === 'stretch') {
        // Stretch to fill container
        canvasW = maxW
        canvasH = maxH
        scale = Math.min(maxW / img.width, maxH / img.height)
      } else if (displayMode === 'fill') {
        // Fill container (cover)
        scale = Math.max(maxW / img.width, maxH / img.height)
        canvasW = maxW
        canvasH = maxH
      } else {
        // Contain (fit)
        scale = Math.min(maxW / img.width, maxH / img.height)
        canvasW = img.width * scale
        canvasH = img.height * scale
      }

      canvas.width = canvasW
      canvas.height = canvasH

      // Calculate offset for contain mode
      const offsetX = displayMode === 'contain' ? (canvasW - img.width * scale) / 2 : 0
      const offsetY = displayMode === 'contain' ? (canvasH - img.height * scale) / 2 : 0

      // Clear and draw
      ctx.fillStyle = 'transparent'
      ctx.fillRect(0, 0, canvasW, canvasH)

      if (displayMode === 'contain') {
        ctx.drawImage(img, offsetX, offsetY, img.width * scale, img.height * scale)
      } else {
        ctx.drawImage(img, 0, 0, canvasW, canvasH)
      }

      // Draw detections
      const scaleX = displayMode === 'contain' ? scale : canvasW / img.width
      const scaleY = displayMode === 'contain' ? scale : canvasH / img.height
      const drawOffsetX = displayMode === 'contain' ? offsetX : 0
      const drawOffsetY = displayMode === 'contain' ? offsetY : 0

      detections.forEach((det, i) => {
        if (!det.bbox) return
        const x = det.bbox.x * scaleX + drawOffsetX
        const y = det.bbox.y * scaleY + drawOffsetY
        const w = det.bbox.width * scaleX
        const h = det.bbox.height * scaleY
        const color = getColor(det.class_id ?? i)

        ctx.strokeStyle = color
        ctx.lineWidth = 2
        ctx.strokeRect(x, y, w, h)

        const label = `${det.label} ${(det.confidence * 100).toFixed(0)}%`
        ctx.font = 'bold 10px sans-serif'
        const textW = ctx.measureText(label).width
        const textH = 14

        ctx.fillStyle = color
        ctx.fillRect(x, y >= textH ? y - textH : y, textW + 6, textH)
        ctx.fillStyle = '#fff'
        ctx.fillText(label, x + 3, (y >= textH ? y - textH : y) + 10)
      })

      resolve()
    }
    img.onerror = () => resolve()
    img.src = `data:image/jpeg;base64,${imageBase64}`
  })
}

// ============================================================================
// Main Component
// ============================================================================

const DeviceInferenceCard = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  ({ title, dataSource, config = {}, className = '', getDevices, getDeviceMetrics, onDataSourceChange, onConfigChange: _onConfigChange }, ref) => {
    const confidence = config.confidence ?? 0.25
    const drawBoxes = config.drawBoxes ?? true
    const showPreview = config.showPreview ?? true
    const displayMode = config.displayMode ?? 'contain' // 'fill' | 'contain' | 'stretch'

    const extensionId = dataSource?.extensionId || EXTENSION_ID

    // Device selection state
    const [devices, setDevices] = useState<Device[]>([])
    const [selectedDevice, setSelectedDevice] = useState<string>(dataSource?.deviceId || dataSource?.device_id || '')
    const [metrics, setMetrics] = useState<Metric[]>([])
    const [selectedMetric, setSelectedMetric] = useState<string>(dataSource?.metricId || dataSource?.metric_id || '')

    // Binding state
    const [status, setStatus] = useState<ExtensionStatus | null>(null)
    const [binding, setBinding] = useState<BindingStatus | null>(null)
    const [loading, setLoading] = useState(false)
    const [error, setError] = useState<string | null>(null)

    const canvasRef = useRef<HTMLCanvasElement>(null)

    // Load devices
    useEffect(() => {
      const loadDevices = async () => {
        let deviceList: Device[] = []
        if (getDevices) {
          deviceList = await getDevices()
        } else {
          deviceList = await fetchDevices()
        }
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
        let metricList: Metric[] = []
        if (getDeviceMetrics) {
          metricList = await getDeviceMetrics(selectedDevice)
        } else {
          metricList = await fetchDeviceMetrics(selectedDevice)
        }
        setMetrics(Array.isArray(metricList) ? metricList : [])

        // Auto-select image metric
        if (metricList.length > 0 && !selectedMetric) {
          const imageMetrics = metricList.filter(m =>
            m.type === 'image' ||
            m.name.toLowerCase().includes('image') ||
            m.name.toLowerCase().includes('frame') ||
            m.id.toLowerCase().includes('image')
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

      // Find binding for selected device
      const found = b.find(x => x.binding.device_id === selectedDevice)
      setBinding(found || null)
    }, [extensionId, selectedDevice])

    useEffect(() => {
      refresh()
      const interval = setInterval(refresh, 3000)
      return () => clearInterval(interval)
    }, [refresh])

    // Draw detections when image updates
    useEffect(() => {
      if (binding?.last_image && binding?.last_detections && canvasRef.current) {
        drawDetections(canvasRef.current, binding.last_image, binding.last_detections, displayMode)
      }
    }, [binding?.last_image, binding?.last_detections, displayMode])

    // Bind device
    const handleBind = async () => {
      if (!selectedDevice) return
      setLoading(true)
      setError(null)

      const result = await executeCommand(extensionId, 'bind_device', {
        device_id: selectedDevice,
        device_name: devices.find(d => d.id === selectedDevice)?.name,
        image_metric: selectedMetric || 'image',
        confidence_threshold: confidence,
        draw_boxes: drawBoxes
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

    const formatTime = (ts: number | null | undefined) => ts ? new Date(ts).toLocaleTimeString() : 'Never'
    const displayTitle = title || 'YOLO Device Inference'
    const isBound = !!binding

    // Filter image metrics
    const imageMetrics = metrics.filter(m =>
      m.type === 'image' ||
      m.name.toLowerCase().includes('image') ||
      m.name.toLowerCase().includes('frame') ||
      m.id.toLowerCase().includes('image') ||
      m.id.toLowerCase().includes('data')
    )

    return (
      <div ref={ref} className={`ydi ${className}`}>
        <div className="ydi-card">
          <div className="ydi-header">
            <div className="ydi-title">
              <Icon name="camera" className="ydi-icon" />
              <span>{displayTitle}</span>
            </div>
            <div className={`ydi-badge ${status?.model_loaded ? 'ydi-badge-success' : ''}`}>
              {status?.model_loaded ? 'Ready' : 'No Model'}
            </div>
          </div>

          <div className="ydi-content">
            {/* Device Selector */}
            <div className="ydi-selector">
              <div className="ydi-select-wrap">
                <span className="ydi-select-label">Device</span>
                <select
                  className="ydi-select"
                  value={selectedDevice}
                  onChange={(e) => {
                    const newDeviceId = e.target.value
                    setSelectedDevice(newDeviceId)
                    setBinding(null)
                    // Persist dataSource change
                    if (onDataSourceChange) {
                      onDataSourceChange({
                        type: 'device',
                        extensionId,
                        deviceId: newDeviceId,
                        metricId: selectedMetric,
                        deviceName: devices.find(d => d.id === newDeviceId)?.name,
                      })
                    }
                  }}
                >
                  <option value="">Select device...</option>
                  {devices.map(d => (
                    <option key={d.id} value={d.id}>{d.name || d.id}</option>
                  ))}
                </select>
              </div>
              <div className="ydi-select-wrap">
                <span className="ydi-select-label">Image Source</span>
                <select
                  className="ydi-select"
                  value={selectedMetric}
                  onChange={(e) => {
                    const newMetricId = e.target.value
                    setSelectedMetric(newMetricId)
                    // Persist dataSource change
                    if (onDataSourceChange && selectedDevice) {
                      onDataSourceChange({
                        type: 'device',
                        extensionId,
                        deviceId: selectedDevice,
                        metricId: newMetricId,
                        deviceName: devices.find(d => d.id === selectedDevice)?.name,
                      })
                    }
                  }}
                  disabled={!selectedDevice}
                >
                  <option value="">Auto detect</option>
                  {(imageMetrics.length > 0 ? imageMetrics : metrics).map(m => (
                    <option key={m.id} value={m.id}>{m.display_name || m.name}</option>
                  ))}
                </select>
              </div>
            </div>

            {/* Preview */}
            {showPreview && (
              <div className={`ydi-preview ydi-preview-${displayMode}`}>
                {binding?.last_image ? (
                  <canvas ref={canvasRef} className="ydi-preview-canvas" />
                ) : (
                  <div className="ydi-preview-placeholder">
                    <Icon name="image" className="ydi-preview-icon" />
                    <div>{isBound ? 'Waiting for inference...' : 'Select device and bind'}</div>
                  </div>
                )}
              </div>
            )}

            {/* Stats */}
            <div className="ydi-stats">
              <div className="ydi-stat">
                <div className="ydi-stat-label">Detections</div>
                <div className="ydi-stat-value">{binding?.total_detections || 0}</div>
              </div>
              <div className="ydi-stat">
                <div className="ydi-stat-label">Inferences</div>
                <div className="ydi-stat-value">{binding?.total_inferences || 0}</div>
              </div>
              <div className="ydi-stat">
                <div className="ydi-stat-label">Last</div>
                <div className="ydi-stat-value" style={{ fontSize: '11px' }}>
                  {formatTime(binding?.last_inference)}
                </div>
              </div>
            </div>

            {/* Detections */}
            {binding?.last_detections && binding.last_detections.length > 0 && (
              <div className="ydi-detections">
                {binding.last_detections.slice(0, 6).map((d, i) => (
                  <span key={i} className="ydi-detection-chip">
                    {d.label}
                    <span className="ydi-detection-chip-conf">{(d.confidence * 100).toFixed(0)}%</span>
                  </span>
                ))}
              </div>
            )}

            {error && <div className="ydi-error">{error}</div>}

            {/* Actions */}
            <div className="ydi-actions">
              {isBound ? (
                <>
                  <button className="ydi-btn" onClick={handleToggle} disabled={loading}>
                    <Icon name={binding?.binding.active ? 'pause' : 'play'} className="ydi-icon-sm" />
                    {binding?.binding.active ? 'Pause' : 'Resume'}
                  </button>
                  <button className="ydi-btn ydi-btn-danger" onClick={handleUnbind} disabled={loading}>
                    <Icon name="trash" className="ydi-icon-sm" />
                    Unbind
                  </button>
                </>
              ) : (
                <button
                  className="ydi-btn ydi-btn-primary"
                  onClick={handleBind}
                  disabled={loading || !selectedDevice}
                >
                  <Icon name="link" className="ydi-icon-sm" />
                  {loading ? 'Binding...' : 'Bind Device'}
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

// Inject styles
if (typeof document !== 'undefined' && !document.getElementById(CSS_ID)) {
  const style = document.createElement('style')
  style.id = CSS_ID
  style.textContent = STYLES
  document.head.appendChild(style)
}

export default { DeviceInferenceCard }