# NeoMind Extension Frontend Design Guide

Complete design specification for extension frontend components, based on the NeoMind main project design system.

> **Core principle:** Extension components run inside the NeoMind Dashboard DOM and share the host's CSS variables and React runtime. All colors, spacing, and radii must be referenced through CSS variables — **never hardcode color values**.

---

## 1. CSS Variable Reference

### 1.1 Base Colors

| Variable | Light Mode | Dark Mode | Usage |
|----------|------------|-----------|-------|
| `--background` | `oklch(1 0 0 / 97%)` near-white | `oklch(0.13 0.008 270 / 97%)` dark | Page background |
| `--foreground` | `oklch(0.18 0.02 270)` near-black | `oklch(0.95 0.005 270)` near-white | Primary text |
| `--card` | `oklch(1 0 0 / 85%)` semi-transparent white | `oklch(0.16 0.008 270 / 80%)` semi-transparent dark | Card background |
| `--card-foreground` | near-black | near-white | Card text |
| `--muted` | `oklch(0.96 0.003 270)` light gray | `oklch(0.20 0.008 270)` dark gray | Muted background |
| `--muted-foreground` | `oklch(0.45 0.01 270)` gray | `oklch(0.63 0.012 270)` light gray | Secondary text |
| `--border` | `oklch(0 0 0 / 8%)` | `oklch(1 0 0 / 8%)` | Borders |
| `--input` | `oklch(0 0 0 / 10%)` | `oklch(1 0 0 / 10%)` | Input borders |

### 1.2 Primary (Buttons, Accents)

| Variable | Light Mode | Dark Mode | Usage |
|----------|------------|-----------|-------|
| `--primary` | `oklch(0.18 0.02 270)` **dark** | `oklch(0.95 0.005 270)` **light** | Primary button bg, brand color |
| `--primary-foreground` | `oklch(1 0 0)` **white** | `oklch(0.13 0.008 270)` **dark** | Text/icons on primary bg |
| `--primary-hover` | `oklch(0.18 0.02 270 / 90%)` | `oklch(0.95 0.005 270 / 90%)` | Primary button hover |

> **Key point:** `--primary-foreground` guarantees sufficient contrast against `--primary` in both modes.
> - Light: dark button + white text
> - Dark: light button + dark text

### 1.3 Secondary / Accent

| Variable | Light Mode | Dark Mode |
|----------|------------|-----------|
| `--secondary` | `oklch(0.96 0.003 270)` | `oklch(0.20 0.008 270)` |
| `--secondary-foreground` | `oklch(0.18 0.02 270)` | `oklch(0.95 0.005 270)` |
| `--accent` | `oklch(0.96 0.003 270)` | `oklch(0.20 0.008 270)` |
| `--accent-foreground` | near-black | near-white |

### 1.4 Semantic Colors

| Variable | Usage | Light Mode | Dark Mode |
|----------|-------|------------|-----------|
| `--color-success` | success / online | `oklch(0.55 0.17 155)` | `oklch(0.72 0.19 155)` |
| `--color-success-bg` | success background | 8% opacity | 10% opacity |
| `--color-warning` | warning / pending | `oklch(0.68 0.17 65)` | `oklch(0.72 0.16 85)` |
| `--color-warning-bg` | warning background | 8% opacity | 10% opacity |
| `--color-error` | error / failure | `oklch(0.55 0.22 25)` | `oklch(0.577 0.245 27)` |
| `--color-error-bg` | error background | 8% opacity | 10% opacity |
| `--color-info` | info / running | `oklch(0.52 0.15 250)` | `oklch(0.65 0.15 250)` |
| `--color-info-bg` | info background | 8% opacity | 10% opacity |

### 1.5 Destructive

| Variable | Usage |
|----------|-------|
| `--destructive` | Delete button background |
| `--destructive-foreground` | Delete button text (white in both modes) |
| `--destructive-hover` | Hover state |

### 1.6 Glass

