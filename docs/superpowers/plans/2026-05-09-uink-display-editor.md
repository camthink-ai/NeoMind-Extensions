# Uink Display Editor Card - Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a frontend component for uink-rms-bridge that displays e-paper screen content and provides a modal canvas editor for composing/pushing new content.

**Architecture:** Single React card component (`DisplayEditorCard`) that acts as a content viewer. Clicking "Edit" opens a fixed-position modal with an HTML Canvas editor. Canvas uses native 2D API for element rendering, hit-testing, and drag/resize. Exported as base64 PNG and pushed via `push_content` command.

**Tech Stack:** React 18 + TypeScript + Vite (UMD build), HTML Canvas 2D API, NeoMind CSS variables.

**Spec:** `docs/superpowers/specs/2026-05-09-uink-display-editor-design.md`

---

## File Map

| File | Responsibility | Create/Modify |
|------|---------------|---------------|
| `extensions/uink-rms-bridge/frontend/package.json` | NPM deps, scripts | Create |
| `extensions/uink-rms-bridge/frontend/tsconfig.json` | TypeScript config | Create |
| `extensions/uink-rms-bridge/frontend/vite.config.ts` | UMD build config | Create |
| `extensions/uink-rms-bridge/frontend/frontend.json` | Component manifest | Create |
| `extensions/uink-rms-bridge/frontend/src/styles.ts` | CSS constants + injectStyles | Create |
| `extensions/uink-rms-bridge/frontend/src/api.ts` | Extension command helpers | Create |
| `extensions/uink-rms-bridge/frontend/src/Canvas.tsx` | Canvas rendering & interaction | Create |
| `extensions/uink-rms-bridge/frontend/src/EditModal.tsx` | Modal overlay + toolbar + properties | Create |
| `extensions/uink-rms-bridge/frontend/src/DisplayEditorCard.tsx` | Card viewer component | Create |
| `extensions/uink-rms-bridge/frontend/src/index.tsx` | Entry point + exports | Create |

---

### Task 1: Project Scaffolding

**Files:**
- Create: `extensions/uink-rms-bridge/frontend/package.json`
- Create: `extensions/uink-rms-bridge/frontend/tsconfig.json`
- Create: `extensions/uink-rms-bridge/frontend/vite.config.ts`
- Create: `extensions/uink-rms-bridge/frontend/frontend.json`

- [ ] **Step 1: Create package.json**

```json
{
  "name": "@neomind/uink-rms-bridge-frontend",
  "version": "0.1.0",
  "description": "Uink RMS Bridge frontend component for NeoMind extension runtime",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc && vite build",
    "preview": "vite preview"
  },
  "dependencies": {
    "react": "^18.2.0",
    "react-dom": "^18.2.0"
  },
  "devDependencies": {
    "@types/react": "^18.2.0",
    "@types/react-dom": "^18.2.0",
    "@vitejs/plugin-react": "^4.2.0",
    "typescript": "^5.3.0",
    "vite": "^5.0.0"
  },
  "peerDependencies": {
    "react": ">=18.0.0",
    "react-dom": ">=18.0.0"
  }
}
```

