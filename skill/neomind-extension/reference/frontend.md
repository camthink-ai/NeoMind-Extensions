# Frontend Component Guide

> **Read this first:** [`EXTENSION_FRONTEND_DESIGN_GUIDE.md`](../../../../CamThink%20Project/NeoMind-Extensions/EXTENSION_FRONTEND_DESIGN_GUIDE.md)
> in the repo root. It is the authoritative spec — this page summarizes the parts you'll
> use most often.

NeoMind extensions can ship React dashboard components as UMD bundles. The host app
injects React/ReactDOM, so your bundle must externalize them.

## Hard Rules (all of these have caused real bugs)

1. **Never use Tailwind.** Extension bundles don't ship Tailwind. Use inline `<style>`
   tags or scoped CSS files with extension-prefixed class names (`.your-ext-...`).
2. **Never hardcode colors** (`#fff`, `rgb(...)`, `hsl(...)`, `white`). Use NeoMind CSS
   variables: `var(--foreground)`, `var(--card)`, `var(--border)`, `var(--muted-foreground)`,
   `var(--primary)`, etc. They adapt to light/dark mode automatically.
3. **Primary button text must use `var(--{prefix}-on-primary)`** — not `var(--primary-foreground)`
   and definitely not `#fff`. Define it in your scoped block. See design guide §5.1.
4. **UMD format, React/ReactDOM + `react/jsx-runtime` external.**
5. **Component `type` in `frontend.json` must be unique across ALL extensions.** The build
   script auto-generates the type as `{extension-name-without-v2}-card`. Don't override
   unless you have a very good reason.
6. **Every component**: `forwardRef`, loading / error / empty states.

## Project Structure

```
frontend/
├── src/
│   └── index.tsx          # Component implementation
├── package.json
├── vite.config.ts
├── tsconfig.json
└── frontend.json          # Component manifest
```

## Minimal Component

```tsx
// src/index.tsx
import { forwardRef, useState, useEffect } from 'react'

export interface ExtensionComponentProps {
  title?: string
  dataSource?: {
    type: string
    deviceId?: string
    device_id?: string
    extensionId?: string
    command?: string
    config?: Record<string, any>
    [key: string]: any
  }
  className?: string
  config?: Record<string, any>
}

const getApiBase = (): string =>
  (typeof window !== 'undefined' && (window as any).__TAURI__)
    ? 'http://localhost:9375/api'
    : '/api'

async function executeExtensionCommand<T>(
  extensionId: string,
  command: string,
  args: Record<string, any>,
): Promise<{ success: boolean; data?: T; error?: string }> {
  const r = await fetch(`${getApiBase()}/extensions/${extensionId}/command`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ command, args }),
  })
  if (!r.ok) throw new Error(`HTTP ${r.status}: ${r.statusText}`)
  return r.json()
}

async function getExtensionMetrics(extensionId: string): Promise<Record<string, any>> {
  const r = await fetch(`${getApiBase()}/extensions/${extensionId}/metrics`)
  if (!r.ok) throw new Error(`HTTP ${r.status}`)
  return r.json()
}

export const YourCard = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function YourCard(props, ref) {
    const { title = 'Your Extension', dataSource, className = '' } = props
    const [data, setData] = useState<any>(null)
    const [loading, setLoading] = useState(false)
    const [error, setError] = useState<string | null>(null)
    const extensionId = dataSource?.extensionId || 'your-extension-v2'

    useEffect(() => {
      (async () => {
        setLoading(true); setError(null)
        try {
          const r = await executeExtensionCommand<any>(extensionId, 'get_data', {})
          if (r.success) setData(r.data)
          else setError(r.error || 'Unknown error')
        } catch (e) {
          setError(e instanceof Error ? e.message : String(e))
        } finally {
          setLoading(false)
        }
      })()
    }, [extensionId])

    return (
      <div ref={ref} className={`your-ext-card ${className}`}>
        <style>{`
          /* === Colors via NeoMind CSS variables — light AND dark mode both work === */
          .your-ext-card {
            --your-ext-bg: var(--card);
            --your-ext-fg: var(--foreground);
            --your-ext-muted: var(--muted-foreground);
            --your-ext-border: var(--border);
            --your-ext-accent: var(--primary);
            --your-ext-on-primary: var(--primary-foreground);  /* see note below */

            background: var(--your-ext-bg);
            color: var(--your-ext-fg);
            border: 1px solid var(--your-ext-border);
            border-radius: 8px;
            padding: 16px;
          }

          .your-ext-card-header {
            display: flex; justify-content: space-between; align-items: center;
            margin-bottom: 16px;
          }
          .your-ext-card-header h3 { margin: 0; font-size: 18px; font-weight: 600; }

          .your-ext-card button.primary {
            background: var(--your-ext-accent);
            color: var(--your-ext-on-primary);   /* NEVER #fff / white */
            border: none;
            padding: 8px 16px;
            border-radius: 4px;
            cursor: pointer;
          }
          .your-ext-card button.primary:hover { opacity: 0.9; }
          .your-ext-card button.primary:disabled { opacity: 0.5; cursor: not-allowed; }

          .your-ext-card .loading, .your-ext-card .error {
            padding: 16px; text-align: center;
          }
          .your-ext-card .error { color: var(--destructive); }
          .your-ext-card .empty {
            padding: 24px; text-align: center; color: var(--muted-foreground);
          }
        `}</style>

        <div className="your-ext-card-header">
          <h3>{title}</h3>
        </div>

        {loading && <div className="loading">Loading…</div>}
        {error && <div className="error">{error}</div>}
        {!loading && !error && !data && <div className="empty">No data</div>}
        {data && !loading && (
          <pre style={{ margin: 0 }}>{JSON.stringify(data, null, 2)}</pre>
        )}
      </div>
    )
  },
)

export default { YourCard }
```

