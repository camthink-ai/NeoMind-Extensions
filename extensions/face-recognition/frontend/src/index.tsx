/**
 * Face Recognition Extension
 * Canvas-based face detection with identity overlays and face management.
 */

import { forwardRef, useEffect, useState, useRef, useCallback, useMemo } from 'react'
import { FaceRegistrationCard } from './FaceRegistrationCard'

// ============================================================================
// Types
// ============================================================================

export interface ExtensionComponentProps {
  title?: string
  dataSource?: DataSource
  className?: string
  config?: Record<string, any>
  extensionId?: string
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

interface FaceBox {
  x: number
  y: number
  width: number
  height: number
  confidence: number
}

interface FaceResult {
  face_box: FaceBox
  name?: string | null
  similarity?: number | null
  face_id?: string | null
}

interface DeviceBinding {
  device_id: string
  metric_name: string
  active: boolean
  created_at: number
}

interface BindingStatus {
  binding: DeviceBinding
  last_image?: string
  last_faces?: FaceResult[]
  total_inferences: number
  total_recognized: number
  total_unknown: number
  last_error?: string | null
}

interface ExtensionStatus {
  model_loaded: boolean
  total_bindings: number
  total_inferences: number
  total_faces_detected: number
  total_faces_recognized: number
  total_errors: number
  config?: {
    confidence_threshold: number
    recognition_threshold: number
    max_faces: number
    auto_detect: boolean
  }
}

interface FaceEntrySummary {
  id: string
  name: string
  registered_at: number
  thumbnail: string
}

// ============================================================================
// API
// ============================================================================

const EXTENSION_ID = 'face-recognition'

const getApiHeaders = (): Record<string, string> => {
  const token = localStorage.getItem('neomind_token') || sessionStorage.getItem('neomind_token_session')
  const headers: Record<string, string> = { 'Content-Type': 'application/json' }
  if (token) headers['Authorization'] = `Bearer ${token}`
  return headers
}

const getApiBase = (): string =>
  (window as any).__TAURI__ ? 'http://localhost:9375/api' : '/api'

async function executeCommand(
  extensionId: string,
  command: string,
  args: Record<string, unknown> = {}
): Promise<{ success: boolean; data?: any; error?: string }> {
  try {
    const res = await fetch(`${getApiBase()}/extensions/${extensionId}/command`, {
      method: 'POST',
      headers: getApiHeaders(),
      body: JSON.stringify({ command, args }),
    })
    if (!res.ok) return { success: false, error: `HTTP ${res.status}` }
    return res.json()
  } catch (e) {
    return { success: false, error: e instanceof Error ? e.message : 'Network error' }
  }
}

async function fetchStatus(extensionId: string): Promise<ExtensionStatus | null> {
  const result = await executeCommand(extensionId, 'get_status', {})
  return result.success && result.data ? result.data : null
}

async function fetchBindings(extensionId: string): Promise<BindingStatus[]> {
  const result = await executeCommand(extensionId, 'get_bindings', {})
  if (!result.success || !result.data?.bindings) return []
  return result.data.bindings.map((b: any) => ({
    binding: { device_id: b.device_id, metric_name: b.metric_name, active: b.active, created_at: b.created_at },
    total_inferences: b.stats?.total_inferences ?? 0,
    total_recognized: b.stats?.total_recognized ?? 0,
    total_unknown: b.stats?.total_unknown ?? 0,
    last_image: b.last_image,
    last_faces: b.last_faces,
    last_error: b.last_error,
  }))
}

async function fetchRegisteredFaces(extensionId: string): Promise<FaceEntrySummary[]> {
  const result = await executeCommand(extensionId, 'list_faces', {})
  if (!result.success || !result.data?.faces) return []
  return result.data.faces
}

async function deleteRegisteredFace(
  extensionId: string,
  faceId: string
): Promise<{ success: boolean; error?: string }> {
  return executeCommand(extensionId, 'delete_face', { face_id: faceId })
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
    const res = await fetch(`${getApiBase()}/devices/${deviceId}/current`, {
      headers: getApiHeaders(),
    })
    if (!res.ok) return []
    const data = await res.json()
    const metrics = data.data?.metrics || data.metrics || {}
    return Object.entries(metrics).map(([id, m]: [string, any]) => ({
      id,
      name: m.name || id,
      display_name: m.display_name || m.name || id,
      type: m.data_type || 'string',
      data_type: m.data_type || 'string',
    }))
  } catch {
    return []
  }
}

