// DeepStream ManagerCard — single comprehensive management card.
//
// Layout:
//   - Compact stats bar (status/GPU/FPS/streams + refresh/restart/add)
//   - Grid with split-screen layout (1/4/9/16 based on stream count)
//   - DetailDrawer floating panel with HLS live video
//   - AddStream modal
//
// Card does NOT fill full height — it sizes to content. The grid uses
// square-ish tiles in 1/2/3/4 column layouts depending on stream count.

import { forwardRef, useEffect, useMemo, useState } from 'react';
import { useStreams } from '../hooks/useStreams';
import { StreamThumbnail } from './StreamThumbnail';
import { StatsBar } from './StatsBar';
import { DetailDrawer } from './DetailDrawer';
import { AddStreamForm } from './AddStreamForm';
import { CameraIcon, CloseIcon } from './icons';
import type { ServerConfig } from '../api';

export interface ManagerCardProps {
  title?: string;
  className?: string;
  serverHost?: string;
  snapshotPort?: number;
  rtspPort?: number;
  webrtcPort?: number;
  hlsPort?: number;
  mjpegPort?: number;
  snapshotToken?: string;
  defaultStreamId?: string;
}

const STYLES = `
.ds-manager {
  --ds-mgr-fg: var(--foreground);
  --ds-mgr-muted: var(--muted-foreground);
  --ds-mgr-card: var(--card);
  --ds-mgr-border: var(--border);
  --ds-mgr-accent: var(--primary);
  --ds-mgr-on-primary: var(--primary-foreground, #ffffff);
  --ds-mgr-success: var(--color-success, #22c55e);
  --ds-mgr-warning: var(--color-warning, #f59e0b);
  --ds-mgr-error: var(--color-error, #ef4444);
  --ds-mgr-info: var(--color-info, #3b82f6);

  /* StatsBar variable aliases */
  --ds-bar-fg: var(--foreground);
  --ds-bar-muted: var(--muted-foreground);
  --ds-bar-card: var(--card);
  --ds-bar-border: var(--border);
  --ds-bar-accent: var(--primary);
  --ds-bar-on-primary: var(--primary-foreground, #ffffff);
  --ds-bar-success: var(--color-success, #22c55e);
  --ds-bar-warning: var(--color-warning, #f59e0b);
  --ds-bar-error: var(--color-error, #ef4444);
  --ds-bar-info: var(--color-info, #3b82f6);
  --ds-bar-destructive: var(--destructive);
  --ds-bar-destructive-fg: var(--destructive-foreground, #ffffff);
  --ds-bar-tile-bg: color-mix(in srgb, var(--muted-foreground) 6%, transparent);

  position: relative;
  display: flex;
  flex-direction: column;
  width: 100%;
  height: 100%;
  min-height: 0;
  padding: 8px;
  background: var(--ds-mgr-card);
  border: 1px solid var(--ds-mgr-border);
  border-radius: var(--radius-lg, 12px);
  box-sizing: border-box;
  font-size: 12px;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  color: var(--ds-mgr-fg);
  gap: 8px;
  overflow: hidden;
}

/* ---- StatsBar (compact, borderless) ---- */
.ds-stats-bar {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
  flex-wrap: wrap;
}
.ds-stats-bar__pill {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 2px 8px;
  border-radius: 9999px;
  font-size: 10px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  white-space: nowrap;
}
.ds-stats-bar__pill::before {
  content: "";
  width: 5px; height: 5px;
  border-radius: 50%;
  background: currentColor;
  animation: ds-mgr-pulse 2s ease-in-out infinite;
}
@keyframes ds-mgr-pulse { 0%,100%{opacity:1} 50%{opacity:.5} }
.ds-stats-bar__chips { display: inline-flex; align-items: center; gap: 5px; flex-wrap: wrap; }
.ds-stats-bar__chip {
  display: inline-flex;
  align-items: baseline;
  gap: 3px;
  padding: 2px 7px;
  border-radius: 6px;
  background: color-mix(in srgb, var(--ds-mgr-muted) 8%, transparent);
}
.ds-stats-bar__chip-value { font-size: 12px; font-weight: 700; font-variant-numeric: tabular-nums; line-height: 1; }
.ds-stats-bar__chip-label { font-size: 8px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.05em; color: var(--ds-mgr-muted); }
.ds-stats-bar__actions { display: inline-flex; align-items: center; gap: 5px; margin-left: auto; }
.ds-stats-bar__btn {
  display: inline-flex; align-items: center; justify-content: center; gap: 4px;
  height: 24px; padding: 0 8px;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--ds-mgr-fg);
  font-size: 11px; font-weight: 500; font-family: inherit;
  cursor: pointer;
  transition: background 160ms ease;
}
.ds-stats-bar__btn:hover { background: color-mix(in srgb, var(--ds-mgr-accent) 12%, transparent); }
.ds-stats-bar__btn:disabled { opacity: 0.4; cursor: not-allowed; }
.ds-stats-bar__btn--icon { width: 24px; padding: 0; }
.ds-stats-bar__btn--primary {
  background: var(--ds-mgr-accent); color: var(--ds-mgr-on-primary);
}
.ds-stats-bar__btn--primary:hover { opacity: 0.88; background: var(--ds-mgr-accent); }
.ds-stats-bar__btn--danger { color: var(--ds-mgr-error); }
.ds-stats-bar__btn--danger:hover { background: color-mix(in srgb, var(--ds-mgr-error) 15%, transparent); }
.ds-stats-bar__btn svg { width: 13px; height: 13px; }
.ds-stats-bar__hint { width: 100%; font-size: 10px; color: var(--ds-mgr-muted); }

/* ---- Grid: split-screen 1/4/9/16 ---- */
.ds-manager__grid {
  display: grid;
  gap: 6px;
  flex: 0 1 auto;
}
.ds-manager__grid--cols-1 { grid-template-columns: 1fr; }
.ds-manager__grid--cols-2 { grid-template-columns: repeat(2, 1fr); }
.ds-manager__grid--cols-3 { grid-template-columns: repeat(3, 1fr); }
.ds-manager__grid--cols-4 { grid-template-columns: repeat(4, 1fr); }

.ds-manager__empty {
  display: flex; flex-direction: column;
  align-items: center; justify-content: center;
  gap: 10px; flex: 1 1 auto; min-height: 0;
  padding: 20px; text-align: center; color: var(--ds-mgr-muted);
}
.ds-manager__empty-icon {
  width: 44px; height: 44px;
  display: flex; align-items: center; justify-content: center;
  border-radius: 50%;
  background: color-mix(in srgb, var(--ds-mgr-muted) 10%, transparent);
}
.ds-manager__empty-icon svg { width: 22px; height: 22px; opacity: 0.6; }
.ds-manager__empty p { margin: 0; font-size: 12px; }

/* ---- AddStream modal ---- */
.ds-manager__overlay {
  position: absolute; inset: 0;
  background: rgba(0, 0, 0, 0.55);
  display: flex; align-items: center; justify-content: center;
  z-index: 20; padding: 8px;
}
.ds-manager__modal {
  position: relative;
  width: min(540px, 100%);
  max-height: 100%;
  overflow-y: auto;
  background: var(--ds-mgr-card);
  background-color: var(--ds-mgr-card);
  border: 1px solid var(--ds-mgr-border);
  border-radius: var(--radius-lg, 12px);
  box-shadow: 0 12px 32px rgba(0,0,0,0.35);
}
.ds-manager__modal-close {
  position: absolute; top: 8px; right: 8px;
  width: 26px; height: 26px;
  display: inline-flex; align-items: center; justify-content: center;
  border: none; border-radius: 6px;
  background: transparent; color: var(--ds-mgr-muted);
  cursor: pointer; z-index: 2;
}
.ds-manager__modal-close:hover { background: color-mix(in srgb, var(--ds-mgr-fg) 10%, transparent); color: var(--ds-mgr-fg); }
.ds-manager__modal-close svg { width: 14px; height: 14px; }

/* ---- DetailDrawer ---- */
.ds-drawer__backdrop {
  position: absolute;
  top: 0; left: 0; bottom: 0;
  width: 54%;
  background: rgba(0, 0, 0, 0.45);
  z-index: 4;
  cursor: pointer;
}
.ds-drawer__panel {
  position: absolute;
  top: 0; right: 0; bottom: 0;
  width: 46%;
  background: var(--card);
  background-color: var(--card);
  border-left: 1px solid var(--border);
  box-shadow: -8px 0 24px rgba(0, 0, 0, 0.3);
  z-index: 5;
  display: flex; flex-direction: column;
  gap: 8px;
  padding: 10px;
  box-sizing: border-box;
  font-size: 12px;
  overflow-y: auto;
}
.ds-drawer__header {
  display: flex; align-items: center; justify-content: space-between;
  gap: 6px; flex-shrink: 0;
}
.ds-drawer__title-group { display: flex; align-items: center; gap: 6px; min-width: 0; }
.ds-drawer__status-dot { width: 8px; height: 8px; border-radius: 50%; flex-shrink: 0; }
.ds-drawer__title {
  margin: 0; font-size: 13px; font-weight: 700;
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}
.ds-drawer__status-label { font-size: 10px; font-weight: 600; flex-shrink: 0; }
.ds-drawer__actions { display: inline-flex; align-items: center; gap: 5px; flex-shrink: 0; }
.ds-drawer__btn {
  display: inline-flex; align-items: center; gap: 3px;
  height: 24px; padding: 0 8px;
  border: none;
  border-radius: 6px;
  background: transparent; color: var(--foreground);
  font-size: 10px; font-weight: 500; font-family: inherit;
  cursor: pointer; transition: background 160ms ease;
}
.ds-drawer__btn:hover { background: color-mix(in srgb, var(--primary) 12%, transparent); }
.ds-drawer__btn--icon { width: 24px; padding: 0; justify-content: center; }
.ds-drawer__btn--danger { color: var(--color-error, #ef4444); }
.ds-drawer__btn--danger:hover { background: color-mix(in srgb, var(--color-error, #ef4444) 15%, transparent); }
.ds-drawer__btn svg { width: 12px; height: 12px; }

.ds-drawer__image {
  position: relative;
  width: 100%;
  aspect-ratio: 16 / 9;
  background: #000;
  border-radius: 6px;
  overflow: hidden;
  flex-shrink: 0;
}
.ds-drawer__video {
  display: block; width: 100%; height: 100%; object-fit: cover;
  border: none;
}
.ds-drawer__image-placeholder {
  position: absolute; inset: 0;
  display: flex; align-items: center; justify-content: center;
  color: var(--muted-foreground);
  background: color-mix(in srgb, var(--muted-foreground) 12%, #000);
  font-size: 11px;
}
.ds-drawer__image-badge {
  position: absolute; top: 6px; left: 6px;
  padding: 2px 8px;
  border-radius: 9999px;
  background: rgba(0,0,0,0.65);
  backdrop-filter: blur(8px);
  font-size: 9px; font-weight: 600;
  text-transform: uppercase; letter-spacing: 0.04em;
  color: #fff;
}

.ds-drawer__counts {
  display: grid; grid-template-columns: repeat(4, 1fr);
  gap: 5px; flex-shrink: 0;
}
.ds-drawer__count {
  display: flex; flex-direction: column; align-items: center; gap: 1px;
  padding: 5px 2px;
  border-radius: 6px;
  background: color-mix(in srgb, var(--muted-foreground) 6%, transparent);
}
.ds-drawer__count-value { font-size: 14px; font-weight: 800; font-variant-numeric: tabular-nums; line-height: 1.1; }
.ds-drawer__count-label { font-size: 8px; font-weight: 500; text-transform: uppercase; letter-spacing: 0.04em; color: var(--muted-foreground); }

.ds-drawer__rtsp { display: flex; align-items: center; gap: 6px; flex-shrink: 0; }
.ds-drawer__rtsp-url {
  flex: 1 1 auto; min-width: 0;
  padding: 5px 8px;
  border-radius: 6px;
  background: color-mix(in srgb, var(--muted-foreground) 6%, transparent);
  color: var(--foreground);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 10px;
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}

.ds-drawer__events { flex: 1 1 auto; min-height: 80px; }
.ds-drawer__msg {
  padding: 6px 8px; font-size: 10px; border-radius: 6px;
  color: var(--muted-foreground);
}
.ds-drawer__msg--error { color: var(--color-error, #ef4444); background: color-mix(in srgb, var(--color-error, #ef4444) 8%, transparent); }
`;

