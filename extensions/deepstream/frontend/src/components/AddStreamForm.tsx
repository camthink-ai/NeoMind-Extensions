// DeepStream extension — AddStreamForm.
//
// JSON-input form for creating a new stream. Per the Phase 1 scope decision,
// this is NOT a canvas drawing tool — line-crossing points and ROI polygons
// are entered as JSON arrays in textareas. On submit the form builds a
// StreamConfig and calls dsCommands.addStream().
//
// CSS uses NeoMind CSS variables exclusively (no hardcoded colors) and is
// scoped with the `.ds-add-form` prefix. forwardRef + loading/error states
// per the Extension Frontend Design Guide. The form is intended to be
// embedded inside a modal or panel by the parent — it does not manage its
// own visibility.

import { forwardRef, useEffect, useState } from 'react';
import { dsCommands } from '../api';
import type { ModelInfo } from '../types';
import { PlusIcon, AlertTriangleIcon } from './icons';

// ---------------------------------------------------------------------------
// Props
// ---------------------------------------------------------------------------

export interface AddStreamFormProps {
  /** Called when the form is submitted successfully with the new stream_id. */
  onCreated?: (stream_id: string, rtsp_url: string) => void;
  /** Called when the user cancels. */
  onCancel?: () => void;
  className?: string;
}

// ---------------------------------------------------------------------------
// Styles
// ---------------------------------------------------------------------------