| Variable | Light Mode | Dark Mode |
|----------|------------|-----------|
| `--glass` | `oklch(1 0 0 / 55%)` | `oklch(0.20 0.008 270 / 50%)` |
| `--glass-heavy` | `oklch(1 0 0 / 82%)` | `oklch(0.18 0.008 270 / 80%)` |
| `--glass-border` | `oklch(0 0 0 / 6%)` | `oklch(1 0 0 / 8%)` |

### 1.7 Shadows

| Variable | Usage |
|----------|-------|
| `--shadow-sm` | Subtle elevation |
| `--shadow-md` | Standard card shadow |
| `--shadow-lg` | Floating panel shadow |
| `--shadow-xl` | Dialog shadow |

### 1.8 Border Radius

| Variable | Value |
|----------|-------|
| `--radius-sm` | 6px |
| `--radius-md` | 8px |
| `--radius-lg` | 10px |
| `--radius-xl` | 12px |
| `--radius-2xl` | 16px |
| `--radius-full` | 9999px (pill/circle) |

### 1.9 Spacing

| Variable | Value |
|----------|-------|
| `--space-1` | 4px |
| `--space-2` | 8px |
| `--space-3` | 12px |
| `--space-4` | 16px |
| `--space-6` | 24px |
| `--space-8` | 32px |

### 1.10 Animation

| Variable | Value | Usage |
|----------|-------|-------|
| `--duration-fast` | 150ms | Hover, focus |
| `--duration-normal` | 200ms | General transitions |
| `--duration-slow` | 300ms | Layout changes |
| `--ease-out` | `cubic-bezier(0.16, 1, 0.3, 1)` | Enter |
| `--ease-in-out` | `cubic-bezier(0.4, 0, 0.2, 1)` | Bidirectional |
| `--ease-spring` | `cubic-bezier(0.34, 1.56, 0.64, 1)` | Spring |

---

## 2. Component Style Templates

### 2.1 Card

```css
.ext-card {
  background: var(--card);
  backdrop-filter: blur(12px);
  border: 1px solid var(--border);
  border-radius: var(--radius-lg, 10px);
  padding: 16px;
  box-shadow: var(--shadow-sm);
  box-sizing: border-box;
}
```

### 2.2 Button

Extensions don't have the main project's `cva` + Tailwind. Use pure CSS equivalents:

```css
/* Primary button — equivalent to variant="default" */
.ext-btn-primary {
  background: var(--primary);
  color: var(--ext-on-primary);   /* NOT #fff, NOT bare var(--primary-foreground) */
  border: none;
  border-radius: var(--radius-md, 8px);
  padding: 8px 16px;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: background var(--duration-fast) var(--ease-out);
}
.ext-btn-primary:hover {
  background: var(--primary-hover);
}
.ext-btn-primary:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* Outline button — equivalent to variant="outline" */
.ext-btn-outline {
  background: transparent;
  color: var(--foreground);
  border: 1px solid var(--border);
  border-radius: var(--radius-md, 8px);
  padding: 8px 16px;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: background var(--duration-fast) var(--ease-out);
}
.ext-btn-outline:hover {
  background: var(--accent);
  color: var(--accent-foreground);
}

/* Destructive button — equivalent to variant="destructive" */
.ext-btn-destructive {
  background: var(--destructive);
  color: var(--destructive-foreground);
  border: none;
  border-radius: var(--radius-md, 8px);
  padding: 8px 16px;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
}
.ext-btn-destructive:hover {
  background: var(--destructive-hover);
}

/* Ghost button */
.ext-btn-ghost {
  background: transparent;
  color: var(--foreground);
  border: none;
  border-radius: var(--radius-md, 8px);
  padding: 8px 16px;
  font-size: 13px;
  cursor: pointer;
}
.ext-btn-ghost:hover {
  background: var(--accent);
}
```

