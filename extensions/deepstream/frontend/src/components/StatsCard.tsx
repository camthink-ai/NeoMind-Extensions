// DeepStream StatsCard — system overview with metric tiles.
//
// Modern card design: header with status pill, 2×2 grid of metric tiles with
// big numbers and colored accents, mini progress bar for GPU utilization.

import { forwardRef, useEffect, useMemo, useState } from 'react';
import { useStreams } from '../hooks/useStreams';
import { useEvents } from '../hooks/useEvents';
import { dsCommands, type ServerConfig } from '../api';
import type { SidecarEvent, Stats, SystemStatus } from '../types';
import { RefreshIcon } from './icons';

export interface StatsCardProps {
  title?: string;
  className?: string;
  serverHost?: string;
  snapshotPort?: number;
  rtspPort?: number;
  dataSource?: { type: string; extensionId?: string; [key: string]: any };
}

type SidecarStatus = 'running' | 'degraded' | 'stalled' | 'not_installed';

function deriveStatus(
  systemStatus: SystemStatus | null,
  stats: Stats | null,
  anyStreamError: boolean,
): SidecarStatus {
  if (systemStatus && systemStatus.deepstream_installed === false) return 'not_installed';
  if (stats) {
    const ageMs = Date.now() - stats.ts;
    if (ageMs > 15_000) return 'stalled';
  }
  if (anyStreamError) return 'degraded';
  return 'running';
}

const STATUS_META: Record<SidecarStatus, { label: string; color: string; bg: string }> = {
  running:       { label: 'Running',    color: 'var(--ds-success)', bg: 'color-mix(in srgb, var(--ds-success) 12%, transparent)' },
  degraded:      { label: 'Degraded',   color: 'var(--ds-warning)', bg: 'color-mix(in srgb, var(--ds-warning) 12%, transparent)' },
  stalled:       { label: 'Stalled',    color: 'var(--ds-error)',   bg: 'color-mix(in srgb, var(--ds-error) 12%, transparent)' },
  not_installed: { label: 'Not Installed', color: 'var(--ds-muted)', bg: 'color-mix(in srgb, var(--ds-muted) 12%, transparent)' },
};