const STYLE_ID = 'ds-add-form-styles';
const STYLES = `
.ds-add-form {
  /* CSS variable aliases — DESIGN_GUIDE §5. */
  --ds-af-fg: var(--foreground);
  --ds-af-muted: var(--muted-foreground);
  --ds-af-card: var(--card);
  --ds-af-border: var(--border);
  --ds-af-input-border: var(--input);
  --ds-af-accent: var(--primary);
  --ds-af-on-primary: var(--primary-foreground, #ffffff);
  --ds-af-error: var(--color-error);
  --ds-af-error-bg: var(--color-error-bg);
  --ds-af-success: var(--color-success);
  --ds-af-radius: var(--radius-lg, 10px);
  --ds-af-radius-md: var(--radius-md, 8px);

  display: flex;
  flex-direction: column;
  width: 100%;
  height: 100%;
  min-height: 0;
  padding: 16px;
  background: var(--ds-af-card);
  border: 1px solid var(--ds-af-border);
  border-radius: var(--ds-af-radius);
  box-sizing: border-box;
  font-size: 13px;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  color: var(--ds-af-fg);
}

.dark .ds-add-form {
  --ds-af-on-primary: var(--primary-foreground, #17172a);
}

.ds-add-form__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
  flex-shrink: 0;
}

.ds-add-form__header h3 {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  color: var(--ds-af-fg);
  line-height: 1.3;
}

.ds-add-form__body {
  display: flex;
  flex-direction: column;
  gap: 12px;
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  padding-right: 2px;
}

.ds-add-form__field {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.ds-add-form__label {
  font-size: 12px;
  font-weight: 500;
  color: var(--ds-af-fg);
  display: flex;
  align-items: center;
  gap: 4px;
}

.ds-add-form__label-hint {
  font-size: 11px;
  font-weight: 400;
  color: var(--ds-af-muted);
}

.ds-add-form__input,
.ds-add-form__select,
.ds-add-form__textarea {
  width: 100%;
  padding: 8px 10px;
  border: 1px solid var(--ds-af-input-border);
  border-radius: var(--ds-af-radius-md);
  background: var(--ds-af-card);
  color: var(--ds-af-fg);
  font-size: 13px;
  font-family: inherit;
  box-sizing: border-box;
  transition: border-color var(--duration-fast) var(--ease-out);
}

.ds-add-form__input:focus,
.ds-add-form__select:focus,
.ds-add-form__textarea:focus {
  outline: none;
  border-color: var(--ds-af-accent);
  box-shadow: 0 0 0 2px oklch(0.18 0.02 270 / 10%);
}

.dark .ds-add-form__input:focus,
.dark .ds-add-form__select:focus,
.dark .ds-add-form__textarea:focus {
  box-shadow: 0 0 0 2px oklch(0.95 0.005 270 / 10%);
}

.ds-add-form__input::placeholder,
.ds-add-form__textarea::placeholder {
  color: var(--ds-af-muted);
}

.ds-add-form__textarea {
  min-height: 80px;
  resize: vertical;
  font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
  font-size: 12px;
  line-height: 1.5;
}

.ds-add-form__select {
  cursor: pointer;
  appearance: none;
  background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12' fill='none' stroke='%23888' stroke-width='1.5' stroke-linecap='round' stroke-linejoin='round'><path d='M2 4.5 L6 8.5 L10 4.5'/></svg>");
  background-repeat: no-repeat;
  background-position: right 10px center;
  padding-right: 30px;
}

.ds-add-form__row {
  display: flex;
  gap: 8px;
}

.ds-add-form__row > .ds-add-form__field {
  flex: 1 1 0;
}

.ds-add-form__check-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  color: var(--ds-af-fg);
  cursor: pointer;
  user-select: none;
}

.ds-add-form__check-row input[type="checkbox"] {
  width: 16px;
  height: 16px;
  cursor: pointer;
  accent-color: var(--ds-af-accent);
}

.ds-add-form__error-text {
  font-size: 11px;
  color: var(--ds-af-error);
  margin-top: 2px;
}

.ds-add-form__input--error,
.ds-add-form__select--error,
.ds-add-form__textarea--error {
  border-color: var(--ds-af-error);
}

.ds-add-form__badge {
  display: inline-flex;
  align-items: center;
  padding: 1px 6px;
  border-radius: var(--radius-full, 9999px);
  font-size: 10px;
  font-weight: 500;
  background: var(--color-info-bg);
  color: var(--color-info);
  margin-left: 6px;
  border: 1px solid transparent;
}

.ds-add-form__footer {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
  padding-top: 12px;
  border-top: 1px solid var(--ds-af-border);
  margin-top: 12px;
  flex-shrink: 0;
}

.ds-add-form__btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
  padding: 8px 16px;
  border-radius: var(--ds-af-radius-md);
  font-size: 13px;
  font-weight: 500;
  font-family: inherit;
  cursor: pointer;
  transition: background var(--duration-fast) var(--ease-out),
              border-color var(--duration-fast) var(--ease-out);
}

.ds-add-form__btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.ds-add-form__btn--ghost {
  background: transparent;
  color: var(--ds-af-fg);
  border: 1px solid var(--ds-af-border);
}

.ds-add-form__btn--ghost:hover:not(:disabled) {
  background: var(--accent);
  color: var(--accent-foreground);
}

.ds-add-form__btn--primary {
  background: var(--ds-af-accent);
  color: var(--ds-af-on-primary);
  border: 1px solid var(--ds-af-accent);
}

.ds-add-form__btn--primary:hover:not(:disabled) {
  background: var(--primary-hover);
  border-color: var(--primary-hover);
}

.ds-add-form__btn svg {
  width: 14px;
  height: 14px;
}

.ds-add-form__submit-error {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  padding: 8px 10px;
  margin-top: 10px;
  border-radius: var(--ds-af-radius-md);
  background: var(--ds-af-error-bg);
  color: var(--ds-af-error);
  font-size: 12px;
  line-height: 1.4;
}

.ds-add-form__submit-error svg {
  width: 14px;
  height: 14px;
  flex-shrink: 0;
  margin-top: 1px;
}

.ds-add-form__success {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 10px;
  margin-top: 10px;
  border-radius: var(--ds-af-radius-md);
  background: var(--color-success-bg);
  color: var(--ds-af-success);
  font-size: 12px;
}
`;

