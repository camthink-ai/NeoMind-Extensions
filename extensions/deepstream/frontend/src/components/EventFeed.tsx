// DeepStream extension — EventFeed.
//
// Scrollable list of recent sidecar events for a bound stream. Auto-scrolls to
// the bottom while new events arrive, with a toggle to disable auto-scroll so
// the user can inspect history without fighting the scroll position. Noisy
// housekeeping events (pong/stats/ready/hello_ack/bye/error_response) are
// filtered out so only actionable analytics remain: detection, line_cross,
// roi_intrusion, analytics_snapshot, stream_added, stream_removed, stream_error.
//
// CSS uses NeoMind variables exclusively — no hardcoded colors. Styles are
// injected once via injectStyles() (singleton pattern, see StatsCard.tsx).

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
  /** Max events to keep visible (default 50). */
  maxEvents?: number;
  className?: string;
}

// ---------------------------------------------------------------------------
// Event → render metadata
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Styles (singleton)
// ---------------------------------------------------------------------------

const STYLE_ID = 'ds-event-feed-styles';
const STYLES = `
.ds-event-feed {
  --ds-ef-fg: var(--foreground);
  --ds-ef-muted: var(--muted-foreground);
  --ds-ef-card: var(--card);
  --ds-ef-border: var(--border);
  --ds-ef-accent: var(--primary);
  --ds-ef-on-primary: var(--primary-foreground, #ffffff);
  --ds-ef-success: var(--color-success);
  --ds-ef-warning: var(--color-warning);
  --ds-ef-error: var(--color-error);
  --ds-ef-info: var(--color-info);
  --ds-ef-radius: var(--radius-md, 6px);

  display: flex;
  flex-direction: column;
  width: 100%;
  min-height: 0;
  height: 100%;
  padding: 8px;
  box-sizing: border-box;
  background: var(--ds-ef-card);
  color: var(--ds-ef-fg);
  border: 1px solid var(--ds-ef-border);
  border-radius: var(--ds-ef-radius);
  font-size: 11px;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
}

.dark .ds-event-feed {
  --ds-ef-on-primary: var(--primary-foreground, #17172a);
}

.ds-event-feed__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 0 2px 6px;
  border-bottom: 1px solid var(--ds-ef-border);
}

.ds-event-feed__title {
  margin: 0;
  font-size: 12px;
  font-weight: 600;
  color: var(--ds-ef-fg);
}

.ds-event-feed__actions {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}

.ds-event-feed__ws {
  font-size: 10px;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--ds-ef-muted);
}
.ds-event-feed__ws--open { color: var(--ds-ef-success); }
.ds-event-feed__ws--connecting { color: var(--ds-ef-info); }
.ds-event-feed__ws--closed { color: var(--ds-ef-error); }

.ds-event-feed__btn {
  display: inline-flex;
  align-items: center;
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
  transition: background 120ms ease;
}
.ds-event-feed__btn:hover { background: var(--accent); }
.ds-event-feed__btn--active {
  background: var(--ds-ef-accent);
  color: var(--ds-ef-on-primary);
  border-color: var(--ds-ef-accent);
}
.ds-event-feed__btn--icon {
  width: 22px;
  padding: 0;
  justify-content: center;
}
.ds-event-feed__btn svg {
  width: 12px;
  height: 12px;
}

.ds-event-feed__list {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  padding: 4px 2px;
}

.ds-event-feed__empty {
  padding: 16px 4px;
  color: var(--ds-ef-muted);
  text-align: center;
  font-style: italic;
}

.ds-event-feed__row {
  display: grid;
  grid-template-columns: 56px 16px 1fr;
  align-items: center;
  gap: 6px;
  padding: 3px 4px;
  border-radius: var(--radius-sm, 4px);
  color: var(--ds-ef-fg);
  line-height: 1.4;
}
.ds-event-feed__row:nth-child(odd) {
  background: color-mix(in srgb, var(--ds-ef-card) 50%, transparent);
}

.ds-event-feed__time {
  font-size: 10px;
  font-variant-numeric: tabular-nums;
  color: var(--ds-ef-muted);
  white-space: nowrap;
}

.ds-event-feed__icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  flex-shrink: 0;
}
.ds-event-feed__icon svg { width: 14px; height: 14px; }

.ds-event-feed__icon--info svg { color: var(--ds-ef-info); }
.ds-event-feed__icon--success svg { color: var(--ds-ef-success); }
.ds-event-feed__icon--warning svg { color: var(--ds-ef-warning); }
.ds-event-feed__icon--error svg { color: var(--ds-ef-error); }

.ds-event-feed__desc {
  color: var(--ds-ef-fg);
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

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

export function EventFeed({ streamId, maxEvents = 50, className }: EventFeedProps) {
  const { events, status, clear } = useEvents(streamId);
  const containerRef = useRef<HTMLDivElement>(null);
  const [autoScroll, setAutoScroll] = useState(true);

  // Inject the singleton stylesheet once on mount.
  useEffect(() => {
    injectStyles();
  }, []);

  // Filter out housekeeping events + ones targeted at other streams. useEvents
  // already filters by stream_id when one is provided, but be defensive about
  // globally-scoped events (stream_added, ready, etc.) that have no stream_id
  // of their own — let the per-type description handle them.
  const filtered = useMemo(() => {
    return events
      .filter((e) => !NOISE_TYPES.has(e.type))
      .filter((e) => {
        // For globally-scoped event types (stream_added/removed/error), allow
        // them through if they reference our stream OR have no stream_id.
        const sid = (e as any).stream_id as string | undefined;
        if (!sid) return true;
        return sid === streamId;
      })
      .slice(-maxEvents);
  }, [events, maxEvents, streamId]);

  // Auto-scroll to bottom whenever a new event arrives.
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
        <h4 className="ds-event-feed__title">Events</h4>
        <div className="ds-event-feed__actions">
          <span className={`ds-event-feed__ws ${wsClass}`}>{status}</span>
          <button
            type="button"
            className={`ds-event-feed__btn ${autoScroll ? 'ds-event-feed__btn--active' : ''}`}
            onClick={() => setAutoScroll((v) => !v)}
            title={autoScroll ? 'Auto-scroll on' : 'Auto-scroll off'}
            aria-pressed={autoScroll}
          >
            {autoScroll ? '↓ auto' : 'manual'}
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