const STYLE_ID = 'ds-stats-card-styles';
const STYLES = `
.ds-stats-card {
  --ds-fg: var(--foreground);
  --ds-muted: var(--muted-foreground);
  --ds-card: var(--card);
  --ds-border: var(--border);
  --ds-accent: var(--primary);
  --ds-on-primary: var(--primary-foreground, #ffffff);
  --ds-success: var(--color-success, #22c55e);
  --ds-warning: var(--color-warning, #f59e0b);
  --ds-error: var(--color-error, #ef4444);
  --ds-info: var(--color-info, #3b82f6);
  --ds-destructive: var(--destructive);
  --ds-destructive-fg: var(--destructive-foreground, #ffffff);
  --ds-tile-bg: color-mix(in srgb, var(--ds-card) 50%, color-mix(in srgb, var(--ds-muted) 8%, transparent));
  --ds-radius: var(--radius-lg, 12px);

  display: flex;
  flex-direction: column;
  width: 100%;
  height: 100%;
  min-height: 0;
  padding: 14px;
  background: var(--ds-card);
  border: 1px solid var(--ds-border);
  border-radius: var(--ds-radius);
  box-sizing: border-box;
  font-size: 12px;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  color: var(--ds-fg);
  gap: 12px;
}
.dark .ds-stats-card {
  --ds-on-primary: var(--primary-foreground, #17172a);
  --ds-destructive-fg: var(--destructive-foreground, #17172a);
}

.ds-stats-card__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  flex-shrink: 0;
}
.ds-stats-card__title-group {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}
.ds-stats-card__title {
  margin: 0;
  font-size: 14px;
  font-weight: 700;
  color: var(--ds-fg);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ds-stats-card__pill {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 2px 10px;
  border-radius: var(--radius-full, 9999px);
  font-size: 10px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  white-space: nowrap;
}
.ds-stats-card__pill::before {
  content: "";
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: currentColor;
  animation: ds-pulse 2s ease-in-out infinite;
}
@keyframes ds-pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.5; }
}

.ds-stats-card__actions {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}
.ds-stats-card__btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
  height: 28px;
  padding: 0 10px;
  border: 1px solid var(--ds-border);
  border-radius: var(--radius-md, 8px);
  background: transparent;
  color: var(--ds-fg);
  font-size: 11px;
  font-weight: 500;
  cursor: pointer;
  transition: all 160ms ease;
}
.ds-stats-card__btn:hover {
  background: color-mix(in srgb, var(--ds-accent) 10%, transparent);
  border-color: var(--ds-accent);
}
.ds-stats-card__btn:disabled { opacity: 0.4; cursor: not-allowed; }
.ds-stats-card__btn--icon { width: 28px; padding: 0; }
.ds-stats-card__btn--danger {
  border-color: color-mix(in srgb, var(--ds-destructive) 50%, transparent);
  color: var(--ds-destructive);
}
.ds-stats-card__btn--danger:hover {
  background: var(--ds-destructive);
  color: var(--ds-destructive-fg);
  border-color: var(--ds-destructive);
}
.ds-stats-card__btn svg { width: 14px; height: 14px; }

.ds-stats-card__grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
  flex: 1 1 auto;
  min-height: 0;
}

.ds-stats-card__tile {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 10px 12px;
  border-radius: var(--radius-md, 8px);
  background: var(--ds-tile-bg);
  border: 1px solid color-mix(in srgb, var(--ds-border) 60%, transparent);
  min-height: 0;
}
.ds-stats-card__tile-value {
  font-size: 22px;
  font-weight: 800;
  line-height: 1.1;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.02em;
}
.ds-stats-card__tile-label {
  font-size: 10px;
  font-weight: 500;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--ds-muted);
}
.ds-stats-card__tile--accent .ds-stats-card__tile-value { color: var(--ds-accent); }
.ds-stats-card__tile--success .ds-stats-card__tile-value { color: var(--ds-success); }
.ds-stats-card__tile--warning .ds-stats-card__tile-value { color: var(--ds-warning); }
.ds-stats-card__tile--info .ds-stats-card__tile-value { color: var(--ds-info); }

.ds-stats-card__bar {
  margin-top: 4px;
  height: 4px;
  border-radius: 9999px;
  background: color-mix(in srgb, var(--ds-muted) 20%, transparent);
  overflow: hidden;
}
.ds-stats-card__bar-fill {
  height: 100%;
  border-radius: 9999px;
  background: linear-gradient(90deg, var(--ds-success), var(--ds-warning));
  transition: width 400ms ease;
}

.ds-stats-card__msg {
  padding: 8px 10px;
  font-size: 11px;
  border-radius: var(--radius-md, 8px);
  flex: 1 1 auto;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--ds-muted);
}
.ds-stats-card__msg--error {
  color: var(--ds-error);
  background: color-mix(in srgb, var(--ds-error) 8%, transparent);
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

export const DeepStreamStatsCard = forwardRef<HTMLDivElement, StatsCardProps>(
  function DeepStreamStatsCard(props, ref) {
    const { title, className } = props;

    const { streams, loading, error, refresh } = useStreams();
    const { events } = useEvents();

    const [stats, setStats] = useState<Stats | null>(null);
    const [systemStatus, setSystemStatus] = useState<SystemStatus | null>(null);
    const [restarting, setRestarting] = useState(false);
    const [restartError, setRestartError] = useState<string | null>(null);

    useEffect(() => { injectStyles(); }, []);

    useEffect(() => {
      let latest: Stats | null = null;
      for (let i = events.length - 1; i >= 0; i--) {
        const ev = events[i] as SidecarEvent;
        if (ev.type === 'stats') { latest = ev as unknown as Stats; break; }
      }
      if (latest) setStats(latest);
    }, [events]);

    useEffect(() => {
      let cancelled = false;
      dsCommands.diagnose().then((r) => {
        if (cancelled) return;
        if (r.success && r.data) setSystemStatus(r.data);
      }).catch(() => {});
      return () => { cancelled = true; };
    }, []);

    const anyStreamError = useMemo(() => streams.some((s) => s.status === 'error'), [streams]);
    const sidecarStatus = useMemo(() => deriveStatus(systemStatus, stats, anyStreamError), [systemStatus, stats, anyStreamError]);
    const totalFps = useMemo(() => {
      if (!stats) return 0;
      if (typeof stats.global_fps === 'number' && stats.global_fps > 0) return stats.global_fps;
      return (stats.per_stream ?? []).reduce((sum, s) => sum + (s.fps ?? 0), 0);
    }, [stats]);

    const handleRestart = async () => {
      if (typeof window !== 'undefined' && !window.confirm('Restart sidecar? Active streams will be reconnected.')) return;
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
    const statusMeta = STATUS_META[sidecarStatus];

    return (
      <div ref={ref} className={`ds-stats-card ${className ?? ''}`}>
        <header className="ds-stats-card__header">
          <div className="ds-stats-card__title-group">
            <h3 className="ds-stats-card__title">{title ?? 'DeepStream'}</h3>
            <span
              className="ds-stats-card__pill"
              style={{ color: statusMeta.color, background: statusMeta.bg }}
            >
              {statusMeta.label}
            </span>
          </div>
          <div className="ds-stats-card__actions">
            <button type="button" className="ds-stats-card__btn ds-stats-card__btn--icon" onClick={refresh} aria-label="Refresh" title="Refresh">
              <RefreshIcon />
            </button>
            <button type="button" className="ds-stats-card__btn ds-stats-card__btn--danger" onClick={handleRestart} disabled={restarting}>
              {restarting ? 'Restarting…' : 'Restart'}
            </button>
          </div>
        </header>

        {(loading || error || restartError) ? (
          <div className={`ds-stats-card__msg ${error || restartError ? 'ds-stats-card__msg--error' : ''}`}>
            {loading ? 'Loading…' : (error || restartError)}
          </div>
        ) : (
          <div className="ds-stats-card__grid">
            <div className="ds-stats-card__tile ds-stats-card__tile--accent">
              <span className="ds-stats-card__tile-value">
                {typeof gpuUtil === 'number' ? `${gpuUtil.toFixed(0)}%` : '—'}
              </span>
              <span className="ds-stats-card__tile-label">GPU Util</span>
              {typeof gpuUtil === 'number' && (
                <div className="ds-stats-card__bar">
                  <div className="ds-stats-card__bar-fill" style={{ width: `${Math.min(gpuUtil, 100)}%` }} />
                </div>
              )}
            </div>

            <div className="ds-stats-card__tile ds-stats-card__tile--info">
              <span className="ds-stats-card__tile-value">
                {typeof gpuMem === 'number' ? gpuMem.toFixed(0) : '—'}
                <span style={{ fontSize: '12px', fontWeight: 500, color: 'var(--ds-muted)', marginLeft: '2px' }}> MB</span>
              </span>
              <span className="ds-stats-card__tile-label">GPU Memory</span>
            </div>

            <div className="ds-stats-card__tile ds-stats-card__tile--success">
              <span className="ds-stats-card__tile-value">{streams.length}</span>
              <span className="ds-stats-card__tile-label">Active Streams</span>
            </div>

            <div className="ds-stats-card__tile ds-stats-card__tile--warning">
              <span className="ds-stats-card__tile-value">{totalFps.toFixed(1)}</span>
              <span className="ds-stats-card__tile-label">Total FPS</span>
            </div>
          </div>
        )}
      </div>
    );
  },
);

export default { DeepStreamStatsCard };