- [ ] **Step 2: Create tsconfig.json**

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "useDefineForClassFields": true,
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "skipLibCheck": true,
    "moduleResolution": "bundler",
    "allowImportingTsExtensions": true,
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "jsx": "react-jsx",
    "strict": true,
    "noUnusedLocals": false,
    "noUnusedParameters": false,
    "noFallthroughCasesInSwitch": true,
    "baseUrl": ".",
    "paths": {
      "@/*": ["src/*"]
    }
  },
  "include": ["src"]
}
```

- [ ] **Step 3: Create vite.config.ts**

```typescript
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  define: {
    'process.env.NODE_ENV': JSON.stringify('production')
  },
  build: {
    lib: {
      entry: 'src/index.tsx',
      name: 'UinkRmsBridgeComponents',
      fileName: 'uink-rms-bridge-components',
      formats: ['umd']
    },
    rollupOptions: {
      external: ['react', 'react-dom', 'react/jsx-runtime'],
      output: {
        exports: 'named',
        globals: {
          react: 'React',
          'react-dom': 'ReactDOM',
          'react/jsx-runtime': 'jsxRuntime',
        },
      },
    },
    outDir: 'dist',
    emptyOutDir: true
  }
})
```

- [ ] **Step 4: Create frontend.json**

```json
{
  "id": "uink-rms-bridge",
  "version": "0.1.0",
  "entrypoint": "uink-rms-bridge-components.umd.cjs",
  "components": [
    {
      "name": "DisplayEditorCard",
      "type": "card",
      "displayName": "E-Paper Display",
      "description": "View and edit e-paper display content",
      "icon": "monitor",
      "defaultSize": { "width": 420, "height": 520 },
      "minSize": { "width": 320, "height": 400 },
      "maxSize": { "width": 600, "height": 700 },
      "refreshable": true,
      "refreshInterval": 30000,
      "configSchema": {
        "deviceId": {
          "type": "string",
          "description": "Default device ID",
          "default": ""
        }
      }
    }
  ],
  "i18n": {
    "defaultLanguage": "en",
    "supportedLanguages": ["en"]
  },
  "dependencies": {
    "react": ">=18.0.0"
  }
}
```

- [ ] **Step 5: Install dependencies**

Run: `cd extensions/uink-rms-bridge/frontend && npm install`

- [ ] **Step 6: Commit scaffolding**

```bash
git add extensions/uink-rms-bridge/frontend/
git commit -m "feat(uink-rms-bridge): scaffold frontend project structure"
```

---

### Task 2: Styles Module

**Files:**
- Create: `extensions/uink-rms-bridge/frontend/src/styles.ts`

This module defines all CSS as a const string and provides `injectStyles()` to append it to the document head exactly once.

- [ ] **Step 1: Create styles.ts**

```typescript
const CSS_ID = 'uink-editor-styles'

