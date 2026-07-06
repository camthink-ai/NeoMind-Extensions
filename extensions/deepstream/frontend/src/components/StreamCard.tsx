// DeepStream extension — StreamCard.
//
// Large detailed view of a single stream. Renders the live snapshot at full
// card width, the RTSP URL with a copy-to-clipboard affordance, a row of
// CountChips summarizing recent analytics activity, and an EventFeed panel
// for the bound stream's event history.
//
// Resolution order for the stream to display:
//   1. explicit `stream` prop (preloaded Stream object)
//   2. explicit `streamId` prop → fetched via useStream()
//   3. `dataSource.stream_id` (bound from the host's data-source picker)
//
// CSS variables only — no hardcoded colors. Styles injected via the singleton
// injectStyles() pattern (see StatsCard.tsx / EXTENSION_FRONTEND_DESIGN_GUIDE).

import { forwardRef, useEffect, useMemo, useState } from 'react';
import { useStream } from '../hooks/useStream';
import { useSnapshot } from '../hooks/useSnapshot';
import { useEvents } from '../hooks/useEvents';
import { getSnapshotUrl, getRtspUrl, dsCommands } from '../api';
import {
  AlertTriangleIcon,
  ArrowRightIcon,
  CameraIcon,
  CarIcon,
  CopyIcon,
  PersonIcon,
  RefreshIcon,
  StatusDotIcon,
} from './icons';
import { CountChip } from './CountChip';
import { EventFeed } from './EventFeed';
import type { SidecarEvent, StreamStatus } from '../types';

export interface StreamCardProps {
  /** Stream to display directly, bypassing the fetch. */
  stream?: import('../types').Stream;
  /** Stream id to fetch via useStream() when no preloaded stream is given. */
  streamId?: string;
  title?: string;
  className?: string;
  /** Snapshot auth token. Without it only a placeholder is shown. */
  snapshotToken?: string;
  /** Host-bound data source (the picker passes `stream_id` here). */
  dataSource?: {
    stream_id?: string;
    extensionId?: string;
    [k: string]: unknown;
  };
}

// ---------------------------------------------------------------------------
// Styles (singleton)
// ---------------------------------------------------------------------------

