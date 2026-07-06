// DeepStream extension — OverviewCard.
//
// A scrollable grid of StreamThumbnail tiles plus a header with refresh and
// "Add Stream" actions. This is the default landing view for the extension;
// clicking a thumbnail delegates to the parent (onSelectStream) which typically
// swaps in the StreamCard detail view. The card uses forwardRef per the design
// guide §10 — required for all exported extension components.

import { forwardRef } from 'react';
import { useStreams } from '../hooks/useStreams';
import { StreamThumbnail } from './StreamThumbnail';
import { PlusIcon, CameraIcon, RefreshIcon } from './icons';

export interface OverviewCardProps {
  title?: string;
  className?: string;
  /** Snapshot token forwarded to StreamThumbnail. When absent, only placeholders render. */
  snapshotToken?: string;
  /** Called when the user clicks "+ Add Stream" — parent shows the AddStreamForm. */
  onRequestAddStream?: () => void;
  /** Called when the user clicks a thumbnail — parent shows the StreamCard. */
  onSelectStream?: (streamId: string) => void;
  dataSource?: { extensionId?: string; [k: string]: unknown };
}

const STYLES = `
.ds-overview-card {
  /* CSS variable aliases — DESIGN_GUIDE §5. */
  --ds-oc-fg: var(--foreground);
  --ds-oc-muted: var(--muted-foreground);
  --ds-oc-card: var(--card);
  --ds-oc-border: var(--border);
  --ds-oc-accent: var(--primary);
  /* Primary button text — fallback per §5.1 (critical). */
  --ds-oc-on-primary: var(--primary-foreground, #ffffff);

  display: flex;
  flex-direction: column;
  width: 100%;
  height: 100%;
  min-height: 0;
  padding: 16px;
  background: var(--ds-oc-card);
  backdrop-filter: blur(12px);
  border: 1px solid var(--ds-oc-border);
  border-radius: var(--radius-lg, 10px);
  box-shadow: var(--shadow-sm);
  box-sizing: border-box;
  font-size: 12px;
  color: var(--ds-oc-fg);
}

.dark .ds-overview-card {
  --ds-oc-on-primary: var(--primary-foreground, #17172a);
}

.ds-overview-card__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 12px;
  flex-shrink: 0;
}

.ds-overview-card__header h3 {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  color: var(--ds-oc-fg);
  line-height: 1.3;
}

.ds-overview-card__actions {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.ds-overview-card__actions button {
  font-family: inherit;
  font-size: 12px;
  font-weight: 500;
  border-radius: var(--radius-md, 8px);
  padding: 6px 10px;
  cursor: pointer;
  transition: background var(--duration-fast) var(--ease-out),
              border-color var(--duration-fast) var(--ease-out);
  display: inline-flex;
  align-items: center;
  gap: 4px;
}

.ds-overview-card__actions button svg {
  width: 14px;
  height: 14px;
}

/* Icon-only refresh button — ghost style. */
.ds-overview-card__actions button[aria-label="Refresh"] {
  background: transparent;
  color: var(--ds-oc-fg);
  border: 1px solid var(--ds-oc-border);
  padding: 6px;
}

.ds-overview-card__actions button[aria-label="Refresh"]:hover {
  background: var(--accent);
  color: var(--accent-foreground);
}

/* Primary "Add Stream" button. */
.ds-overview-card__add-btn {
  background: var(--ds-oc-accent);
  color: var(--ds-oc-on-primary);
  border: 1px solid var(--ds-oc-accent);
}

.ds-overview-card__add-btn:hover {
  background: var(--primary-hover);
  border-color: var(--primary-hover);
}

.ds-overview-card__grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
  gap: 10px;
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  padding-right: 2px; /* avoid scrollbar overlapping card border */
}

.ds-overview-card__loading,
.ds-overview-card__error,
.ds-overview-card__empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  flex: 1 1 auto;
  min-height: 120px;
  padding: 24px;
  text-align: center;
  color: var(--ds-oc-muted);
  font-size: 12px;
}

.ds-overview-card__error {
  color: var(--color-error);
  background: var(--color-error-bg);
  border: 1px solid var(--color-error);
  border-radius: var(--radius-md, 8px);
}

.ds-overview-card__empty svg {
  width: 40px;
  height: 40px;
  opacity: 0.55;
}

.ds-overview-card__empty p {
  margin: 0;
}
`;

export const DeepStreamOverviewCard = forwardRef<HTMLDivElement, OverviewCardProps>(
  function DeepStreamOverviewCard(props, ref) {
    const {
      title = 'DeepStream Streams',
      className,
      snapshotToken,
      onRequestAddStream,
      onSelectStream,
    } = props;
    const { streams, loading, error, refresh } = useStreams();

    return (
      <>
        <style>{STYLES}</style>
        <div ref={ref} className={`ds-overview-card ${className ?? ''}`}>
          <header className="ds-overview-card__header">
            <h3>{title}</h3>
            <div className="ds-overview-card__actions">
              <button onClick={refresh} aria-label="Refresh" title="Refresh">
                <RefreshIcon />
              </button>
              {onRequestAddStream && (
                <button
                  onClick={onRequestAddStream}
                  className="ds-overview-card__add-btn"
                >
                  <PlusIcon /> Add Stream
                </button>
              )}
            </div>
          </header>

          {loading && <div className="ds-overview-card__loading">Loading…</div>}

          {!loading && error && (
            <div className="ds-overview-card__error">{error}</div>
          )}

          {!loading && !error && streams.length === 0 && (
            <div className="ds-overview-card__empty">
              <CameraIcon />
              <p>No streams yet. Click &quot;Add Stream&quot; to begin.</p>
            </div>
          )}

          {!loading && !error && streams.length > 0 && (
            <div className="ds-overview-card__grid">
              {streams.map((s) => (
                <StreamThumbnail
                  key={s.stream_id}
                  stream={s}
                  snapshotToken={snapshotToken}
                  onClick={onSelectStream}
                />
              ))}
            </div>
          )}
        </div>
      </>
    );
  },
);

DeepStreamOverviewCard.displayName = 'DeepStreamOverviewCard';

export default { DeepStreamOverviewCard };