/** Determine grid columns for split-screen layout (1/4/9/16). */
function gridCols(count: number): number {
  if (count <= 1) return 1;
  if (count <= 4) return 2;
  if (count <= 9) return 3;
  return 4;
}

export const DeepStreamManagerCard = forwardRef<HTMLDivElement, ManagerCardProps>(
  function DeepStreamManagerCard(props, ref) {
    const {
      title,
      className,
      serverHost,
      snapshotPort,
      rtspPort,
      hlsPort = 8888,
      snapshotToken,
      defaultStreamId,
    } = props;

    const { streams, serverHost: extServerHost, loading, error, refresh } = useStreams();
    const [selected, setSelected] = useState<string | null>(null);
    const [showAddForm, setShowAddForm] = useState(false);

    useEffect(() => {
      if (selected || !defaultStreamId) return;
      if (streams.some((s) => s.stream_id === defaultStreamId)) setSelected(defaultStreamId);
    }, [defaultStreamId, streams, selected]);

    const effectiveHost = serverHost || extServerHost || undefined;
    const server: ServerConfig | undefined = effectiveHost ? { host: effectiveHost, snapshotPort, rtspPort } : undefined;

    const selectedStream = useMemo(
      () => streams.find((s) => s.stream_id === selected) ?? null,
      [streams, selected],
    );

    // HLS URL for the selected stream (detail drawer).
    const hlsUrl = useMemo(() => {
      if (!selected) return null;
      const host = effectiveHost ?? (typeof window !== 'undefined' ? window.location.hostname : 'localhost');
      return `http://${host}:${hlsPort}/ds/${encodeURIComponent(selected)}/index.m3u8`;
    }, [selected, effectiveHost, hlsPort]);

    // Per-stream MJPEG URL builder for grid tiles (live preview).
    const mjpegPort = props.mjpegPort ?? 8090;
    const buildMjpegUrl = (streamId: string): string => {
      const host = effectiveHost ?? (typeof window !== 'undefined' ? window.location.hostname : 'localhost');
      return `http://${host}:${mjpegPort}/mjpeg/${encodeURIComponent(streamId)}`;
    };

    const cols = gridCols(streams.length);

    const handleCreated = (streamId: string) => {
      setShowAddForm(false);
      void refresh();
      void streamId;
    };

    return (
      <>
        <style>{STYLES}</style>
        <div ref={ref} className={`ds-manager ${className ?? ''}`}>
          <StatsBar onAddStream={() => setShowAddForm(true)} onRefreshed={() => refresh()} />

          {loading && streams.length === 0 ? (
            <div className="ds-manager__empty"><p>Loading streams…</p></div>
          ) : error ? (
            <div className="ds-manager__empty"><p style={{ color: 'var(--ds-mgr-error)' }}>{error}</p></div>
          ) : streams.length === 0 ? (
            <div className="ds-manager__empty">
              <div className="ds-manager__empty-icon"><CameraIcon /></div>
              <p>No streams yet. Click <strong>Add</strong> to add one.</p>
            </div>
          ) : (
            <div className={`ds-manager__grid ds-manager__grid--cols-${cols}`}>
              {streams.map((s) => (
                <StreamThumbnail
                  key={s.stream_id}
                  stream={s}
                  mjpegUrl={buildMjpegUrl(s.stream_id)}
                  onClick={(sid) => setSelected(sid)}
                />
              ))}
            </div>
          )}

          {selected && selectedStream ? (
            <DetailDrawer
              streamId={selected}
              snapshotToken={snapshotToken ?? (selectedStream.snapshot_token ?? undefined) ?? undefined}
              server={server}
              hlsUrl={hlsUrl}
              onClose={() => setSelected(null)}
              onStreamRemoved={() => refresh()}
            />
          ) : null}

          {showAddForm && (
            <div className="ds-manager__overlay" onClick={() => setShowAddForm(false)}>
              <div className="ds-manager__modal" onClick={(e) => e.stopPropagation()}>
                <button
                  className="ds-manager__modal-close"
                  onClick={() => setShowAddForm(false)}
                  aria-label="Close"
                  title="Close"
                >
                  <CloseIcon />
                </button>
                <AddStreamForm onCreated={handleCreated} onCancel={() => setShowAddForm(false)} />
              </div>
            </div>
          )}
        </div>
      </>
    );
  },
);

DeepStreamManagerCard.displayName = 'DeepStreamManagerCard';
export default { DeepStreamManagerCard };
