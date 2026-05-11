# Frontend Component Guide

## Overview

NeoMind extensions can include React-based dashboard components that interact with backend commands through a REST API.

## Project Structure

```
frontend/
├── src/
│   └── index.tsx          # Component implementation
├── package.json           # Dependencies and build scripts
├── vite.config.ts         # Vite configuration
├── tsconfig.json          # TypeScript configuration
├── frontend.json          # Component manifest
└── README.md              # Documentation
```

## Component Template

```tsx
import { forwardRef, useState, useEffect, useCallback } from 'react'

// SDK Type Definitions
export interface ExtensionComponentProps {
  title?: string
  dataSource?: DataSource
  className?: string
  config?: Record<string, any>
}

export interface DataSource {
  type: string
  extensionId?: string
  [key: string]: any
}

// Component Implementation
export const YourCard = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function YourCard(props, ref) {
    const { title = 'Default Title', dataSource, className = '', config } = props

    // State
    const [data, setData] = useState<any>(null)
    const [loading, setLoading] = useState(false)
    const [error, setError] = useState<string | null>(null)

    // Extension ID from dataSource or default
    const extensionId = dataSource?.extensionId || 'your-extension-v2'

    // Fetch data from extension
    const fetchData = useCallback(async () => {
      setLoading(true)
      setError(null)

      try {
        const result = await executeExtensionCommand(
          extensionId,
          'get_data',
          {}
        )

        if (result.success) {
          setData(result.data)
        } else {
          setError(result.error || 'Unknown error')
        }
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err))
      } finally {
        setLoading(false)
      }
    }, [extensionId])

    // Load data on mount
    useEffect(() => {
      fetchData()
    }, [fetchData])

    return (
      <div ref={ref} className={`your-card ${className}`}>
        <style>{styles}</style>

        <div className="your-card-header">
          <h3>{title}</h3>
          <button onClick={fetchData} disabled={loading}>
            Refresh
          </button>
        </div>

        <div className="your-card-content">
          {loading && <div className="loading">Loading...</div>}
          {error && <div className="error">{error}</div>}
          {data && !loading && (
            <div className="data-display">
              {/* Render your data */}
              <pre>{JSON.stringify(data, null, 2)}</pre>
            </div>
          )}
        </div>
      </div>
    )
  }
)

// CSS Styles with CSS Variables
const styles = `
  .your-card {
    /* Light mode (default) */
    --ext-bg: rgba(255, 255, 255, 0.25);
    --ext-fg: hsl(240 10% 10%);
    --ext-muted: hsl(240 5% 40%);
    --ext-border: rgba(255, 255, 255, 0.5);
    --ext-accent: hsl(221 83% 53%);

    background: var(--ext-bg);
    color: var(--ext-fg);
    border: 1px solid var(--ext-border);
    border-radius: 8px;
    padding: 16px;
    backdrop-filter: blur(10px);
  }

  /* Dark mode */
  .dark .your-card {
    --ext-bg: rgba(30, 30, 30, 0.4);
    --ext-fg: hsl(0 0% 95%);
    --ext-muted: hsl(0 0% 65%);
  }

  .your-card-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 16px;
  }

  .your-card-header h3 {
    margin: 0;
    font-size: 18px;
    font-weight: 600;
  }

  button {
    background: var(--ext-accent);
    color: white;
    border: none;
    padding: 8px 16px;
    border-radius: 4px;
    cursor: pointer;
  }

  button:hover {
    opacity: 0.8;
  }

  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .loading, .error {
    padding: 16px;
    text-align: center;
  }

  .error {
    color: #ef4444;
  }
`

// Export component
export default { YourCard }
```

## API Helper Functions

```typescript
// Get API base URL (works in both browser and Tauri)
const getApiBase = (): string => {
  if (typeof window !== 'undefined' && (window as any).__TAURI__) {
    return 'http://localhost:9375/api'
  }
  return '/api'
}

// Execute extension command
async function executeExtensionCommand<T>(
  extensionId: string,
  command: string,
  args: Record<string, any>
): Promise<{ success: boolean; data?: T; error?: string }> {
  const apiBase = getApiBase()

  const response = await fetch(`${apiBase}/extensions/${extensionId}/command`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ command, args })
  })

  if (!response.ok) {
    throw new Error(`HTTP ${response.status}: ${response.statusText}`)
  }

  return response.json()
}

// Get extension metrics
async function getExtensionMetrics(
  extensionId: string
): Promise<Record<string, any>> {
  const apiBase = getApiBase()

  const response = await fetch(`${apiBase}/extensions/${extensionId}/metrics`)

  if (!response.ok) {
    throw new Error(`HTTP ${response.status}: ${response.statusText}`)
  }

  return response.json()
}
```

