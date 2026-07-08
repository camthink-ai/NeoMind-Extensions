// DeepStream StreamCard — single stream detail view.
//
// Modern design: compact header, large snapshot with overlay badge, horizontal
// stat row with icon tiles, clean RTSP bar, compact event feed.

import { forwardRef, useEffect, useMemo, useState } from 'react';
import { useStream } from '../hooks/useStream';
import { useSnapshot } from '../hooks/useSnapshot';
import { useEvents } from '../hooks/useEvents';
import { getSnapshotUrl, getRtspUrl, dsCommands, type ServerConfig } from '../api';
import { CopyIcon, RefreshIcon } from './icons';
import { EventFeed } from './EventFeed';
import type { SidecarEvent, StreamStatus } from '../types';

export interface StreamCardProps {
  stream?: import('../types').Stream;
  streamId?: string;
  title?: string;
  className?: string;
  snapshotToken?: string;
  serverHost?: string;
  snapshotPort?: number;
  rtspPort?: number;
  dataSource?: {
    stream_id?: string;
    extensionId?: string;
    [k: string]: unknown;
  };
}

const STATUS_META: Record<string, { label: string; color: string }> = {
  running:       { label: 'Running',       color: 'var(--ds-sc-success)' },
  connecting:    { label: 'Connecting',    color: 'var(--ds-sc-info)' },
  degraded:      { label: 'Degraded',      color: 'var(--ds-sc-warning)' },
  reconnecting:  { label: 'Reconnecting',  color: 'var(--ds-sc-warning)' },
  error:         { label: 'Error',         color: 'var(--ds-sc-error)' },
  stopped:       { label: 'Stopped',       color: 'var(--ds-sc-muted)' },
};

