// DeepStream StatsCard — smallest card showing global sidecar/GPU stats.
//
// Per spec §5.2: displays sidecar status, GPU utilization, GPU memory, active
// stream count, total throughput, and a restart button. Stats are derived from
// the `stats` sidecar event stream + periodic `diagnose()` calls — the frontend
// has no direct access to the dynamic metrics registry.
//
// CSS uses NeoMind CSS variables exclusively (no hardcoded colors). Scoped with
// `.ds-stats-card` prefix. forwardRef + loading/error/empty states per the
// Extension Frontend Design Guide.

import { forwardRef, useEffect, useMemo, useState } from 'react';
import { useStreams } from '../hooks/useStreams';
import { useEvents } from '../hooks/useEvents';
import { dsCommands } from '../api';
import type { SidecarEvent, Stats, SystemStatus } from '../types';
import { CameraIcon, GaugeIcon, RefreshIcon, StatusDotIcon } from './icons';

// ---------------------------------------------------------------------------
// Props
// ---------------------------------------------------------------------------

export interface StatsCardProps {
  title?: string;
  className?: string;
  dataSource?: {
    type: string;
    extensionId?: string;
    [key: string]: any;
  };
}

// ---------------------------------------------------------------------------
// Sidecar status derivation
// ---------------------------------------------------------------------------

type SidecarStatus = 'running' | 'degraded' | 'stalled' | 'not_installed';

