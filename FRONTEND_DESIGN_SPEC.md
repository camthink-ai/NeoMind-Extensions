# NeoMind Extension Frontend Design Specification

This document defines the frontend component standards for NeoMind extensions. All extension frontend components must follow these rules to ensure visual consistency with the main NeoMind platform.

> **Reference:** The main NeoMind design system is documented at `../NeoMind/web/DESIGN_SPEC.md`. Extensions share the same CSS variable-based theme system.

---

## 1. Extension Frontend Architecture

### How Extension Components Work

Extension frontend components are **React components** built as **UMD bundles** that get dynamically loaded into the NeoMind dashboard. They run inside the host app and share the same DOM, CSS variables, and React runtime.

```
Extension Build (Vite) → UMD Bundle (.umd.cjs) → Loaded by NeoMind DynamicRegistry
```

**Key constraints:**
- React and ReactDOM are **external** — provided by the host app, NOT bundled
- Components must use **CSS variables** for theming (NOT Tailwind classes — extensions don't have access to Tailwind)
- Components are rendered inside the NeoMind dashboard's card/widget system

### Project Structure

```
extensions/my-extension/frontend/
├── src/
│   └── index.tsx          # React component (entry point)
├── package.json           # npm config (React as peerDependency)
├── vite.config.ts         # UMD build config
├── tsconfig.json          # TypeScript config
└── frontend.json          # Component manifest
```

---

## 2. Component Template

### Required Component Props

Every extension component MUST accept these props:

```tsx
export interface ExtensionComponentProps {
  title?: string              // Card title from dashboard config
  dataSource?: DataSource     // Data binding configuration
  className?: string          // Additional CSS class from host
  config?: Record<string, any>  // User-configured settings
}

export interface DataSource {
  type: string
  extensionId?: string
  [key: string]: any
}
```

### Component Pattern

```tsx
import { forwardRef, useState, useEffect, useCallback } from 'react'

const EXTENSION_ID = 'my-extension'

export const MyCard = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function MyCard(props, ref) {
    const { title = 'My Extension', dataSource, className = '', config } = props
    const [data, setData] = useState(null)
    const [loading, setLoading] = useState(false)
    const [error, setError] = useState<string | null>(null)

    // Component logic...

    return (
      <div ref={ref} className={`my-ext ${className}`}>
        <style>{CSS}</style>
        {/* Component content */}
      </div>
    )
  }
)

export default { MyCard }
```

**Rules:**
- MUST use `forwardRef` with `ref` on the root `<div>`
- MUST export both named and default export
- MUST handle `loading`, `error`, and `empty` states

---

## 3. Theming & Colors

### Use NeoMind CSS Variables

Extensions share the host app's CSS variables. Use them directly:

```css
.my-ext {
  /* Use NeoMind's design tokens */
  --ext-fg: var(--foreground);
  --ext-muted: var(--muted-foreground);
  --ext-bg: var(--card);
  --ext-border: var(--border);
  --ext-accent: var(--primary);
  --ext-accent-fg: var(--primary-foreground);
  --ext-success: var(--color-success);
  --ext-warning: var(--color-warning);
  --ext-error: var(--color-error);
  --ext-info: var(--color-info);
  --ext-radius: var(--radius);
}
```

### Available NeoMind CSS Variables

#### Base Colors
| Variable | Light | Dark | Usage |
|----------|-------|------|-------|
| `--background` | White | Dark | Page background |
| `--foreground` | Near-black | Near-white | Primary text |
| `--card` | White | Dark | Card background |
| `--card-foreground` | Near-black | Near-white | Card text |
| `--muted` | Light gray | Dark gray | Subtle backgrounds |
| `--muted-foreground` | Gray | Light gray | Secondary text |
| `--border` | 10% black | 10% white | Borders |
| `--primary` | Near-black | Near-white | Primary actions |
| `--primary-foreground` | White | White | Text on primary |

#### Semantic Colors
| Variable | Usage |
|----------|-------|
| `--color-success` | Success / online / active states |
| `--color-success-bg` | Success background (8% opacity) |
| `--color-warning` | Warning / pending states |
| `--color-warning-bg` | Warning background |
| `--color-error` | Error / offline / failed states |
| `--color-error-bg` | Error background |
| `--color-info` | Info / running states |
| `--color-info-bg` | Info background |

#### Accent Colors
| Variable | Category |
|----------|----------|
| `--accent-purple` / `--accent-purple-bg` | Purple |
| `--accent-orange` / `--accent-orange-bg` | Orange |
| `--accent-cyan` / `--accent-cyan-bg` | Cyan |
| `--accent-emerald` / `--accent-emerald-bg` | Emerald |
| `--accent-indigo` / `--accent-indigo-bg` | Indigo |

#### Spacing & Radius
| Variable | Value |
|----------|-------|
| `--radius` | 10px (default) |
| `--radius-sm` | 6px |
| `--radius-2xl` | 16px |
| `--space-1` through `--space-8` | 4px to 32px |

#### Shadows
| Variable | Usage |
|----------|-------|
| `--shadow-sm` | Subtle elevation |
| `--shadow-md` | Cards |
| `--shadow-lg` | Elevated panels |

### Dark Mode

Dark mode is automatic. The host app applies `.dark` class on `<html>`. Use CSS variables and they'll resolve correctly.

**NEVER hardcode light/dark color pairs.** Use NeoMind's CSS variables:

```css
/* Good — uses host theme */
.my-ext-card {
  background: var(--card);
  color: var(--foreground);
  border: 1px solid var(--border);
}

/* Bad — hardcoded colors */
.my-ext-card {
  background: white;
  color: #333;
  border: 1px solid #eee;
}
```

### Inline Styles Pattern

Since extensions don't have Tailwind, use scoped CSS via `<style>` tag:

```tsx
const CSS = `
  .my-ext {
    --ext-radius: var(--radius);
    font-family: system-ui, -apple-system, sans-serif;
    height: 100%;
    display: flex;
    flex-direction: column;
  }

  .my-ext-card {
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: var(--ext-radius);
    padding: 16px;
    height: 100%;
  }

  .my-ext-title {
    font-size: 14px;
    font-weight: 600;
    color: var(--foreground);
  }

  .my-ext-muted {
    color: var(--muted-foreground);
    font-size: 12px;
  }

  .dark .my-ext-badge {
    /* Dark mode specific overrides if needed */
  }
`

// In component:
<div ref={ref} className={`my-ext ${className}`}>
  <style>{CSS}</style>
  {/* ... */}
</div>
```

---

## 4. API Integration

### Command Execution Pattern

All extension components communicate with their backend via the NeoMind extension API:

```tsx
const EXTENSION_ID = 'my-extension'

function getApiBase(): string {
  return (window as any).__TAURI__ ? 'http://localhost:9375/api' : '/api'
}

function getAuthHeaders(): Record<string, string> {
  const token = localStorage.getItem('neomind_token') || sessionStorage.getItem('neomind_token_session')
  const headers: Record<string, string> = { 'Content-Type': 'application/json' }
  if (token) headers['Authorization'] = `Bearer ${token}`
  return headers
}

async function executeCommand<T = any>(
  command: string,
  args: Record<string, unknown> = {}
): Promise<{ success: boolean; data?: T; error?: string }> {
  try {
    const res = await fetch(`${getApiBase()}/extensions/${EXTENSION_ID}/command`, {
      method: 'POST',
      headers: getAuthHeaders(),
      body: JSON.stringify({ command, args })
    })
    if (!res.ok) return { success: false, error: `HTTP ${res.status}` }
    return res.json()
  } catch (e) {
    return { success: false, error: e instanceof Error ? e.message : 'Network error' }
  }
}
```

### Data Fetching with Auto-Refresh

```tsx
const [data, setData] = useState<WeatherData | null>(null)
const [loading, setLoading] = useState(true)
const [error, setError] = useState<string | null>(null)

const fetchData = useCallback(async () => {
  setLoading(true)
  setError(null)
  const result = await executeCommand<WeatherData>('get_weather', { city: config?.defaultCity })
  if (result.success && result.data) {
    setData(result.data)
  } else {
    setError(result.error || 'Failed to fetch data')
  }
  setLoading(false)
}, [config?.defaultCity])

useEffect(() => {
  fetchData()
  const interval = config?.refreshInterval || 300000  // 5 min default
  const timer = setInterval(fetchData, interval)
  return () => clearInterval(timer)
}, [fetchData])
```

---

## 5. Icon System

Extensions must use **inline SVG** icons (not icon libraries, since they can't bundle external dependencies):

```tsx
const ICONS: Record<string, string> = {
  cloud: '<path d="M18 10h-1.26A8 8 0 1 0 9 20h9a5 5 0 0 0 0-10z"/>',
  refresh: '<path d="M21 12a9 9 0 1 1-9-9c2.5 0 4.9 1 6.7 2.7L21 8M21 3v5h-5"/>',
  settings: '<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"/>',
  alert: '<path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/>',
  camera: '<path d="M23 19a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4l2-3h6l2 3h4a2 2 0 0 1 2 2z"/><circle cx="12" cy="13" r="4"/>',
}

const Icon = ({ name, size = 16, className = '' }: { name: string; size?: number; className?: string }) => (
  <svg
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
    width={size}
    height={size}
    className={className}
    dangerouslySetInnerHTML={{ __html: ICONS[name] || '' }}
  />
)
```

---

## 6. UI Patterns

### Card Layout

Every extension component should follow this card structure:

```tsx
<div ref={ref} className={`my-ext ${className}`}>
  <style>{CSS}</style>
  <div className="my-ext-card">
    {/* Header */}
    <div className="my-ext-header">
      <div className="my-ext-title-row">
        <Icon name="cloud" size={16} />
        <span className="my-ext-title">{title}</span>
      </div>
      <button className="my-ext-refresh" onClick={fetchData} title="Refresh">
        <Icon name="refresh" size={14} />
      </button>
    </div>

    {/* Content — varies by component type */}
    <div className="my-ext-content">
      {loading ? <LoadingState /> : error ? <ErrorState /> : data ? <Content /> : <EmptyState />}
    </div>
  </div>
</div>
```

### Loading State

```tsx
function LoadingState() {
  return (
    <div className="my-ext-loading">
      <div className="my-ext-spinner" />
      <span>Loading...</span>
    </div>
  )
}

/* CSS */
.my-ext-loading {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  flex: 1;
  gap: 8px;
  color: var(--muted-foreground);
  font-size: 12px;
}

.my-ext-spinner {
  width: 24px;
  height: 24px;
  border: 2px solid var(--border);
  border-top-color: var(--primary);
  border-radius: 50%;
  animation: ext-spin 0.8s linear infinite;
}

@keyframes ext-spin {
  to { transform: rotate(360deg); }
}
```

### Error State

```tsx
function ErrorState({ message, onRetry }: { message: string; onRetry: () => void }) {
  return (
    <div className="my-ext-error">
      <Icon name="alert" size={20} />
      <span className="my-ext-error-msg">{message}</span>
      <button className="my-ext-retry" onClick={onRetry}>Retry</button>
    </div>
  )
}

/* CSS */
.my-ext-error {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  flex: 1;
  gap: 8px;
  color: var(--color-error);
  font-size: 12px;
}

.my-ext-retry {
  padding: 4px 12px;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm, 6px);
  background: var(--muted);
  color: var(--foreground);
  font-size: 12px;
  cursor: pointer;
}
```

### Empty State

```tsx
function EmptyState({ message }: { message: string }) {
  return (
    <div className="my-ext-empty">
      <span>{message}</span>
    </div>
  )
}

/* CSS */
.my-ext-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  flex: 1;
  color: var(--muted-foreground);
  font-size: 13px;
}
```

### Status Badges

```tsx
function StatusBadge({ status }: { status: 'online' | 'offline' | 'error' }) {
  const colors: Record<string, string> = {
    online: 'var(--color-success)',
    offline: 'var(--muted-foreground)',
    error: 'var(--color-error)',
  }
  return (
    <span className="my-ext-badge" style={{ color: colors[status] }}>
      ● {status}
    </span>
  )
}
```

---

## 7. Build Configuration

### vite.config.ts

```typescript
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

const EXTENSION_ID = 'my-extension'

export default defineConfig({
  plugins: [react()],
  define: {
    'process.env.NODE_ENV': JSON.stringify('production')
  },
  build: {
    lib: {
      entry: 'src/index.tsx',
      name: `${EXTENSION_ID}Components`,
      fileName: `${EXTENSION_ID}-components`,
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

**Critical rules:**
- React, ReactDOM MUST be external (provided by host)
- Output format MUST be UMD
- File name MUST match `frontend.json` entrypoint
- Use `exports: 'named'` for named exports

### package.json

```json
{
  "name": "@neomind/my-extension-frontend",
  "version": "1.0.0",
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

---

## 8. Frontend Manifest (frontend.json)

```json
{
  "id": "my-extension",
  "version": "1.0.0",
  "entrypoint": "my-extension-components.umd.cjs",
  "components": [
    {
      "name": "MyCard",
      "type": "card",
      "displayName": "My Extension",
      "description": "Description shown in dashboard",
      "icon": "cloud",
      "defaultSize": { "width": 340, "height": 320 },
      "minSize": { "width": 240, "height": 260 },
      "maxSize": { "width": 480, "height": 400 },
      "configSchema": {
        "defaultCity": {
          "type": "string",
          "required": false,
          "default": "Beijing",
          "description": "Default city for weather"
        }
      },
      "refreshable": true,
      "refreshInterval": 300000
    }
  ],
  "i18n": {
    "defaultLanguage": "en",
    "supportedLanguages": ["en", "zh"]
  },
  "dependencies": {
    "react": ">=18.0.0"
  }
}
```

### Component Types

| Type | Description | Typical Size |
|------|-------------|--------------|
| `card` | Static data display | 300-400px |
| `widget` | Interactive component | 400-600px |
| `dialog` | Modal component | Varies |

### configSchema Types

| Type | Values |
|------|--------|
| `string` | Text input |
| `number` | Numeric input |
| `boolean` | Toggle switch |
| `select` | Dropdown with `options` array |

---

## 9. Do's and Don'ts

### Do

- Use NeoMind CSS variables for all colors (`var(--foreground)`, `var(--card)`, etc.)
- Use `forwardRef` and pass `ref` to root element
- Handle all three states: loading, error, data
- Use scoped CSS class names (prefix with extension ID: `weather-`)
- Include `<style>` tag inside component for scoped styles
- Use inline SVG icons (not icon libraries)
- Support both light and dark mode via CSS variables
- Implement auto-refresh with configurable intervals
- Include `configSchema` for user-configurable options
- Use semantic HTML elements
- Test on both Tauri (`localhost:9375/api`) and web (`/api`) environments

### Don't

- **NEVER** import or use Tailwind CSS — extensions don't have Tailwind
- **NEVER** bundle React/ReactDOM — they're provided by the host
- **NEVER** use hardcoded color values (`#fff`, `rgb(...)`) — use CSS variables
- **NEVER** use `alert()`, `confirm()`, or `prompt()` — use inline UI instead
- **NEVER** make CSS selectors too broad (`.card`, `.title`) — always prefix
- **NEVER** use external icon libraries (lucide, heroicons) — use inline SVG
- **NEVER** depend on npm packages that bundle their own CSS
- **NEVER** use `window.location` for API URLs — use `getApiBase()` helper
- **NEVER** forget authentication headers — use `getAuthHeaders()` helper

---

## 10. CSS Class Naming Convention

Use BEM-like naming with extension prefix:

```
.{extension-id}              /* Root container */
.{extension-id}-card         /* Card wrapper */
.{extension-id}-header       /* Header section */
.{extension-id}-title        /* Title text */
.{extension-id}-content      /* Main content area */
.{extension-id}-footer       /* Footer section */
.{extension-id}-loading      /* Loading state */
.{extension-id}-error        /* Error state */
.{extension-id}-empty        /* Empty state */
.{extension-id}-badge        /* Status badge */
.{extension-id}-spinner      /* Loading spinner */
.{extension-id}-button       /* Action button */
.{extension-id}-muted        /* Muted/secondary text */
.{extension-id}-value        /* Primary data value */
.{extension-id}-label        /* Data label */
.{extension-id}-row          /* Data row */
.{extension-id}-divider      /* Visual divider */
```

---

## Quick Reference

| Resource | Location |
|----------|----------|
| NeoMind Design Spec | `../NeoMind/web/DESIGN_SPEC.md` |
| NeoMind CSS Variables | `../NeoMind/web/src/index.css` |
| Extension Template | `neomind-ext/templates/basic/` |
| Existing Extensions | `extensions/` |
| Build Script | `./build.sh` |
| Frontend Guide | `EXTENSION_GUIDE.md` section "Frontend Components" |
