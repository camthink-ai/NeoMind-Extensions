// DeepStream extension — EventFeed.
//
// Compact timeline of recent sidecar events for a bound stream. Auto-scrolls
// to the bottom while new events arrive, with a toggle to disable auto-scroll.
// Noisy housekeeping events (pong/stats/ready/hello_ack/bye/error_response)
// are filtered out so only actionable analytics remain.

import { useEffect, useMemo, useRef, useState } from 'react';
import type { SidecarEvent } from '../types';
import { useEvents } from '../hooks/useEvents';
import {
  AlertTriangleIcon,
  ArrowRightIcon,
  CameraIcon,
  PersonIcon,
  RefreshIcon,
} from './icons';

export interface EventFeedProps {
  streamId: string;
  maxEvents?: number;
  className?: string;
}

type Severity = 'info' | 'success' | 'warning' | 'error';

interface EventRender {
  icon: React.ReactNode;
  severity: Severity;
  description: string;
}

const NOISE_TYPES = new Set([
  'pong',
  'stats',
  'ready',
  'hello_ack',
  'bye',
  'error_response',
]);

const SEVERITY_COLOR: Record<Severity, string> = {
  info: 'var(--ds-ef-info)',
  success: 'var(--ds-ef-success)',
  warning: 'var(--ds-ef-warning)',
  error: 'var(--ds-ef-error)',
};

function describeEvent(ev: SidecarEvent): EventRender | null {
  switch (ev.type) {
    case 'detection': {
      const count = ev.objects?.length ?? 0;
      return {
        icon: <PersonIcon />,
        severity: 'info',
        description: `${count} object${count === 1 ? '' : 's'}`,
      };
    }
    case 'line_cross': {
      const dir = ev.direction ? ` (${ev.direction})` : '';
      return {
        icon: <ArrowRightIcon />,
        severity: 'info',
        description: `Track ${ev.track_id} crossed ${ev.line_id}${dir}`,
      };
    }
    case 'roi_intrusion': {
      const mode = ev.mode ? ` (${ev.mode})` : '';
      return {
        icon: <AlertTriangleIcon />,
        severity: 'warning',
        description: `Track ${ev.track_id} ROI ${ev.roi_id}${mode}`,
      };
    }
    case 'analytics_snapshot': {
      return {
        icon: <RefreshIcon />,
        severity: 'info',
        description: 'analytics snapshot',
      };
    }
    case 'stream_added': {
      return {
        icon: <CameraIcon />,
        severity: 'success',
        description: `stream added: ${ev.stream_id}`,
      };
    }
    case 'stream_removed': {
      return {
        icon: <CameraIcon />,
        severity: 'info',
        description: `stream removed: ${ev.stream_id}`,
      };
    }
    case 'stream_error': {
      const msg = ev.message ?? ev.code ?? 'stream error';
      return {
        icon: <AlertTriangleIcon />,
        severity: 'error',
        description: msg,
      };
    }
    default:
      return null;
  }
}

