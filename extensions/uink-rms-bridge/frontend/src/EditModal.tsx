import { useState, useRef, useCallback, useEffect } from 'react'
import { CanvasEditor, CanvasElement, CanvasEditorHandle, createTextElement, createImageElement } from './Canvas'
import { pushTextContent, pushBase64Image, pushImageUrl } from './api'

type EditTab = 'text' | 'image' | 'canvas'

export interface EditModalContentProps {
  deviceId: string
  deviceWidth: number
  deviceHeight: number
  onClose: () => void
  onPushSuccess: () => void
  extensionId: string
}

const TABS: { key: EditTab; label: string; icon: string }[] = [
  { key: 'text', label: 'Text', icon: '<path d="M4 7V4h16v3"/><path d="M9 20h6"/><path d="M12 4v16"/>' },
  { key: 'image', label: 'Image', icon: '<rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/>' },
  { key: 'canvas', label: 'Canvas', icon: '<rect x="3" y="3" width="18" height="18" rx="2"/><path d="M3 9h18"/><path d="M9 21V9"/>' },
]

export function EditModalContent({ deviceId, deviceWidth, deviceHeight, onClose, onPushSuccess, extensionId }: EditModalContentProps) {
  const [tab, setTab] = useState<EditTab>('text')
  const [pushing, setPushing] = useState(false)
  const [toast, setToast] = useState<{ msg: string; type: 'success' | 'error' } | null>(null)

  // Text tab
  const [textContent, setTextContent] = useState('')
  const [textType, setTextType] = useState<'text' | 'markdown'>('markdown')

  // Image tab (merged upload + URL)
  const [uploadPreview, setUploadPreview] = useState<string | null>(null)
  const [uploadBase64, setUploadBase64] = useState<string | null>(null)
  const [imageUrl, setImageUrl] = useState('')
  const [dragOver, setDragOver] = useState(false)
  const fileInputRef = useRef<HTMLInputElement>(null)

  // Canvas tab
  const [elements, setElements] = useState<CanvasElement[]>([])
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const canvasRef = useRef<CanvasEditorHandle>(null)
  const canvasFileRef = useRef<HTMLInputElement>(null)
  const [flipH, setFlipH] = useState(false)
  const [flipV, setFlipV] = useState(false)

  // Toast
  useEffect(() => {
    if (!toast) return
    const t = setTimeout(() => setToast(null), 3000)
    return () => clearTimeout(t)
  }, [toast])

  const showToast = useCallback((msg: string, type: 'success' | 'error') => {
    setToast({ msg, type })
  }, [])

  // Push
  const doPush = useCallback(async (label: string, fn: () => Promise<{ success: boolean; error?: string }>) => {
    setPushing(true)
    const result = await fn()
    setPushing(false)
    if (result.success) {
      showToast('Pushed to device!', 'success')
      setTimeout(() => { onPushSuccess(); onClose() }, 800)
    } else {
      showToast(result.error || `${label} failed`, 'error')
    }
  }, [onClose, onPushSuccess, showToast])

  const handlePush = useCallback(() => {
    if (tab === 'text') {
      if (!textContent.trim()) { showToast('Enter some text first', 'error'); return }
      doPush('Text', () => pushTextContent(deviceId, textType, textContent, extensionId))
    } else if (tab === 'image') {
      if (uploadBase64) {
        doPush('Upload', () => pushBase64Image(deviceId, uploadBase64, extensionId))
      } else if (imageUrl.trim()) {
        doPush('URL', () => pushImageUrl(deviceId, imageUrl.trim(), extensionId))
      } else {
        showToast('Upload an image or enter a URL first', 'error')
      }
    } else {
      if (!canvasRef.current) return
      const base64 = canvasRef.current.exportAsBase64()
      if (!base64) { showToast('Canvas export failed', 'error'); return }
      doPush('Canvas', () => pushBase64Image(deviceId, base64, extensionId))
    }
  }, [tab, textContent, textType, uploadBase64, imageUrl, deviceId, extensionId, doPush, showToast])

  // File upload handler
  const readFile = useCallback((file: File, callback: (dataUrl: string, base64: string) => void) => {
    const reader = new FileReader()
    reader.onload = () => {
      const dataUrl = reader.result as string
      callback(dataUrl, dataUrl.split(',')[1])
    }
    reader.readAsDataURL(file)
  }, [])

  const handleFileSelected = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file) return
    readFile(file, (dataUrl, base64) => {
      setUploadPreview(dataUrl)
      setUploadBase64(base64)
    })
    e.target.value = ''
  }, [readFile])

  // Drag & drop
  const handleDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    setDragOver(false)
    const file = e.dataTransfer.files?.[0]
    if (file && file.type.startsWith('image/')) {
      readFile(file, (dataUrl, base64) => {
        setUploadPreview(dataUrl)
        setUploadBase64(base64)
      })
    }
  }, [readFile])

  // Canvas helpers
  const handleCanvasAddImage = useCallback(() => { canvasFileRef.current?.click() }, [])

  const handleCanvasFileSelected = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file) return
    const reader = new FileReader()
    reader.onload = () => {
      const dataUrl = reader.result as string
      const img = new Image()
      img.onload = () => {
        const el = createImageElement(dataUrl, img, 20 + Math.random() * 60, 20 + Math.random() * 60)
        setElements(prev => [...prev, el])
        setSelectedId(el.id)
      }
      img.src = dataUrl
    }
    reader.readAsDataURL(file)
    e.target.value = ''
  }, [])

  const handleCanvasDelete = useCallback(() => {
    if (!selectedId) return
    setElements(prev => prev.filter(e => e.id !== selectedId))
    setSelectedId(null)
  }, [selectedId])

  const handleElementChange = useCallback((id: string, changes: Partial<CanvasElement>) => {
    setElements(prev => prev.map(el => el.id === id ? { ...el, ...changes } : el))
  }, [])

  const selectedElement = elements.find(e => e.id === selectedId)

  return (
    <div style={{ display: 'flex', flexDirection: 'column', width: '100%', height: '100%', overflow: 'hidden' }}>
      {/* Tab bar */}
      <div style={{ display: 'flex', gap: 2, padding: '0 16px', borderBottom: '1px solid var(--border)' }}>
        {TABS.map(t => (
          <button
            key={t.key}
            onClick={() => setTab(t.key)}
            style={{
              display: 'flex', alignItems: 'center', gap: 5,
              padding: '8px 14px', fontSize: 13, fontWeight: 500,
              border: 'none', background: 'transparent',
              color: tab === t.key ? 'var(--primary)' : 'var(--muted-foreground)',
              cursor: 'pointer',
              borderBottom: tab === t.key ? '2px solid var(--primary)' : '2px solid transparent',
              transition: 'color 150ms, border-color 150ms',
            }}
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">{t.icon}</svg>
            {t.label}
          </button>
        ))}
        <span style={{ marginLeft: 'auto', alignSelf: 'center', fontSize: 11, color: 'var(--muted-foreground)' }}>
          {deviceWidth}&times;{deviceHeight}
        </span>
      </div>

      {/* Content area */}
      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', overflow: 'hidden', minHeight: 0 }}>
        {/* ---- TEXT TAB ---- */}
        {tab === 'text' && (
          <div style={{ flex: 1, display: 'flex', flexDirection: 'column', padding: '12px 16px', gap: 8 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
              {(['text', 'markdown'] as const).map(t => (
                <button
                  key={t}
                  onClick={() => setTextType(t)}
                  style={{
                    padding: '2px 10px', fontSize: 12,
                    border: `1px solid ${textType === t ? 'var(--primary)' : 'var(--border)'}`,
                    borderRadius: 6,
                    background: textType === t ? 'var(--primary)' : 'transparent',
                    color: textType === t ? 'var(--primary-foreground, #fff)' : 'var(--muted-foreground)',
                    cursor: 'pointer',
                  }}
                >
                  {t === 'text' ? 'Plain' : 'Markdown'}
                </button>
              ))}
              <span style={{ marginLeft: 'auto', fontSize: 11, color: 'var(--muted-foreground)' }}>{textContent.length}</span>
            </div>
            <textarea
              value={textContent}
              onChange={e => setTextContent(e.target.value)}
              placeholder={textType === 'markdown' ? '# Heading\n\nWrite **markdown** here...' : 'Type your text here...'}
              autoFocus
              style={{
                flex: 1, minHeight: 120, padding: 10,
                fontSize: 13, fontFamily: 'inherit',
                border: '1px solid var(--border)', borderRadius: 8,
                background: 'transparent', color: 'var(--foreground)',
                resize: 'none', outline: 'none',
              }}
            />
          </div>
        )}

        {/* ---- IMAGE TAB (merged Upload + URL) ---- */}
        {tab === 'image' && (
          <div
            onDragOver={e => { e.preventDefault(); setDragOver(true) }}
            onDragLeave={() => setDragOver(false)}
            onDrop={handleDrop}
            style={{ flex: 1, display: 'flex', flexDirection: 'column', padding: 16, gap: 10, overflow: 'auto' }}
          >
            <input ref={fileInputRef} type="file" accept="image/*" style={{ display: 'none' }} onChange={handleFileSelected} />

            {/* Upload area */}
            {uploadPreview ? (
              <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 8 }}>
                <img src={uploadPreview} alt="Preview" style={{ maxWidth: '100%', maxHeight: 200, objectFit: 'contain', borderRadius: 8 }} />
                <div style={{ display: 'flex', gap: 6 }}>
                  <button onClick={() => fileInputRef.current?.click()} style={{ padding: '4px 12px', fontSize: 12, border: '1px solid var(--border)', borderRadius: 6, background: 'transparent', color: 'var(--foreground)', cursor: 'pointer' }}>Replace</button>
                  <button onClick={() => { setUploadPreview(null); setUploadBase64(null) }} style={{ padding: '4px 12px', fontSize: 12, border: '1px solid var(--border)', borderRadius: 6, background: 'transparent', color: 'var(--foreground)', cursor: 'pointer' }}>Remove</button>
                </div>
              </div>
            ) : (
              <div
                onClick={() => fileInputRef.current?.click()}
                style={{
                  display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center',
                  gap: 10, width: '100%', minHeight: 140,
                  border: `2px dashed ${dragOver ? 'var(--primary)' : 'var(--border)'}`,
                  borderRadius: 10, color: 'var(--muted-foreground)', fontSize: 13, cursor: 'pointer',
                }}
              >
                <svg width="36" height="36" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
                  <rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/>
                </svg>
                <span>Drop image here or click to browse</span>
                <span style={{ fontSize: 10, opacity: 0.6 }}>PNG, JPG, GIF, WEBP</span>
              </div>
            )}

            {/* Divider */}
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <div style={{ flex: 1, height: 1, background: 'var(--border)' }} />
              <span style={{ fontSize: 11, color: 'var(--muted-foreground)', whiteSpace: 'nowrap' }}>or enter URL</span>
              <div style={{ flex: 1, height: 1, background: 'var(--border)' }} />
            </div>

            {/* URL input */}
            <input
              type="text"
              value={imageUrl}
              onChange={e => setImageUrl(e.target.value)}
              placeholder="https://example.com/image.png"
              style={{
                padding: '8px 10px', fontSize: 13,
                border: '1px solid var(--border)', borderRadius: 8,
                background: 'transparent', color: 'var(--foreground)', width: '100%', outline: 'none',
              }}
            />
            {imageUrl && !uploadPreview && (
              <div style={{ display: 'flex', justifyContent: 'center' }}>
                <img src={imageUrl} alt="Preview" onError={e => { (e.target as HTMLImageElement).style.display = 'none' }} style={{ maxWidth: '100%', maxHeight: 160, objectFit: 'contain', borderRadius: 8 }} />
              </div>
            )}
          </div>
        )}

        {/* ---- CANVAS TAB ---- */}
        {tab === 'canvas' && (
          <>
            <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', padding: 12, overflow: 'auto', background: 'var(--muted)', position: 'relative', minHeight: 0 }}>
              <CanvasEditor ref={canvasRef} width={deviceWidth} height={deviceHeight} elements={elements} onElementsChange={setElements} selectedId={selectedId} onSelectedChange={setSelectedId} flipH={flipH} flipV={flipV} />
              {selectedElement && (
                <div style={{ position: 'absolute', top: 8, right: 8, width: 200, background: 'var(--card)', border: '1px solid var(--border)', borderRadius: 8, boxShadow: 'var(--shadow-lg)', zIndex: 5, overflow: 'hidden' }}>
                  <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', padding: '6px 10px', borderBottom: '1px solid var(--border)', fontSize: 11, fontWeight: 600 }}>
                    <span>{selectedElement.type === 'text' ? 'Text' : 'Image'}</span>
                    <button onClick={() => setSelectedId(null)} style={{ border: 'none', background: 'transparent', color: 'var(--muted-foreground)', cursor: 'pointer', display: 'flex', alignItems: 'center' }}>
                      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
                    </button>
                  </div>
                  {selectedElement.type === 'text' && (
                    <div style={{ padding: '8px 10px', display: 'flex', flexDirection: 'column', gap: 6 }}>
                      <label style={{ display: 'flex', flexDirection: 'column', gap: 3, fontSize: 10, color: 'var(--muted-foreground)' }}>
                        <span>Content</span>
                        <input type="text" value={selectedElement.content || ''} onChange={e => handleElementChange(selectedElement.id, { content: e.target.value })} placeholder="Enter text..." style={{ padding: '4px 6px', fontSize: 12, border: '1px solid var(--border)', borderRadius: 4, background: 'transparent', color: 'var(--foreground)' }} />
                      </label>
                      <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                        <label style={{ display: 'flex', flexDirection: 'column', gap: 3, fontSize: 10, color: 'var(--muted-foreground)' }}>
                          <span>Size</span>
                          <input type="number" value={selectedElement.fontSize || 18} min={8} max={120} onChange={e => handleElementChange(selectedElement.id, { fontSize: Number(e.target.value) })} style={{ width: 52, padding: '4px 6px', fontSize: 12, border: '1px solid var(--border)', borderRadius: 4, background: 'transparent', color: 'var(--foreground)' }} />
                        </label>
                        <label style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 4, fontSize: 10, color: 'var(--muted-foreground)' }}>
                          <input type="checkbox" checked={selectedElement.bold || false} onChange={e => handleElementChange(selectedElement.id, { bold: e.target.checked })} />
                          <span>Bold</span>
                        </label>
                      </div>
                      {/* Rotation control */}
                      <label style={{ display: 'flex', flexDirection: 'column', gap: 3, fontSize: 10, color: 'var(--muted-foreground)' }}>
                        <span>Rotation: {selectedElement.rotation || 0}&deg;</span>
                        <input type="range" min={0} max={360} value={selectedElement.rotation || 0} onChange={e => handleElementChange(selectedElement.id, { rotation: Number(e.target.value) })} style={{ width: '100%' }} />
                      </label>
                    </div>
                  )}
                  {selectedElement.type === 'image' && (
                    <div style={{ padding: '8px 10px', display: 'flex', flexDirection: 'column', gap: 6 }}>
                      <div style={{ fontSize: 11, color: 'var(--muted-foreground)' }}>
                        Size: {selectedElement.width} &times; {selectedElement.height}
                      </div>
                      {/* Rotation control */}
                      <label style={{ display: 'flex', flexDirection: 'column', gap: 3, fontSize: 10, color: 'var(--muted-foreground)' }}>
                        <span>Rotation: {selectedElement.rotation || 0}&deg;</span>
                        <input type="range" min={0} max={360} value={selectedElement.rotation || 0} onChange={e => handleElementChange(selectedElement.id, { rotation: Number(e.target.value) })} style={{ width: '100%' }} />
                      </label>
                    </div>
                  )}
                  <button onClick={handleCanvasDelete} style={{ display: 'block', width: 'calc(100% - 20px)', margin: '0 10px 8px', padding: '4px 0', fontSize: 11, color: 'var(--destructive)', background: 'transparent', border: '1px solid var(--destructive)', borderRadius: 4, cursor: 'pointer' }}>
                    Delete
                  </button>
                </div>
              )}
            </div>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 6, padding: '6px 16px', borderTop: '1px solid var(--border)' }}>
              <button onClick={() => { const el = createTextElement(20 + Math.random() * 60, 20 + Math.random() * 60); setElements(prev => [...prev, el]); setSelectedId(el.id) }} style={{ display: 'inline-flex', alignItems: 'center', gap: 4, padding: '4px 10px', fontSize: 11, border: '1px solid var(--border)', borderRadius: 6, background: 'transparent', color: 'var(--foreground)', cursor: 'pointer' }}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><path d="M4 7V4h16v3"/><path d="M9 20h6"/><path d="M12 4v16"/></svg>
                Text
              </button>
              <button onClick={handleCanvasAddImage} style={{ display: 'inline-flex', alignItems: 'center', gap: 4, padding: '4px 10px', fontSize: 11, border: '1px solid var(--border)', borderRadius: 6, background: 'transparent', color: 'var(--foreground)', cursor: 'pointer' }}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg>
                Image
              </button>
              <button onClick={handleCanvasDelete} disabled={!selectedId} style={{ display: 'inline-flex', alignItems: 'center', gap: 4, padding: '4px 10px', fontSize: 11, border: '1px solid var(--border)', borderRadius: 6, background: 'transparent', color: 'var(--foreground)', cursor: selectedId ? 'pointer' : 'not-allowed', opacity: selectedId ? 1 : 0.5 }}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
                Delete
              </button>
              <input ref={canvasFileRef} type="file" accept="image/*" style={{ display: 'none' }} onChange={handleCanvasFileSelected} />
              <div style={{ marginLeft: 'auto', display: 'flex', gap: 4 }}>
                <button onClick={() => setFlipH(f => !f)} title="Flip horizontal" style={{ display: 'inline-flex', alignItems: 'center', justifyContent: 'center', width: 28, height: 28, border: `1px solid ${flipH ? 'var(--primary)' : 'var(--border)'}`, borderRadius: 6, background: flipH ? 'rgba(59,130,246,0.1)' : 'transparent', color: flipH ? 'var(--primary)' : 'var(--foreground)', cursor: 'pointer' }}>
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><path d="M8 3H5a2 2 0 00-2 2v14a2 2 0 002 2h3"/><path d="M16 3h3a2 2 0 012 2v14a2 2 0 01-2 2h-3"/><line x1="12" y1="3" x2="12" y2="21"/></svg>
                </button>
                <button onClick={() => setFlipV(f => !f)} title="Flip vertical" style={{ display: 'inline-flex', alignItems: 'center', justifyContent: 'center', width: 28, height: 28, border: `1px solid ${flipV ? 'var(--primary)' : 'var(--border)'}`, borderRadius: 6, background: flipV ? 'rgba(59,130,246,0.1)' : 'transparent', color: flipV ? 'var(--primary)' : 'var(--foreground)', cursor: 'pointer' }}>
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><path d="M3 8V5a2 2 0 012-2h14a2 2 0 012 2v3"/><path d="M3 16v3a2 2 0 002 2h14a2 2 0 002-2v-3"/><line x1="3" y1="12" x2="21" y2="12"/></svg>
                </button>
              </div>
            </div>
          </>
        )}
      </div>

      {/* Footer */}
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'flex-end', gap: 8, padding: '10px 16px', borderTop: '1px solid var(--border)' }}>
        <button onClick={onClose} disabled={pushing} style={{ padding: '6px 16px', fontSize: 13, fontWeight: 500, border: '1px solid var(--border)', borderRadius: 8, background: 'transparent', color: 'var(--muted-foreground)', cursor: 'pointer' }}>
          Cancel
        </button>
        <button onClick={handlePush} disabled={pushing} style={{ padding: '6px 16px', fontSize: 13, fontWeight: 500, border: 'none', borderRadius: 8, background: 'var(--primary)', color: 'var(--primary-foreground, #fff)', cursor: 'pointer', display: 'flex', alignItems: 'center', gap: 6 }}>
          {pushing ? (
            <>Pushing...</>
          ) : (
            <>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><path d="M22 2L11 13"/><path d="M22 2L15 22L11 13L2 9L22 2Z"/></svg>
              Push to Device
            </>
          )}
        </button>
      </div>

      {toast && (
        <div style={{
          position: 'absolute', bottom: 12, left: '50%', transform: 'translateX(-50%)',
          padding: '5px 14px', borderRadius: 8, fontSize: 12, fontWeight: 500, zIndex: 10,
          background: toast.type === 'success' ? 'var(--color-success)' : 'var(--destructive)',
          color: '#fff', pointerEvents: 'none', whiteSpace: 'nowrap',
        }}>
          {toast.msg}
        </div>
      )}
    </div>
  )
}
