// DeepStream OverviewCard — stream grid with live thumbnails.
//
// Modern design: header with count badge, responsive grid of tiles with
// gradient overlays, polished empty state.

import { forwardRef } from 'react';
import { useStreams } from '../hooks/useStreams';
import { StreamThumbnail } from './StreamThumbnail';
import { PlusIcon, CameraIcon, RefreshIcon } from './icons';
import type { ServerConfig } from '../api';

export interface OverviewCardProps {
  title?: string;
  className?: string;
  snapshotToken?: string;
  onRequestAddStream?: () => void;
  onSelectStream?: (streamId: string) => void;
  serverHost?: string;
  snapshotPort?: number;
  rtspPort?: number;
  dataSource?: { extensionId?: string; [k: string]: unknown };
}

const STYLES = `
.ds-overview-card {
  --ds-oc-fg: var(--foreground);
  --ds-oc-muted: var(--muted-foreground);
  --ds-oc-card: var(--card);
  --ds-oc-border: var(--border);
  --ds-oc-accent: var(--primary);
  --ds-oc-on-primary: var(--primary-foreground, #ffffff);

  display: flex;
  flex-direction: column;
  width: 100%;
  height: 100%;
  min-height: 0;
  padding: 14px;
  background: var(--ds-oc-card);
  border: 1px solid var(--ds-oc-border);
  border-radius: var(--radius-lg, 12px);
  box-sizing: border-box;
  font-size: 12px;
  color: var(--ds-oc-fg);
  gap: 12px;
}
.dark .ds-overview-card { --ds-oc-on-primary: var(--primary-foreground, #17172a); }

.ds-overview-card__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  flex-shrink: 0;
}
.ds-overview-card__title-group {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}
.ds-overview-card__title-group h3 {
  margin: 0;
  font-size: 14px;
  font-weight: 700;
  color: var(--ds-oc-fg);
  white-space: nowrap;
}
.ds-overview-card__count {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 22px;
  height: 22px;
  padding: 0 7px;
  border-radius: var(--radius-full, 9999px);
  background: color-mix(in srgb, var(--ds-oc-accent) 12%, transparent);
  color: var(--ds-oc-accent);
  font-size: 11px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}
.ds-overview-card__actions {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}
.ds-overview-card__btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  height: 28px;
  padding: 0 10px;
  border: 1px solid var(--ds-oc-border);
  border-radius: var(--radius-md, 8px);
  background: transparent;
  color: var(--ds-oc-fg);
  font-size: 11px;
  font-weight: 500;
  font-family: inherit;
  cursor: pointer;
  transition: all 160ms ease;
}
.ds-overview-card__btn:hover {
  background: color-mix(in srgb, var(--ds-oc-accent) 10%, transparent);
  border-color: var(--ds-oc-accent);
}
.ds-overview-card__btn--icon { width: 28px; padding: 0; justify-content: center; }
.ds-overview-card__btn--primary {
  background: var(--ds-oc-accent);
  color: var(--ds-oc-on-primary);
  border-color: var(--ds-oc-accent);
}
.ds-overview-card__btn--primary:hover {
  opacity: 0.9;
  background: var(--ds-oc-accent);
  border-color: var(--ds-oc-accent);
}
.ds-overview-card__btn svg { width: 14px; height: 14px; }

.ds-overview-card__grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 10px;
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  padding-right: 2px;
}

.ds-overview-card__empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  flex: 1 1 auto;
  min-height: 160px;
  padding: 32px;
  text-align: center;
  color: var(--ds-oc-muted);
}
.ds-overview-card__empty-icon {
  width: 56px;
  height: 56px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  background: color-mix(in srgb, var(--ds-oc-muted) 10%, transparent);
}
.ds-overview-card__empty-icon svg { width: 28px; height: 28px; opacity: 0.6; }
.ds-overview-card__empty p { margin: 0; font-size: 13px; }

.ds-overview-card__msg {
  display: flex;
  align-items: center;
  justify-content: center;
  flex: 1 1 auto;
  min-height: 120px;
  color: var(--ds-oc-muted);
  font-size: 13px;
}
`;

export const DeepStreamOverviewCard = forwardRef<HTMLDivElement, OverviewCardProps>(
  function DeepStreamOverviewCard(props, ref) {
    const {
      title = 'DeepStream 流总览',
      className,
      snapshotToken,
      onRequestAddStream,
      onSelectStream,
      serverHost,
      snapshotPort,
      rtspPort,
    } = props;
    const { streams, loading, error, refresh } = useStreams();
    const server: ServerConfig | undefined = serverHost ? { host: serverHost, snapshotPort, rtspPort } : undefined;

    return (
      <>
        <style>{STYLES}</style>
        <div ref={ref} className={`ds-overview-card ${className ?? ''}`}>
          <header className="ds-overview-card__header">
            <div className="ds-overview-card__title-group">
              <h3>{title}</h3>
              {streams.length > 0 && (
                <span className="ds-overview-card__count">{streams.length}</span>
              )}
            </div>
            <div className="ds-overview-card__actions">
              <button onClick={refresh} className="ds-overview-card__btn ds-overview-card__btn--icon" aria-label="Refresh" title="Refresh">
                <RefreshIcon />
              </button>
              {onRequestAddStream && (
                <button onClick={onRequestAddStream} className="ds-overview-card__btn ds-overview-card__btn--primary">
                  <PlusIcon /> Add Stream
                </button>
              )}
            </div>
          </header>

          {loading && <div className="ds-overview-card__msg">Loading…</div>}
          {!loading && error && <div className="ds-overview-card__msg" style={{ color: 'var(--color-error)' }}>{error}</div>}
          {!loading && !error && streams.length === 0 && (
            <div className="ds-overview-card__empty">
              <div className="ds-overview-card__empty-icon"><CameraIcon /></div>
              <p>暂无视频流。点击 "Add Stream" 添加。</p>
            </div>
          )}
          {!loading && !error && streams.length > 0 && (
            <div className="ds-overview-card__grid">
              {streams.map((s) => (
                <StreamThumbnail
                  key={s.stream_id}
                  stream={s}
                  snapshotToken={snapshotToken}
                  server={server}
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