> **Note on `--on-primary`:** design guide §5.1 wants `var(--{prefix}-on-primary)`. The
> host app defines this on a parent element; you should reference the prefixed form so
> your button adapts when nested inside a colored container. If the host doesn't define
> it yet, fall back with `var(--your-ext-on-primary, var(--primary-foreground))`.

## frontend.json — full schema

```jsonc
{
  "id": "your-extension-v2",
  "version": "2.0.0",
  "entrypoint": "your-extension-v2-components.umd.cjs",
  "components": [
    {
      "name": "YourCard",
      "type": "your-extension-card",             // MUST be unique across all extensions
      "displayName": "Your Extension Card",
      "description": "Displays data from your extension",
      "icon": "cpu",                              // inline SVG icon name
      "defaultSize": { "width": 340, "height": 320 },
      "minSize": { "width": 240, "height": 260 },
      "maxSize": { "width": 480, "height": 400 },

      "refreshable": true,                        // shows refresh button
      "refreshInterval": 30000,                   // auto-refresh ms

      "hasDataSource": true,                      // shows Data Source tab in config dialog
      "dataSourceAllowedTypes": ["device"],       // see allowed-types table below

      "configSchema": {                           // FLAT object — each key is a field
        "contentType": {
          "type": "string",
          "title": "Content Type",
          "description": "What to show",
          "enum": ["none", "text", "markdown", "image-url"],
          "enumTitles": ["None", "Plain Text", "Markdown", "Image URL"],
          "default": "none"
        },
        "textContent": {
          "type": "string",
          "title": "Text Content",
          "description": "Content for text/markdown/html mode"
        },
        "imageUrl": {
          "type": "string",
          "title": "Image URL"
        }
      },

      "uiHints": {
        "fieldOrder": ["contentType", "textContent", "imageUrl"],
        "visibilityRules": [
          { "field": "contentType", "condition": "equals", "value": "text",
            "thenShow": ["textContent"] },
          { "field": "contentType", "condition": "equals", "value": "markdown",
            "thenShow": ["textContent"] },
          { "field": "contentType", "condition": "equals", "value": "image-url",
            "thenShow": ["imageUrl"] }
        ]
      }
    }
  ],
  "dependencies": { "react": ">=18.0.0" }
}
```

### configSchema field properties

| Property | Applies to | Effect |
|---|---|---|
| `type` | all | `"string"` / `"number"` / `"integer"` / `"boolean"` |
| `title` | all | Field label |
| `description` | all | Help text / placeholder |
| `default` | all | Default value |
| `enum` + `enumTitles` | string | Renders as **dropdown** instead of text input |
| `min` / `max` | number/integer | Range validation |

### uiHints — conditional field visibility

Fields listed in any `thenShow` are **hidden by default** and only shown when their rule
matches. Fields not listed in any `thenShow` rule are always visible.

```jsonc
"uiHints": {
  "fieldOrder": ["mode", "url", "token"],
  "visibilityRules": [
    { "field": "mode", "condition": "equals", "value": "advanced",
      "thenShow": ["token"] }
  ]
}
```

**Supported conditions:** `equals`, `not_equals`, `contains`, `empty`, `not_empty`

### dataSourceAllowedTypes

