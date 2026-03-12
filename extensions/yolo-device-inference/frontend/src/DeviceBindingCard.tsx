import React, { useState, useEffect, useCallback } from 'react';

// Types
export interface DeviceBinding {
  device_id: string;
  image_metric: string;
  result_metric_prefix: string;
  confidence_threshold: number;
  draw_boxes: boolean;
  active: boolean;
}

export interface BindingStatus {
  binding: DeviceBinding;
  last_inference: number | null;
  total_inferences: number;
  total_detections: number;
  last_error: string | null;
}

export interface Detection {
  label: string;
  confidence: number;
  bbox: {
    x: number;
    y: number;
    width: number;
    height: number;
  };
}

export interface InferenceResult {
  device_id: string;
  detections: Detection[];
  inference_time_ms: number;
  image_width: number;
  image_height: number;
  timestamp: number;
  annotated_image_base64: string | null;
}

export interface DeviceBindingCardProps {
  // Extension API
  executeCommand: (command: string, args: Record<string, unknown>) => Promise<unknown>;
  // Optional: device list for selection
  devices?: Array<{ id: string; name: string; metrics: string[] }>;
  // Optional: initial binding (for future use)
  // initialBinding?: BindingStatus;
  // Callbacks
  onBindingChange?: (bindings: BindingStatus[]) => void;
  onError?: (error: string) => void;
  // UI customization
  title?: string;
  showStats?: boolean;
}

