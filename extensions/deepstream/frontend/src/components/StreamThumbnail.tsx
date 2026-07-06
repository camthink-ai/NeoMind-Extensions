// DeepStream extension — stream thumbnail tile.
//
// A small (default ~180×135px, 4:3) tile that polls the sidecar snapshot JPEG
// endpoint at a low rate (default 2s) and renders the latest frame. When the
// tile scrolls out of view an IntersectionObserver pauses the polling hook so
// off-screen thumbnails don't keep the network busy. Without a snapshot token
// only a placeholder (CameraIcon) is rendered — this keeps the OverviewCard
// functional in setups where the snapshot port isn't reachable.

import { useRef, useEffect, useState } from 'react';
import { useSnapshot } from '../hooks/useSnapshot';
import { getSnapshotUrl } from '../api';
import type { Stream } from '../types';
import { CameraIcon } from './icons';

export interface StreamThumbnailProps {
  stream: Stream;
  /** Click handler — when provided, the tile becomes a button-like element. */
  onClick?: (streamId: string) => void;
  /** Snapshot auth token. If absent, no image is fetched. */
  snapshotToken?: string;
  /** Polling interval in ms (default 2000 for thumbnails). */
  intervalMs?: number;
  className?: string;
}

const STYLES = `
.ds-thumb {
  /* CSS variable aliases — DESIGN_GUIDE §5. */
  --ds-thumb-fg: var(--foreground);
  --ds-thumb-muted: var(--muted-foreground);
  --ds-thumb-card: var(--card);
  --ds-thumb-border: var(--border);

  /* Semantic status colors reused by the dot modifier classes below. */
  --ds-thumb-ok: var(--color-success);
  --ds-thumb-warn: var(--color-warning);
  --ds-thumb-err: var(--color-error);
  --ds-thumb-info: var(--color-info);
  --ds-thumb-idle: var(--muted-foreground);

  position: relative;
  display: flex;
  flex-direction: column;
  background: var(--ds-thumb-card);
  border: 1px solid var(--ds-thumb-border);
  border-radius: var(--radius-md, 8px);
  overflow: hidden;
  cursor: default;
  transition: border-color var(--duration-fast) var(--ease-out),
              box-shadow var(--duration-fast) var(--ease-out);
}

.ds-thumb[role="button"] {
  cursor: pointer;
}

.ds-thumb[role="button"]:hover {
  border-color: var(--primary);
  box-shadow: var(--shadow-sm);
}

.ds-thumb[role="button"]:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 2px;
}

.ds-thumb__image-wrap {
  position: relative;
  width: 100%;
  /* 4:3 aspect ratio for the image area */
  aspect-ratio: 4 / 3;
  background: var(--muted);
  overflow: hidden;
}

.ds-thumb__img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
  transition: opacity var(--duration-normal) var(--ease-out);
}

.ds-thumb__placeholder {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--ds-thumb-muted);
  background: var(--muted);
}

.ds-thumb__placeholder svg {
  width: 28px;
  height: 28px;
  opacity: 0.7;
}

.ds-thumb__status-dot {
  position: absolute;
  top: 6px;
  right: 6px;
  width: 8px;
  height: 8px;
  border-radius: var(--radius-full, 9999px);
  background: var(--ds-thumb-idle);
  border: 1px solid var(--ds-thumb-card);
  z-index: 2;
  pointer-events: none;
}

/* Status → color mapping. Mirrors the StreamStatus union in types.ts. */
.ds-thumb__status-dot--running       { background: var(--ds-thumb-ok); }
.ds-thumb__status-dot--connecting    { background: var(--ds-thumb-info); }
.ds-thumb__status-dot--degraded      { background: var(--ds-thumb-warn); opacity: 0.85; }
.ds-thumb__status-dot--reconnecting  { background: var(--ds-thumb-warn); opacity: 0.85; }
.ds-thumb__status-dot--error         { background: var(--ds-thumb-err); }
.ds-thumb__status-dot--stopped       { background: var(--ds-thumb-idle); }

.ds-thumb__caption {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 6px 8px;
  font-size: 11px;
  line-height: 1.3;
  background: var(--ds-thumb-card);
  color: var(--ds-thumb-fg);
}

.ds-thumb__id {
  font-weight: 500;
  color: var(--ds-thumb-fg);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1 1 auto;
  min-width: 0;
}

.ds-thumb__status {
  font-size: 10px;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--ds-thumb-muted);
  flex-shrink: 0;
}
`;

export function StreamThumbnail({
  stream,
  onClick,
  snapshotToken,
  intervalMs = 2000,
  className,
}: StreamThumbnailProps) {
  const ref = useRef<HTMLDivElement>(null);
  // useSnapshot guards against missing streamId internally; passing the id is
  // safe even before the snapshot token is known — the tick just won't be used.
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
        if (e.isIntersecting) resume();
        else pause();
      },
      { threshold: 0.1 },
    );
    io.observe(el);
    return () => io.disconnect();
  }, [pause, resume]);

  const snapshotUrl =
    snapshotToken && tick > 0
      ? getSnapshotUrl(stream.stream_id, snapshotToken, tick)
      : null;

  const statusClass = `ds-thumb__status-dot--${stream.status}`;

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
        <div className={`ds-thumb__status-dot ${statusClass}`} />
        <div className="ds-thumb__image-wrap">
          {snapshotUrl && intersecting ? (
            <img
              src={snapshotUrl}
              alt={stream.stream_id}
              className="ds-thumb__img"
              loading="lazy"
              onError={(e) => {
                const img = e.currentTarget as HTMLImageElement;
                img.style.opacity = '0.3';
              }}
            />
          ) : (
            <div className="ds-thumb__placeholder">
              <CameraIcon />
            </div>
          )}
        </div>
        <div className="ds-thumb__caption">
          <span className="ds-thumb__id">{stream.stream_id}</span>
          <span className="ds-thumb__status">{stream.status}</span>
        </div>
      </div>
    </>
  );
}

export default StreamThumbnail;