> **Most important rule:** Text on primary buttons **must** use `var(--{prefix}-on-primary)` with a fallback — never `#fff`, `white`, or bare `var(--primary-foreground)`.
> See [Section 5.1](#51---on-primary-fallback-pattern-critical) for details.

### 2.3 Badge

```css
.ext-badge {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 2px 8px;
  border-radius: var(--radius-full, 9999px);
  font-size: 11px;
  font-weight: 500;
  border: 1px solid transparent;
}

.ext-badge-default {
  background: var(--primary);
  color: var(--ext-on-primary);
}

.ext-badge-secondary {
  background: var(--secondary);
  color: var(--secondary-foreground);
}

.ext-badge-destructive {
  background: var(--destructive);
  color: var(--destructive-foreground);
}

.ext-badge-success {
  background: var(--color-success-bg);
  color: var(--color-success);
}
.ext-badge-warning {
  background: var(--color-warning-bg);
  color: var(--color-warning);
}
.ext-badge-error {
  background: var(--color-error-bg);
  color: var(--color-error);
}
.ext-badge-info {
  background: var(--color-info-bg);
  color: var(--color-info);
}
```

### 2.4 Input

```css
.ext-input {
  width: 100%;
  padding: 8px 12px;
  border: 1px solid var(--input);
  border-radius: var(--radius-md, 8px);
  background: var(--card);
  color: var(--foreground);
  font-size: 13px;
  box-sizing: border-box;
  transition: border-color var(--duration-fast) var(--ease-out);
}
.ext-input:focus {
  outline: none;
  border-color: var(--primary);
  box-shadow: 0 0 0 2px oklch(0.18 0.02 270 / 10%);
}
.ext-input::placeholder {
  color: var(--muted-foreground);
}
.dark .ext-input:focus {
  box-shadow: 0 0 0 2px oklch(0.95 0.005 270 / 10%);
}
```

### 2.5 Select

```css
.ext-select {
  width: 100%;
  padding: 8px 12px;
  border: 1px solid var(--input);
  border-radius: var(--radius-md, 8px);
  background: var(--card);
  color: var(--foreground);
  font-size: 13px;
  box-sizing: border-box;
  cursor: pointer;
}
.ext-select:focus {
  outline: none;
  border-color: var(--primary);
}
```

### 2.6 Error Display

```css
.ext-error-box {
  padding: 8px 12px;
  background: var(--color-error-bg);
  border: 1px solid var(--color-error);
  border-radius: var(--radius-md, 8px);
  color: var(--color-error);
  font-size: 12px;
}
```

### 2.7 Loading Spinner

```css
.ext-loading {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  flex: 1;
  gap: 8px;
  color: var(--muted-foreground);
  font-size: 12px;
}

.ext-spinner {
  width: 24px;
  height: 24px;
  border: 2px solid var(--border);
  border-top-color: var(--primary);
  border-radius: 50%;
  animation: ext-spin 0.7s linear infinite;
}

@keyframes ext-spin {
  to { transform: rotate(360deg); }
}
```

---

## 3. Component Structure Template

### 3.1 Standard Extension Card

```tsx
import { forwardRef, useState, useEffect, useCallback, useMemo } from 'react'

// ============================================================================
// Types
// ============================================================================

export interface ExtensionComponentProps {
  title?: string
  dataSource?: { type: string; extensionId?: string; [key: string]: any }
  className?: string
  config?: Record<string, any>
}

// ============================================================================
// API
// ============================================================================

const EXTENSION_ID = 'my-extension'

const getApiBase = (): string =>
  (window as any).__TAURI__ ? 'http://localhost:9375/api' : '/api'

const getAuthHeaders = (): Record<string, string> => {
  const token =
    localStorage.getItem('neomind_token') ||
    sessionStorage.getItem('neomind_token_session')
  const headers: Record<string, string> = { 'Content-Type': 'application/json' }
  if (token) headers['Authorization'] = `Bearer ${token}`
  return headers
}

async function executeCommand<T = any>(
  extensionId: string,
  command: string,
  args: Record<string, unknown> = {}
): Promise<{ success: boolean; data?: T; error?: string }> {
  try {
    const res = await fetch(
      `${getApiBase()}/extensions/${extensionId}/command`,
      {
        method: 'POST',
        headers: getAuthHeaders(),
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

// ============================================================================
// Scoped CSS (injected once)
// ============================================================================

const STYLE_ID = 'my-ext-styles-v1'

const STYLES = `
.my-ext {
  --ext-fg: var(--foreground);
  --ext-muted: var(--muted-foreground);
  --ext-accent: var(--primary);
  --ext-on-primary: var(--primary-foreground, #ffffff);
  --ext-card: var(--card);
  --ext-border: var(--border);
  --ext-success: var(--color-success);
  --ext-warning: var(--color-warning);
  --ext-error: var(--color-error);
  --ext-info: var(--color-info);
  width: 100%;
  height: 100%;
  font-size: 12px;
}

.dark .my-ext {
  --ext-on-primary: var(--primary-foreground, #17172a);
}

.my-ext-card {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: 16px;
  background: var(--ext-card);
  backdrop-filter: blur(12px);
  border: 1px solid var(--ext-border);
  border-radius: var(--radius-lg, 10px);
  box-shadow: var(--shadow-sm);
  box-sizing: border-box;
}

.my-ext-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-shrink: 0;
  margin-bottom: 12px;
}

.my-ext-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 14px;
  font-weight: 600;
  color: var(--ext-fg);
}

.my-ext-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 0;
  overflow: hidden;
}