const STYLE_ID = 'ds-stream-card-styles';
const STYLES = `
.ds-stream-card {
  --ds-sc-fg: var(--foreground);
  --ds-sc-muted: var(--muted-foreground);
  --ds-sc-card: var(--card);
  --ds-sc-border: var(--border);
  --ds-sc-accent: var(--primary);
  --ds-sc-on-primary: var(--primary-foreground, #ffffff);
  --ds-sc-success: var(--color-success, #22c55e);
  --ds-sc-warning: var(--color-warning, #f59e0b);
  --ds-sc-error: var(--color-error, #ef4444);
  --ds-sc-info: var(--color-info, #3b82f6);
  --ds-sc-destructive: var(--destructive);
  --ds-sc-destructive-fg: var(--destructive-foreground, #ffffff);
  --ds-sc-tile-bg: color-mix(in srgb, var(--ds-sc-card) 50%, color-mix(in srgb, var(--ds-sc-muted) 8%, transparent));
  --ds-sc-radius: var(--radius-lg, 12px);

  display: flex;
  flex-direction: column;
  gap: 10px;
  width: 100%;
  height: 100%;
  min-height: 0;
  padding: 14px;
  background: var(--ds-sc-card);
  border: 1px solid var(--ds-sc-border);
  border-radius: var(--ds-sc-radius);
  box-sizing: border-box;
  font-size: 12px;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  color: var(--ds-sc-fg);
}
.dark .ds-stream-card {
  --ds-sc-on-primary: var(--primary-foreground, #17172a);
  --ds-sc-destructive-fg: var(--destructive-foreground, #17172a);
}

.ds-stream-card__msg {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  min-height: 120px;
  color: var(--ds-sc-muted);
}

.ds-stream-card__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  flex-shrink: 0;
}
.ds-stream-card__title-group {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}
.ds-stream-card__status-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
}
.ds-stream-card__title {
  margin: 0;
  font-size: 14px;
  font-weight: 700;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.ds-stream-card__actions { display: inline-flex; align-items: center; gap: 6px; flex-shrink: 0; }

.ds-stream-card__btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  height: 28px;
  padding: 0 10px;
  border: 1px solid var(--ds-sc-border);
  border-radius: var(--radius-md, 8px);
  background: transparent;
  color: var(--ds-sc-fg);
  font-size: 11px;
  font-weight: 500;
  cursor: pointer;
  transition: all 160ms ease;
}
.ds-stream-card__btn:hover { background: color-mix(in srgb, var(--ds-sc-accent) 10%, transparent); border-color: var(--ds-sc-accent); }
.ds-stream-card__btn--icon { width: 28px; padding: 0; justify-content: center; }
.ds-stream-card__btn--danger { border-color: color-mix(in srgb, var(--ds-sc-destructive) 50%, transparent); color: var(--ds-sc-destructive); }
.ds-stream-card__btn--danger:hover { background: var(--ds-sc-destructive); color: var(--ds-sc-destructive-fg); border-color: var(--ds-sc-destructive); }
.ds-stream-card__btn svg { width: 14px; height: 14px; }

.ds-stream-card__snapshot {
  position: relative;
  width: 100%;
  aspect-ratio: 16 / 9;
  background: #000;
  border-radius: var(--radius-md, 10px);
  overflow: hidden;
  flex-shrink: 0;
}
.ds-stream-card__img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.ds-stream-card__snapshot-placeholder {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  color: var(--ds-sc-muted);
  background: color-mix(in srgb, var(--ds-sc-muted) 15%, #000);
  font-size: 11px;
}
.ds-stream-card__snapshot-badge {
  position: absolute;
  top: 8px;
  left: 8px;
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 3px 10px;
  border-radius: var(--radius-full, 9999px);
  background: rgba(0,0,0,0.65);
  backdrop-filter: blur(8px);
  font-size: 10px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: #fff;
}
.ds-stream-card__snapshot-badge-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
}

.ds-stream-card__stats {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 8px;
  flex-shrink: 0;
}
.ds-stream-card__stat {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  padding: 8px 4px;
  border-radius: var(--radius-md, 8px);
  background: var(--ds-sc-tile-bg);
  border: 1px solid color-mix(in srgb, var(--ds-sc-border) 60%, transparent);
}
.ds-stream-card__stat-value {
  font-size: 18px;
  font-weight: 800;
  font-variant-numeric: tabular-nums;
  line-height: 1.1;
}
.ds-stream-card__stat-label {
  font-size: 9px;
  font-weight: 500;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--ds-sc-muted);
}

.ds-stream-card__rtsp {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}
.ds-stream-card__rtsp-url {
  flex: 1 1 auto;
  min-width: 0;
  padding: 6px 10px;
  border: 1px solid var(--ds-sc-border);
  border-radius: var(--radius-md, 8px);
  background: var(--ds-sc-tile-bg);
  color: var(--ds-sc-fg);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 11px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.ds-stream-card__events {
  flex: 1 1 auto;
  min-height: 100px;
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

function summarizeCounts(events: SidecarEvent[]) {
  let persons = 0, vehicles = 0, lineCrosses = 0, roiAlerts = 0;
  for (const ev of events) {
    switch (ev.type) {
      case 'detection':
        for (const obj of ev.objects ?? []) {
          if (obj.class === 0) persons++;
          else if (obj.class >= 1 && obj.class <= 9) vehicles++;
        }
        break;
      case 'line_cross': lineCrosses++; break;
      case 'roi_intrusion': roiAlerts++; break;
    }
  }
  return { persons, vehicles, lineCrosses, roiAlerts };
}

export const DeepStreamStreamCard = forwardRef<HTMLDivElement, StreamCardProps>(
  function DeepStreamStreamCard(props, ref) {
    const {
      stream: streamProp, streamId: streamIdProp, title, className,
      snapshotToken, serverHost, snapshotPort, rtspPort, dataSource,
    } = props;

    useEffect(() => { injectStyles(); }, []);

    const sid: string | undefined = streamProp?.stream_id ?? streamIdProp ?? dataSource?.stream_id;
    const { stream: fetched, loading, error, refresh } = useStream(sid);
    const stream = streamProp ?? fetched;
    const { tick } = useSnapshot(sid, 1000);
    const { events } = useEvents(sid);
    const [copyOk, setCopyOk] = useState(false);

    const counts = useMemo(() => summarizeCounts(events), [events]);
    const server: ServerConfig | undefined = serverHost ? { host: serverHost, snapshotPort, rtspPort } : undefined;

    if (!sid) return <div ref={ref} className={`ds-stream-card ${className ?? ''}`}><div className="ds-stream-card__msg">No stream selected.</div></div>;
    if (loading && !stream) return <div ref={ref} className={`ds-stream-card ${className ?? ''}`}><div className="ds-stream-card__msg">Loading…</div></div>;
    if (error && !stream) return <div ref={ref} className={`ds-stream-card ${className ?? ''}`}><div className="ds-stream-card__msg" style={{ color: 'var(--ds-sc-error)' }}>{error}</div></div>;
    if (!stream) return <div ref={ref} className={`ds-stream-card ${className ?? ''}`}><div className="ds-stream-card__msg">Stream "{sid}" not found.</div></div>;

    const rtspUrl = stream.rtsp_url ?? getRtspUrl(sid, server);
    const snapshotUrl = snapshotToken && tick > 0 ? getSnapshotUrl(sid, snapshotToken, tick, server) : null;
    const statusMeta = STATUS_META[stream.status as StreamStatus] ?? STATUS_META.stopped;

    const copyRtsp = async () => {
      try { await navigator.clipboard.writeText(rtspUrl); setCopyOk(true); setTimeout(() => setCopyOk(false), 1500); } catch {}
    };
    const onStop = () => { dsCommands.removeStream(stream.stream_id).then(() => refresh()); };

    return (
      <div ref={ref} className={`ds-stream-card ${className ?? ''}`}>
        <header className="ds-stream-card__header">
          <div className="ds-stream-card__title-group">
            <span className="ds-stream-card__status-dot" style={{ background: statusMeta.color }} />
            <h3 className="ds-stream-card__title">{title ?? stream.stream_id}</h3>
          </div>
          <div className="ds-stream-card__actions">
            <button type="button" className="ds-stream-card__btn ds-stream-card__btn--icon" onClick={refresh} aria-label="Refresh" title="Refresh">
              <RefreshIcon />
            </button>
            {stream.status !== 'stopped' && (
              <button type="button" className="ds-stream-card__btn ds-stream-card__btn--danger" onClick={onStop}>Stop</button>
            )}
          </div>
        </header>

        <div className="ds-stream-card__snapshot">
          {snapshotUrl ? (
            <img src={snapshotUrl} alt={stream.stream_id} className="ds-stream-card__img" onError={(e) => { (e.currentTarget as HTMLImageElement).style.opacity = '0.2'; }} />
          ) : (
            <div className="ds-stream-card__snapshot-placeholder">
              <span>No snapshot{snapshotToken ? '' : ' (no token)'}</span>
            </div>
          )}
          <div className="ds-stream-card__snapshot-badge">
            <span className="ds-stream-card__snapshot-badge-dot" style={{ background: statusMeta.color }} />
            {statusMeta.label}
          </div>
        </div>

        <div className="ds-stream-card__stats">
          <div className="ds-stream-card__stat">
            <span className="ds-stream-card__stat-value" style={{ color: 'var(--ds-sc-info)' }}>{counts.persons}</span>
            <span className="ds-stream-card__stat-label">Persons</span>
          </div>
          <div className="ds-stream-card__stat">
            <span className="ds-stream-card__stat-value" style={{ color: 'var(--ds-sc-success)' }}>{counts.vehicles}</span>
            <span className="ds-stream-card__stat-label">Vehicles</span>
          </div>
          <div className="ds-stream-card__stat">
            <span className="ds-stream-card__stat-value" style={{ color: 'var(--ds-sc-warning)' }}>{counts.lineCrosses}</span>
            <span className="ds-stream-card__stat-label">Line Cross</span>
          </div>
          <div className="ds-stream-card__stat">
            <span className="ds-stream-card__stat-value" style={{ color: 'var(--ds-sc-error)' }}>{counts.roiAlerts}</span>
            <span className="ds-stream-card__stat-label">ROI Alerts</span>
          </div>
        </div>

        <div className="ds-stream-card__rtsp">
          <code className="ds-stream-card__rtsp-url" title={rtspUrl}>{rtspUrl}</code>
          <button type="button" className="ds-stream-card__btn" onClick={copyRtsp} aria-label="Copy RTSP URL">
            <CopyIcon /> {copyOk ? 'Copied' : 'Copy'}
          </button>
        </div>

        <div className="ds-stream-card__events">
          <EventFeed streamId={stream.stream_id} />
        </div>
      </div>
    );
  },
);

export default { DeepStreamStreamCard };
