const EXTENSION_ID = 'uink-rms-bridge'

const getApiHeaders = () => {
  const token = localStorage.getItem('neomind_token') || sessionStorage.getItem('neomind_token_session')
  const headers: Record<string, string> = { 'Content-Type': 'application/json' }
  if (token) headers['Authorization'] = `Bearer ${token}`
  return headers
}

const getApiBase = () => (window as any).__TAURI__ ? 'http://localhost:9375/api' : '/api'

export interface CommandResult<T = any> {
  success: boolean
  data?: T
  error?: string
}

export async function executeCommand<T = any>(
  command: string,
  args: Record<string, any> = {},
  extensionId: string = EXTENSION_ID
): Promise<CommandResult<T>> {
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

export interface DeviceInfo {
  device_id: string
  name: string
  online: boolean
}

export interface DisplaySlot {
  image_id: string
  preview_url: string
  preview_thumbnail_url: string
  is_pending: boolean
  pending_preview_url: string
  pending_preview_thumbnail_url: string
  refresh_count: number
}

// ---- Read commands ----

export const listDevices = (extId?: string) =>
  executeCommand<{ count: number; devices: DeviceInfo[] }>('list_devices', {}, extId)

export const getDisplaySize = (deviceId: string, extId?: string) =>
  executeCommand<{ width: number; height: number }>('get_display_size', { device_id: deviceId }, extId)

export const getDisplay = (deviceId: string, extId?: string) =>
  executeCommand<{ slots: DisplaySlot[] }>('get_display', { device_id: deviceId }, extId)

// ---- Push commands (match all backend modes) ----

/** Push text/markdown content — backend auto-converts to image */
export const pushTextContent = (
  deviceId: string,
  contentType: 'text' | 'markdown',
  content: string,
  extId?: string
) => executeCommand('push_content', {
  device_id: deviceId,
  content_type: contentType,
  content,
}, extId)

/** Push base64-encoded image via push_content */
export const pushBase64Image = (deviceId: string, base64Image: string, extId?: string) =>
  executeCommand('push_content', {
    device_id: deviceId,
    content_type: 'image',
    content: base64Image,
    dither_algorithm: 'floyd-steinberg',
    resize_mode: 'fit',
  }, extId)

/** Push image from URL — backend downloads it */
export const pushImageUrl = (deviceId: string, imageUrl: string, extId?: string) =>
  executeCommand('push_image', {
    device_id: deviceId,
    image_url: imageUrl,
  }, extId)

/** Push raw base64 image via push_image (with optional processing params) */
export const pushRawImage = (
  deviceId: string,
  base64Image: string,
  extId?: string,
  dither?: string,
  resize?: string,
) => executeCommand('push_image', {
  device_id: deviceId,
  image_base64: base64Image,
  ...(dither ? { dither_algorithm: dither } : {}),
  ...(resize ? { resize_mode: resize } : {}),
}, extId)
