// DeepStream StreamThumbnail — modern tile with gradient overlay.
//
// Larger tiles with bottom gradient overlay showing stream ID and status.
// Status badge in top-right corner. Hover scale + border highlight.

import { useRef, useEffect, useState } from 'react';
import { useSnapshot } from '../hooks/useSnapshot';
import { getSnapshotUrl, type ServerConfig } from '../api';
import type { Stream } from '../types';
import { CameraIcon } from './icons';

export interface StreamThumbnailProps {
  stream: Stream;
  onClick?: (streamId: string) => void;
  snapshotToken?: string;
  intervalMs?: number;
  server?: ServerConfig;
  className?: string;
}

const STATUS_COLORS: Record<string, string> = {
  running: 'var(--ds-thumb-ok)',
  connecting: 'var(--ds-thumb-info)',
  degraded: 'var(--ds-thumb-warn)',
  reconnecting: 'var(--ds-thumb-warn)',
  error: 'var(--ds-thumb-err)',
  stopped: 'var(--ds-thumb-idle)',
};

const STYLES = `
.ds-thumb {
  --ds-thumb-fg: var(--foreground);
  --ds-thumb-muted: var(--muted-foreground);
  --ds-thumb-card: var(--card);
  --ds-thumb-border: var(--border);
  --ds-thumb-ok: var(--color-success, #22c55e);
  --ds-thumb-warn: var(--color-warning, #f59e0b);
  --ds-thumb-err: var(--color-error, #ef4444);
  --ds-thumb-info: var(--color-info, #3b82f6);
  --ds-thumb-idle: var(--muted-foreground);

  position: relative;
  border-radius: var(--radius-md, 10px);
  overflow: hidden;
  background: #000;
  border: 1px solid var(--ds-thumb-border);
  cursor: default;
  transition: transform 200ms ease, box-shadow 200ms ease, border-color 200ms ease;
}
.ds-thumb[role="button"] { cursor: pointer; }
.ds-thumb[role="button"]:hover {
  transform: translateY(-2px);
  border-color: var(--primary);
  box-shadow: 0 8px 24px rgba(0,0,0,0.2);
}
.ds-thumb[role="button"]:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 2px;
}

.ds-thumb__image-wrap {
  position: relative;
  width: 100%;
  aspect-ratio: 16 / 9;
  overflow: hidden;
}
.ds-thumb__img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
  transition: opacity 300ms ease;
}
.ds-thumb__placeholder {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: color-mix(in srgb, var(--ds-thumb-muted) 15%, #000);
}
.ds-thumb__placeholder svg { width: 32px; height: 32px; opacity: 0.5; color: var(--ds-thumb-muted); }

.ds-thumb__badge {
  position: absolute;
  top: 8px;
  right: 8px;
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 3px 8px;
  border-radius: var(--radius-full, 9999px);
  background: rgba(0, 0, 0, 0.65);
  backdrop-filter: blur(8px);
  font-size: 9px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: #fff;
  z-index: 2;
}
.ds-thumb__badge-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
}

.ds-thumb__overlay {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  padding: 24px 12px 10px;
  background: linear-gradient(to top, rgba(0,0,0,0.85), rgba(0,0,0,0.4) 60%, transparent);
  z-index: 1;
}
.ds-thumb__id {
  font-size: 13px;
  font-weight: 600;
  color: #fff;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.ds-thumb__sub {
  font-size: 10px;
  color: rgba(255,255,255,0.7);
  margin-top: 1px;
}
`;

export function StreamThumbnail({
  stream,
  onClick,
  snapshotToken,
  intervalMs = 2000,
  server,
  className,
}: StreamThumbnailProps) {
  const ref = useRef<HTMLDivElement>(null);
  const { tick, pause, resume } = useSnapshot(stream.stream_id, intervalMs);
  const [intersecting, setIntersecting] = useState(true);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const io = new IntersectionObserver(
      (entries) => {
        const e = entries[0];
        if (!e) return;
        setIntersecting(e.isIntersecting);
        if (e.isIntersecting) resume(); else pause();
      },
      { threshold: 0.1 },
    );
    io.observe(el);
    return () => io.disconnect();
  }, [pause, resume]);

  const snapshotUrl = snapshotToken && tick > 0
    ? getSnapshotUrl(stream.stream_id, snapshotToken, tick, server)
    : null;

  const statusColor = STATUS_COLORS[stream.status] ?? STATUS_COLORS.stopped;

  return (
    <>
      <style>{STYLES}</style>
      <div
        ref={ref}
        className={`ds-thumb ${className ?? ''}`}
        onClick={onClick ? () => onClick(stream.stream_id) : undefined}
        role={onClick ? 'button' : undefined}
        tabIndex={onClick ? 0 : undefined}
        aria-label={onClick ? `Open stream ${stream.stream_id}` : undefined}
      >
        <div className="ds-thumb__image-wrap">
          {snapshotUrl && intersecting ? (
            <img
              src={snapshotUrl}
              alt={stream.stream_id}
              className="ds-thumb__img"
              loading="lazy"
              onError={(e) => { (e.currentTarget as HTMLImageElement).style.opacity = '0.2'; }}
            />
          ) : (
            <div className="ds-thumb__placeholder"><CameraIcon /></div>
          )}
        </div>
        <div className="ds-thumb__badge">
          <span className="ds-thumb__badge-dot" style={{ background: statusColor }} />
          {stream.status}
        </div>
        <div className="ds-thumb__overlay">
          <div className="ds-thumb__id">{stream.stream_id}</div>
          <div className="ds-thumb__sub">{stream.model}</div>
        </div>
      </div>
    </>
  );
}

export default StreamThumbnail;