.my-ext-btn {
  padding: 8px 16px;
  border: 1px solid var(--ext-border);
  border-radius: var(--radius-md, 8px);
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: all var(--duration-fast) var(--ease-out);
  background: transparent;
  color: var(--ext-fg);
}
.my-ext-btn:hover {
  background: var(--accent);
}
.my-ext-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.my-ext-btn-primary {
  background: var(--ext-accent);
  border-color: var(--ext-accent);
  color: var(--ext-on-primary);
}
.my-ext-btn-primary:hover {
  background: var(--primary-hover);
}

.my-ext-error {
  padding: 8px 12px;
  background: var(--color-error-bg);
  border: 1px solid var(--color-error);
  border-radius: var(--radius-md, 8px);
  color: var(--color-error);
  font-size: 12px;
}
`

function injectStyles() {
  if (typeof document === 'undefined' || document.getElementById(STYLE_ID)) return
  const style = document.createElement('style')
  style.id = STYLE_ID
  style.textContent = STYLES
  document.head.appendChild(style)
}

// ============================================================================
// Component
// ============================================================================

export const MyCard = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function MyCard(props, ref) {
    const { title = 'My Extension', dataSource, className = '', config } = props
    const extensionId = dataSource?.extensionId || EXTENSION_ID

    const [data, setData] = useState<any>(null)
    const [loading, setLoading] = useState(false)
    const [error, setError] = useState<string | null>(null)

    useEffect(() => injectStyles(), [])

    // ... business logic ...

    return (
      <div ref={ref} className={`my-ext ${className}`}>
        <div className="my-ext-card">
          {/* Header */}
          <div className="my-ext-header">
            <div className="my-ext-title">{title}</div>
          </div>

          {/* Content */}
          <div className="my-ext-content">
            {loading ? (
              <div className="ext-loading">
                <div className="ext-spinner" />
                <span>Loading...</span>
              </div>
            ) : error ? (
              <div className="my-ext-error">{error}</div>
            ) : data ? (
              <div>{/* Data display */}</div>
            ) : (
              <div style={{ color: 'var(--muted-foreground)', textAlign: 'center', padding: 20 }}>
                No data
              </div>
            )}
          </div>
        </div>
      </div>
    )
  }
)

MyCard.displayName = 'MyCard'
export default { MyCard }
```

---

## 4. Style Injection Strategy

### Recommended: `injectStyles()` function

```tsx
const STYLE_ID = 'my-ext-styles-v1'
const STYLES = `...`

function injectStyles() {
  if (typeof document === 'undefined' || document.getElementById(STYLE_ID)) return
  const style = document.createElement('style')
  style.id = STYLE_ID
  style.textContent = STYLES
  document.head.appendChild(style)
}

// Call in component
useEffect(() => injectStyles(), [])
```

**Advantages:**
- Styles injected only once (even if component is instantiated multiple times)
- Deduplication via `STYLE_ID`
- Does not depend on inline `<style>` tags