const STYLE_ID = 'ds-stream-card-styles';
const STYLES = `
.ds-stream-card {
  --ds-sc-fg: var(--foreground);
  --ds-sc-muted: var(--muted-foreground);
  --ds-sc-card: var(--card);
  --ds-sc-border: var(--border);
  --ds-sc-accent: var(--primary);
  --ds-sc-on-primary: var(--primary-foreground, #ffffff);
  --ds-sc-success: var(--color-success);
  --ds-sc-warning: var(--color-warning);
  --ds-sc-error: var(--color-error);
  --ds-sc-info: var(--color-info);
  --ds-sc-destructive: var(--destructive);
  --ds-sc-destructive-fg: var(--destructive-foreground, #ffffff);
  --ds-sc-radius: var(--radius-lg, 10px);

  display: flex;
  flex-direction: column;
  gap: 10px;
  width: 100%;
  height: 100%;
  min-height: 0;
  padding: 12px;
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

.ds-stream-card--empty,
.ds-stream-card--loading,
.ds-stream-card--error,
.ds-stream-card--notfound {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  color: var(--ds-sc-muted);
  text-align: center;
}
.ds-stream-card--error { color: var(--ds-sc-error); }

.ds-stream-card__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.ds-stream-card__title {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}
.ds-stream-card__title h3 {
  margin: 0;
  font-size: 13px;
  font-weight: 600;
  color: var(--ds-sc-fg);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.ds-stream-card__status {
  width: 10px;
  height: 10px;
  flex-shrink: 0;
}
.ds-stream-card__status--running { color: var(--ds-sc-success); }
.ds-stream-card__status--connecting { color: var(--ds-sc-info); }
.ds-stream-card__status--degraded { color: var(--ds-sc-warning); }
.ds-stream-card__status--reconnecting { color: var(--ds-sc-warning); }
.ds-stream-card__status--error { color: var(--ds-sc-error); }
.ds-stream-card__status--stopped { color: var(--ds-sc-muted); }

.ds-stream-card__actions {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}

.ds-stream-card__btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
  height: 24px;
  padding: 0 8px;
  border: 1px solid var(--ds-sc-border);
  border-radius: var(--radius-md, 6px);
  background: transparent;
  color: var(--ds-sc-fg);
  font-size: 11px;
  font-weight: 500;
  cursor: pointer;
  transition: background 120ms ease;
}
.ds-stream-card__btn:hover { background: var(--accent); }
.ds-stream-card__btn--icon {
  width: 24px;
  padding: 0;
}
.ds-stream-card__btn svg {
  width: 14px;
  height: 14px;
}
.ds-stream-card__btn--danger {
  border-color: var(--ds-sc-destructive);
  color: var(--ds-sc-destructive);
}
.ds-stream-card__btn--danger:hover {
  background: var(--ds-sc-destructive);
  color: var(--ds-sc-destructive-fg);
}

.ds-stream-card__snapshot {
  position: relative;
  width: 100%;
  aspect-ratio: 16 / 9;
  background: var(--muted);
  border: 1px solid var(--ds-sc-border);
  border-radius: var(--radius-md, 6px);
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
  background: var(--muted);
}
.ds-stream-card__snapshot-placeholder svg {
  width: 36px;
  height: 36px;
  opacity: 0.6;
}

.ds-stream-card__rtsp-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 0;
}

.ds-stream-card__rtsp-url {
  flex: 1 1 auto;
  min-width: 0;
  padding: 4px 8px;
  border: 1px solid var(--ds-sc-border);
  border-radius: var(--radius-sm, 4px);
  background: var(--muted);
  color: var(--ds-sc-fg);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 11px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.ds-stream-card__counts {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.ds-stream-card__events {
  flex: 1 1 auto;
  min-height: 120px;
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

interface CountSummary {
  persons: number;
  vehicles: number;
  lineCrosses: number;
  roiAlerts: number;
}

/** COCO class id → coarse category. 0 = person, 1-9 = vehicle-ish, rest = other. */
function classifyObject(cls: number): 'person' | 'vehicle' | 'other' {
  if (cls === 0) return 'person';
  if (cls >= 1 && cls <= 9) return 'vehicle';
  return 'other';
}

function summarizeCounts(events: SidecarEvent[]): CountSummary {
  const out: CountSummary = { persons: 0, vehicles: 0, lineCrosses: 0, roiAlerts: 0 };
  for (const ev of events) {
    switch (ev.type) {
      case 'detection': {
        for (const obj of ev.objects ?? []) {
          const cat = classifyObject(obj.class);
          if (cat === 'person') out.persons += 1;
          else if (cat === 'vehicle') out.vehicles += 1;
        }
        break;
      }
      case 'line_cross': {
        out.lineCrosses += 1;
        break;
      }
      case 'roi_intrusion': {
        out.roiAlerts += 1;
        break;
      }
      default:
        break;
    }
  }
  return out;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

export const DeepStreamStreamCard = forwardRef<HTMLDivElement, StreamCardProps>(
  function DeepStreamStreamCard(props, ref) {
    const { stream: streamProp, streamId: streamIdProp, title, className, snapshotToken, dataSource } = props;

    // Inject the singleton stylesheet once on mount.
    useEffect(() => {
      injectStyles();
    }, []);

    // Resolve which stream id we're displaying.
    const sid: string | undefined =
      streamProp?.stream_id ?? streamIdProp ?? dataSource?.stream_id;

    const { stream: fetched, loading, error, refresh } = useStream(sid);
    const stream = streamProp ?? fetched;
    const { tick } = useSnapshot(sid, 1000);
    const { events } = useEvents(sid);
    const [copyOk, setCopyOk] = useState(false);

    const counts = useMemo(() => summarizeCounts(events), [events]);

    if (!sid) {
      return (
        <div ref={ref} className={`ds-stream-card ds-stream-card--empty ${className ?? ''}`}>
          No stream selected.
        </div>
      );
    }
    if (loading && !stream) {
      return (
        <div ref={ref} className={`ds-stream-card ds-stream-card--loading ${className ?? ''}`}>
          Loading…
        </div>
      );
    }
    if (error && !stream) {
      return (
        <div ref={ref} className={`ds-stream-card ds-stream-card--error ${className ?? ''}`}>
          {error}
        </div>
      );
    }
    if (!stream) {
      return (
        <div ref={ref} className={`ds-stream-card ds-stream-card--notfound ${className ?? ''}`}>
          Stream &ldquo;{sid}&rdquo; not found.
        </div>
      );
    }

    const rtspUrl = stream.rtsp_url ?? (sid ? getRtspUrl(sid) : null);
    const snapshotUrl =
      snapshotToken && sid && tick > 0
        ? getSnapshotUrl(sid, snapshotToken, tick)
        : null;

    const status = stream.status as StreamStatus;

    const copyRtsp = async () => {
      if (!rtspUrl) return;
      try {
        await navigator.clipboard.writeText(rtspUrl);
        setCopyOk(true);
        setTimeout(() => setCopyOk(false), 1500);
      } catch {
        /* ignore — clipboard may be unavailable */
      }
    };

    const onStop = () => {
      // Fire-and-forget; the next refresh() will reflect the new status.
      dsCommands.removeStream(stream.stream_id).then(() => refresh());
    };

    return (
      <div ref={ref} className={`ds-stream-card ${className ?? ''}`}>
        <header className="ds-stream-card__header">
          <div className="ds-stream-card__title">
            <StatusDotIcon className={`ds-stream-card__status ds-stream-card__status--${status}`} />
            <h3>{title ?? stream.stream_id}</h3>
          </div>
          <div className="ds-stream-card__actions">
            <button
              type="button"
              className="ds-stream-card__btn ds-stream-card__btn--icon"
              onClick={() => refresh()}
              aria-label="Refresh stream"
              title="Refresh"
            >
              <RefreshIcon />
            </button>
            {status !== 'stopped' && (
              <button
                type="button"
                className="ds-stream-card__btn ds-stream-card__btn--danger"
                onClick={onStop}
                title="Stop and remove this stream"
              >
                Stop
              </button>
            )}
          </div>
        </header>

        <div className="ds-stream-card__snapshot">
          {snapshotUrl ? (
            <img
              src={snapshotUrl}
              alt={stream.stream_id}
              className="ds-stream-card__img"
              onError={(e) => {
                const img = e.currentTarget as HTMLImageElement;
                img.style.opacity = '0.25';
              }}
            />
          ) : (
            <div className="ds-stream-card__snapshot-placeholder">
              <CameraIcon />
              <span>No snapshot{snapshotToken ? '' : ' (no token)'}</span>
            </div>
          )}
        </div>

        {rtspUrl && (
          <div className="ds-stream-card__rtsp-row">
            <code className="ds-stream-card__rtsp-url" title={rtspUrl}>
              {rtspUrl}
            </code>
            <button
              type="button"
              className="ds-stream-card__btn"
              onClick={copyRtsp}
              aria-label="Copy RTSP URL"
              title="Copy RTSP URL"
            >
              <CopyIcon />
              {copyOk ? 'Copied' : 'Copy'}
            </button>
          </div>
        )}

        <div className="ds-stream-card__counts">
          <CountChip icon={<PersonIcon />} value={counts.persons} label="Persons" />
          <CountChip icon={<CarIcon />} value={counts.vehicles} label="Vehicles" />
          <CountChip icon={<ArrowRightIcon />} value={counts.lineCrosses} label="Line crosses" />
          <CountChip icon={<AlertTriangleIcon />} value={counts.roiAlerts} label="ROI alerts" />
        </div>

        <div className="ds-stream-card__events">
          <EventFeed streamId={stream.stream_id} />
        </div>
      </div>
    );
  },
);

export default { DeepStreamStreamCard };
