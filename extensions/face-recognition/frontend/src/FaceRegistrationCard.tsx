/**
 * FaceRegistrationCard — Standalone face registration dialog component.
 * Supports drag-and-drop image upload, Chinese localized error messages,
 * and can be used independently or embedded in FaceRecognitionCard.
 */

import { useState, useRef, useCallback, useEffect } from 'react'

// ============================================================================
// Types
// ============================================================================

export interface FaceRegistrationCardProps {
  extensionId: string
  onRegistered?: () => void
  onClose?: () => void
}

// ============================================================================
// API helpers (duplicated from index.tsx to keep the component standalone)
// ============================================================================

const getApiHeaders = (): Record<string, string> => {
  const token =
    localStorage.getItem('neomind_token') ||
    sessionStorage.getItem('neomind_token_session')
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
    const res = await fetch(
      `${getApiBase()}/extensions/${extensionId}/command`,
      {
        method: 'POST',
        headers: getApiHeaders(),
        body: JSON.stringify({ command, args }),
      }
    )
    if (!res.ok) return { success: false, error: `HTTP ${res.status}` }
    return res.json()
  } catch (e) {
    return {
      success: false,
      error: e instanceof Error ? e.message : 'Network error',
    }
  }
}

async function registerFace(
  extensionId: string,
  name: string,
  imageBase64: string
): Promise<{ success: boolean; error?: string; error_code?: string }> {
  return executeCommand(extensionId, 'register_face', {
    name,
    image: imageBase64,
  })
}

// ============================================================================
// Error code mapping
// ============================================================================

const ERROR_MESSAGES: Record<string, string> = {
  DUPLICATE_NAME: '姓名已存在',
  MULTIPLE_FACES: '图片包含多张人脸',
  NO_FACE_DETECTED: '未检测到人脸',
  IMAGE_TOO_LARGE: '图片过大（最大10MB）',
  MAX_FACES_EXCEEDED: '人脸库已满',
  MODEL_NOT_LOADED: '模型未加载',
}

function mapErrorMessage(errorCode?: string, fallback?: string): string {
  if (errorCode && ERROR_MESSAGES[errorCode]) return ERROR_MESSAGES[errorCode]
  return fallback || '注册失败'
}

// ============================================================================
// Icons (inline SVG strokes — same set as index.tsx)
// ============================================================================

const ICON_PATHS: Record<string, string> = {
  upload:
    '<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/>',
  check: '<polyline points="20 6 9 17 4 12"/>',
  x: '<line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>',
}

const SvgIcon = ({
  name,
  className = '',
  style,
}: {
  name: string
  className?: string
  style?: React.CSSProperties
}) => (
  <svg
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
    className={className}
    style={style}
    dangerouslySetInnerHTML={{ __html: ICON_PATHS[name] || ICON_PATHS.upload }}
  />
)

// ============================================================================
// Styles (scoped to this component, injected once)
// ============================================================================

const STYLE_ID = 'frc-reg-card-styles-v1'