### Not recommended: Inline `<style>` tag

```tsx
// Bad — creates a new style element on every render
<div>
  <style>{CSS}</style>
  {/* ... */}
</div>
```

---

## 5. CSS Variable Mapping Pattern

Each extension should define its own CSS variable aliases in the root class, mapping to NeoMind variables:

```css
.my-ext {
  --ext-fg: var(--foreground);
  --ext-muted: var(--muted-foreground);
  --ext-accent: var(--primary);
  --ext-card: var(--card);
  --ext-border: var(--border);
  --ext-success: var(--color-success);
  --ext-warning: var(--color-warning);
  --ext-error: var(--color-error);
  --ext-info: var(--color-info);
  /* Primary button text — MUST have fallback */
  --ext-on-primary: var(--primary-foreground, #ffffff);
}

.dark .my-ext {
  --ext-on-primary: var(--primary-foreground, #17172a);
}
```

**Benefits:**
- Child classes use `var(--ext-fg)` instead of `var(--foreground)`
- Override a single color by changing only the mapping layer
- Built-in fallback values

### 5.1 `--on-primary` Fallback Pattern (Critical)

`var(--primary-foreground)` may be **undefined** in some environments (e.g. older NeoMind versions). When undefined, button text inherits the parent's dark foreground color, becoming invisible against the dark `--primary` background.

**Always** use a local variable with fallback:

```css
/* Light mode fallback = white */
.my-ext {
  --ext-on-primary: var(--primary-foreground, #ffffff);
}
/* Dark mode fallback = dark */
.dark .my-ext {
  --ext-on-primary: var(--primary-foreground, #17172a);
}

/* Usage */
.my-ext-btn-primary {
  background: var(--ext-accent);
  color: var(--ext-on-primary);  /* Never use bare var(--primary-foreground) */
}
```

> **Rule:** Any text or icon rendered on a `--primary` background MUST use `--{prefix}-on-primary` with fallback — never bare `var(--primary-foreground)`.

---

## 6. Hover & Interaction States

### Hover backgrounds in light/dark mode

```css
.ext-item:hover {
  background: rgba(0, 0, 0, 0.03);  /* Light: subtle darken */
}

.dark .ext-item:hover {
  background: rgba(255, 255, 255, 0.03);  /* Dark: subtle lighten */
}
```

Or use the main project's `--accent` variable:

```css
.ext-item:hover {
  background: var(--accent);
}
```

---

## 7. Special Scenarios

### 7.1 Video/Image Overlays (White text allowed)

Video player controls and image annotation overlays **may** use `color: white` since they overlay media content:

```css
/* Allowed: video control overlay */
.ext-video-overlay {
  background: linear-gradient(to top, rgba(0,0,0,0.75), transparent);
  color: white;
}

/* Allowed: video area background */
.ext-video-wrap {
  background: #000;
}
```

### 7.2 Popups / Dialogs

```css
.ext-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.5);  /* Allowed */
  z-index: 1000;
  display: flex;
  align-items: center;
  justify-content: center;
}

.ext-dialog {
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: var(--radius-xl, 12px);
  padding: 20px;
  box-shadow: var(--shadow-xl);
}
```

---

## 8. Common Mistakes

| Wrong | Correct | Reason |
|-------|---------|--------|
| `color: #fff` | `color: var(--ext-on-primary)` | Button text on primary bg |
| `color: white` | `color: var(--ext-on-primary)` | Button text on primary bg |
| `color: var(--primary-foreground)` | `color: var(--ext-on-primary)` | Missing fallback — may be invisible |
| `color: #000` | `color: var(--foreground)` | General text |
| `background: white` | `background: var(--card)` | Card background |
| `background: #f5f5f5` | `background: var(--muted)` | Muted background |
| `border: 1px solid #ddd` | `border: 1px solid var(--border)` | Border |
| `color: #666` | `color: var(--muted-foreground)` | Secondary text |
| `color: #ef4444` | `color: var(--color-error)` | Error text |
| `background: #ffebee` | `background: var(--color-error-bg)` | Error background |
| `border-radius: 8px` | `border-radius: var(--radius-md, 8px)` | Border radius |
| `className="hidden"` | `style={{ display: 'none' }}` | No Tailwind available |