// ============================================================================
// Styles
// ============================================================================

const CSS_ID = 'frc-styles-v1'

const STYLES = `
.frc {
  --frc-fg: hsl(240 10% 10%);
  --frc-muted: hsl(240 5% 45%);
  --frc-accent: hsl(210 80% 55%);
  --frc-card: rgba(255,255,255,0.5);
  --frc-border: rgba(0,0,0,0.06);
  --frc-hover: rgba(0,0,0,0.03);
  --frc-green: hsl(142 70% 45%);
  --frc-yellow: hsl(45 90% 50%);
  width: 100%;
  height: 100%;
  font-size: 12px;
}
.dark .frc {
  --frc-fg: hsl(0 0% 95%);
  --frc-muted: hsl(0 0% 60%);
  --frc-card: rgba(30,30,30,0.5);
  --frc-border: rgba(255,255,255,0.08);
  --frc-hover: rgba(255,255,255,0.03);
  --frc-green: hsl(142 70% 55%);
  --frc-yellow: hsl(45 90% 60%);
}

.frc-card {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: 10px;
  background: var(--frc-card);
  backdrop-filter: blur(12px);
  border: 1px solid var(--frc-border);
  border-radius: 8px;
  box-sizing: border-box;
}

.frc-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-shrink: 0;
  margin-bottom: 8px;
}

.frc-title {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--frc-fg);
  font-size: 13px;
  font-weight: 600;
}

.frc-badge {
  padding: 2px 6px;
  background: rgba(210, 80, 55, 0.1);
  color: var(--frc-accent);
  border-radius: 4px;
  font-size: 9px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.3px;
}
.frc-badge-active { background: hsl(142 70% 90%); color: hsl(142 70% 30%); }
.dark .frc-badge-active { background: hsl(142 70% 20%); color: hsl(142 70% 70%); }

.frc-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 0;
  overflow: visible;
}

/* Canvas preview */
.frc-preview-wrapper {
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
.dark .frc-preview-wrapper {
  background: rgba(0,0,0,0.2);
}

.frc-preview {
  position: relative;
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
}

.frc-canvas {
  max-width: 100%;
  max-height: 100%;
  width: auto;
  height: auto;
  object-fit: contain;
}

.frc-placeholder {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  color: var(--frc-muted);
  font-size: 11px;
  padding: 20px;
  text-align: center;
}

.frc-placeholder-icon {
  width: 40px;
  height: 40px;
  opacity: 0.4;
}

/* Overlay */
.frc-overlay {
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
.frc-overlay::-webkit-scrollbar { width: 4px; }
.frc-overlay::-webkit-scrollbar-track { background: transparent; }
.frc-overlay::-webkit-scrollbar-thumb { background: rgba(255,255,255,0.3); border-radius: 2px; }

.frc-stats-overlay {
  display: flex;
  gap: 12px;
}

.frc-stat-overlay {
  display: flex;
  align-items: baseline;
  gap: 4px;
}

.frc-stat-label-overlay {
  font-size: 10px;
  color: rgba(255,255,255,0.7);
  text-transform: uppercase;
  letter-spacing: 0.3px;
}

.frc-stat-value-overlay {
  font-size: 14px;
  font-weight: 700;
  color: #fff;
}

/* Face tags in overlay */
.frc-face-tags-overlay {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}

.frc-face-tag-overlay {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 2px 6px;
  border-radius: 4px;
  font-size: 10px;
  color: #fff;
  border: 1px solid rgba(255,255,255,0.2);
}
.frc-face-tag-recognized {
  background: rgba(34, 197, 94, 0.4);
  border-color: rgba(34, 197, 94, 0.6);
}
.frc-face-tag-unknown {
  background: rgba(234, 179, 8, 0.4);
  border-color: rgba(234, 179, 8, 0.6);
}

/* Error overlay */
.frc-error-overlay {
  padding: 6px 8px;
  background: rgba(239, 68, 68, 0.3);
  backdrop-filter: blur(4px);
  border-radius: 4px;
  color: #fff;
  font-size: 10px;
}

/* Dropdown */
.frc-dropdown-container {
  position: relative;
}

.frc-dropdown-trigger {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 4px;
  width: 100%;
  padding: 6px 10px;
  border: 1px solid var(--frc-border);
  border-radius: 6px;
  background: var(--frc-card);
  color: var(--frc-fg);
  font-size: 11px;
  cursor: pointer;
  transition: all 0.15s;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.frc-dropdown-trigger:hover {
  background: var(--frc-hover);
  border-color: var(--frc-accent);
}
.frc-dropdown-trigger:focus {
  outline: none;
  border-color: var(--frc-accent);
  box-shadow: 0 0 0 2px rgba(210, 80, 55, 0.1);
}
.frc-dropdown-trigger-placeholder {
  color: var(--frc-muted);
}

.frc-dropdown-menu {
  position: absolute;
  top: 100%;
  left: 0;
  right: 0;
  margin-top: 4px;
  background: var(--frc-card);
  border: 1px solid var(--frc-border);
  border-radius: 6px;
  box-shadow: 0 4px 12px rgba(0,0,0,0.15);
  z-index: 100;
  max-height: 150px;
  overflow-y: auto;
  backdrop-filter: blur(12px);
}
.frc-dropdown-menu::-webkit-scrollbar { width: 4px; }
.frc-dropdown-menu::-webkit-scrollbar-thumb { background: var(--frc-border); border-radius: 2px; }

.frc-dropdown-item {
  padding: 6px 10px;
  font-size: 11px;
  color: var(--frc-fg);
  cursor: pointer;
  transition: background 0.1s;
}
.frc-dropdown-item:hover {
  background: var(--frc-hover);
}
.frc-dropdown-item-selected {
  background: rgba(210, 80, 55, 0.1);
  color: var(--frc-accent);
  font-weight: 500;
}

.frc-dropdown-empty {
  padding: 8px 10px;
  font-size: 10px;
  color: var(--frc-muted);
  text-align: center;
}

/* Control bar */
.frc-control-bar {
  display: flex;
  gap: 6px;
  flex-shrink: 0;
}

.frc-selector {
  flex: 1;
  min-width: 0;
}

.frc-selector-label {
  font-size: 9px;
  color: var(--frc-muted);
  text-transform: uppercase;
  letter-spacing: 0.3px;
  margin-bottom: 2px;
}

/* Actions */
.frc-actions {
  display: flex;
  gap: 6px;
  flex-shrink: 0;
  padding-top: 4px;
  border-top: 1px solid var(--frc-border);
}

.frc-btn {
  flex: 1;
  padding: 6px 12px;
  border: 1px solid var(--frc-border);
  border-radius: 6px;
  font-size: 11px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.2s;
  background: transparent;
  color: var(--frc-fg);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
}
.frc-btn:hover {
  background: var(--frc-hover);
}
.frc-btn-primary {
  background: var(--frc-accent);
  border-color: var(--frc-accent);
  color: #fff;
}
.frc-btn-primary:hover {
  opacity: 0.9;
  background: var(--frc-accent);
}
.frc-btn-danger {
  color: hsl(0 72% 51%);
  border-color: hsl(0 72% 51% 0.3);
}
.frc-btn-danger:hover {
  background: hsl(0 72% 51% 0.1);
}
.frc-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.frc-icon { width: 14px; height: 14px; flex-shrink: 0; }

.frc-spinner {
  width: 20px;
  height: 20px;
  border: 2px solid var(--frc-border);
  border-top-color: var(--frc-accent);
  border-radius: 50%;
  animation: frc-spin 0.7s linear infinite;
}
@keyframes frc-spin {
  to { transform: rotate(360deg); }
}

/* Face list panel */
.frc-face-list {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(72px, 1fr));
  gap: 6px;
  max-height: 120px;
  overflow-y: auto;
  padding-top: 4px;
}
.frc-face-list::-webkit-scrollbar { width: 4px; }
.frc-face-list::-webkit-scrollbar-thumb { background: var(--frc-border); border-radius: 2px; }

.frc-face-item {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  padding: 4px;
  border: 1px solid var(--frc-border);
  border-radius: 6px;
  position: relative;
  background: var(--frc-card);
}

.frc-face-thumb {
  width: 48px;
  height: 48px;
  border-radius: 4px;
  object-fit: cover;
  background: rgba(0,0,0,0.05);
}

.frc-face-name {
  font-size: 9px;
  color: var(--frc-fg);
  text-align: center;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.frc-face-delete {
  position: absolute;
  top: 2px;
  right: 2px;
  width: 16px;
  height: 16px;
  padding: 0;
  border: none;
  border-radius: 50%;
  background: rgba(239, 68, 68, 0.8);
  color: #fff;
  font-size: 9px;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  opacity: 0;
  transition: opacity 0.15s;
}
.frc-face-item:hover .frc-face-delete {
  opacity: 1;
}

`