export const STYLES = `
/* Root variables with fallbacks */
.uink-editor-root {
  --uink-editor-fg: var(--foreground, #17172a);
  --uink-editor-muted: var(--muted-foreground, #6b7280);
  --uink-editor-card: var(--card, rgba(255,255,255,0.85));
  --uink-editor-border: var(--border, rgba(0,0,0,0.08));
  --uink-editor-accent: var(--primary, #17172a);
  --uink-editor-on-primary: var(--primary-foreground, #ffffff);
  --uink-editor-success: var(--color-success, oklch(0.55 0.17 155));
  --uink-editor-error: var(--color-error, oklch(0.55 0.22 25));
  --uink-editor-info: var(--color-info, oklch(0.52 0.15 250));
  --uink-editor-radius: var(--radius-lg, 10px);
  --uink-editor-radius-xl: var(--radius-xl, 12px);
  --uink-editor-shadow: var(--shadow-lg, 0 8px 32px rgba(0,0,0,0.12));
  --uink-editor-shadow-xl: var(--shadow-xl, 0 16px 48px rgba(0,0,0,0.18));
  width: 100%;
  height: 100%;
  font-size: 13px;
  box-sizing: border-box;
}
.dark .uink-editor-root {
  --uink-editor-fg: var(--foreground, #f0f0f0);
  --uink-editor-muted: var(--muted-foreground, #9ca3af);
  --uink-editor-card: var(--card, rgba(30,30,30,0.8));
  --uink-editor-border: var(--border, rgba(255,255,255,0.08));
  --uink-editor-accent: var(--primary, #f0f0f0);
  --uink-editor-on-primary: var(--primary-foreground, #17172a);
}
* { box-sizing: border-box; }

/* Card layout */
.uink-editor-card {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: 12px;
  background: var(--uink-editor-card);
  backdrop-filter: blur(12px);
  border: 1px solid var(--uink-editor-border);
  border-radius: var(--uink-editor-radius);
}

/* Header */
.uink-editor-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 8px;
  gap: 8px;
}
.uink-editor-device-name {
  font-weight: 600;
  font-size: 13px;
  color: var(--uink-editor-fg);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  flex: 1;
  min-width: 0;
}
.uink-editor-status {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  color: var(--uink-editor-muted);
  flex-shrink: 0;
}
.uink-editor-status-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--uink-editor-success);
}
.uink-editor-status-dot.offline {
  background: var(--uink-editor-muted);
}
.uink-editor-resolution {
  font-size: 11px;
  color: var(--uink-editor-muted);
  flex-shrink: 0;
}

/* Preview area */
.uink-editor-preview {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 0;
  overflow: hidden;
  border-radius: 6px;
  background: #fff;
  border: 1px solid var(--uink-editor-border);
  position: relative;
}
.uink-editor-preview img {
  max-width: 100%;
  max-height: 100%;
  object-fit: contain;
}
.uink-editor-preview-placeholder {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  color: var(--uink-editor-muted);
  font-size: 12px;
  text-align: center;
  padding: 16px;
}

/* Edit button */
.uink-editor-footer {
  display: flex;
  justify-content: center;
  margin-top: 10px;
}
.uink-editor-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 6px 16px;
  font-size: 12px;
  font-weight: 500;
  border: none;
  border-radius: 6px;
  cursor: pointer;
  transition: opacity 0.15s;
}
.uink-editor-btn:hover { opacity: 0.85; }
.uink-editor-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.uink-editor-btn-primary {
  background: var(--uink-editor-accent);
  color: var(--uink-editor-on-primary);
}
.uink-editor-btn-ghost {
  background: transparent;
  color: var(--uink-editor-muted);
  border: 1px solid var(--uink-editor-border);
}
.uink-editor-btn-danger {
  background: var(--uink-editor-error);
  color: #fff;
}

/* Loading / error states */
.uink-editor-loading,
.uink-editor-error {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  flex: 1;
  gap: 8px;
  color: var(--uink-editor-muted);
  font-size: 12px;
}
.uink-editor-spinner {
  width: 20px;
  height: 20px;
  border: 2px solid var(--uink-editor-border);
  border-top-color: var(--uink-editor-accent);
  border-radius: 50%;
  animation: uink-editor-spin 0.6s linear infinite;
}
@keyframes uink-editor-spin {
  to { transform: rotate(360deg); }
}

/* ======================== MODAL ======================== */
.uink-editor-modal-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.5);
  z-index: 9999;
  display: flex;
  align-items: center;
  justify-content: center;
  animation: uink-editor-fade-in 0.15s ease-out;
}
@keyframes uink-editor-fade-in {
  from { opacity: 0; }
  to { opacity: 1; }
}
.uink-editor-modal {
  background: var(--uink-editor-card);
  border-radius: var(--uink-editor-radius-xl);
  box-shadow: var(--uink-editor-shadow-xl);
  width: 720px;
  max-width: 92vw;
  max-height: 90vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  animation: uink-editor-scale-in 0.15s ease-out;
}
@keyframes uink-editor-scale-in {
  from { transform: scale(0.95); opacity: 0; }
  to { transform: scale(1); opacity: 1; }
}
.uink-editor-modal-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 16px;
  border-bottom: 1px solid var(--uink-editor-border);
  font-weight: 600;
  font-size: 14px;
  color: var(--uink-editor-fg);
}
.uink-editor-modal-close {
  width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--uink-editor-muted);
  cursor: pointer;
  border-radius: 6px;
  font-size: 18px;
}
.uink-editor-modal-close:hover {
  background: var(--uink-editor-border);
}

/* Canvas container */
.uink-editor-canvas-wrap {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 16px;
  overflow: auto;
  background: var(--uink-editor-muted);
  min-height: 200px;
}
.uink-editor-canvas-wrap canvas {
  background: #fff;
  box-shadow: 0 2px 12px rgba(0,0,0,0.15);
  cursor: crosshair;
  max-width: 100%;
  max-height: 100%;
}

/* Toolbar */
.uink-editor-toolbar {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 8px 16px;
  border-top: 1px solid var(--uink-editor-border);
  background: var(--uink-editor-card);
}
.uink-editor-toolbar-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 4px 10px;
  font-size: 11px;
  border: 1px solid var(--uink-editor-border);
  border-radius: 4px;
  background: transparent;
  color: var(--uink-editor-fg);
  cursor: pointer;
}
.uink-editor-toolbar-btn:hover {
  background: var(--uink-editor-border);
}
.uink-editor-toolbar-btn.active {
  background: var(--uink-editor-accent);
  color: var(--uink-editor-on-primary);
  border-color: var(--uink-editor-accent);
}

/* Property panel */
.uink-editor-props {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 16px;
  border-top: 1px solid var(--uink-editor-border);
  font-size: 12px;
  color: var(--uink-editor-fg);
}
.uink-editor-props label {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  color: var(--uink-editor-muted);
}
.uink-editor-props input[type="text"],
.uink-editor-props input[type="number"] {
  padding: 3px 6px;
  font-size: 12px;
  border: 1px solid var(--uink-editor-border);
  border-radius: 4px;
  background: transparent;
  color: var(--uink-editor-fg);
  width: auto;
}
.uink-editor-props input[type="text"] { flex: 1; min-width: 120px; }
.uink-editor-props input[type="number"] { width: 50px; }

/* Modal footer */
.uink-editor-modal-footer {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
  padding: 10px 16px;
  border-top: 1px solid var(--uink-editor-border);
}

/* Toast */
.uink-editor-toast {
  position: fixed;
  bottom: 24px;
  left: 50%;
  transform: translateX(-50%);
  padding: 8px 20px;
  border-radius: 8px;
  font-size: 12px;
  font-weight: 500;
  z-index: 10001;
  animation: uink-editor-toast-in 0.2s ease-out;
}
.uink-editor-toast.success {
  background: var(--uink-editor-success);
  color: #fff;
}
.uink-editor-toast.error {
  background: var(--uink-editor-error);
  color: #fff;
}
@keyframes uink-editor-toast-in {
  from { opacity: 0; transform: translateX(-50%) translateY(8px); }
  to { opacity: 1; transform: translateX(-50%) translateY(0); }
}
`