| Type | Description |
|---|---|
| `device` | Whole device selection (use for device-targeting components) |
| `device-metric` | Specific metric on a device |
| `extension` | Another extension |
| `extension-command` | A command of another extension |
| `system` | System metrics |
| `ai-metric` | AI-derived metric |
| `transform` | Transformed metric |

Default (when `hasDataSource: true` but no types specified): `["device-metric", "extension", "extension-command"]`.

The bound data source is passed to the component as `props.dataSource`. Read the device id
from either `dataSource.deviceId` or `dataSource.device_id` (both appear in the wild):

```ts
const boundDeviceId = props.dataSource?.deviceId || props.dataSource?.device_id
```

## Vite config (UMD, all React packages external)

```ts
// frontend/vite.config.ts
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'path'

export default defineConfig({
  plugins: [react()],
  build: {
    lib: {
      entry: path.resolve(__dirname, 'src/index.tsx'),
      name: 'YourExtensionV2Components',
      formats: ['umd', 'cjs'],
      fileName: (format) =>
        `your-extension-v2-components.${format === 'umd' ? 'umd.js' : 'umd.cjs'}`,
    },
    rollupOptions: {
      // ALL of these are provided by the host — bundling them breaks at runtime.
      external: ['react', 'react-dom', 'react/jsx-runtime'],
      output: {
        globals: {
          react: 'React',
          'react-dom': 'ReactDOM',
        },
      },
    },
    outDir: 'dist',
    emptyOutDir: true,
  },
})
```

> **`react/jsx-runtime` MUST be external** — without this, recent commits caused subtle
> bundle-format bugs (commit e7dc399 fixed it across all extensions).

## package.json

```json
{
  "name": "your-extension-v2-frontend",
  "version": "2.0.0",
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
  }
}
```

## Build & install

```bash
cd frontend
npm install
npm run build
# Output: dist/your-extension-v2-components.umd.cjs

# Dev install (auto-handled by build.sh --dev):
./build.sh --dev --single your-extension-v2
```

## CSS variable reference

Prefer these over any hardcoded color:

| Variable | Typical use |
|---|---|
| `var(--background)` / `var(--foreground)` | Page-level bg / text |
| `var(--card)` / `var(--card-foreground)` | Card surfaces |
| `var(--muted)` / `var(--muted-foreground)` | Muted surfaces / secondary text |
| `var(--border)` | Borders |
| `var(--primary)` / `var(--primary-foreground)` | Primary accent + text on it |
| `var(--secondary)` / `var(--secondary-foreground)` | Secondary accent |
| `var(--destructive)` | Error / delete actions |
| `var(--accent)` / `var(--accent-foreground)` | Highlight accent |
| `var(--radius)` | Standard border radius |

If your component sits inside a colored container (e.g. a primary-colored panel), define
your own `--your-ext-on-primary` locally so text adapts:

```css
.your-ext-card {
  --your-ext-on-primary: var(--primary-foreground);
}
.your-ext-card .on-primary {
  background: var(--primary);
  color: var(--your-ext-on-primary);   /* not #fff */
}
```

## Common patterns

### Polling

```tsx
useEffect(() => {
  const id = setInterval(() => fetchData(), config?.updateInterval ?? 5000)
  return () => clearInterval(id)
}, [fetchData, config])
```

### Loading / error / empty states

```tsx
{loading && <div className="loading">Loading…</div>}
{!loading && error && <div className="error">{error}</div>}
{!loading && !error && !data && <div className="empty">No data</div>}
{!loading && !error && data && <DataDisplay data={data} />}
```

### Inline SVG icons (no icon libraries)

```tsx
const CpuIcon = ({ size = 16 }: { size?: number }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none"
       stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <rect x="4" y="4" width="16" height="16" rx="2" />
    <rect x="9" y="9" width="6" height="6" />
    <line x1="9" y1="1" x2="9" y2="4" />
    {/* ... */}
  </svg>
)
```

## Common pitfalls

| Symptom | Cause |
|---|---|
| Component missing from UI | Another extension has the same `type`. Rename yours. |
| "Failed to load details" in marketplace | `metadata.json` has `frontend.components` as objects instead of string array. Run `./scripts/update-versions.sh <ver>`. |
| Bundle works locally but breaks in host | Forgot to externalize `react/jsx-runtime`. |
| Colors wrong in dark mode | Hardcoded `#fff` / `rgb(...)` / `hsl(...)`. Replace with CSS vars. |
| Primary button text invisible on colored bg | Used `#fff` instead of `var(--{prefix}-on-primary)`. |
| Build works but Tailwind classes do nothing | Tailwind isn't available to extensions. Rewrite styles in plain CSS with vars. |
