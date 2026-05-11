import { useRef, useEffect, useState, useCallback, forwardRef, useImperativeHandle } from 'react'

// ---- Types ----
export interface CanvasElement {
  id: string
  type: 'text' | 'image'
  x: number
  y: number
  width: number
  height: number
  content?: string
  fontSize?: number
  bold?: boolean
  imageSrc?: string
  _imageObj?: HTMLImageElement  // loaded Image object, not serialized
  rotation?: number  // degrees, default 0
}

export interface CanvasEditorHandle {
  exportAsBase64: () => string | null
  getElements: () => CanvasElement[]
}

interface CanvasEditorProps {
  width: number    // device resolution width
  height: number   // device resolution height
  elements: CanvasElement[]
  onElementsChange: (elements: CanvasElement[]) => void
  selectedId: string | null
  onSelectedChange: (id: string | null) => void
}

const HANDLE_SIZE = 6
const MIN_SIZE = 20

let _idCounter = 0
export function newId() { return 'el-' + (++_idCounter) }

export function createTextElement(x = 40, y = 40): CanvasElement {
  return {
    id: newId(), type: 'text',
    x, y, width: 200, height: 30,
    content: 'Text', fontSize: 18, bold: false,
  }
}

export function createImageElement(src: string, img: HTMLImageElement, x = 40, y = 40): CanvasElement {
  const aspect = img.naturalWidth / img.naturalHeight
  const w = Math.min(200, img.naturalWidth)
  const h = w / aspect
  return {
    id: newId(), type: 'image',
    x, y, width: Math.round(w), height: Math.round(h),
    imageSrc: src, _imageObj: img,
  }
}

/** Inverse-rotate a point into an element's local coordinate system */
function inverseRotatePoint(px: number, py: number, el: CanvasElement): { x: number; y: number } {
  const rotation = el.rotation || 0
  if (rotation === 0) return { x: px, y: py }
  const cx = el.x + el.width / 2
  const cy = el.y + el.height / 2
  const angle = -rotation * Math.PI / 180
  const dx = px - cx
  const dy = py - cy
  return {
    x: cx + dx * Math.cos(angle) - dy * Math.sin(angle),
    y: cy + dx * Math.sin(angle) + dy * Math.cos(angle),
  }
}

/** Draw an element with rotation transform applied */
function drawElementWithRotation(ctx: CanvasRenderingContext2D, el: CanvasElement, drawFn: () => void) {
  const rotation = el.rotation || 0
  if (rotation === 0) {
    drawFn()
    return
  }
  const cx = el.x + el.width / 2
  const cy = el.y + el.height / 2
  const angle = rotation * Math.PI / 180
  ctx.save()
  ctx.translate(cx, cy)
  ctx.rotate(angle)
  ctx.translate(-cx, -cy)
  drawFn()
  ctx.restore()
}