export function injectStyles() {
  if (typeof document === 'undefined' || document.getElementById(CSS_ID)) return
  const style = document.createElement('style')
  style.id = CSS_ID
  style.textContent = STYLES
  document.head.appendChild(style)
}
```

- [ ] **Step 2: Commit**

```bash
git add extensions/uink-rms-bridge/frontend/src/styles.ts
git commit -m "feat(uink-rms-bridge): add CSS styles module with NeoMind variable aliases"
```

---

### Task 3: API Helpers

**Files:**
- Create: `extensions/uink-rms-bridge/frontend/src/api.ts`

Extension command invocation helpers following the weather-forecast-v2 pattern.

- [ ] **Step 1: Create api.ts**

```typescript
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

export const listDevices = (extId?: string) =>
  executeCommand<{ count: number; devices: DeviceInfo[] }>('list_devices', {}, extId)

export const getDisplaySize = (deviceId: string, extId?: string) =>
  executeCommand<{ width: number; height: number }>('get_display_size', { device_id: deviceId }, extId)

export const getDisplay = (deviceId: string, extId?: string) =>
  executeCommand<{ slots: DisplaySlot[] }>('get_display', { device_id: deviceId }, extId)

export const pushContent = (deviceId: string, base64Image: string, extId?: string) =>
  executeCommand('push_content', {
    device_id: deviceId,
    content_type: 'image',
    content: base64Image,
    dither_algorithm: 'floyd-steinberg',
    resize_mode: 'fit',
  }, extId)