function injectStyles() {
  if (typeof document === 'undefined') return;
  if (document.getElementById(STYLE_ID)) return;
  const el = document.createElement('style');
  el.id = STYLE_ID;
  el.textContent = STYLES;
  document.head.appendChild(el);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

interface LabeledFieldProps {
  label: string;
  hint?: string;
  error?: string;
  children: React.ReactNode;
}

function LabeledField({ label, hint, error, children }: LabeledFieldProps) {
  return (
    <div className="ds-add-form__field">
      <label className="ds-add-form__label">
        {label}
        {hint && <span className="ds-add-form__label-hint">— {hint}</span>}
      </label>
      {children}
      {error && <div className="ds-add-form__error-text">{error}</div>}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

export const AddStreamForm = forwardRef<HTMLDivElement, AddStreamFormProps>(
  function AddStreamForm(props, ref) {
    const { onCreated, onCancel, className } = props;

    const [streamId, setStreamId] = useState('');
    const [sourceUrl, setSourceUrl] = useState('');
    const [sourceType, setSourceType] = useState('rtsp');
    const [model, setModel] = useState('yolov8n-coco');
    const [models, setModels] = useState<ModelInfo[]>([]);
    const [trackerEnabled, setTrackerEnabled] = useState(true);
    const [trackerType, setTrackerType] = useState('NvDCF');
    const [lineCrossingJson, setLineCrossingJson] = useState(
      '[\n  { "id": "L1", "points": [[100, 200], [500, 200]], "mode": "bidirectional", "classes": [0] }\n]',
    );
    const [roiJson, setRoiJson] = useState('');
    const [encoder, setEncoder] = useState('h264');
    const [bitrate, setBitrate] = useState(4000);
    const [errors, setErrors] = useState<Record<string, string>>({});
    const [submitting, setSubmitting] = useState(false);
    const [submitError, setSubmitError] = useState<string | null>(null);
    const [createdId, setCreatedId] = useState<string | null>(null);

    // Inject scoped styles once on mount.
    useEffect(() => {
      injectStyles();
    }, []);

    // Populate model dropdown.
    useEffect(() => {
      let cancelled = false;
      dsCommands
        .listModels()
        .then((r) => {
          if (cancelled) return;
          if (r.success && r.data) {
            const list = r.data.models ?? [];
            setModels(list);
            // If the default isn't in the list, pick the first available.
            if (list.length > 0 && !list.some((m) => m.id === model)) {
              setModel(list[0].id);
            }
          }
        })
        .catch(() => {
          /* models list is best-effort — dropdown stays empty */
        });
      return () => {
        cancelled = true;
      };
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    const validate = (): boolean => {
      const errs: Record<string, string> = {};
      if (!/^[a-z0-9_]+$/.test(streamId)) {
        errs.streamId = 'Must match [a-z0-9_]+';
      }
      if (!sourceUrl.trim()) {
        errs.sourceUrl = 'Required';
      }
      if (lineCrossingJson.trim()) {
        try {
          const parsed = JSON.parse(lineCrossingJson);
          if (!Array.isArray(parsed)) {
            errs.lineCrossing = 'Must be a JSON array';
          }
        } catch (e: any) {
          errs.lineCrossing = e?.message ?? 'Invalid JSON';
        }
      }
      if (roiJson.trim()) {
        try {
          const parsed = JSON.parse(roiJson);
          if (!Array.isArray(parsed)) {
            errs.roiJson = 'Must be a JSON array';
          }
        } catch (e: any) {
          errs.roiJson = e?.message ?? 'Invalid JSON';
        }
      }
      setErrors(errs);
      return Object.keys(errs).length === 0;
    };

    const handleSubmit = async () => {
      if (!validate()) return;
      setSubmitting(true);
      setSubmitError(null);
      setCreatedId(null);
      try {
        const config: Record<string, unknown> = {
          stream_id: streamId,
          source: { type: sourceType, url: sourceUrl },
          model,
          tracker: trackerEnabled
            ? { enabled: true, type: trackerType }
            : { enabled: false },
          output: { encoder, bitrate_kbps: bitrate },
        };
        // Only attach analytics if at least one rule array is non-empty.
        const lcTrim = lineCrossingJson.trim();
        const roiTrim = roiJson.trim();
        if (lcTrim || roiTrim) {
          config.analytics = {
            ...(lcTrim && { line_crossing: JSON.parse(lcTrim) }),
            ...(roiTrim && { roi: JSON.parse(roiTrim) }),
          };
        }

        const r = await dsCommands.addStream({ stream_id: streamId, config });
        if (r.success && r.data) {
          setCreatedId(r.data.stream_id);
          onCreated?.(r.data.stream_id, r.data.rtsp_url);
        } else {
          setSubmitError(r.error ?? 'Failed to add stream');
        }
      } catch (e: any) {
        setSubmitError(e?.message ?? String(e));
      } finally {
        setSubmitting(false);
      }
    };

    return (
      <div ref={ref} className={`ds-add-form ${className ?? ''}`}>
        <header className="ds-add-form__header">
          <h3>Add Stream</h3>
        </header>

        <div className="ds-add-form__body">
          <LabeledField
            label="Stream ID"
            hint="lowercase, digits, underscore"
            error={errors.streamId}
          >
            <input
              className={`ds-add-form__input ${errors.streamId ? 'ds-add-form__input--error' : ''}`}
              value={streamId}
              onChange={(e) => setStreamId(e.target.value)}
              placeholder="e.g. front_door"
              autoComplete="off"
              spellCheck={false}
            />
          </LabeledField>

          <LabeledField
            label="Source URL"
            hint="rtsp://user:pass@host/path"
            error={errors.sourceUrl}
          >
            <input
              className={`ds-add-form__input ${errors.sourceUrl ? 'ds-add-form__input--error' : ''}`}
              value={sourceUrl}
              onChange={(e) => setSourceUrl(e.target.value)}
              placeholder="rtsp://admin:pass@192.168.1.50/Streaming/Channels/101"
              autoComplete="off"
              spellCheck={false}
            />
          </LabeledField>

          <div className="ds-add-form__row">
            <LabeledField label="Source Type">
              <select
                className="ds-add-form__select"
                value={sourceType}
                onChange={(e) => setSourceType(e.target.value)}
              >
                <option value="rtsp">rtsp</option>
                <option value="file">file</option>
                <option value="csi">csi</option>
              </select>
            </LabeledField>

            <LabeledField label="Model">
              <select
                className="ds-add-form__select"
                value={model}
                onChange={(e) => setModel(e.target.value)}
              >
                {models.length === 0 && <option value={model}>{model}</option>}
                {models.map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.name}
                    {m.preset ? ' (preset)' : ''}
                  </option>
                ))}
              </select>
            </LabeledField>
          </div>

          <label className="ds-add-form__check-row">
            <input
              type="checkbox"
              checked={trackerEnabled}
              onChange={(e) => setTrackerEnabled(e.target.checked)}
            />
            Tracker enabled
          </label>

          {trackerEnabled && (
            <LabeledField label="Tracker Type">
              <select
                className="ds-add-form__select"
                value={trackerType}
                onChange={(e) => setTrackerType(e.target.value)}
              >
                <option value="NvDCF">NvDCF</option>
                <option value="NvSORT">NvSORT</option>
              </select>
            </LabeledField>
          )}

          <LabeledField
            label="Line Crossing Rules (JSON)"
            hint="array of { id, points, mode, classes }"
            error={errors.lineCrossing}
          >
            <textarea
              className={`ds-add-form__textarea ${errors.lineCrossing ? 'ds-add-form__textarea--error' : ''}`}
              value={lineCrossingJson}
              onChange={(e) => setLineCrossingJson(e.target.value)}
              spellCheck={false}
            />
          </LabeledField>

          <LabeledField
            label="ROI Rules (JSON)"
            hint="array of { id, polygon, mode, classes }"
            error={errors.roiJson}
          >
            <textarea
              className={`ds-add-form__textarea ${errors.roiJson ? 'ds-add-form__textarea--error' : ''}`}
              value={roiJson}
              onChange={(e) => setRoiJson(e.target.value)}
              placeholder='[\n  { "id": "R1", "polygon": [[100,100],[500,100],[500,400],[100,400]], "mode": "entry", "classes": [0] }\n]'
              spellCheck={false}
            />
          </LabeledField>

          <div className="ds-add-form__row">
            <LabeledField label="Encoder">
              <select
                className="ds-add-form__select"
                value={encoder}
                onChange={(e) => setEncoder(e.target.value)}
              >
                <option value="h264">h264</option>
                <option value="h265">h265</option>
              </select>
            </LabeledField>

            <LabeledField label="Bitrate (kbps)">
              <input
                className="ds-add-form__input"
                type="number"
                min={100}
                max={20000}
                step={100}
                value={bitrate}
                onChange={(e) => setBitrate(Number(e.target.value) || 0)}
              />
            </LabeledField>
          </div>

          {models.length > 0 && models.find((m) => m.id === model)?.preset && (
            <div>
              <span className="ds-add-form__badge">preset model</span>
            </div>
          )}
        </div>

        {submitError && (
          <div className="ds-add-form__submit-error">
            <AlertTriangleIcon /> {submitError}
          </div>
        )}

        {createdId && !submitError && (
          <div className="ds-add-form__success">
            Created stream &quot;{createdId}&quot;.
          </div>
        )}

        <footer className="ds-add-form__footer">
          {onCancel && (
            <button
              type="button"
              onClick={onCancel}
              className="ds-add-form__btn ds-add-form__btn--ghost"
              disabled={submitting}
            >
              Cancel
            </button>
          )}
          <button
            type="button"
            onClick={handleSubmit}
            disabled={submitting}
            className="ds-add-form__btn ds-add-form__btn--primary"
          >
            <PlusIcon /> {submitting ? 'Adding…' : 'Add Stream'}
          </button>
        </footer>
      </div>
    );
  },
);

AddStreamForm.displayName = 'AddStreamForm';

export default { AddStreamForm };
