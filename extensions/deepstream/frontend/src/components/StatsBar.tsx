// DeepStream StatsBar — compact horizontal status bar for the ManagerCard.
//
// Extracted from StatsCard.tsx. Same data sources (useEvents stats events +
// dsCommands.diagnose + useStreams active count) and the same deriveStatus
// logic (15s freshness threshold). Rendered as a single row: status pill,
// metric chips, then refresh/restart/add actions.

import { forwardRef, useEffect, useMemo, useState } from 'react';
import { useStreams } from '../hooks/useStreams';
import { useEvents } from '../hooks/useEvents';
import { dsCommands } from '../api';
import type { SidecarEvent, Stats, SystemStatus } from '../types';
import { RefreshIcon, PlusIcon } from './icons';

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
  running:       { label: 'Running',    color: 'var(--ds-bar-success)', bg: 'color-mix(in srgb, var(--ds-bar-success) 12%, transparent)' },
  degraded:      { label: 'Degraded',   color: 'var(--ds-bar-warning)', bg: 'color-mix(in srgb, var(--ds-bar-warning) 12%, transparent)' },
  stalled:       { label: 'Stalled',    color: 'var(--ds-bar-error)',   bg: 'color-mix(in srgb, var(--ds-bar-error) 12%, transparent)' },
  not_installed: { label: 'Not Installed', color: 'var(--ds-bar-muted)', bg: 'color-mix(in srgb, var(--ds-bar-muted) 12%, transparent)' },
};

export interface StatsBarProps {
  className?: string;
  onAddStream?: () => void;
  onRefreshed?: () => void;
}

export const StatsBar = forwardRef<HTMLDivElement, StatsBarProps>(
  function StatsBar(props, ref) {
    const { className, onAddStream } = props;
    const { streams, loading, error, refresh } = useStreams();
    const { events } = useEvents();

    const [stats, setStats] = useState<Stats | null>(null);
    const [systemStatus, setSystemStatus] = useState<SystemStatus | null>(null);
    const [restarting, setRestarting] = useState(false);
    const [restartError, setRestartError] = useState<string | null>(null);

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

    const gpuUtil = stats?.gpu_utilization_percent;
    const gpuMem = stats?.gpu_memory_used_mb;
    const statusMeta = STATUS_META[sidecarStatus];

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

    const handleRefresh = () => { refresh(); props.onRefreshed?.(); };

    return (
      <div ref={ref} className={`ds-stats-bar ${className ?? ''}`}>
        <span
          className="ds-stats-bar__pill"
          style={{ color: statusMeta.color, background: statusMeta.bg }}
          title={restartError ?? undefined}
        >
          {statusMeta.label}
        </span>

        <div className="ds-stats-bar__chips">
          <span className="ds-stats-bar__chip" title="GPU utilization">
            <span className="ds-stats-bar__chip-value">
              {typeof gpuUtil === 'number' ? `${gpuUtil.toFixed(0)}%` : '—'}
            </span>
            <span className="ds-stats-bar__chip-label">GPU</span>
          </span>
          <span className="ds-stats-bar__chip" title="GPU memory used">
            <span className="ds-stats-bar__chip-value">
              {typeof gpuMem === 'number' ? gpuMem.toFixed(0) : '—'}
            </span>
            <span className="ds-stats-bar__chip-label">MB</span>
          </span>
          <span className="ds-stats-bar__chip" title="Aggregate throughput">
            <span className="ds-stats-bar__chip-value">{totalFps.toFixed(1)}</span>
            <span className="ds-stats-bar__chip-label">FPS</span>
          </span>
          <span className="ds-stats-bar__chip" title="Active streams">
            <span className="ds-stats-bar__chip-value">{streams.length}</span>
            <span className="ds-stats-bar__chip-label">Streams</span>
          </span>
        </div>

        <div className="ds-stats-bar__actions">
          <button type="button" className="ds-stats-bar__btn ds-stats-bar__btn--icon" onClick={handleRefresh} aria-label="Refresh" title="Refresh">
            <RefreshIcon />
          </button>
          <button
            type="button"
            className="ds-stats-bar__btn ds-stats-bar__btn--danger"
            onClick={handleRestart}
            disabled={restarting}
            title="Restart sidecar"
          >
            {restarting ? 'Restarting…' : 'Restart'}
          </button>
          {onAddStream && (
            <button type="button" className="ds-stats-bar__btn ds-stats-bar__btn--primary" onClick={onAddStream}>
              <PlusIcon /> Add
            </button>
          )}
        </div>

        {(loading || error) && !stats && (
          <span className="ds-stats-bar__hint">
            {loading ? 'Loading…' : (error || restartError)}
          </span>
        )}
      </div>
    );
  },
);

StatsBar.displayName = 'StatsBar';
export default { StatsBar };