function formatTime(ts: number | undefined): string {
  if (!ts) return '';
  const d = new Date(ts);
  if (Number.isNaN(d.getTime())) return '';
  return d.toLocaleTimeString(undefined, {
    hour12: false,
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
}

const STYLE_ID = 'ds-event-feed-styles';
const STYLES = `
.ds-event-feed {
  --ds-ef-fg: var(--foreground);
  --ds-ef-muted: var(--muted-foreground);
  --ds-ef-card: var(--card);
  --ds-ef-border: var(--border);
  --ds-ef-accent: var(--primary);
  --ds-ef-on-primary: var(--primary-foreground, #ffffff);
  --ds-ef-success: var(--color-success, #22c55e);
  --ds-ef-warning: var(--color-warning, #f59e0b);
  --ds-ef-error: var(--color-error, #ef4444);
  --ds-ef-info: var(--color-info, #3b82f6);
  --ds-ef-tile-bg: color-mix(in srgb, var(--ds-ef-card) 50%, color-mix(in srgb, var(--ds-ef-muted) 8%, transparent));

  display: flex;
  flex-direction: column;
  width: 100%;
  min-height: 0;
  height: 100%;
  box-sizing: border-box;
  font-size: 11px;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  color: var(--ds-ef-fg);
  gap: 6px;
}
.dark .ds-event-feed { --ds-ef-on-primary: var(--primary-foreground, #17172a); }

.ds-event-feed__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
  flex-shrink: 0;
}
.ds-event-feed__title-group {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}
.ds-event-feed__title {
  margin: 0;
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--ds-ef-muted);
}
.ds-event-feed__ws {
  font-size: 9px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  padding: 1px 6px;
  border-radius: var(--radius-full, 9999px);
  background: var(--ds-ef-tile-bg);
}
.ds-event-feed__ws--open { color: var(--ds-ef-success); }
.ds-event-feed__ws--connecting { color: var(--ds-ef-info); }
.ds-event-feed__ws--closed { color: var(--ds-ef-error); }

.ds-event-feed__actions { display: inline-flex; align-items: center; gap: 4px; }
.ds-event-feed__btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
  height: 22px;
  padding: 0 8px;
  border: 1px solid var(--ds-ef-border);
  border-radius: var(--radius-md, 6px);
  background: transparent;
  color: var(--ds-ef-fg);
  font-size: 10px;
  font-weight: 500;
  cursor: pointer;
  transition: all 160ms ease;
}
.ds-event-feed__btn:hover {
  background: color-mix(in srgb, var(--ds-ef-accent) 10%, transparent);
  border-color: var(--ds-ef-accent);
}
.ds-event-feed__btn--active {
  background: var(--ds-ef-accent);
  color: var(--ds-ef-on-primary);
  border-color: var(--ds-ef-accent);
}
.ds-event-feed__btn--icon { width: 22px; padding: 0; }
.ds-event-feed__btn svg { width: 12px; height: 12px; }

.ds-event-feed__list {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding-right: 2px;
}

.ds-event-feed__empty {
  padding: 16px 4px;
  color: var(--ds-ef-muted);
  text-align: center;
  font-style: italic;
  font-size: 11px;
}

.ds-event-feed__row {
  display: grid;
  grid-template-columns: 52px 14px 4px 1fr;
  align-items: center;
  gap: 6px;
  padding: 3px 4px;
  border-radius: var(--radius-sm, 4px);
  color: var(--ds-ef-fg);
  line-height: 1.4;
  transition: background 120ms ease;
}
.ds-event-feed__row:hover {
  background: color-mix(in srgb, var(--ds-ef-accent) 6%, transparent);
}

.ds-event-feed__time {
  font-size: 10px;
  font-variant-numeric: tabular-nums;
  color: var(--ds-ef-muted);
  white-space: nowrap;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}

.ds-event-feed__icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 14px;
  height: 14px;
  flex-shrink: 0;
}
.ds-event-feed__icon svg { width: 13px; height: 13px; }
.ds-event-feed__icon--info svg { color: var(--ds-ef-info); }
.ds-event-feed__icon--success svg { color: var(--ds-ef-success); }
.ds-event-feed__icon--warning svg { color: var(--ds-ef-warning); }
.ds-event-feed__icon--error svg { color: var(--ds-ef-error); }

.ds-event-feed__bar {
  width: 3px;
  height: 14px;
  border-radius: 9999px;
  flex-shrink: 0;
}

.ds-event-feed__desc {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
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

export function EventFeed({ streamId, maxEvents = 50, className }: EventFeedProps) {
  const { events, status, clear } = useEvents(streamId);
  const containerRef = useRef<HTMLDivElement>(null);
  const [autoScroll, setAutoScroll] = useState(true);

  useEffect(() => { injectStyles(); }, []);

  const filtered = useMemo(() => {
    return events
      .filter((e) => !NOISE_TYPES.has(e.type))
      .filter((e) => {
        const sid = (e as any).stream_id as string | undefined;
        if (!sid) return true;
        return sid === streamId;
      })
      .slice(-maxEvents);
  }, [events, maxEvents, streamId]);

  useEffect(() => {
    if (autoScroll && containerRef.current) {
      containerRef.current.scrollTop = containerRef.current.scrollHeight;
    }
  }, [filtered.length, autoScroll]);

  const wsClass =
    status === 'open'
      ? 'ds-event-feed__ws--open'
      : status === 'connecting'
        ? 'ds-event-feed__ws--connecting'
        : 'ds-event-feed__ws--closed';

  return (
    <div className={`ds-event-feed ${className ?? ''}`}>
      <header className="ds-event-feed__header">
        <div className="ds-event-feed__title-group">
          <h4 className="ds-event-feed__title">Events</h4>
          <span className={`ds-event-feed__ws ${wsClass}`}>{status}</span>
        </div>
        <div className="ds-event-feed__actions">
          <button
            type="button"
            className={`ds-event-feed__btn ${autoScroll ? 'ds-event-feed__btn--active' : ''}`}
            onClick={() => setAutoScroll((v) => !v)}
            title={autoScroll ? 'Auto-scroll on' : 'Auto-scroll off'}
            aria-pressed={autoScroll}
          >
            {autoScroll ? '↓ Auto' : 'Manual'}
          </button>
          <button
            type="button"
            className="ds-event-feed__btn ds-event-feed__btn--icon"
            onClick={clear}
            title="Clear events"
            aria-label="Clear events"
          >
            <RefreshIcon />
          </button>
        </div>
      </header>

      <div className="ds-event-feed__list" ref={containerRef}>
        {filtered.length === 0 ? (
          <div className="ds-event-feed__empty">No events yet.</div>
        ) : (
          filtered.map((ev, i) => {
            const meta = describeEvent(ev);
            if (!meta) return null;
            const ts = (ev as any).ts as number | undefined;
            return (
              <div key={`${i}-${ev.type}-${ts ?? ''}`} className="ds-event-feed__row">
                <span className="ds-event-feed__time">{formatTime(ts)}</span>
                <span className={`ds-event-feed__icon ds-event-feed__icon--${meta.severity}`}>
                  {meta.icon}
                </span>
                <span
                  className="ds-event-feed__bar"
                  style={{ background: SEVERITY_COLOR[meta.severity] }}
                />
                <span className="ds-event-feed__desc" title={meta.description}>
                  {meta.description}
                </span>
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}

export default EventFeed;