```

- [ ] **Step 2: Commit**

```bash
git add extensions/uink-rms-bridge/frontend/src/api.ts
git commit -m "feat(uink-rms-bridge): add API helpers for extension commands"
```

---

### Task 4: Canvas Component

**Files:**
- Create: `extensions/uink-rms-bridge/frontend/src/Canvas.tsx`

The core canvas editor with native 2D API. Handles rendering, hit-testing, dragging, resizing, text editing.

- [ ] **Step 1: Create Canvas.tsx**

```tsx
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

    // Render loop
    useEffect(() => {
      const canvas = canvasRef.current
      if (!canvas) return
      const ctx = canvas.getContext('2d')
      if (!ctx) return

      // White background
      ctx.fillStyle = '#ffffff'
      ctx.fillRect(0, 0, width, height)

      // Draw elements
      elements.forEach(el => {
        if (el.type === 'text' && el.content) {
          const weight = el.bold ? 'bold' : 'normal'
          ctx.font = `${weight} ${el.fontSize || 18}px sans-serif`
          ctx.fillStyle = '#000000'
          ctx.textBaseline = 'top'
          // Word-wrap within element width
          const lines = wrapText(ctx, el.content, el.width)
          lines.forEach((line, i) => {
            ctx.fillText(line, el.x, el.y + i * (el.fontSize || 18) * 1.2)
          })
        } else if (el.type === 'image' && el._imageObj) {
          ctx.drawImage(el._imageObj, el.x, el.y, el.width, el.height)
        }
      })

      // Draw selection
      if (selectedId) {
        const sel = elements.find(e => e.id === selectedId)
        if (sel) {
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
        }
      }
    }, [elements, selectedId, width, height])

    // Export handle
    useImperativeHandle(ref, () => ({
      exportAsBase64: () => {
        const canvas = canvasRef.current
        if (!canvas) return null
        // Temporarily deselect for clean export
        const prevSelected = selectedId
        onSelectedChange(null)
        // Re-render without selection
        const ctx = canvas.getContext('2d')
        if (!ctx) return null
        ctx.fillStyle = '#ffffff'
        ctx.fillRect(0, 0, width, height)
        elements.forEach(el => {
          if (el.type === 'text' && el.content) {
            const weight = el.bold ? 'bold' : 'normal'
            ctx.font = `${weight} ${el.fontSize || 18}px sans-serif`
            ctx.fillStyle = '#000000'
            ctx.textBaseline = 'top'
            const lines = wrapText(ctx, el.content, el.width)
            lines.forEach((line, i) => {
              ctx.fillText(line, el.x, el.y + i * (el.fontSize || 18) * 1.2)
            })
          } else if (el.type === 'image' && el._imageObj) {
            ctx.drawImage(el._imageObj, el.x, el.y, el.width, el.height)
          }
        })
        const dataUrl = canvas.toDataURL('image/png')
        // Restore selection
        onSelectedChange(prevSelected)
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
          const handles = getHandles(sel)
          for (let i = 0; i < handles.length; i++) {
            if (Math.abs(x - handles[i].x) < HANDLE_SIZE && Math.abs(y - handles[i].y) < HANDLE_SIZE) {
              setDragging({ type: 'resize', handleIdx: i, startX: x, startY: y, startEl: { ...sel } })
              return
            }
          }
        }
      }

      // Hit test elements (reverse z-order)
      for (let i = elements.length - 1; i >= 0; i--) {
        const el = elements[i]
        if (x >= el.x && x <= el.x + el.width && y >= el.y && y <= el.y + el.height) {
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
        // handleIdx: 0=TL, 1=TR, 2=BR, 3=BL
        if (dragging.handleIdx === 0) { nx = se.x + dx; ny = se.y + dy; nw = se.width - dx; nh = se.height - dy }
        else if (dragging.handleIdx === 1) { ny = se.y + dy; nw = se.width + dx; nh = se.height - dy }
        else if (dragging.handleIdx === 2) { nw = se.width + dx; nh = se.height + dy }
        else if (dragging.handleIdx === 3) { nx = se.x + dx; nw = se.width - dx; nh = se.height + dy }
        // Enforce minimum size
        if (nw < MIN_SIZE) { nw = MIN_SIZE; nx = se.x + se.width - MIN_SIZE * (dragging.handleIdx === 0 || dragging.handleIdx === 3 ? 1 : 0) }
        if (nh < MIN_SIZE) { nh = MIN_SIZE; ny = se.y + se.height - MIN_SIZE * (dragging.handleIdx === 0 || dragging.handleIdx === 1 ? 1 : 0) }

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
    { x: el.x, y: el.y },                     // TL
    { x: el.x + el.width, y: el.y },           // TR
    { x: el.x + el.width, y: el.y + el.height }, // BR
    { x: el.x, y: el.y + el.height },          // BL
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
```

- [ ] **Step 2: Commit**

```bash
git add extensions/uink-rms-bridge/frontend/src/Canvas.tsx
git commit -m "feat(uink-rms-bridge): add Canvas editor with drag/resize/text rendering"
```

---

### Task 5: Edit Modal

**Files:**
- Create: `extensions/uink-rms-bridge/frontend/src/EditModal.tsx`

Modal overlay with canvas editor, floating toolbar, property panel, and push button.

- [ ] **Step 1: Create EditModal.tsx**

```tsx
import { useState, useRef, useCallback, useEffect } from 'react'
import { CanvasEditor, CanvasElement, CanvasEditorHandle, createTextElement, createImageElement } from './Canvas'
import { pushContent } from './api'

interface EditModalProps {
  deviceId: string
  deviceWidth: number
  deviceHeight: number
  onClose: () => void
  onPushSuccess: () => void
  extensionId: string
}

export function EditModal({ deviceId, deviceWidth, deviceHeight, onClose, onPushSuccess, extensionId }: EditModalProps) {
  const [elements, setElements] = useState<CanvasElement[]>([])
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [pushing, setPushing] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [toast, setToast] = useState<{ msg: string; type: 'success' | 'error' } | null>(null)
  const canvasRef = useRef<CanvasEditorHandle>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)

  // Close on Escape
  useEffect(() => {
    const handler = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose() }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [onClose])

  // Auto-dismiss toast
  useEffect(() => {
    if (!toast) return
    const t = setTimeout(() => setToast(null), 3000)
    return () => clearTimeout(t)
  }, [toast])

  const selectedElement = elements.find(e => e.id === selectedId)

  const handleAddText = useCallback(() => {
    const el = createTextElement(
      20 + Math.random() * 60,
      20 + Math.random() * 60
    )
    setElements(prev => [...prev, el])
    setSelectedId(el.id)
  }, [])

  const handleAddImage = useCallback(() => {
    fileInputRef.current?.click()
  }, [])

  const handleFileSelected = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file) return
    const reader = new FileReader()
    reader.onload = () => {
      const dataUrl = reader.result as string
      const img = new Image()
      img.onload = () => {
        const el = createImageElement(
          dataUrl, img,
          20 + Math.random() * 60,
          20 + Math.random() * 60
        )
        setElements(prev => [...prev, el])
        setSelectedId(el.id)
      }
      img.src = dataUrl
    }
    reader.readAsDataURL(file)
    e.target.value = ''
  }, [])

  const handleDelete = useCallback(() => {
    if (!selectedId) return
    setElements(prev => prev.filter(e => e.id !== selectedId))
    setSelectedId(null)
  }, [selectedId])

  const handleElementChange = useCallback((id: string, changes: Partial<CanvasElement>) => {
    setElements(prev => prev.map(el => el.id === id ? { ...el, ...changes } : el))
  }, [])

  const handlePush = useCallback(async () => {
    if (!canvasRef.current) return
    const base64 = canvasRef.current.exportAsBase64()
    if (!base64) { setError('Failed to export canvas'); return }

    setPushing(true)
    setError(null)
    const result = await pushContent(deviceId, base64, extensionId)
    setPushing(false)

    if (result.success) {
      setToast({ msg: 'Pushed successfully!', type: 'success' })
      setTimeout(() => { onPushSuccess(); onClose() }, 800)
    } else {
      const err = result.error || 'Push failed'
      setError(err)
      setToast({ msg: err, type: 'error' })
    }
  }, [deviceId, extensionId, onClose, onPushSuccess])

  return (
    <div className="uink-editor-modal-overlay" onClick={(e) => { if (e.target === e.currentTarget) onClose() }}>
      <div className="uink-editor-modal">
        {/* Header */}
        <div className="uink-editor-modal-header">
          <span>Edit Screen Content</span>
          <button className="uink-editor-modal-close" onClick={onClose}>&times;</button>
        </div>

        {/* Canvas */}
        <div className="uink-editor-canvas-wrap">
          <CanvasEditor
            ref={canvasRef}
            width={deviceWidth}
            height={deviceHeight}
            elements={elements}
            onElementsChange={setElements}
            selectedId={selectedId}
            onSelectedChange={setSelectedId}
          />
        </div>

        {/* Toolbar */}
        <div className="uink-editor-toolbar">
          <button className="uink-editor-toolbar-btn" onClick={handleAddText} title="Add text">
            <span style={{ fontSize: 14 }}>T+</span>
          </button>
          <button className="uink-editor-toolbar-btn" onClick={handleAddImage} title="Add image">
            <span style={{ fontSize: 14 }}>&#128444;+</span>
          </button>
          <button
            className="uink-editor-toolbar-btn"
            onClick={handleDelete}
            disabled={!selectedId}
            title="Delete selected"
          >
            <span style={{ fontSize: 14 }}>&#128465;</span>
          </button>
          <input
            ref={fileInputRef}
            type="file"
            accept="image/*"
            style={{ display: 'none' }}
            onChange={handleFileSelected}
          />
        </div>

        {/* Property panel */}
        {selectedElement && (
          <div className="uink-editor-props">
            {selectedElement.type === 'text' && (
              <>
                <label>
                  Text
                  <input
                    type="text"
                    value={selectedElement.content || ''}
                    onChange={e => handleElementChange(selectedElement.id, { content: e.target.value })}
                  />
                </label>
                <label>
                  Size
                  <input
                    type="number"
                    value={selectedElement.fontSize || 18}
                    min={8}
                    max={120}
                    onChange={e => handleElementChange(selectedElement.id, { fontSize: Number(e.target.value) })}
                  />
                </label>
                <label>
                  <input
                    type="checkbox"
                    checked={selectedElement.bold || false}
                    onChange={e => handleElementChange(selectedElement.id, { bold: e.target.checked })}
                  />
                  Bold
                </label>
              </>
            )}
            {selectedElement.type === 'image' && (
              <span style={{ fontSize: 11, color: 'var(--uink-editor-muted)' }}>
                {selectedElement.width} &times; {selectedElement.height} — drag handles to resize
              </span>
            )}
          </div>
        )}

        {/* Footer */}
        <div className="uink-editor-modal-footer">
          <button className="uink-editor-btn uink-editor-btn-ghost" onClick={onClose} disabled={pushing}>
            Cancel
          </button>
          <button
            className="uink-editor-btn uink-editor-btn-primary"
            onClick={handlePush}
            disabled={pushing}
          >
            {pushing ? (
              <><span className="uink-editor-spinner" style={{ width: 14, height: 14, borderWidth: 2 }} /> Pushing...</>
            ) : (
              'Push to Device'
            )}
          </button>
        </div>

        {/* Error */}
        {error && !toast && (
          <div style={{ padding: '6px 16px', fontSize: 11, color: 'var(--uink-editor-error)', borderTop: '1px solid var(--uink-editor-border)' }}>
            {error}
          </div>
        )}

        {/* Toast */}
        {toast && (
          <div className={`uink-editor-toast ${toast.type}`}>
            {toast.msg}
          </div>
        )}
      </div>
    </div>
  )
}
```

- [ ] **Step 2: Commit**

```bash
git add extensions/uink-rms-bridge/frontend/src/EditModal.tsx
git commit -m "feat(uink-rms-bridge): add edit modal with canvas editor and push"
```

---

### Task 6: Display Editor Card

**Files:**
- Create: `extensions/uink-rms-bridge/frontend/src/DisplayEditorCard.tsx`

The main card component. Content viewer with device info, preview image, and edit button.

- [ ] **Step 1: Create DisplayEditorCard.tsx**

```tsx
import { forwardRef, useEffect, useState, useCallback, useRef } from 'react'
import { injectStyles } from './styles'
import { listDevices, getDisplay, getDisplaySize, DeviceInfo, DisplaySlot } from './api'
import { EditModal } from './EditModal'