// ---- Component ----
export const CanvasEditor = forwardRef<CanvasEditorHandle, CanvasEditorProps>(
  function CanvasEditor({ width, height, elements, onElementsChange, selectedId, onSelectedChange }, ref) {
    const canvasRef = useRef<HTMLCanvasElement>(null)
    const [dragging, setDragging] = useState<{ type: 'move' | 'resize'; handleIdx: number; startX: number; startY: number; startEl: CanvasElement } | null>(null)
    const scaleRef = useRef(1)

    // Scale factor: fit device resolution into the displayed canvas
    useEffect(() => {
      const canvas = canvasRef.current
      if (!canvas) return
      const container = canvas.parentElement
      if (!container) return

      const maxW = container.clientWidth - 32
      const maxH = container.clientHeight - 32
      const scaleX = maxW / width
      const scaleY = maxH / height
      const s = Math.min(scaleX, scaleY, 1)
      scaleRef.current = s

      canvas.style.width = Math.round(width * s) + 'px'
      canvas.style.height = Math.round(height * s) + 'px'
      canvas.width = width
      canvas.height = height
    }, [width, height])

    // Load images for image elements
    useEffect(() => {
      elements.forEach(el => {
        if (el.type === 'image' && el.imageSrc && !el._imageObj) {
          const img = new Image()
          img.onload = () => {
            onElementsChange(elements.map(e => e.id === el.id ? { ...e, _imageObj: img } : e))
          }
          img.src = el.imageSrc
        }
      })
    }, [elements, onElementsChange])

    // Render elements (shared between display and export)
    const renderElements = useCallback((ctx: CanvasRenderingContext2D) => {
      ctx.fillStyle = '#ffffff'
      ctx.fillRect(0, 0, width, height)

      elements.forEach(el => {
        if (el.type === 'text' && el.content) {
          drawElementWithRotation(ctx, el, () => {
            const weight = el.bold ? 'bold' : 'normal'
            ctx.font = `${weight} ${el.fontSize || 18}px sans-serif`
            ctx.fillStyle = '#000000'
            ctx.textBaseline = 'top'
            const lines = wrapText(ctx, el.content!, el.width)
            lines.forEach((line, i) => {
              ctx.fillText(line, el.x, el.y + i * (el.fontSize || 18) * 1.2)
            })
          })
        } else if (el.type === 'image' && el._imageObj) {
          drawElementWithRotation(ctx, el, () => {
            ctx.drawImage(el._imageObj!, el.x, el.y, el.width, el.height)
          })
        }
      })
    }, [elements, width, height])

    // Render loop
    useEffect(() => {
      const canvas = canvasRef.current
      if (!canvas) return
      const ctx = canvas.getContext('2d')
      if (!ctx) return

      renderElements(ctx)

      // Draw selection
      if (selectedId) {
        const sel = elements.find(e => e.id === selectedId)
        if (sel) {
          drawElementWithRotation(ctx, sel, () => {
            ctx.strokeStyle = '#3b82f6'
            ctx.lineWidth = 2
            ctx.setLineDash([4, 3])
            ctx.strokeRect(sel.x - 1, sel.y - 1, sel.width + 2, sel.height + 2)
            ctx.setLineDash([])

            // Corner handles
            const handles = getHandles(sel)
            ctx.fillStyle = '#3b82f6'
            handles.forEach(h => {
              ctx.fillRect(h.x - HANDLE_SIZE / 2, h.y - HANDLE_SIZE / 2, HANDLE_SIZE, HANDLE_SIZE)
            })
          })
        }
      }
    }, [elements, selectedId, width, height, renderElements])

    // Export handle
    useImperativeHandle(ref, () => ({
      exportAsBase64: () => {
        const canvas = canvasRef.current
        if (!canvas) return null
        const ctx = canvas.getContext('2d')
        if (!ctx) return null
        // Ensure canvas is at device resolution before export
        canvas.width = width
        canvas.height = height
        renderElements(ctx)
        const dataUrl = canvas.toDataURL('image/png')
        return dataUrl.split(',')[1]
      },
      getElements: () => elements,
    }))

    // ---- Mouse handlers (in canvas coordinate space) ----
    const toCanvasCoords = useCallback((e: React.MouseEvent) => {
      const canvas = canvasRef.current!
      const rect = canvas.getBoundingClientRect()
      const s = scaleRef.current
      return {
        x: (e.clientX - rect.left) / s,
        y: (e.clientY - rect.top) / s,
      }
    }, [])

    const handleMouseDown = useCallback((e: React.MouseEvent) => {
      const { x, y } = toCanvasCoords(e)

      // Check resize handles on selected element first
      if (selectedId) {
        const sel = elements.find(el => el.id === selectedId)
        if (sel) {
          const local = inverseRotatePoint(x, y, sel)
          const handles = getHandles(sel)
          for (let i = 0; i < handles.length; i++) {
            if (Math.abs(local.x - handles[i].x) < HANDLE_SIZE && Math.abs(local.y - handles[i].y) < HANDLE_SIZE) {
              setDragging({ type: 'resize', handleIdx: i, startX: x, startY: y, startEl: { ...sel } })
              return
            }
          }
        }
      }

      // Hit test elements (reverse z-order), using inverse rotation
      for (let i = elements.length - 1; i >= 0; i--) {
        const el = elements[i]
        const local = inverseRotatePoint(x, y, el)
        if (local.x >= el.x && local.x <= el.x + el.width && local.y >= el.y && local.y <= el.y + el.height) {
          onSelectedChange(el.id)
          setDragging({ type: 'move', handleIdx: -1, startX: x, startY: y, startEl: { ...el } })
          return
        }
      }

      // Clicked on empty space
      onSelectedChange(null)
    }, [elements, selectedId, toCanvasCoords, onSelectedChange])

    const handleMouseMove = useCallback((e: React.MouseEvent) => {
      if (!dragging) return
      const { x, y } = toCanvasCoords(e)
      const dx = x - dragging.startX
      const dy = y - dragging.startY

      if (dragging.type === 'move') {
        onElementsChange(elements.map(el =>
          el.id === dragging.startEl.id
            ? { ...el, x: dragging.startEl.x + dx, y: dragging.startEl.y + dy }
            : el
        ))
      } else if (dragging.type === 'resize') {
        const se = dragging.startEl
        let nx = se.x, ny = se.y, nw = se.width, nh = se.height
        if (dragging.handleIdx === 0) { nx = se.x + dx; ny = se.y + dy; nw = se.width - dx; nh = se.height - dy }
        else if (dragging.handleIdx === 1) { ny = se.y + dy; nw = se.width + dx; nh = se.height - dy }
        else if (dragging.handleIdx === 2) { nw = se.width + dx; nh = se.height + dy }
        else if (dragging.handleIdx === 3) { nx = se.x + dx; nw = se.width - dx; nh = se.height + dy }
        if (nw < MIN_SIZE) { nw = MIN_SIZE }
        if (nh < MIN_SIZE) { nh = MIN_SIZE }

        onElementsChange(elements.map(el =>
          el.id === dragging.startEl.id ? { ...el, x: Math.round(nx), y: Math.round(ny), width: Math.round(nw), height: Math.round(nh) } : el
        ))
      }
    }, [dragging, elements, toCanvasCoords, onElementsChange])

    const handleMouseUp = useCallback(() => {
      setDragging(null)
    }, [])

    return (
      <canvas
        ref={canvasRef}
        onMouseDown={handleMouseDown}
        onMouseMove={handleMouseMove}
        onMouseUp={handleMouseUp}
        onMouseLeave={handleMouseUp}
      />
    )
  }
)

// ---- Helpers ----

function getHandles(el: CanvasElement) {
  return [
    { x: el.x, y: el.y },
    { x: el.x + el.width, y: el.y },
    { x: el.x + el.width, y: el.y + el.height },
    { x: el.x, y: el.y + el.height },
  ]
}

function wrapText(ctx: CanvasRenderingContext2D, text: string, maxWidth: number): string[] {
  const lines: string[] = []
  const paragraphs = text.split('\n')
  for (const para of paragraphs) {
    let line = ''
    for (const char of para) {
      const test = line + char
      if (ctx.measureText(test).width > maxWidth && line.length > 0) {
        lines.push(line)
        line = char
      } else {
        line = test
      }
    }
    lines.push(line)
  }
  return lines
}
