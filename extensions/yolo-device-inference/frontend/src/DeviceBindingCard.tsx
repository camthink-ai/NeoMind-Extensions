import { forwardRef, useState, useEffect, useCallback } from 'react'

// Types
export interface DeviceBinding {
  device_id: string
  image_metric: string
  result_metric_prefix: string
  confidence_threshold: number
  draw_boxes: boolean
  active: boolean
}

export interface BindingStatus {
  binding: DeviceBinding
  last_inference: number | null
  total_inferences: number
  total_detections: number
  last_error: string | null
}

export interface Detection {
  label: string
  confidence: number
  bbox: {
    x: number
    y: number
    width: number
    height: number
  }
}

export interface InferenceResult {
  device_id: string
  detections: Detection[]
  inference_time_ms: number
  image_width: number
  image_height: number
  timestamp: number
  annotated_image_base64: string | null
}

export interface DeviceBindingCardProps {
  executeCommand: (command: string, args: Record<string, unknown>) => Promise<unknown>
  devices?: Array<{ id: string; name: string; metrics: string[] }>
  onBindingChange?: (bindings: BindingStatus[]) => void
  onError?: (error: string) => void
  title?: string
  showStats?: boolean
}

// ============================================================================
// Scoped CSS
// ============================================================================

const STYLE_ID = 'dbc-styles-v1'

const STYLES = `
.dbc {
  --dbc-fg: var(--foreground);
  --dbc-muted: var(--muted-foreground);
  --dbc-accent: var(--primary);
  --dbc-card: var(--card);
  --dbc-border: var(--border);
  --dbc-hover: rgba(0,0,0,0.03);
  --dbc-success: var(--color-success, #22c55e);
  --dbc-error: var(--color-error, #ef4444);
  --dbc-error-bg: var(--color-error-bg, rgba(239, 68, 68, 0.1));
  --dbc-on-primary: var(--primary-foreground, #ffffff);
  width: 100%;
  font-size: 12px;
}
.dark .dbc {
  --dbc-hover: rgba(255,255,255,0.03);
  --dbc-on-primary: var(--primary-foreground, #17172a);
}
.dbc-card {
  background: var(--dbc-card);
  backdrop-filter: blur(12px);
  border: 1px solid var(--dbc-border);
  border-radius: 8px;
  padding: 16px;
}
.dbc-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 16px;
}
.dbc-title {
  margin: 0;
  font-size: 15px;
  font-weight: 600;
  color: var(--dbc-fg);
}
.dbc-status-badge {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  color: var(--dbc-muted);
}
.dbc-status-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  display: inline-block;
}
.dbc-status-dot-ready { background: var(--dbc-success); }
.dbc-status-dot-error { background: var(--dbc-error); }

.dbc-stats-bar {
  display: flex;
  gap: 24px;
  padding: 10px 12px;
  background: var(--dbc-hover);
  border-radius: 6px;
  margin-bottom: 16px;
}
.dbc-stat {
  display: flex;
  flex-direction: column;
  align-items: center;
}
.dbc-stat-label {
  font-size: 10px;
  color: var(--dbc-muted);
  text-transform: uppercase;
  letter-spacing: 0.3px;
}
.dbc-stat-value {
  font-size: 16px;
  font-weight: 600;
  color: var(--dbc-fg);
}

.dbc-add-form {
  padding: 12px;
  background: var(--dbc-hover);
  border-radius: 6px;
  margin-bottom: 16px;
}
.dbc-form-title {
  margin: 0 0 12px 0;
  font-size: 13px;
  font-weight: 600;
  color: var(--dbc-fg);
}
.dbc-form-row {
  display: flex;
  gap: 12px;
  align-items: flex-end;
  margin-bottom: 12px;
}
.dbc-form-group {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.dbc-label {
  font-size: 11px;
  color: var(--dbc-muted);
}
.dbc-input, .dbc-select {
  padding: 6px 10px;
  border: 1px solid var(--dbc-border);
  border-radius: 4px;
  font-size: 12px;
  background: var(--dbc-card);
  color: var(--dbc-fg);
  box-sizing: border-box;
}
.dbc-input:focus, .dbc-select:focus {
  outline: none;
  border-color: var(--dbc-accent);
}
.dbc-input::placeholder { color: var(--dbc-muted); }
.dbc-checkbox-label {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--dbc-muted);
  flex: 1;
}
.dbc-btn {
  padding: 6px 14px;
  border: 1px solid var(--dbc-border);
  border-radius: 6px;
  background: transparent;
  color: var(--dbc-fg);
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.15s;
}
.dbc-btn:hover { background: var(--dbc-hover); }
.dbc-btn:disabled { opacity: 0.5; cursor: not-allowed; }
.dbc-btn-primary {
  background: var(--dbc-accent);
  border-color: var(--dbc-accent);
  color: var(--dbc-on-primary);
}
.dbc-btn-primary:hover { opacity: 0.9; background: var(--dbc-accent); }
.dbc-btn-danger {
  color: var(--dbc-error);
  border-color: var(--dbc-error);
}

.dbc-bindings-list { margin-top: 8px; }
.dbc-empty-state {
  padding: 24px;
  text-align: center;
  color: var(--dbc-muted);
  font-size: 12px;
}
.dbc-binding-card {
  border: 1px solid var(--dbc-border);
  border-radius: 6px;
  margin-bottom: 8px;
  overflow: hidden;
}
.dbc-binding-card-active { border-color: var(--dbc-success); }
.dbc-binding-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 10px 12px;
  cursor: pointer;
}
.dbc-binding-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.dbc-device-id {
  font-weight: 600;
  font-size: 13px;
  color: var(--dbc-fg);
}
.dbc-metric-name {
  font-size: 11px;
  color: var(--dbc-muted);
}
.dbc-binding-actions {
  display: flex;
  align-items: center;
  gap: 12px;
}
.dbc-active-badge {
  padding: 2px 8px;
  border-radius: 10px;
  font-size: 10px;
  color: var(--dbc-on-primary);
}
.dbc-active-badge-on { background: var(--dbc-success); }
.dbc-active-badge-off { background: var(--dbc-muted); }

.dbc-binding-details {
  padding: 12px;
  border-top: 1px solid var(--dbc-border);
  background: var(--dbc-hover);
}
.dbc-detail-row {
  display: flex;
  justify-content: space-between;
  padding: 3px 0;
  font-size: 12px;
  color: var(--dbc-fg);
}
.dbc-error-row {
  padding: 6px 8px;
  background: var(--dbc-error-bg);
  border-radius: 4px;
  color: var(--dbc-error);
  font-size: 11px;
  margin-top: 8px;
}
.dbc-detail-actions {
  display: flex;
  gap: 8px;
  margin-top: 12px;
}
`