export interface ExtensionComponentProps {
  title?: string
  dataSource?: {
    type: string
    extensionId?: string
    [key: string]: any
  }
  className?: string
  config?: Record<string, any>
}

export const DisplayEditorCard = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function DisplayEditorCard(props, ref) {
    const { className = '', config, dataSource } = props
    const extensionId = dataSource?.extensionId || 'uink-rms-bridge'

    useEffect(() => { injectStyles() }, [])

    const [device, setDevice] = useState<DeviceInfo | null>(null)
    const [devices, setDevices] = useState<DeviceInfo[]>([])
    const [displaySize, setDisplaySize] = useState<{ width: number; height: number } | null>(null)
    const [previewUrl, setPreviewUrl] = useState<string | null>(null)
    const [loading, setLoading] = useState(true)
    const [error, setError] = useState<string | null>(null)
    const [editOpen, setEditOpen] = useState(false)
    const mountedRef = useRef(true)

    useEffect(() => {
      mountedRef.current = true
      return () => { mountedRef.current = false }
    }, [])

    // Load device data
    const loadData = useCallback(async () => {
      setLoading(true)
      setError(null)

      const configDeviceId = config?.deviceId

      // 1. List devices
      const listRes = await listDevices(extensionId)
      if (!mountedRef.current) return
      if (!listRes.success || !listRes.data) {
        setError(listRes.error || 'Failed to load devices')
        setLoading(false)
        return
      }

      const deviceList = listRes.data.devices || []
      setDevices(deviceList)

      if (deviceList.length === 0) {
        setError('No devices synced. Run sync_devices first.')
        setLoading(false)
        return
      }

      // Pick device: config > first available
      const target = configDeviceId
        ? deviceList.find(d => d.device_id === configDeviceId) || deviceList[0]
        : deviceList[0]
      setDevice(target)

      // 2. Get display size
      const sizeRes = await getDisplaySize(target.device_id, extensionId)
      if (!mountedRef.current) return
      if (sizeRes.success && sizeRes.data) {
        setDisplaySize({ width: sizeRes.data.width, height: sizeRes.data.height })
      }

      // 3. Get display content
      const displayRes = await getDisplay(target.device_id, extensionId)
      if (!mountedRef.current) return
      if (displayRes.success && displayRes.data) {
        const slots = displayRes.data.slots || []
        if (slots.length > 0) {
          // Prefer the first slot's preview_thumbnail_url or preview_url
          const slot = slots[0]
          setPreviewUrl(slot.preview_thumbnail_url || slot.preview_url || null)
        }
      }

      setLoading(false)
    }, [extensionId, config?.deviceId])

    useEffect(() => { loadData() }, [loadData])

    const handleRefresh = useCallback(() => { loadData() }, [loadData])

    const handlePushSuccess = useCallback(() => {
      // Refresh preview after push
      loadData()
    }, [loadData])

    return (
      <div ref={ref} className={`uink-editor-root ${className}`}>
        <div className="uink-editor-card">
          {/* Loading */}
          {loading && (
            <div className="uink-editor-loading">
              <div className="uink-editor-spinner" />
              <span>Loading...</span>
            </div>
          )}

          {/* Error */}
          {!loading && error && (
            <div className="uink-editor-error">
              <span>{error}</span>
              <button className="uink-editor-btn uink-editor-btn-ghost" onClick={handleRefresh} style={{ marginTop: 4, fontSize: 11 }}>
                Retry
              </button>
            </div>
          )}

          {/* Content */}
          {!loading && !error && device && (
            <>
              {/* Header */}
              <div className="uink-editor-header">
                <span className="uink-editor-device-name">{device.name || device.device_id}</span>
                <span className="uink-editor-status">
                  <span className={`uink-editor-status-dot ${device.online ? '' : 'offline'}`} />
                  {device.online ? 'Online' : 'Offline'}
                </span>
                {displaySize && (
                  <span className="uink-editor-resolution">{displaySize.width}&times;{displaySize.height}</span>
                )}
              </div>

              {/* Preview */}
              <div className="uink-editor-preview">
                {previewUrl ? (
                  <img src={previewUrl} alt="Screen preview" />
                ) : (
                  <div className="uink-editor-preview-placeholder">
                    <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
                      <rect x="2" y="3" width="20" height="14" rx="2" />
                      <line x1="8" y1="21" x2="16" y2="21" />
                      <line x1="12" y1="17" x2="12" y2="21" />
                    </svg>
                    <span>No preview available</span>
                  </div>
                )}
              </div>

              {/* Footer */}
              <div className="uink-editor-footer">
                <button
                  className="uink-editor-btn uink-editor-btn-primary"
                  onClick={() => setEditOpen(true)}
                  disabled={!displaySize}
                >
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
                    <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
                    <path d="M18.5 2.5a2.12 2.12 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" />
                  </svg>
                  Edit
                </button>
              </div>
            </>
          )}
        </div>

        {/* Modal */}
        {editOpen && device && displaySize && (
          <EditModal
            deviceId={device.device_id}
            deviceWidth={displaySize.width}
            deviceHeight={displaySize.height}
            onClose={() => setEditOpen(false)}
            onPushSuccess={handlePushSuccess}
            extensionId={extensionId}
          />
        )}
      </div>
    )
  }
)