function deriveStatus(
  systemStatus: SystemStatus | null,
  stats: Stats | null,
  anyStreamError: boolean,
): SidecarStatus {
  if (systemStatus && systemStatus.deepstream_installed === false) {
    return 'not_installed';
  }
  // Stats events should arrive every ~1s; treat >15s as stalled.
  if (stats) {
    const ageMs = Date.now() - stats.ts;
    if (ageMs > 15_000) return 'stalled';
  } else {
    // No stats yet — if system is installed but silent, also treat as stalled
    // after a short grace. We can't measure time-since-mount cleanly here, so
    // we fall through and let the loading/empty state handle the silence.
  }
  if (anyStreamError) return 'degraded';
  return 'running';
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const STYLE_ID = 'ds-stats-card-styles';
const STYLES = `
.ds-stats-card {
  --ds-fg: var(--foreground);
  --ds-muted: var(--muted-foreground);
  --ds-card: var(--card);
  --ds-border: var(--border);
  --ds-accent: var(--primary);
  --ds-on-primary: var(--primary-foreground, #ffffff);
  --ds-success: var(--color-success);
  --ds-warning: var(--color-warning);
  --ds-error: var(--color-error);
  --ds-destructive: var(--destructive);
  --ds-destructive-fg: var(--destructive-foreground, #ffffff);
  --ds-radius: var(--radius-lg, 10px);
  display: flex;
  flex-direction: column;
  width: 100%;
  height: 100%;
  padding: 12px;
  background: var(--ds-card);
  border: 1px solid var(--ds-border);
  border-radius: var(--ds-radius);
  box-sizing: border-box;
  font-size: 12px;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  color: var(--ds-fg);
}

.dark .ds-stats-card {
  --ds-on-primary: var(--primary-foreground, #17172a);
  --ds-destructive-fg: var(--destructive-foreground, #17172a);
}

.ds-stats-card__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 10px;
}

.ds-stats-card__title {
  margin: 0;
  font-size: 13px;
  font-weight: 600;
  color: var(--ds-fg);
}

.ds-stats-card__actions {
  display: flex;
  align-items: center;
  gap: 6px;
}

.ds-stats-card__btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
  height: 24px;
  padding: 0 8px;
  border: 1px solid var(--ds-border);
  border-radius: var(--radius-md, 6px);
  background: transparent;
  color: var(--ds-fg);
  font-size: 11px;
  font-weight: 500;
  cursor: pointer;
  transition: background 120ms ease;
}
.ds-stats-card__btn:hover {
  background: var(--accent);
}
.ds-stats-card__btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.ds-stats-card__btn--icon {
  width: 24px;
  padding: 0;
}
.ds-stats-card__btn--danger {
  border-color: var(--ds-destructive);
  color: var(--ds-destructive);
}
.ds-stats-card__btn--danger:hover {
  background: var(--ds-destructive);
  color: var(--ds-destructive-fg);
}

.ds-stats-card__loading,
.ds-stats-card__error {
  padding: 12px 4px;
  font-size: 12px;
  color: var(--ds-muted);
}
.ds-stats-card__error {
  color: var(--ds-error);
}

.ds-stats-card__body {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.ds-stats-card__row {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--ds-fg);
  line-height: 1.4;
}
.ds-stats-card__row svg {
  flex-shrink: 0;
  color: var(--ds-muted);
}

.ds-status {
  width: 10px;
  height: 10px;
}
.ds-status--running { color: var(--ds-success); }
.ds-status--degraded { color: var(--ds-warning); }
.ds-status--stalled { color: var(--ds-error); }
.ds-status--not_installed { color: var(--ds-muted); }
`;

function injectStyles() {
  if (typeof document === 'undefined') return;
  if (document.getElementById(STYLE_ID)) return;
  const el = document.createElement('style');
  el.id = STYLE_ID;
  el.textContent = STYLES;
  document.head.appendChild(el);
}

export const DeepStreamStatsCard = forwardRef<HTMLDivElement, StatsCardProps>(
  function DeepStreamStatsCard(props, ref) {
    const { title, className, dataSource } = props;

    const { streams, loading, error, refresh } = useStreams();
    const { events } = useEvents();

    const [stats, setStats] = useState<Stats | null>(null);
    const [systemStatus, setSystemStatus] = useState<SystemStatus | null>(null);
    const [restarting, setRestarting] = useState(false);
    const [restartError, setRestartError] = useState<string | null>(null);

    // Inject the scoped stylesheet once on mount.
    useEffect(() => {
      injectStyles();
    }, []);

    // Track the latest stats event from the sidecar event stream.
    useEffect(() => {
      let latest: Stats | null = null;
      for (let i = events.length - 1; i >= 0; i--) {
        const ev = events[i] as SidecarEvent;
        if (ev.type === 'stats') {
          latest = ev as unknown as Stats;
          break;
        }
      }
      if (latest) setStats(latest);
    }, [events]);

    // One-shot diagnose on mount (we don't poll it aggressively — diagnose is
    // relatively expensive and the result is mostly static during a session).
    useEffect(() => {
      let cancelled = false;
      dsCommands.diagnose().then((r) => {
        if (cancelled) return;
        if (r.success && r.data) setSystemStatus(r.data);
      }).catch(() => { /* surfaced via systemStatus === null */ });
      return () => { cancelled = true; };
    }, []);

    // Re-derive sidecar status whenever inputs change.
    const anyStreamError = useMemo(
      () => streams.some((s) => s.status === 'error'),
      [streams],
    );
    const sidecarStatus = useMemo(
      () => deriveStatus(systemStatus, stats, anyStreamError),
      [systemStatus, stats, anyStreamError],
    );

    // Sum of per-stream fps from the latest Stats event; falls back to 0.
    const totalFps = useMemo(() => {
      if (!stats) return 0;
      if (typeof stats.global_fps === 'number' && stats.global_fps > 0) {
        return stats.global_fps;
      }
      return (stats.per_stream ?? []).reduce((sum, s) => sum + (s.fps ?? 0), 0);
    }, [stats]);

    const handleRestart = async () => {
      if (typeof window !== 'undefined' && !window.confirm('Restart sidecar? Active streams will be reconnected.')) {
        return;
      }
      setRestarting(true);
      setRestartError(null);
      try {
        const r = await dsCommands.restartSidecar();
        if (!r.success) setRestartError(r.error ?? 'restart_sidecar failed');
        else refresh();
      } catch (e: any) {
        setRestartError(e?.message ?? String(e));
      } finally {
        setRestarting(false);
      }
    };

    const gpuUtil = stats?.gpu_utilization_percent;
    const gpuMem = stats?.gpu_memory_used_mb;

    return (
      <div ref={ref} className={`ds-stats-card ${className ?? ''}`}>
        <header className="ds-stats-card__header">
          <h3 className="ds-stats-card__title">{title ?? 'DeepStream'}</h3>
          <div className="ds-stats-card__actions">
            <button
              type="button"
              className="ds-stats-card__btn ds-stats-card__btn--icon"
              onClick={() => { refresh(); }}
              aria-label="Refresh stats"
              title="Refresh"
            >
              <RefreshIcon />
            </button>
            <button
              type="button"
              className="ds-stats-card__btn ds-stats-card__btn--danger"
              onClick={handleRestart}
              disabled={restarting}
              title="Restart the Python sidecar process"
            >
              {restarting ? 'Restarting…' : 'Restart'}
            </button>
          </div>
        </header>

        {loading && (
          <div className="ds-stats-card__loading">Loading…</div>
        )}
        {error && !loading && (
          <div className="ds-stats-card__error">{error}</div>
        )}
        {restartError && !loading && !error && (
          <div className="ds-stats-card__error">{restartError}</div>
        )}
        {!loading && !error && (
          <div className="ds-stats-card__body">
            <div className="ds-stats-card__row">
              <StatusDotIcon className={`ds-status ds-status--${sidecarStatus}`} />
              <span>{sidecarStatus.replace('_', ' ')}</span>
            </div>

            <div className="ds-stats-card__row">
              <GaugeIcon />
              <span>
                GPU: {typeof gpuUtil === 'number' ? gpuUtil.toFixed(1) : '—'}%
              </span>
            </div>

            <div className="ds-stats-card__row">
              <span>
                GPU mem: {typeof gpuMem === 'number' ? gpuMem.toFixed(0) : '—'} MB
              </span>
            </div>

            <div className="ds-stats-card__row">
              <CameraIcon />
              <span>{streams.length} streams</span>
            </div>

            <div className="ds-stats-card__row">
              <span>{totalFps.toFixed(1)} fps total</span>
            </div>
          </div>
        )}
      </div>
    );
  },
);

export default { DeepStreamStatsCard };
