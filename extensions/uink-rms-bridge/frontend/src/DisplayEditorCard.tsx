import { forwardRef, useEffect, useState, useCallback, useRef } from 'react'
import { injectStyles } from './styles'
import {
  listDevices, getDisplay, getDisplaySize,
  DeviceInfo,
} from './api'
import { EditModalContent } from './EditModal'

export interface ExtensionComponentProps {
  title?: string
  className?: string
  config?: Record<string, any>
  /** Open a fullscreen dialog with arbitrary React content (provided by host) */
  openFullscreen?: (content: React.ReactNode) => void
  /** Close the fullscreen dialog (provided by host) */
  closeFullscreen?: () => void
}

export const DisplayEditorCard = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function DisplayEditorCard(props, ref) {
    const { className = '', config } = props
    const extensionId = config?.extensionId || 'uink-rms-bridge'

    useEffect(() => { injectStyles() }, [])

    const [devices, setDevices] = useState<DeviceInfo[]>([])
    const [selectedId, setSelectedId] = useState<string | null>(null)
    const [device, setDevice] = useState<DeviceInfo | null>(null)
    const [displaySize, setDisplaySize] = useState<{ width: number; height: number } | null>(null)
    const [previewUrl, setPreviewUrl] = useState<string | null>(null)
    const [loading, setLoading] = useState(true)
    const [error, setError] = useState<string | null>(null)
    const [toast, setToast] = useState<{ msg: string; type: 'success' | 'error' } | null>(null)
    const mountedRef = useRef(true)

    useEffect(() => {
      mountedRef.current = true
      return () => { mountedRef.current = false }
    }, [])

    useEffect(() => {
      if (!toast) return
      const t = setTimeout(() => setToast(null), 2500)
      return () => clearTimeout(t)
    }, [toast])

    // Step 1: load device list once
    const loadDeviceList = useCallback(async () => {
      const listRes = await listDevices(extensionId)
      if (!mountedRef.current) return null
      if (!listRes.success || !listRes.data) {
        setError(listRes.error || 'Failed to load devices')
        setLoading(false)
        return null
      }
      const deviceList = listRes.data.devices || []
      if (deviceList.length === 0) {
        setError('No devices synced')
        setLoading(false)
        return null
      }
      setDevices(deviceList)
      return deviceList
    }, [extensionId])

    // Step 2: load preview for a specific device
    const loadDevicePreview = useCallback(async (deviceId: string) => {
      setPreviewUrl(null)
      setDisplaySize(null)

      const sizeRes = await getDisplaySize(deviceId, extensionId)
      if (!mountedRef.current) return
      if (sizeRes.success && sizeRes.data) {
        setDisplaySize({ width: sizeRes.data.width, height: sizeRes.data.height })
      }

      const displayRes = await getDisplay(deviceId, extensionId)
      if (!mountedRef.current) return
      if (displayRes.success && displayRes.data) {
        const slots = displayRes.data.slots || []
        if (slots.length > 0) {
          setPreviewUrl(slots[0].preview_url || slots[0].preview_thumbnail_url || null)
        }
      }
    }, [extensionId])

    // Initial load
    useEffect(() => {
      let cancelled = false
      setLoading(true)
      setError(null)

      loadDeviceList().then(deviceList => {
        if (cancelled || !deviceList) return
        // Pick initial device: previously selected > first
        const initial = selectedId
          ? deviceList.find(d => d.device_id === selectedId) || deviceList[0]
          : deviceList[0]
        setDevice(initial)
        setSelectedId(initial.device_id)

        loadDevicePreview(initial.device_id).then(() => {
          if (!cancelled) setLoading(false)
        })
      })

      return () => { cancelled = true }
    }, [extensionId])

    // Handle device switch
    const handleDeviceChange = useCallback(async (e: React.ChangeEvent<HTMLSelectElement>) => {
      const newId = e.target.value
      const target = devices.find(d => d.device_id === newId)
      if (!target) return
      setSelectedId(newId)
      setDevice(target)
      setPreviewUrl(null)
      await loadDevicePreview(target.device_id)
    }, [devices, loadDevicePreview])

    // Refresh current device preview
    const refreshPreview = useCallback(async () => {
      if (!device) return
      await loadDevicePreview(device.device_id)
    }, [device, loadDevicePreview])

    // Determine if the click is on the preview area (not overlay controls)
    const handlePreviewClick = useCallback((e: React.MouseEvent) => {
      // Don't open editor if clicking on overlay controls (select, etc.)
      const target = e.target as HTMLElement
      if (target.closest('.uink-overlay-controls')) return
      if (!device) return

      // Use host's fullscreen dialog if available
      if (props.openFullscreen) {
        props.openFullscreen(
          <EditModalContent
            deviceId={device.device_id}
            deviceWidth={displaySize?.width || 800}
            deviceHeight={displaySize?.height || 480}
            onClose={() => props.closeFullscreen?.()}
            onPushSuccess={() => refreshPreview()}
            extensionId={extensionId}
          />
        )
      }
    }, [device, displaySize, props.openFullscreen, props.closeFullscreen, refreshPreview, extensionId])

    return (
      <div ref={ref} className={`uink-root ${className}`}>
        <div className="uink-card">
          {loading && (
            <div className="uink-loading">
              <div className="uink-spinner" />
              <span>Loading...</span>
            </div>
          )}

          {!loading && error && (
            <div className="uink-error">
              <span>{error}</span>
              <button className="uink-btn uink-btn-ghost" onClick={async () => { setLoading(true); setError(null); const dl = await loadDeviceList(); if (dl && dl[0]) { setDevice(dl[0]); setSelectedId(dl[0].device_id); await loadDevicePreview(dl[0].device_id); } setLoading(false); }} style={{ marginTop: 4, fontSize: 11 }}>
                Retry
              </button>
            </div>
          )}

          {!loading && !error && device && (
            <div className="uink-preview" style={{ cursor: 'pointer' }} onClick={handlePreviewClick}>
              {previewUrl ? (
                <img src={previewUrl} alt="Screen" />
              ) : (
                <div className="uink-preview-placeholder">
                  <svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
                    <rect x="2" y="3" width="20" height="14" rx="2" />
                    <line x1="8" y1="21" x2="16" y2="21" />
                    <line x1="12" y1="17" x2="12" y2="21" />
                  </svg>
                  <span>No preview</span>
                </div>
              )}

              {/* Floating device info overlay */}
              <div className="uink-overlay">
                <div className="uink-overlay-text">
                  <span className={`uink-status-dot ${device.online ? '' : 'offline'}`} />
                  <span className="uink-device-name">{device.name || device.device_id}</span>
                </div>
                <button className="uink-refresh-btn" onClick={(e) => { e.stopPropagation(); refreshPreview() }} title="Refresh preview">
                  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <polyline points="23 4 23 10 17 10" />
                    <polyline points="1 20 1 14 7 14" />
                    <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15" />
                  </svg>
                </button>
                {displaySize && (
                  <span className="uink-resolution">{displaySize.width}&times;{displaySize.height}</span>
                )}
              </div>

              {/* Device selector overlay (bottom-right) */}
              {devices.length > 1 && (
                <div className="uink-overlay-controls">
                  <select className="uink-device-select" value={device.device_id} onChange={handleDeviceChange}>
                    {devices.map(d => (
                      <option key={d.device_id} value={d.device_id}>
                        {d.name || d.device_id}
                      </option>
                    ))}
                  </select>
                </div>
              )}

              {/* Hover edit hint */}
              <div className="uink-edit-hint">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
                  <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" />
                </svg>
                Click to edit
              </div>
            </div>
          )}

          {toast && (
            <div className={`uink-toast ${toast.type}`}>
              {toast.msg}
            </div>
          )}
        </div>
      </div>
    )
  }
)

DisplayEditorCard.displayName = 'DisplayEditorCard'