const STYLES = `
.frc-reg-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: rgba(0,0,0,0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
  animation: frc-reg-fade-in 0.15s ease-out;
}
@keyframes frc-reg-fade-in {
  from { opacity: 0; }
  to { opacity: 1; }
}

.frc-reg-dialog {
  --frc-fg: hsl(240 10% 10%);
  --frc-muted: hsl(240 5% 45%);
  --frc-accent: hsl(210 80% 55%);
  --frc-card: rgba(255,255,255,0.85);
  --frc-border: rgba(0,0,0,0.08);
  --frc-hover: rgba(0,0,0,0.03);
  background: var(--frc-card);
  backdrop-filter: blur(16px);
  border: 1px solid var(--frc-border);
  border-radius: 12px;
  padding: 20px;
  width: 340px;
  display: flex;
  flex-direction: column;
  gap: 14px;
  animation: frc-reg-scale-in 0.15s ease-out;
  box-shadow: 0 8px 32px rgba(0,0,0,0.15);
}
@keyframes frc-reg-scale-in {
  from { transform: scale(0.95); opacity: 0; }
  to { transform: scale(1); opacity: 1; }
}
.dark .frc-reg-dialog {
  --frc-fg: hsl(0 0% 95%);
  --frc-muted: hsl(0 0% 60%);
  --frc-card: rgba(30,30,30,0.85);
  --frc-border: rgba(255,255,255,0.08);
  --frc-hover: rgba(255,255,255,0.03);
}

.frc-reg-title {
  font-size: 15px;
  font-weight: 600;
  color: var(--frc-fg);
}

.frc-reg-input {
  width: 100%;
  padding: 8px 12px;
  border: 1px solid var(--frc-border);
  border-radius: 6px;
  background: var(--frc-card);
  color: var(--frc-fg);
  font-size: 12px;
  box-sizing: border-box;
  transition: border-color 0.15s;
}
.frc-reg-input:focus {
  outline: none;
  border-color: var(--frc-accent);
  box-shadow: 0 0 0 2px rgba(210, 80, 55, 0.1);
}
.frc-reg-input::placeholder {
  color: var(--frc-muted);
}

.frc-reg-dropzone {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 20px 12px;
  border: 2px dashed var(--frc-border);
  border-radius: 8px;
  cursor: pointer;
  color: var(--frc-muted);
  font-size: 11px;
  text-align: center;
  transition: border-color 0.15s, background 0.15s, color 0.15s;
}
.frc-reg-dropzone:hover {
  border-color: var(--frc-accent);
  color: var(--frc-accent);
  background: var(--frc-hover);
}
.frc-reg-dropzone-active {
  border-color: var(--frc-accent);
  background: rgba(210, 80, 55, 0.05);
  color: var(--frc-accent);
}

.frc-reg-preview-container {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
}

.frc-reg-preview {
  width: 100%;
  max-height: 160px;
  object-fit: contain;
  border-radius: 6px;
  border: 1px solid var(--frc-border);
}

.frc-reg-change-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 4px 10px;
  border: 1px solid var(--frc-border);
  border-radius: 4px;
  background: transparent;
  color: var(--frc-muted);
  font-size: 10px;
  cursor: pointer;
  transition: all 0.15s;
}
.frc-reg-change-btn:hover {
  color: var(--frc-accent);
  border-color: var(--frc-accent);
}

.frc-reg-error {
  padding: 6px 10px;
  background: hsl(0 72% 51% / 0.1);
  border: 1px solid hsl(0 72% 51% / 0.2);
  border-radius: 6px;
  color: hsl(0 72% 51%);
  font-size: 11px;
}

.frc-reg-actions {
  display: flex;
  gap: 8px;
  justify-content: flex-end;
}

.frc-reg-btn {
  padding: 7px 16px;
  border: 1px solid var(--frc-border);
  border-radius: 6px;
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.2s;
  background: transparent;
  color: var(--frc-fg);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 5px;
}
.frc-reg-btn:hover {
  background: var(--frc-hover);
}
.frc-reg-btn-primary {
  background: var(--frc-accent);
  border-color: var(--frc-accent);
  color: #fff;
}
.frc-reg-btn-primary:hover {
  opacity: 0.9;
  background: var(--frc-accent);
}
.frc-reg-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.frc-reg-spinner {
  width: 14px;
  height: 14px;
  border: 2px solid rgba(255,255,255,0.3);
  border-top-color: #fff;
  border-radius: 50%;
  animation: frc-reg-spin 0.7s linear infinite;
}
@keyframes frc-reg-spin {
  to { transform: rotate(360deg); }
}
`

function injectStyles(): void {
  if (typeof document === 'undefined' || document.getElementById(STYLE_ID)) return
  const style = document.createElement('style')
  style.id = STYLE_ID
  style.textContent = STYLES
  document.head.appendChild(style)
}

// ============================================================================
// Component
// ============================================================================