function injectStyles() {
  if (typeof document === 'undefined' || document.getElementById(STYLE_ID)) return
  const style = document.createElement('style')
  style.id = STYLE_ID
  style.textContent = STYLES
  document.head.appendChild(style)
}

// ============================================================================
// Component
// ============================================================================

export const DeviceBindingCard = forwardRef<HTMLDivElement, DeviceBindingCardProps>(
  function DeviceBindingCard({
    executeCommand,
    devices = [],
    onBindingChange,
    onError,
    title = 'YOLO Device Inference',
    showStats = true,
  }, ref) {
    const [bindings, setBindings] = useState<BindingStatus[]>([])
    const [status, setStatus] = useState<Record<string, unknown>>({})
    const [loading, setLoading] = useState(false)
    const [newBinding, setNewBinding] = useState<Partial<DeviceBinding>>({
      device_id: '',
      image_metric: 'image',
      result_metric_prefix: 'yolo_',
      confidence_threshold: 0.25,
      draw_boxes: true,
      active: true,
    })
    const [expandedBinding, setExpandedBinding] = useState<string | null>(null)

    useEffect(() => injectStyles(), [])

    const fetchBindings = useCallback(async () => {
      try {
        const result = await executeCommand('get_bindings', {}) as { bindings: BindingStatus[] }
        setBindings(result.bindings || [])
        onBindingChange?.(result.bindings || [])
      } catch (err) {
        console.error('Failed to fetch bindings:', err)
        onError?.('Failed to fetch device bindings')
      }
    }, [executeCommand, onBindingChange, onError])

    const fetchStatus = useCallback(async () => {
      try {
        const result = await executeCommand('get_status', {})
        setStatus(result as Record<string, unknown>)
      } catch (err) {
        console.error('Failed to fetch status:', err)
      }
    }, [executeCommand])

    useEffect(() => {
      fetchBindings()
      fetchStatus()
    }, [fetchBindings, fetchStatus])

    const handleBind = async () => {
      if (!newBinding.device_id) {
        onError?.('Please enter a device ID')
        return
      }
      setLoading(true)
      try {
        await executeCommand('bind_device', {
          device_id: newBinding.device_id,
          image_metric: newBinding.image_metric || 'image',
          result_metric_prefix: newBinding.result_metric_prefix || 'yolo_',
          confidence_threshold: newBinding.confidence_threshold || 0.25,
          draw_boxes: newBinding.draw_boxes ?? true,
        })
        await fetchBindings()
        setNewBinding({
          device_id: '',
          image_metric: 'image',
          result_metric_prefix: 'yolo_',
          confidence_threshold: 0.25,
          draw_boxes: true,
          active: true,
        })
      } catch (err) {
        onError?.(`Failed to bind device: ${err}`)
      } finally {
        setLoading(false)
      }
    }

    const handleUnbind = async (deviceId: string) => {
      setLoading(true)
      try {
        await executeCommand('unbind_device', { device_id: deviceId })
        await fetchBindings()
      } catch (err) {
        onError?.(`Failed to unbind device: ${err}`)
      } finally {
        setLoading(false)
      }
    }

    const handleToggle = async (deviceId: string, active: boolean) => {
      try {
        await executeCommand('toggle_binding', { device_id: deviceId, active })
        await fetchBindings()
      } catch (err) {
        onError?.(`Failed to toggle binding: ${err}`)
      }
    }

    const formatTime = (ts: number | null) => {
      if (!ts) return 'Never'
      return new Date(ts).toLocaleString()
    }

    const modelLoaded = status.model_loaded as boolean

    return (
      <div ref={ref} className="dbc">
        <div className="dbc-card">
          <div className="dbc-header">
            <h3 className="dbc-title">{title}</h3>
            <div className="dbc-status-badge">
              <span className={`dbc-status-dot ${modelLoaded ? 'dbc-status-dot-ready' : 'dbc-status-dot-error'}`} />
              {modelLoaded ? 'Model Ready' : 'Model Not Loaded'}
            </div>
          </div>

          {showStats && (
            <div className="dbc-stats-bar">
              <div className="dbc-stat">
                <span className="dbc-stat-label">Bound</span>
                <span className="dbc-stat-value">{bindings.length}</span>
              </div>
              <div className="dbc-stat">
                <span className="dbc-stat-label">Inferences</span>
                <span className="dbc-stat-value">{(status.total_inferences as number) || 0}</span>
              </div>
              <div className="dbc-stat">
                <span className="dbc-stat-label">Detections</span>
                <span className="dbc-stat-value">{(status.total_detections as number) || 0}</span>
              </div>
            </div>
          )}

          <div className="dbc-add-form">
            <h4 className="dbc-form-title">Add Device Binding</h4>
            <div className="dbc-form-row">
              <div className="dbc-form-group">
                <label className="dbc-label">Device ID</label>
                {devices.length > 0 ? (
                  <select
                    className="dbc-select"
                    value={newBinding.device_id}
                    onChange={(e) => setNewBinding({ ...newBinding, device_id: e.target.value })}
                  >
                    <option value="">Select device...</option>
                    {devices.map((d) => (
                      <option key={d.id} value={d.id}>{d.name} ({d.id})</option>
                    ))}
                  </select>
                ) : (
                  <input
                    className="dbc-input"
                    type="text"
                    placeholder="Enter device ID"
                    value={newBinding.device_id || ''}
                    onChange={(e) => setNewBinding({ ...newBinding, device_id: e.target.value })}
                  />
                )}
              </div>
              <div className="dbc-form-group">
                <label className="dbc-label">Image Metric</label>
                <input
                  className="dbc-input"
                  type="text"
                  placeholder="image"
                  value={newBinding.image_metric || ''}
                  onChange={(e) => setNewBinding({ ...newBinding, image_metric: e.target.value })}
                />
              </div>
              <div className="dbc-form-group">
                <label className="dbc-label">Confidence</label>
                <input
                  className="dbc-input"
                  style={{ width: '80px' }}
                  type="number"
                  min="0"
                  max="1"
                  step="0.05"
                  value={newBinding.confidence_threshold || 0.25}
                  onChange={(e) => setNewBinding({ ...newBinding, confidence_threshold: parseFloat(e.target.value) })}
                />
              </div>
            </div>
            <div className="dbc-form-row">
              <label className="dbc-checkbox-label">
                <input
                  type="checkbox"
                  checked={newBinding.draw_boxes ?? true}
                  onChange={(e) => setNewBinding({ ...newBinding, draw_boxes: e.target.checked })}
                />
                Draw detection boxes
              </label>
              <button
                className="dbc-btn dbc-btn-primary"
                onClick={handleBind}
                disabled={loading || !newBinding.device_id}
              >
                {loading ? 'Adding...' : 'Bind Device'}
              </button>
            </div>
          </div>

          <div className="dbc-bindings-list">
            <h4 className="dbc-form-title">Active Bindings ({bindings.length})</h4>
            {bindings.length === 0 ? (
              <div className="dbc-empty-state">
                No devices bound. Add a device above to start automatic inference.
              </div>
            ) : (
              bindings.map((bs) => (
                <div
                  key={bs.binding.device_id}
                  className={`dbc-binding-card ${bs.binding.active ? 'dbc-binding-card-active' : ''}`}
                >
                  <div
                    className="dbc-binding-header"
                    onClick={() => setExpandedBinding(
                      expandedBinding === bs.binding.device_id ? null : bs.binding.device_id
                    )}
                  >
                    <div className="dbc-binding-info">
                      <span className="dbc-device-id">{bs.binding.device_id}</span>
                      <span className="dbc-metric-name">→ {bs.binding.image_metric}</span>
                    </div>
                    <div className="dbc-binding-actions">
                      <span className={`dbc-active-badge ${bs.binding.active ? 'dbc-active-badge-on' : 'dbc-active-badge-off'}`}>
                        {bs.binding.active ? 'Active' : 'Paused'}
                      </span>
                      <span className="dbc-stat-value" style={{ fontSize: '12px' }}>
                        {bs.total_detections} detections
                      </span>
                    </div>
                  </div>

                  {expandedBinding === bs.binding.device_id && (
                    <div className="dbc-binding-details">
                      <div className="dbc-detail-row">
                        <span>Last inference:</span>
                        <span>{formatTime(bs.last_inference)}</span>
                      </div>
                      <div className="dbc-detail-row">
                        <span>Total inferences:</span>
                        <span>{bs.total_inferences}</span>
                      </div>
                      <div className="dbc-detail-row">
                        <span>Confidence:</span>
                        <span>{(bs.binding.confidence_threshold * 100).toFixed(0)}%</span>
                      </div>
                      {bs.last_error && (
                        <div className="dbc-error-row">
                          Error: {bs.last_error}
                        </div>
                      )}
                      <div className="dbc-detail-actions">
                        <button
                          className="dbc-btn"
                          onClick={() => handleToggle(bs.binding.device_id, !bs.binding.active)}
                        >
                          {bs.binding.active ? 'Pause' : 'Resume'}
                        </button>
                        <button
                          className="dbc-btn dbc-btn-danger"
                          onClick={() => handleUnbind(bs.binding.device_id)}
                        >
                          Unbind
                        </button>
                      </div>
                    </div>
                  )}
                </div>
              ))
            )}
          </div>
        </div>
      </div>
    )
  }
)

DeviceBindingCard.displayName = 'DeviceBindingCard'

export default DeviceBindingCard