export const DeviceBindingCard: React.FC<DeviceBindingCardProps> = ({
  executeCommand,
  devices = [],
  onBindingChange,
  onError,
  title = 'YOLO Device Inference',
  showStats = true,
}) => {
  // State
  const [bindings, setBindings] = useState<BindingStatus[]>([]);
  const [status, setStatus] = useState<Record<string, unknown>>({});
  const [loading, setLoading] = useState(false);
  const [newBinding, setNewBinding] = useState<Partial<DeviceBinding>>({
    device_id: '',
    image_metric: 'image',
    result_metric_prefix: 'yolo_',
    confidence_threshold: 0.25,
    draw_boxes: true,
    active: true,
  });
  const [expandedBinding, setExpandedBinding] = useState<string | null>(null);

  // Fetch bindings and status
  const fetchBindings = useCallback(async () => {
    try {
      const result = await executeCommand('get_bindings', {}) as { bindings: BindingStatus[] };
      setBindings(result.bindings || []);
      onBindingChange?.(result.bindings || []);
    } catch (err) {
      console.error('Failed to fetch bindings:', err);
      onError?.('Failed to fetch device bindings');
    }
  }, [executeCommand, onBindingChange, onError]);

  const fetchStatus = useCallback(async () => {
    try {
      const result = await executeCommand('get_status', {});
      setStatus(result as Record<string, unknown>);
    } catch (err) {
      console.error('Failed to fetch status:', err);
    }
  }, [executeCommand]);

  useEffect(() => {
    fetchBindings();
    fetchStatus();
  }, [fetchBindings, fetchStatus]);

  // Bind device
  const handleBind = async () => {
    if (!newBinding.device_id) {
      onError?.('Please enter a device ID');
      return;
    }

    setLoading(true);
    try {
      await executeCommand('bind_device', {
        device_id: newBinding.device_id,
        image_metric: newBinding.image_metric || 'image',
        result_metric_prefix: newBinding.result_metric_prefix || 'yolo_',
        confidence_threshold: newBinding.confidence_threshold || 0.25,
        draw_boxes: newBinding.draw_boxes ?? true,
      });
      await fetchBindings();
      setNewBinding({
        device_id: '',
        image_metric: 'image',
        result_metric_prefix: 'yolo_',
        confidence_threshold: 0.25,
        draw_boxes: true,
        active: true,
      });
    } catch (err) {
      onError?.(`Failed to bind device: ${err}`);
    } finally {
      setLoading(false);
    }
  };

  // Unbind device
  const handleUnbind = async (deviceId: string) => {
    setLoading(true);
    try {
      await executeCommand('unbind_device', { device_id: deviceId });
      await fetchBindings();
    } catch (err) {
      onError?.(`Failed to unbind device: ${err}`);
    } finally {
      setLoading(false);
    }
  };

  // Toggle binding
  const handleToggle = async (deviceId: string, active: boolean) => {
    try {
      await executeCommand('toggle_binding', { device_id: deviceId, active });
      await fetchBindings();
    } catch (err) {
      onError?.(`Failed to toggle binding: ${err}`);
    }
  };

  // Format timestamp
  const formatTime = (ts: number | null) => {
    if (!ts) return 'Never';
    return new Date(ts).toLocaleString();
  };

  // COCO class colors (for future use in drawing)
  // const getClassColor = (label: string) => {
  //   const colors: Record<string, string> = {
  //     person: '#FF6B6B',
  //     car: '#4ECDC4',
  //     dog: '#45B7D1',
  //     cat: '#96CEB4',
  //     bicycle: '#FFEAA7',
  //     motorcycle: '#DDA0DD',
  //     bus: '#98D8C8',
  //     truck: '#F7DC6F',
  //   };
  //   return colors[label] || '#A0A0A0';
  // };

  return (
    <div style={styles.container}>
      <div style={styles.header}>
        <h3 style={styles.title}>{title}</h3>
        <div style={styles.statusBadge}>
          <span style={{
            ...styles.statusDot,
            backgroundColor: (status.model_loaded as boolean) ? '#4CAF50' : '#f44336'
          }} />
          {(status.model_loaded as boolean) ? 'Model Ready' : 'Model Not Loaded'}
        </div>
      </div>

      {/* Stats Bar */}
      {showStats && (
        <div style={styles.statsBar}>
          <div style={styles.stat}>
            <span style={styles.statLabel}>Bound</span>
            <span style={styles.statValue}>{bindings.length}</span>
          </div>
          <div style={styles.stat}>
            <span style={styles.statLabel}>Inferences</span>
            <span style={styles.statValue}>{status.total_inferences as number || 0}</span>
          </div>
          <div style={styles.stat}>
            <span style={styles.statLabel}>Detections</span>
            <span style={styles.statValue}>{status.total_detections as number || 0}</span>
          </div>
        </div>
      )}

      {/* Add New Binding Form */}
      <div style={styles.addForm}>
        <h4 style={styles.formTitle}>Add Device Binding</h4>
        <div style={styles.formRow}>
          <div style={styles.formGroup}>
            <label style={styles.label}>Device ID</label>
            {devices.length > 0 ? (
              <select
                style={styles.select}
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
                style={styles.input}
                type="text"
                placeholder="Enter device ID"
                value={newBinding.device_id || ''}
                onChange={(e) => setNewBinding({ ...newBinding, device_id: e.target.value })}
              />
            )}
          </div>
          <div style={styles.formGroup}>
            <label style={styles.label}>Image Metric</label>
            <input
              style={styles.input}
              type="text"
              placeholder="image"
              value={newBinding.image_metric || ''}
              onChange={(e) => setNewBinding({ ...newBinding, image_metric: e.target.value })}
            />
          </div>
          <div style={styles.formGroup}>
            <label style={styles.label}>Confidence</label>
            <input
              style={{ ...styles.input, width: '80px' }}
              type="number"
              min="0"
              max="1"
              step="0.05"
              value={newBinding.confidence_threshold || 0.25}
              onChange={(e) => setNewBinding({ ...newBinding, confidence_threshold: parseFloat(e.target.value) })}
            />
          </div>
        </div>
        <div style={styles.formRow}>
          <label style={styles.checkboxLabel}>
            <input
              type="checkbox"
              checked={newBinding.draw_boxes ?? true}
              onChange={(e) => setNewBinding({ ...newBinding, draw_boxes: e.target.checked })}
            />
            Draw detection boxes
          </label>
          <button
            style={{ ...styles.button, ...styles.primaryButton }}
            onClick={handleBind}
            disabled={loading || !newBinding.device_id}
          >
            {loading ? 'Adding...' : 'Bind Device'}
          </button>
        </div>
      </div>

      {/* Bindings List */}
      <div style={styles.bindingsList}>
        <h4 style={styles.formTitle}>Active Bindings ({bindings.length})</h4>
        {bindings.length === 0 ? (
          <div style={styles.emptyState}>
            No devices bound. Add a device above to start automatic inference.
          </div>
        ) : (
          bindings.map((status) => (
            <div
              key={status.binding.device_id}
              style={{
                ...styles.bindingCard,
                borderColor: status.binding.active ? '#4CAF50' : '#ccc'
              }}
            >
              <div
                style={styles.bindingHeader}
                onClick={() => setExpandedBinding(
                  expandedBinding === status.binding.device_id ? null : status.binding.device_id
                )}
              >
                <div style={styles.bindingInfo}>
                  <span style={styles.deviceId}>{status.binding.device_id}</span>
                  <span style={styles.metricName}>→ {status.binding.image_metric}</span>
                </div>
                <div style={styles.bindingActions}>
                  <span style={{
                    ...styles.activeBadge,
                    backgroundColor: status.binding.active ? '#4CAF50' : '#ccc'
                  }}>
                    {status.binding.active ? 'Active' : 'Paused'}
                  </span>
                  <span style={styles.statValue}>
                    {status.total_detections} detections
                  </span>
                </div>
              </div>
              
              {expandedBinding === status.binding.device_id && (
                <div style={styles.bindingDetails}>
                  <div style={styles.detailRow}>
                    <span>Last inference:</span>
                    <span>{formatTime(status.last_inference)}</span>
                  </div>
                  <div style={styles.detailRow}>
                    <span>Total inferences:</span>
                    <span>{status.total_inferences}</span>
                  </div>
                  <div style={styles.detailRow}>
                    <span>Confidence:</span>
                    <span>{(status.binding.confidence_threshold * 100).toFixed(0)}%</span>
                  </div>
                  {status.last_error && (
                    <div style={styles.errorRow}>
                      Error: {status.last_error}
                    </div>
                  )}
                  <div style={styles.detailActions}>
                    <button
                      style={styles.button}
                      onClick={() => handleToggle(status.binding.device_id, !status.binding.active)}
                    >
                      {status.binding.active ? 'Pause' : 'Resume'}
                    </button>
                    <button
                      style={{ ...styles.button, ...styles.dangerButton }}
                      onClick={() => handleUnbind(status.binding.device_id)}
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
  );
};

// Styles
const styles: Record<string, React.CSSProperties> = {
  container: {
    fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif',
    backgroundColor: '#fff',
    borderRadius: '8px',
    padding: '16px',
    boxShadow: '0 2px 8px rgba(0,0,0,0.1)',
    maxWidth: '500px',
  },
  header: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
    marginBottom: '16px',
  },
  title: {
    margin: 0,
    fontSize: '18px',
    fontWeight: 600,
  },
  statusBadge: {
    display: 'flex',
    alignItems: 'center',
    gap: '6px',
    fontSize: '12px',
    color: '#666',
  },
  statusDot: {
    width: '8px',
    height: '8px',
    borderRadius: '50%',
  },
  statsBar: {
    display: 'flex',
    gap: '24px',
    padding: '12px',
    backgroundColor: '#f5f5f5',
    borderRadius: '6px',
    marginBottom: '16px',
  },
  stat: {
    display: 'flex',
    flexDirection: 'column' as const,
    alignItems: 'center',
  },
  statLabel: {
    fontSize: '11px',
    color: '#666',
    textTransform: 'uppercase' as const,
  },
  statValue: {
    fontSize: '18px',
    fontWeight: 600,
    color: '#333',
  },
  addForm: {
    padding: '12px',
    backgroundColor: '#fafafa',
    borderRadius: '6px',
    marginBottom: '16px',
  },
  formTitle: {
    margin: '0 0 12px 0',
    fontSize: '14px',
    fontWeight: 600,
    color: '#333',
  },
  formRow: {
    display: 'flex',
    gap: '12px',
    alignItems: 'flex-end',
    marginBottom: '12px',
  },
  formGroup: {
    flex: 1,
    display: 'flex',
    flexDirection: 'column' as const,
    gap: '4px',
  },
  label: {
    fontSize: '12px',
    color: '#666',
  },
  input: {
    padding: '8px 12px',
    border: '1px solid #ddd',
    borderRadius: '4px',
    fontSize: '14px',
  },
  select: {
    padding: '8px 12px',
    border: '1px solid #ddd',
    borderRadius: '4px',
    fontSize: '14px',
    backgroundColor: '#fff',
  },
  checkboxLabel: {
    display: 'flex',
    alignItems: 'center',
    gap: '8px',
    fontSize: '13px',
    color: '#666',
    flex: 1,
  },
  button: {
    padding: '8px 16px',
    border: '1px solid #ddd',
    borderRadius: '4px',
    backgroundColor: '#fff',
    fontSize: '13px',
    cursor: 'pointer',
  },
  primaryButton: {
    backgroundColor: '#2196F3',
    color: '#fff',
    border: 'none',
  },
  dangerButton: {
    color: '#f44336',
    borderColor: '#f44336',
  },
  bindingsList: {
    marginTop: '8px',
  },
  emptyState: {
    padding: '24px',
    textAlign: 'center' as const,
    color: '#999',
    fontSize: '13px',
  },
  bindingCard: {
    border: '1px solid',
    borderRadius: '6px',
    marginBottom: '8px',
    overflow: 'hidden',
  },
  bindingHeader: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
    padding: '12px',
    cursor: 'pointer',
  },
  bindingInfo: {
    display: 'flex',
    flexDirection: 'column' as const,
    gap: '2px',
  },
  deviceId: {
    fontWeight: 600,
    fontSize: '14px',
  },
  metricName: {
    fontSize: '12px',
    color: '#666',
  },
  bindingActions: {
    display: 'flex',
    alignItems: 'center',
    gap: '12px',
  },
  activeBadge: {
    padding: '2px 8px',
    borderRadius: '10px',
    fontSize: '11px',
    color: '#fff',
  },
  bindingDetails: {
    padding: '12px',
    borderTop: '1px solid #eee',
    backgroundColor: '#fafafa',
  },
  detailRow: {
    display: 'flex',
    justifyContent: 'space-between',
    padding: '4px 0',
    fontSize: '13px',
  },
  errorRow: {
    padding: '8px',
    backgroundColor: '#ffebee',
    borderRadius: '4px',
    color: '#c62828',
    fontSize: '12px',
    marginTop: '8px',
  },
  detailActions: {
    display: 'flex',
    gap: '8px',
    marginTop: '12px',
  },
};

export default DeviceBindingCard;