DisplayEditorCard.displayName = 'DisplayEditorCard'
```

- [ ] **Step 2: Commit**

```bash
git add extensions/uink-rms-bridge/frontend/src/DisplayEditorCard.tsx
git commit -m "feat(uink-rms-bridge): add DisplayEditorCard viewer component"
```

---

### Task 7: Entry Point

**Files:**
- Create: `extensions/uink-rms-bridge/frontend/src/index.tsx`

- [ ] **Step 1: Create index.tsx**

```tsx
export { DisplayEditorCard } from './DisplayEditorCard'
export default { DisplayEditorCard }
```

- [ ] **Step 2: Commit**

```bash
git add extensions/uink-rms-bridge/frontend/src/index.tsx
git commit -m "feat(uink-rms-bridge): add entry point with DisplayEditorCard export"
```

---

### Task 8: Build and Verify

**Files:** None (verification only)

- [ ] **Step 1: Build the frontend**

Run: `cd extensions/uink-rms-bridge/frontend && npm run build`
Expected: Build succeeds, `dist/uink-rms-bridge-components.umd.cjs` created

- [ ] **Step 2: Verify output file exists**

Run: `ls -la extensions/uink-rms-bridge/frontend/dist/`
Expected: `uink-rms-bridge-components.umd.cjs` file present, reasonable size

- [ ] **Step 3: Verify UMD exports**

Run: `head -5 extensions/uink-rms-bridge/frontend/dist/uink-rms-bridge-components.umd.cjs`
Expected: File starts with UMD wrapper pattern, contains `DisplayEditorCard`

- [ ] **Step 4: Final commit (if any changes)**

```bash
git add extensions/uink-rms-bridge/frontend/
git commit -m "feat(uink-rms-bridge): complete DisplayEditorCard frontend component"
```