export const FaceRegistrationCard = ({
  extensionId,
  onRegistered,
  onClose,
}: FaceRegistrationCardProps) => {
  const [name, setName] = useState('')
  const [imageData, setImageData] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [isDragging, setIsDragging] = useState(false)
  const fileInputRef = useRef<HTMLInputElement>(null)

  useEffect(() => injectStyles(), [])

  // ---- File handling ----

  const readFileAsDataUrl = useCallback((file: File): void => {
    if (!file.type.startsWith('image/')) {
      setError('请选择图片文件')
      return
    }
    const reader = new FileReader()
    reader.onload = () => {
      setImageData(reader.result as string)
      setError(null)
    }
    reader.onerror = () => setError('读取文件失败')
    reader.readAsDataURL(file)
  }, [])

  const handleFileChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0]
      if (file) readFileAsDataUrl(file)
    },
    [readFileAsDataUrl]
  )

  // ---- Drag-and-drop ----

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    e.stopPropagation()
    setIsDragging(true)
  }, [])

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    e.stopPropagation()
    setIsDragging(false)
  }, [])

  const handleDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault()
      e.stopPropagation()
      setIsDragging(false)
      const file = e.dataTransfer.files?.[0]
      if (file) readFileAsDataUrl(file)
    },
    [readFileAsDataUrl]
  )

  // ---- Registration ----

  const handleSubmit = useCallback(async () => {
    if (!name.trim() || !imageData) return
    setLoading(true)
    setError(null)

    const result = await registerFace(extensionId, name.trim(), imageData)

    if (result.success) {
      onRegistered?.()
      onClose?.()
    } else {
      setError(mapErrorMessage(result.error_code, result.error))
    }
    setLoading(false)
  }, [name, imageData, extensionId, onRegistered, onClose])

  // ---- Close ----

  const handleOverlayClick = useCallback(() => {
    onClose?.()
  }, [onClose])

  const handleDialogClick = useCallback(
    (e: React.MouseEvent) => e.stopPropagation(),
    []
  )

  const canSubmit = name.trim().length > 0 && imageData !== null && !loading

  return (
    <div className="frc-reg-overlay" onClick={handleOverlayClick}>
      <div className="frc-reg-dialog" onClick={handleDialogClick}>
        {/* Title */}
        <div className="frc-reg-title">注册人脸</div>

        {/* Name input */}
        <input
          className="frc-reg-input"
          type="text"
          placeholder="输入姓名"
          value={name}
          onChange={(e) => setName(e.target.value)}
          autoFocus
          aria-label="姓名"
        />

        {/* Hidden file input */}
        <input
          ref={fileInputRef}
          type="file"
          accept="image/*"
          style={{ display: 'none' }}
          onChange={handleFileChange}
          aria-hidden="true"
        />

        {/* Drop zone / preview */}
        {imageData ? (
          <div className="frc-reg-preview-container">
            <img className="frc-reg-preview" src={imageData} alt="预览" />
            <button
              className="frc-reg-change-btn"
              onClick={() => fileInputRef.current?.click()}
              type="button"
            >
              <SvgIcon name="upload" style={{ width: '12px', height: '12px' }} />
              更换图片
            </button>
          </div>
        ) : (
          <div
            className={`frc-reg-dropzone ${isDragging ? 'frc-reg-dropzone-active' : ''}`}
            onClick={() => fileInputRef.current?.click()}
            onDragOver={handleDragOver}
            onDragLeave={handleDragLeave}
            onDrop={handleDrop}
            role="button"
            tabIndex={0}
            aria-label="上传人脸图片"
            onKeyDown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault()
                fileInputRef.current?.click()
              }
            }}
          >
            <SvgIcon
              name="upload"
              style={{ width: '24px', height: '24px', opacity: 0.5 }}
            />
            <span>
              {isDragging ? '释放以上传图片' : '点击或拖拽图片到此处'}
            </span>
            <span style={{ fontSize: '10px', opacity: 0.6 }}>
              支持 JPG、PNG 格式，最大 10MB
            </span>
          </div>
        )}

        {/* Error display */}
        {error && <div className="frc-reg-error">{error}</div>}

        {/* Actions */}
        <div className="frc-reg-actions">
          <button className="frc-reg-btn" onClick={onClose} type="button">
            取消
          </button>
          <button
            className="frc-reg-btn frc-reg-btn-primary"
            onClick={handleSubmit}
            disabled={!canSubmit}
            type="button"
          >
            {loading ? (
              <>
                <div className="frc-reg-spinner" />
                注册中...
              </>
            ) : (
              <>
                <SvgIcon name="check" style={{ width: '14px', height: '14px' }} />
                注册
              </>
            )}
          </button>
        </div>
      </div>
    </div>
  )
}

FaceRegistrationCard.displayName = 'FaceRegistrationCard'

export default FaceRegistrationCard