function injectStyles(): void {
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
  user: '<path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/>',
  camera: '<path d="M23 19a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4l2-3h6l2 3h4a2 2 0 0 1 2 2z"/><circle cx="12" cy="13" r="4"/>',
  image: '<rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/>',
  link: '<path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/>',
  trash: '<polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>',
  plus: '<line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/>',
  chevron: '<polyline points="6 9 12 15 18 9"/>',
  x: '<line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>',
  upload: '<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/>',
  check: '<polyline points="20 6 9 17 4 12"/>',
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
    dangerouslySetInnerHTML={{ __html: ICONS[name] || ICONS.user }}
  />
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
    <div className="frc-dropdown-container" ref={ref}>
      <button
        className="frc-dropdown-trigger"
        onClick={() => !disabled && setOpen(!open)}
        disabled={disabled}
      >
        <span className={!selected ? 'frc-dropdown-trigger-placeholder' : ''}>
          {selected?.label || placeholder || 'Select...'}
        </span>
        <Icon name="chevron" style={{ width: '12px', height: '12px', opacity: 0.5 }} />
      </button>
      {open && (
        <div className="frc-dropdown-menu">
          {options.length === 0 ? (
            <div className="frc-dropdown-empty">No options</div>
          ) : (
            options.map(opt => (
              <div
                key={opt.value}
                className={`frc-dropdown-item ${opt.value === value ? 'frc-dropdown-item-selected' : ''}`}
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
// Draw Face Detections on Canvas
// ============================================================================

function drawFaceDetections(
  canvas: HTMLCanvasElement,
  imageBase64: string,
  faces: FaceResult[]
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

      ctx.fillStyle = 'transparent'
      ctx.fillRect(0, 0, canvasW, canvasH)
      ctx.drawImage(img, 0, 0, canvasW, canvasH)

      faces.forEach((face) => {
        const fb = face.face_box
        const x = fb.x * scale
        const y = fb.y * scale
        const w = fb.width * scale
        const h = fb.height * scale

        const recognized = !!face.name
        const color = recognized ? 'rgba(34, 197, 94, 0.9)' : 'rgba(234, 179, 8, 0.9)'

        // Box
        ctx.strokeStyle = color
        ctx.lineWidth = 2
        ctx.strokeRect(x, y, w, h)

        // Label
        const simPct = face.similarity != null ? ` ${(face.similarity * 100).toFixed(0)}%` : ''
        const label = recognized ? `${face.name}${simPct}` : 'Unknown'
        ctx.font = 'bold 11px sans-serif'
        const textW = ctx.measureText(label).width
        const textH = 16

        const labelY = y >= textH ? y - textH : y
        ctx.fillStyle = color
        ctx.fillRect(x, labelY, textW + 8, textH)
        ctx.fillStyle = '#fff'
        ctx.fillText(label, x + 4, labelY + 12)
      })

      resolve()
    }
    img.onerror = () => resolve()
    img.src = imageBase64.startsWith('data:') ? imageBase64 : `data:image/jpeg;base64,${imageBase64}`
  })
}

// (Registration dialog is now provided by FaceRegistrationCard component)

// ============================================================================
// Main Component
// ============================================================================

export const FaceRecognitionCard = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function FaceRecognitionCard(props, ref) {
    const {
      title,
      dataSource,
      config: _config = {},
      className = '',
      extensionId: propExtensionId,
      getDevices,
      getDeviceMetrics,
      onDataSourceChange,
      onConfigChange: _onConfigChange,
    } = props

    useEffect(() => injectStyles(), [])

    const extensionId = propExtensionId || dataSource?.extensionId || EXTENSION_ID

    // Device selection state
    const [devices, setDevices] = useState<Device[]>([])
    const [selectedDevice, setSelectedDevice] = useState<string>(dataSource?.deviceId || '')
    const [metrics, setMetrics] = useState<Metric[]>([])
    const [selectedMetric, setSelectedMetric] = useState<string>(dataSource?.metricId || '')

    // Binding / status state
    const [status, setStatus] = useState<ExtensionStatus | null>(null)
    const [binding, setBinding] = useState<BindingStatus | null>(null)
    const [loading, setLoading] = useState(false)
    const [error, setError] = useState<string | null>(null)

    // Face management
    const [registeredFaces, setRegisteredFaces] = useState<FaceEntrySummary[]>([])
    const [showRegistration, setShowRegistration] = useState(false)

    const canvasRef = useRef<HTMLCanvasElement>(null)

    // ---- Load devices ----
    useEffect(() => {
      const loadDevices = async () => {
        const deviceList = getDevices ? await getDevices() : await fetchDevices()
        setDevices(Array.isArray(deviceList) ? deviceList : [])
      }
      loadDevices()
    }, [getDevices])

    // ---- Load metrics when device changes ----
    useEffect(() => {
      const loadMetrics = async () => {
        if (!selectedDevice) {
          setMetrics([])
          return
        }
        const metricList = getDeviceMetrics
          ? await getDeviceMetrics(selectedDevice)
          : await fetchDeviceMetrics(selectedDevice)
        setMetrics(Array.isArray(metricList) ? metricList : [])

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

    // ---- Refresh status, bindings, faces (3-second polling) ----
    const refresh = useCallback(async () => {
      const [s, b, faces] = await Promise.all([
        fetchStatus(extensionId),
        fetchBindings(extensionId),
        fetchRegisteredFaces(extensionId),
      ])
      setStatus(s)
      setRegisteredFaces(faces)

      const found = b.find((x: BindingStatus) => x.binding.device_id === selectedDevice)
      setBinding(found || null)
    }, [extensionId, selectedDevice])

    useEffect(() => {
      refresh()
      const interval = setInterval(refresh, 3000)
      return () => clearInterval(interval)
    }, [refresh])

    // ---- Draw face detections when image updates ----
    useEffect(() => {
      if (binding?.last_image && binding?.last_faces && canvasRef.current) {
        drawFaceDetections(canvasRef.current, binding.last_image, binding.last_faces)
      }
    }, [binding?.last_image, binding?.last_faces])

    // ---- Bind device ----
    const handleBind = async () => {
      if (!selectedDevice) return
      setLoading(true)
      setError(null)

      const result = await executeCommand(extensionId, 'bind_device', {
        device_id: selectedDevice,
        metric_name: selectedMetric || 'image',
      })

      if (result.success) {
        await refresh()
      } else {
        setError(result.error || 'Failed to bind device')
      }
      setLoading(false)
    }

    // ---- Unbind device ----
    const handleUnbind = async () => {
      if (!selectedDevice) return
      setLoading(true)
      await executeCommand(extensionId, 'unbind_device', { device_id: selectedDevice })
      setBinding(null)
      await refresh()
      setLoading(false)
    }

    // ---- Delete face ----
    const handleDeleteFace = async (faceId: string) => {
      await deleteRegisteredFace(extensionId, faceId)
      await refresh()
    }

    const displayTitle = title || 'Face Recognition'
    const isBound = !!binding

    // Filter image metrics
    const imageMetrics = useMemo(() => metrics.filter(m =>
      m.type === 'image' || m.name.toLowerCase().includes('image') || m.id.toLowerCase().includes('image')
    ), [metrics])

    const deviceOptions = useMemo(() =>
      devices.map(d => ({ value: d.id, label: d.name || d.id })),
    [devices])

    const metricOptions = useMemo(() =>
      (imageMetrics.length > 0 ? imageMetrics : metrics).map(m => ({
        value: m.id,
        label: m.display_name || m.name,
      })),
    [imageMetrics, metrics])

    // Face tag counts for overlay
    const faceTagCounts = useMemo(() => {
      if (!binding?.last_faces) return { recognized: 0, unknown: 0 }
      let recognized = 0
      let unknown = 0
      binding.last_faces.forEach(f => {
        if (f.name) recognized++
        else unknown++
      })
      return { recognized, unknown }
    }, [binding?.last_faces])

    return (
      <div ref={ref} className={`frc ${className}`}>
        <div className="frc-card">
          {/* Header */}
          <div className="frc-header">
            <div className="frc-title">
              <Icon name="user" style={{ width: '16px', height: '16px' }} />
              <span>{displayTitle}</span>
            </div>
            <div className={`frc-badge ${status?.model_loaded && isBound ? 'frc-badge-active' : ''}`}>
              {status?.model_loaded ? (isBound ? 'Active' : 'Ready') : 'No Model'}
            </div>
          </div>

          {/* Content */}
          <div className="frc-content">
            {/* Control bar */}
            <div className="frc-control-bar">
              <div className="frc-selector">
                <div className="frc-selector-label">Device</div>
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
              <div className="frc-selector">
                <div className="frc-selector-label">Image Source</div>
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

            {/* Canvas preview with overlay */}
            <div className="frc-preview-wrapper">
              <div className="frc-preview">
                {binding?.last_image ? (
                  <canvas ref={canvasRef} className="frc-canvas" />
                ) : (
                  <div className="frc-placeholder">
                    <Icon name="image" className="frc-placeholder-icon" />
                    <div>{isBound ? 'Waiting for image...' : 'Bind a device to start'}</div>
                  </div>
                )}
              </div>

              {/* Stats + tags overlay */}
              {binding?.last_faces && binding.last_faces.length > 0 && (
                <div className="frc-overlay">
                  <div className="frc-stats-overlay">
                    <div className="frc-stat-overlay">
                      <span className="frc-stat-label-overlay">Inferences</span>
                      <span className="frc-stat-value-overlay">{binding.total_inferences}</span>
                    </div>
                    <div className="frc-stat-overlay">
                      <span className="frc-stat-label-overlay">Recognized</span>
                      <span className="frc-stat-value-overlay">{faceTagCounts.recognized}</span>
                    </div>
                    <div className="frc-stat-overlay">
                      <span className="frc-stat-label-overlay">Unknown</span>
                      <span className="frc-stat-value-overlay">{faceTagCounts.unknown}</span>
                    </div>
                  </div>

                  <div className="frc-face-tags-overlay">
                    {binding.last_faces.map((face, i) => (
                      <div
                        key={face.face_id || i}
                        className={`frc-face-tag-overlay ${face.name ? 'frc-face-tag-recognized' : 'frc-face-tag-unknown'}`}
                      >
                        <Icon name="user" style={{ width: '10px', height: '10px' }} />
                        <span>
                          {face.name || 'Unknown'}
                          {face.similarity != null ? ` ${(face.similarity * 100).toFixed(0)}%` : ''}
                        </span>
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {/* Error overlay */}
              {error && (
                <div className="frc-overlay">
                  <div className="frc-error-overlay">{error}</div>
                </div>
              )}
            </div>

            {/* Face list panel */}
            {registeredFaces.length > 0 && (
              <div className="frc-face-list">
                {registeredFaces.map(face => (
                  <div key={face.id} className="frc-face-item">
                    <button
                      className="frc-face-delete"
                      onClick={() => handleDeleteFace(face.id)}
                      title="Delete face"
                    >
                      <Icon name="x" style={{ width: '10px', height: '10px' }} />
                    </button>
                    {face.thumbnail && (
                      <img
                        className="frc-face-thumb"
                        src={face.thumbnail.startsWith('data:') ? face.thumbnail : `data:image/jpeg;base64,${face.thumbnail}`}
                        alt={face.name}
                      />
                    )}
                    <span className="frc-face-name" title={face.name}>{face.name}</span>
                  </div>
                ))}
              </div>
            )}

            {/* Actions */}
            <div className="frc-actions">
              {isBound ? (
                <>
                  <button
                    className="frc-btn"
                    onClick={() => setShowRegistration(true)}
                  >
                    <Icon name="plus" style={{ width: '14px', height: '14px' }} />
                    Register Face
                  </button>
                  <button className="frc-btn frc-btn-danger" onClick={handleUnbind} disabled={loading}>
                    <Icon name="trash" style={{ width: '14px', height: '14px' }} />
                    Unbind
                  </button>
                </>
              ) : (
                <button
                  className="frc-btn frc-btn-primary"
                  onClick={handleBind}
                  disabled={loading || !selectedDevice}
                >
                  {loading ? (
                    <>
                      <div className="frc-spinner" style={{ width: '14px', height: '14px' }} />
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

        {/* Registration dialog */}
        {showRegistration && (
          <FaceRegistrationCard
            extensionId={extensionId}
            onClose={() => setShowRegistration(false)}
            onRegistered={() => refresh()}
          />
        )}
      </div>
    )
  }
)

FaceRecognitionCard.displayName = 'FaceRecognitionCard'
export { FaceRegistrationCard }
export default { FaceRecognitionCard, FaceRegistrationCard }
