# Uink Display Editor Card - Design Spec

## Overview

Frontend component for the `uink-rms-bridge` extension. Displays current e-paper screen content and provides a modal canvas editor for composing and pushing new content to devices.

## Component Identity

- **Name:** `DisplayEditorCard`
- **Extension:** `uink-rms-bridge`
- **Type:** `card`
- **Default size:** 420 x 520
- **Min size:** 320 x 400
- **Max size:** 600 x 700
- **Must use `forwardRef<HTMLDivElement, ExtensionComponentProps>`** per CLAUDE.md requirement
- **Export:** `export default { DisplayEditorCard }`

## Architecture

### Component Body (always visible)

A content viewer showing the device's current screen. Minimal UI chrome.

```
┌──────────────────────────────────┐
│  <device-name>  <status>  <res>  │  Header bar
├──────────────────────────────────┤
│  ┌────────────────────────────┐  │
│  │                            │  │
│  │   Current screen preview   │  │  Main area: preview_url image
│  │   (from get_display)       │  │
│  │                            │  │
│  └────────────────────────────┘  │
│            [ Edit ]              │  Single action button
└──────────────────────────────────┘
```

### Modal Editor (overlay)

Opened by clicking "Edit". Does not affect component layout.

```
┌─────────────────────────────────────────┐
│  Edit Screen Content              [×]   │
├─────────────────────────────────────────┤
│  ┌─────────────────────────────────┐    │
│  │     Canvas (scaled to device    │    │
│  │     resolution, e.g. 800x480)  │    │
│  │     Drag text/image elements    │    │
│  └─────────────────────────────────┘    │
│       [T+] [Image+] [Delete]            │  Floating toolbar
├─────────────────────────────────────────┤
│  Property panel (when element selected) │
│  Content: [___________] FontSize: [24]  │
├─────────────────────────────────────────┤
│        [Cancel]    [Push to Device]     │
└─────────────────────────────────────────┘
```

## Data Model

```typescript
interface CanvasElement {
  id: string
  type: 'text' | 'image'
  x: number          // position relative to canvas origin
  y: number
  width: number
  height: number
  // text fields
  content?: string
  fontSize?: number
  bold?: boolean
  // image fields
  imageSrc?: string   // base64 data URL
}
```

Canvas state **does not persist** between modal opens — it resets each time the modal is opened with a fresh canvas.

## API Interactions

### On Mount
1. `list_devices` - fetch synced devices; use config `deviceId` or first device
2. `get_display_size` - get canvas dimensions (e.g. 800x480)
3. `get_display` - fetch current screen content (slots with preview_url)

### On Push
1. `canvas.toDataURL('image/png')` - export canvas as base64
2. `push_content` with `{ device_id, content_type: "image", content: <base64>, dither_algorithm: "floyd-steinberg", resize_mode: "fit" }` - push to device
   - Default dither: `"floyd-steinberg"` (best for e-paper quality)
   - Default resize: `"fit"` (letterbox within display resolution)
   - These defaults are applied automatically; user does not need to configure them
3. On success: close modal, refresh preview via `get_display`
4. On auth error (HTTP 401): show "Authentication expired. Please reconfigure extension credentials." message

## Component Config Schema

```json
{
  "deviceId": {
    "type": "string",
    "description": "Default device ID to display",
    "default": ""
  }
}
```

## Canvas Editor Details

### Implementation: HTML Canvas native API

No external libraries. All rendering, hit-testing, drag handling via Canvas 2D API.

### Core Operations
- **Hit test:** On mousedown, iterate elements (reverse z-order) to find which one contains the click point
- **Drag:** On mousemove while dragging, update element x/y
- **Resize:** Selection handles at corners; drag handle updates width/height
- **Text editing:** Double-click text element → show input overlay at element position
- **Render loop:** Clear canvas → draw white background → draw all elements → draw selection UI if any

### Rendering
- White background (e-paper simulation)
- Text: `ctx.fillText()` with specified font size
- Image: `ctx.drawImage()` from loaded Image objects
- Selection: dashed border + corner handles (drawn on canvas, not CSS)

### Export
```typescript
// Clear selection UI before export
clearSelection()
ctx.drawImage(canvasWithoutSelection, 0, 0)
const dataUrl = canvas.toDataURL('image/png')
const base64 = dataUrl.split(',')[1]  // strip data:image/png;base64, prefix
```

## Styling

- **No Tailwind** - use NeoMind CSS variables
- Scoped prefix: `uink-editor-`
- Style injection: `injectStyles()` pattern (const string + `document.head.appendChild`)

### CSS Variable Aliases (with fallbacks)

All colors go through local aliases with fallback values:

```css
.uink-editor-root {
  --uink-editor-fg: var(--foreground, #17172a);
  --uink-editor-muted: var(--muted-foreground, #6b7280);
  --uink-editor-card: var(--card, rgba(255,255,255,0.85));
  --uink-editor-border: var(--border, rgba(0,0,0,0.08));
  --uink-editor-accent: var(--primary, #17172a);
  --uink-editor-on-primary: var(--primary-foreground, #ffffff);
  --uink-editor-radius: var(--radius-lg, 10px);
  --uink-editor-radius-xl: var(--radius-xl, 12px);
  --uink-editor-shadow: var(--shadow-lg, 0 8px 32px rgba(0,0,0,0.12));
  --uink-editor-success: var(--color-success, oklch(0.55 0.17 155));
  --uink-editor-error: var(--color-error, oklch(0.55 0.22 25));
}
.dark .uink-editor-root {
  --uink-editor-fg: var(--foreground, #f0f0f0);
  --uink-editor-muted: var(--muted-foreground, #9ca3af);
  --uink-editor-card: var(--card, rgba(30,30,30,0.8));
  --uink-editor-border: var(--border, rgba(255,255,255,0.08));
  --uink-editor-accent: var(--primary, #f0f0f0);
  --uink-editor-on-primary: var(--primary-foreground, #17172a);
}
```

**Primary button text MUST use `var(--uink-editor-on-primary)`** — never bare `var(--primary-foreground)` or hardcoded `#fff`.

### Modal Overlay
```css
.uink-editor-modal-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.5);
  z-index: 9999;
  display: flex;
  align-items: center;
  justify-content: center;
}
.uink-editor-modal {
  background: var(--uink-editor-card);
  border-radius: var(--uink-editor-radius-xl);
  box-shadow: var(--uink-editor-shadow);
  max-width: 90vw;
  max-height: 90vh;
  overflow: auto;
}
```

## File Structure

```
extensions/uink-rms-bridge/frontend/
├── frontend.json
├── package.json
├── vite.config.ts
├── tsconfig.json
└── src/
    ├── index.tsx          # Main component entry + export
    ├── DisplayEditorCard.tsx  # Card component (viewer + edit button)
    ├── EditModal.tsx       # Modal overlay with canvas editor
    ├── Canvas.tsx          # Canvas rendering & interaction logic
    ├── api.ts              # Extension command helpers
    └── styles.ts           # CSS string constant + injectStyles()
```

## Vite Config

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

## frontend.json

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

## States

| State | Display |
|-------|---------|
| Loading | Skeleton/spinner |
| No devices | "No devices synced. Run sync_devices first." |
| Device offline | Show last preview (if available) + offline badge |
| Online, has preview | Show preview image + edit button |
| Pushing | Disable edit button, show spinner |
| Push success | Flash success badge, refresh preview |
| Push failed | Error toast/banner, keep modal open |
| Auth error | "Authentication expired. Please reconfigure extension credentials." |