---

## 9. CSS Class Naming Convention

Use `{extension-prefix}-{element}` format:

```
.{ext}                → Root container
.{ext}-card           → Card wrapper
.{ext}-header         → Header section
.{ext}-title          → Title text
.{ext}-content        → Main content area
.{ext}-footer         → Footer section
.{ext}-btn            → Button
.{ext}-btn-primary    → Primary button
.{ext}-btn-danger     → Destructive button
.{ext}-badge          → Badge
.{ext}-input          → Input field
.{ext}-error          → Error message
.{ext}-spinner        → Loading spinner
.{ext}-loading        → Loading state
.{ext}-empty          → Empty state
```

Extension ID prefix reference:
- `weather-` → weather-forecast-v2
- `ia-` → image-analyzer-v2
- `yolo-` → yolo-video-v2
- `ydi-` → yolo-device-inference
- `ocr-` → ocr-device-inference
- `frc-` → face-recognition
- `dbc-` → device-binding-card (yolo-device-inference sub-component)

---

## 10. forwardRef Requirement

All exported extension components **must** use `forwardRef` and pass `ref` to the root DOM element:

```tsx
// Correct
export const MyCard = forwardRef<HTMLDivElement, Props>(
  function MyCard(props, ref) {
    return (
      <div ref={ref} className="my-ext">
        {/* ... */}
      </div>
    )
  }
)

// Wrong — missing forwardRef
export const MyCard: React.FC<Props> = (props) => {
  return <div className="my-ext">{/* ... */}</div>
}
```

---

## 11. i18n Pattern (Lightweight)

Extensions don't have the main project's i18next. Use a lightweight approach:

```tsx
type Locale = 'en' | 'zh'

function detectLocale(): Locale {
  const stored = localStorage.getItem('i18nextLng') || ''
  if (stored.startsWith('zh')) return 'zh'
  if (stored.startsWith('en')) return 'en'
  return navigator.language.startsWith('zh') ? 'zh' : 'en'
}

const T: Record<string, Record<Locale, string>> = {
  title:    { en: 'My Extension', zh: '我的扩展' },
  loading:  { en: 'Loading...', zh: '加载中...' },
  error:    { en: 'Error', zh: '错误' },
}

// Usage in component
const locale = useMemo(() => detectLocale(), [])
const t = useCallback((key: string) => T[key]?.[locale] ?? key, [locale])
```

---

## 12. Build Requirements

### Vite Config

```typescript
export default defineConfig({
  plugins: [react()],
  build: {
    lib: {
      entry: 'src/index.tsx',
      name: 'MyExtensionComponents',
      formats: ['umd'],
      fileName: () => 'my-extension-components.umd.cjs'
    },
    rollupOptions: {
      external: ['react', 'react-dom', 'react/jsx-runtime'],
      output: {
        globals: {
          react: 'React',
          'react-dom': 'ReactDOM',
          'react/jsx-runtime': 'jsxRuntime',
        },
      },
    },
  },
})
```

### Key rules
- React/ReactDOM **must** be external
- Output format **must** be UMD (`.umd.cjs`)
- Filename **must** match `frontend.json` `entrypoint`
- **No** Tailwind CSS — extensions don't have Tailwind
- Use `tsconfig.json` strict mode

---

## Quick Reference

| Resource | Location |
|----------|----------|
| Main project design spec | `NeoMind/web/DESIGN_SPEC.md` |
| CSS variable definitions | `NeoMind/web/src/index.css` |
| Main project Button component | `NeoMind/web/src/components/ui/button.tsx` |
| Main project Card component | `NeoMind/web/src/components/ui/card.tsx` |
| Dashboard component wrapper | `NeoMind/web/src/components/dashboard/DashboardComponentWrapper.tsx` |
| Extension loader | `NeoMind/web/src/components/dashboard/ExtensionCardWrapper.tsx` |
| Existing extension examples | `NeoMind-Extensions/extensions/` |