## Frontend Manifest (frontend.json)

```json
{
  "id": "your-extension-v2",
  "version": "2.0.0",
  "entrypoint": "your-extension-v2-components.umd.js",
  "components": [
    {
      "name": "YourCard",
      "type": "card",
      "displayName": "Your Extension Card",
      "description": "Displays data from your extension",
      "defaultSize": {
        "width": 300,
        "height": 200
      },
      "minSize": {
        "width": 200,
        "height": 150
      },
      "maxSize": {
        "width": 800,
        "height": 600
      },
      "refreshable": true,
      "refreshInterval": 5000,
      "hasDataSource": true,
      "dataSourceAllowedTypes": ["device-metric", "extension"],
      "configSchema": {
        "showDetails": {
          "type": "boolean",
          "title": "Show Details",
          "default": true
        },
        "updateInterval": {
          "type": "number",
          "title": "Update Interval (ms)",
          "default": 5000
        },
        "displayMode": {
          "type": "string",
          "title": "Display Mode",
          "enum": ["auto", "dark", "light"],
          "enumTitles": ["Auto", "Dark", "Light"],
          "default": "auto"
        }
      },
      "uiHints": {
        "fieldOrder": ["displayMode", "showDetails", "updateInterval"],
        "visibilityRules": []
      }
    }
  ],
  "dependencies": {
    "react": ">=18.0.0",
    "react-dom": ">=18.0.0"
  }
}
```

## Vite Configuration (vite.config.ts)

```typescript
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'path'

export default defineConfig({
  plugins: [react()],
  build: {
    lib: {
      entry: path.resolve(__dirname, 'src/index.tsx'),
      name: 'YourExtensionComponents',
      fileName: () => 'your-extension-v2-components.umd.js',
      formats: ['umd']
    },
    rollupOptions: {
      external: ['react', 'react-dom'],
      output: {
        globals: {
          react: 'React',
          'react-dom': 'ReactDOM'
        }
      }
    },
    outDir: 'dist',
    emptyOutDir: true
  }
})
```

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

## Building Frontend

```bash
cd frontend
npm install
npm run build

# Output: dist/your-extension-v2-components.umd.js
```

## Installing Frontend

```bash
# Copy frontend to extension directory
mkdir -p ~/.neomind/extensions/your-extension-v2/frontend
cp -r frontend/dist/* ~/.neomind/extensions/your-extension-v2/frontend/
cp frontend/frontend.json ~/.neomind/extensions/your-extension-v2/
```

## CSS Variable Theming

Use CSS variables for consistent theming:

```css
/* Light Mode */
.your-component {
  --ext-bg: rgba(255, 255, 255, 0.25);
  --ext-fg: hsl(240 10% 10%);
  --ext-muted: hsl(240 5% 40%);
  --ext-border: rgba(255, 255, 255, 0.5);
  --ext-accent: hsl(221 83% 53%);
  --ext-success: hsl(142 76% 36%);
  --ext-warning: hsl(38 92% 50%);
  --ext-error: hsl(0 84% 60%);
}

/* Dark Mode */
.dark .your-component {
  --ext-bg: rgba(30, 30, 30, 0.4);
  --ext-fg: hsl(0 0% 95%);
  --ext-muted: hsl(0 0% 65%);
  --ext-border: rgba(255, 255, 255, 0.1);
}
```

## Common Patterns

### Polling for Updates

```typescript
useEffect(() => {
  const interval = setInterval(() => {
    fetchData()
  }, config?.updateInterval || 5000)

  return () => clearInterval(interval)
}, [fetchData, config])
```

### Loading States

```typescript
{loading && <div className="spinner" />}
{!loading && data && <DataDisplay data={data} />}
{!loading && !data && <EmptyState />}
```

### Error Boundary

```typescript
try {
  const result = await executeExtensionCommand(...)
  if (result.success) {
    // Handle success
  } else {
    setError(result.error || 'Unknown error')
  }
} catch (err) {
  setError(err instanceof Error ? err.message : String(err))
}
```
