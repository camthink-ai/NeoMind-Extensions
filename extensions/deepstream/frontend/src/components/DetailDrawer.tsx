// DeepStream DetailDrawer — floating overlay for the ManagerCard.
//
// Two siblings over the card body (card root must be position:relative):
//   - a dim backdrop covering the left ~54% (z-4), click closes
//   - a panel on the right ~46% (z-5) with shadow
// Image area: HLS live video via hls.js.

import { forwardRef, useEffect, useMemo, useState } from 'react';
import { useStream } from '../hooks/useStream';
import { useEvents } from '../hooks/useEvents';
import { useHls } from '../hooks/useHls';
import { getRtspUrl, dsCommands, type ServerConfig } from '../api';
import { CopyIcon, RefreshIcon, CloseIcon } from './icons';
import { EventFeed } from './EventFeed';
import type { SidecarEvent, StreamStatus } from '../types';

export interface DetailDrawerProps {
  streamId: string;
  className?: string;
  snapshotToken?: string;
  server?: ServerConfig;
  hlsUrl: string | null;
  onClose: () => void;
  onStreamRemoved?: () => void;
}

const STATUS_META: Record<string, { label: string; color: string }> = {
  running:       { label: 'Running',       color: 'var(--color-success, #22c55e)' },
  connecting:    { label: 'Connecting',    color: 'var(--color-info, #3b82f6)' },
  degraded:      { label: 'Degraded',      color: 'var(--color-warning, #f59e0b)' },
  reconnecting:  { label: 'Reconnecting',  color: 'var(--color-warning, #f59e0b)' },
  error:         { label: 'Error',         color: 'var(--color-error, #ef4444)' },
  stopped:       { label: 'Stopped',       color: 'var(--muted-foreground)' },
};

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

export const DetailDrawer = forwardRef<HTMLDivElement, DetailDrawerProps>(
  function DetailDrawer(props, ref) {
    const { streamId, className, server, hlsUrl, onClose, onStreamRemoved } = props;

    const { stream, serverHost: extServerHost, loading, error, refresh } = useStream(streamId);
    const { events } = useEvents(streamId);
    const { videoRef, status: hlsStatus } = useHls(hlsUrl);
    const [copyOk, setCopyOk] = useState(false);

    useEffect(() => {
      const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose(); };
      window.addEventListener('keydown', onKey);
      return () => window.removeEventListener('keydown', onKey);
    }, [onClose]);

    const counts = useMemo(() => summarizeCounts(events), [events]);
    const effectiveServer: ServerConfig | undefined = server ?? (extServerHost ? { host: extServerHost } : undefined);
    const rtspUrl = stream?.rtsp_url ?? getRtspUrl(streamId, effectiveServer);
    const statusMeta = stream ? (STATUS_META[stream.status as StreamStatus] ?? STATUS_META.stopped) : STATUS_META.stopped;
    const isLive = hlsStatus === 'live';

    const copyRtsp = async () => {
      try { await navigator.clipboard.writeText(rtspUrl); setCopyOk(true); setTimeout(() => setCopyOk(false), 1500); } catch {}
    };
    const onRemove = () => {
      if (stream && !window.confirm(`Remove stream "${stream.stream_id}"?`)) return;
      const sid = stream?.stream_id ?? streamId;
      dsCommands.removeStream(sid).then(() => { onStreamRemoved?.(); onClose(); });
    };

    return (
      <>
        <div className="ds-drawer__backdrop" onClick={onClose} />
        <div ref={ref} className={`ds-drawer__panel ${className ?? ''}`} onClick={(e) => e.stopPropagation()}>
          <header className="ds-drawer__header">
            <div className="ds-drawer__title-group">
              <span className="ds-drawer__status-dot" style={{ background: statusMeta.color }} />
              <h3 className="ds-drawer__title">{stream?.stream_id ?? streamId}</h3>
              {stream && <span className="ds-drawer__status-label" style={{ color: statusMeta.color }}>{statusMeta.label}</span>}
            </div>
            <div className="ds-drawer__actions">
              <button type="button" className="ds-drawer__btn ds-drawer__btn--icon" onClick={refresh} aria-label="Refresh" title="Refresh">
                <RefreshIcon />
              </button>
              <button type="button" className="ds-drawer__btn ds-drawer__btn--danger" onClick={onRemove} title="Remove stream">Remove</button>
              <button type="button" className="ds-drawer__btn ds-drawer__btn--icon" onClick={onClose} aria-label="Close" title="Close">
                <CloseIcon />
              </button>
            </div>
          </header>

          <div className="ds-drawer__image">
            <video
              ref={videoRef}
              className="ds-drawer__video"
              autoPlay
              playsInline
              muted
            />
            {!isLive && (
              <div className="ds-drawer__image-placeholder">
                {hlsStatus === 'loading' ? 'Connecting…' : 'No video'}
              </div>
            )}
            <div className="ds-drawer__image-badge">
              {isLive ? '● Live' : 'Connecting…'}
            </div>
          </div>

          {error && <div className="ds-drawer__msg ds-drawer__msg--error">{error}</div>}
          {loading && !stream && <div className="ds-drawer__msg">Loading…</div>}

          <div className="ds-drawer__counts">
            <div className="ds-drawer__count">
              <span className="ds-drawer__count-value" style={{ color: 'var(--color-info, #3b82f6)' }}>{counts.persons}</span>
              <span className="ds-drawer__count-label">Persons</span>
            </div>
            <div className="ds-drawer__count">
              <span className="ds-drawer__count-value" style={{ color: 'var(--color-success, #22c55e)' }}>{counts.vehicles}</span>
              <span className="ds-drawer__count-label">Vehicles</span>
            </div>
            <div className="ds-drawer__count">
              <span className="ds-drawer__count-value" style={{ color: 'var(--color-warning, #f59e0b)' }}>{counts.lineCrosses}</span>
              <span className="ds-drawer__count-label">Line Cross</span>
            </div>
            <div className="ds-drawer__count">
              <span className="ds-drawer__count-value" style={{ color: 'var(--color-error, #ef4444)' }}>{counts.roiAlerts}</span>
              <span className="ds-drawer__count-label">ROI Alerts</span>
            </div>
          </div>

          <div className="ds-drawer__rtsp">
            <code className="ds-drawer__rtsp-url" title={rtspUrl}>{rtspUrl}</code>
            <button type="button" className="ds-drawer__btn" onClick={copyRtsp} aria-label="Copy RTSP URL">
              <CopyIcon /> {copyOk ? 'Copied' : 'Copy'}
            </button>
          </div>

          <div className="ds-drawer__events">
            <EventFeed streamId={stream?.stream_id ?? streamId} />
          </div>
        </div>
      </>
    );
  },
);

DetailDrawer.displayName = 'DetailDrawer';
export default { DetailDrawer };
