const CSS_ID = 'uink-styles'

export const STYLES = `
/* Root variable aliases */
.uink-root {
  --uink-fg: var(--foreground);
  --uink-muted: var(--muted-foreground);
  --uink-accent: var(--primary);
  --uink-on-primary: var(--primary-foreground, #ffffff);
  --uink-card: var(--card);
  --uink-border: var(--border);
  --uink-success: var(--color-success);
  --uink-error: var(--color-error);
  width: 100%;
  height: 100%;
  font-size: 13px;
  box-sizing: border-box;
}
.dark .uink-root {
  --uink-on-primary: var(--primary-foreground, #17172a);
}

/* Card — full bleed, no border */
.uink-card {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--uink-card);
  border: none;
  border-radius: var(--radius-lg, 10px);
  box-shadow: var(--shadow-sm);
  box-sizing: border-box;
  position: relative;
  overflow: hidden;
}

/* Preview — full bleed */
.uink-preview {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 0;
  overflow: hidden;
  position: relative;
  background: #1a1a1a;
}
.uink-preview img {
  width: 100%;
  height: 100%;
  object-fit: contain;
  display: block;
}
.uink-preview-placeholder {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  color: var(--uink-muted);
  font-size: 12px;
}

/* Floating overlay — bottom gradient with device info */
.uink-overlay {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 24px 12px 10px;
  background: linear-gradient(to top, rgba(0,0,0,0.55) 0%, rgba(0,0,0,0.25) 60%, transparent 100%);
  pointer-events: none;
}
.uink-overlay-text {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 1;
  min-width: 0;
}
.uink-device-name {
  font-weight: 600;
  font-size: 12px;
  color: #ffffff;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  text-shadow: 0 1px 3px rgba(0,0,0,0.4);
}
.uink-status {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  color: rgba(255,255,255,0.8);
  flex-shrink: 0;
  text-shadow: 0 1px 3px rgba(0,0,0,0.4);
}
.uink-status-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--uink-success);
  box-shadow: 0 0 4px rgba(52, 211, 153, 0.5);
}
.uink-status-dot.offline {
  background: rgba(255,255,255,0.4);
  box-shadow: none;
}
.uink-resolution {
  font-size: 10px;
  color: rgba(255,255,255,0.55);
  flex-shrink: 0;
  text-shadow: 0 1px 3px rgba(0,0,0,0.4);
}
.uink-refresh-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  padding: 0;
  border: none;
  border-radius: 50%;
  background: rgba(255,255,255,0.18);
  color: rgba(255,255,255,0.7);
  cursor: pointer;
  pointer-events: auto;
  flex-shrink: 0;
  transition: background 0.15s ease, color 0.15s ease;
}
.uink-refresh-btn:hover {
  background: rgba(255,255,255,0.35);
  color: #fff;
}

/* Device selector — floating bottom-right */
.uink-overlay-controls {
  position: absolute;
  bottom: 8px;
  right: 8px;
  z-index: 3;
}
.uink-device-select {
  appearance: none;
  -webkit-appearance: none;
  padding: 4px 24px 4px 8px;
  font-size: 11px;
  font-weight: 500;
  color: #fff;
  background: rgba(0,0,0,0.45);
  border: 1px solid rgba(255,255,255,0.15);
  border-radius: var(--radius-sm, 6px);
  cursor: pointer;
  backdrop-filter: blur(8px);
  max-width: 140px;
  text-overflow: ellipsis;
  overflow: hidden;
  white-space: nowrap;
  background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='white' stroke-width='2' stroke-linecap='round'%3E%3Cpolyline points='6 9 12 15 18 9'/%3E%3C/svg%3E");
  background-repeat: no-repeat;
  background-position: right 6px center;
}
.uink-device-select option {
  background: #1a1a2e;
  color: #fff;
}
.uink-device-select:hover {
  background-color: rgba(0,0,0,0.6);
  border-color: rgba(255,255,255,0.3);
}

/* Hover hint — shows "Click to edit" */
.uink-preview:hover .uink-edit-hint {
  opacity: 1;
}
.uink-edit-hint {
  position: absolute;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 14px;
  border-radius: var(--radius-md, 8px);
  background: rgba(0,0,0,0.6);
  color: #fff;
  font-size: 12px;
  font-weight: 500;
  opacity: 0;
  transition: opacity 0.2s ease;
  pointer-events: none;
  backdrop-filter: blur(4px);
}

/* Buttons */
.uink-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 6px 14px;
  font-size: 12px;
  font-weight: 500;
  border: none;
  border-radius: var(--radius-md, 8px);
  cursor: pointer;
  transition: background var(--duration-fast, 150ms) var(--ease-out, cubic-bezier(0.16,1,0.3,1));
}
.uink-btn:hover { opacity: 0.85; }
.uink-btn:disabled { opacity: 0.5; cursor: not-allowed; }
.uink-btn-primary {
  background: var(--uink-accent);
  color: var(--uink-on-primary);
}
.uink-btn-primary:hover {
  background: var(--primary-hover, var(--uink-accent));
}
.uink-btn-ghost {
  background: transparent;
  color: var(--uink-muted);
  border: 1px solid var(--uink-border);
}
.uink-btn-ghost:hover {
  background: var(--accent);
  color: var(--accent-foreground);
}
.uink-action-btn {
  padding: 4px 10px !important;
  font-size: 11px !important;
  gap: 4px !important;
  border-radius: var(--radius-sm, 6px) !important;
}

/* Loading / Error */
.uink-loading,
.uink-error {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  flex: 1;
  gap: 8px;
  color: var(--uink-muted);
  font-size: 12px;
}
.uink-spinner {
  width: 18px;
  height: 18px;
  border: 2px solid var(--uink-border);
  border-top-color: var(--uink-accent);
  border-radius: 50%;
  animation: uink-spin 0.7s linear infinite;
}
@keyframes uink-spin {
  to { transform: rotate(360deg); }
}

/* Toast */
.uink-toast {
  position: absolute;
  bottom: 10px;
  left: 50%;
  transform: translateX(-50%);
  padding: 5px 14px;
  border-radius: var(--radius-md, 8px);
  font-size: 11px;
  font-weight: 500;
  z-index: 10;
  animation: uink-toast-in 0.2s ease-out;
  pointer-events: none;
  white-space: nowrap;
}
.uink-toast.success { background: var(--uink-success); color: #fff; }
.uink-toast.error { background: var(--uink-error); color: #fff; }
@keyframes uink-toast-in {
  from { opacity: 0; transform: translateX(-50%) translateY(4px); }
  to { opacity: 1; transform: translateX(-50%) translateY(0); }
}

/* ======================== EDIT DIALOG ======================== */
.uink-overlay-backdrop {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  z-index: 9999;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.5);
  animation: uink-fade-in 0.15s ease-out;
  pointer-events: auto;
}
@keyframes uink-fade-in {
  from { opacity: 0; }
  to { opacity: 1; }
}
.uink-modal {
  background: var(--uink-card);
  border: 1px solid var(--uink-border);
  border-radius: var(--radius-xl, 12px);
  box-shadow: var(--shadow-xl);
  width: 520px;
  max-width: 92vw;
  max-height: 90vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  height: 480px;
  animation: uink-scale-in 0.15s ease-out;
}
@keyframes uink-scale-in {
  from { transform: scale(0.95); opacity: 0; }
  to { transform: scale(1); opacity: 1; }
}
.uink-modal-header {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 14px;
  border-bottom: 1px solid var(--uink-border);
  font-weight: 600;
  font-size: 13px;
  color: var(--uink-fg);
}
.uink-modal-header span:first-child { flex: 1; }
.uink-modal-close {
  width: 24px;
  height: 24px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--uink-muted);
  cursor: pointer;
  border-radius: var(--radius-sm, 6px);
}
.uink-modal-close:hover { background: var(--accent); }

/* Tabs */
.uink-tabs {
  display: flex;
  gap: 2px;
  padding: 0 14px;
  border-bottom: 1px solid var(--uink-border);
}
.uink-tab {
  display: flex;
  align-items: center;
  gap: 5px;
  padding: 7px 12px;
  font-size: 12px;
  font-weight: 500;
  border: none;
  background: transparent;
  color: var(--uink-muted);
  cursor: pointer;
  border-bottom: 2px solid transparent;
  transition: color var(--duration-fast, 150ms), border-color var(--duration-fast, 150ms);
}
.uink-tab:hover { color: var(--uink-fg); }
.uink-tab.active {
  color: var(--uink-accent);
  border-bottom-color: var(--uink-accent);
}

/* Tab content */
.uink-tab-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow-y: auto;
  min-height: 0;
}

/* Text panel */
.uink-text-panel {
  flex: 1;
  display: flex;
  flex-direction: column;
  padding: 10px 14px;
  gap: 6px;
}
.uink-text-type-row {
  display: flex;
  align-items: center;
  gap: 4px;
}
.uink-type-chip {
  padding: 2px 8px;
  font-size: 10px;
  border: 1px solid var(--uink-border);
  border-radius: var(--radius-sm, 6px);
  background: transparent;
  color: var(--uink-muted);
  cursor: pointer;
  transition: all var(--duration-fast, 150ms);
}
.uink-type-chip:hover { color: var(--uink-fg); }
.uink-type-chip.active {
  background: var(--uink-accent);
  color: var(--uink-on-primary);
  border-color: var(--uink-accent);
}
.uink-char-count {
  margin-left: auto;
  font-size: 10px;
  color: var(--uink-muted);
}

/* Textarea */
.uink-textarea {
  flex: 1;
  min-height: 100px;
  padding: 8px;
  font-size: 12px;
  font-family: inherit;
  border: 1px solid var(--input, var(--uink-border));
  border-radius: var(--radius-md, 8px);
  background: transparent;
  color: var(--uink-fg);
  resize: none;
}
.uink-textarea:focus {
  outline: none;
  border-color: var(--uink-accent);
  box-shadow: 0 0 0 2px oklch(0.18 0.02 270 / 10%);
}
.dark .uink-textarea:focus {
  box-shadow: 0 0 0 2px oklch(0.95 0.005 270 / 10%);
}

/* Upload panel */
.uink-upload-panel {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 20px 14px;
}
.uink-upload-panel.uink-drag-over .uink-upload-drop {
  border-color: var(--uink-accent);
  background: oklch(0.5 0.15 250 / 8%);
}
.uink-upload-drop {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  width: 100%;
  min-height: 160px;
  border: 2px dashed var(--uink-border);
  border-radius: var(--radius-lg, 10px);
  color: var(--uink-muted);
  font-size: 13px;
  cursor: pointer;
  transition: border-color var(--duration-fast, 150ms);
}
.uink-upload-drop:hover { border-color: var(--uink-accent); }
.uink-upload-preview {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
}
.uink-upload-preview img {
  max-width: 100%;
  max-height: 200px;
  object-fit: contain;
  border-radius: var(--radius-md, 8px);
}
.uink-upload-actions {
  display: flex;
  gap: 6px;
}

/* URL panel */
.uink-url-panel {
  flex: 1;
  display: flex;
  flex-direction: column;
  padding: 14px;
  gap: 10px;
}
.uink-field-label {
  font-size: 12px;
  color: var(--uink-muted);
}
.uink-url-input {
  padding: 8px 10px;
  font-size: 12px;
  border: 1px solid var(--input, var(--uink-border));
  border-radius: var(--radius-md, 8px);
  background: transparent;
  color: var(--uink-fg);
  width: 100%;
  box-sizing: border-box;
}
.uink-url-input:focus {
  outline: none;
  border-color: var(--uink-accent);
  box-shadow: 0 0 0 2px oklch(0.18 0.02 270 / 10%);
}
.dark .uink-url-input:focus {
  box-shadow: 0 0 0 2px oklch(0.95 0.005 270 / 10%);
}
.uink-url-preview {
  display: flex;
  justify-content: center;
}
.uink-url-preview img {
  max-width: 100%;
  max-height: 180px;
  object-fit: contain;
  border-radius: var(--radius-md, 8px);
}

/* Canvas container */
.uink-canvas-wrap {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 12px;
  overflow: auto;
  background: var(--muted);
  min-height: 0;
  position: relative;
}
.uink-canvas-wrap canvas {
  background: var(--card);
  box-shadow: var(--shadow-md);
  cursor: crosshair;
  max-width: 100%;
  max-height: 100%;
}

/* Floating property panel for canvas selected element */
.uink-float-props {
  position: absolute;
  top: 8px;
  right: 8px;
  width: 200px;
  background: var(--uink-card);
  border: 1px solid var(--uink-border);
  border-radius: var(--radius-md, 8px);
  box-shadow: var(--shadow-lg);
  z-index: 5;
  overflow: hidden;
  animation: uink-float-in 0.15s ease-out;
}
@keyframes uink-float-in {
  from { opacity: 0; transform: translateY(-4px); }
  to { opacity: 1; transform: translateY(0); }
}
.uink-float-props-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 10px;
  border-bottom: 1px solid var(--uink-border);
  font-size: 11px;
  font-weight: 600;
  color: var(--uink-fg);
}
.uink-float-props-close {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  border: none;
  background: transparent;
  color: var(--uink-muted);
  cursor: pointer;
  border-radius: var(--radius-sm, 4px);
}
.uink-float-props-close:hover { background: var(--accent); }
.uink-float-props-body {
  padding: 8px 10px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.uink-prop-field {
  display: flex;
  flex-direction: column;
  gap: 3px;
  font-size: 10px;
  color: var(--uink-muted);
}
.uink-prop-field input[type="text"],
.uink-prop-field input[type="number"] {
  padding: 4px 6px;
  font-size: 12px;
  border: 1px solid var(--uink-border);
  border-radius: var(--radius-sm, 4px);
  background: transparent;
  color: var(--uink-fg);
}
.uink-prop-row {
  display: flex;
  gap: 8px;
  align-items: center;
}
.uink-prop-field-sm { flex: 0 0 auto; }
.uink-prop-field-sm input { width: 52px; }
.uink-prop-toggle {
  flex-direction: row;
  align-items: center;
  gap: 4px;
}
.uink-prop-toggle input[type="checkbox"] {
  margin: 0;
}
.uink-float-props-delete {
  display: block;
  width: calc(100% - 20px);
  margin: 0 10px 8px;
  padding: 4px 0;
  font-size: 11px;
  color: var(--uink-error);
  background: transparent;
  border: 1px solid var(--uink-error);
  border-radius: var(--radius-sm, 4px);
  cursor: pointer;
  opacity: 0.8;
  transition: opacity 0.15s;
}
.uink-float-props-delete:hover { opacity: 1; background: oklch(0.6 0.2 25 / 10%); }

/* Toolbar */
.uink-toolbar {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 6px 14px;
  border-top: 1px solid var(--uink-border);
  background: var(--uink-card);
}
.uink-toolbar-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 4px 10px;
  font-size: 11px;
  border: 1px solid var(--uink-border);
  border-radius: var(--radius-sm, 6px);
  background: transparent;
  color: var(--uink-fg);
  cursor: pointer;
}
.uink-toolbar-btn:hover { background: var(--accent); }
.uink-toolbar-btn:disabled { opacity: 0.5; cursor: not-allowed; }

/* Modal footer */
.uink-modal-footer {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
  padding: 8px 14px;
  border-top: 1px solid var(--uink-border);
}
`

export function injectStyles() {
  if (typeof document === 'undefined' || document.getElementById(CSS_ID)) return
  const style = document.createElement('style')
  style.id = CSS_ID
  style.textContent = STYLES
  document.head.appendChild(style)
}
