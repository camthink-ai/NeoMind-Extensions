// DeepStream StreamThumbnail — MJPEG live preview tile.
//
// Uses multipart/x-mixed-replace MJPEG stream from the Jetson's mjpeg_server.
// Each tile is a single long-lived HTTP connection (no browser concurrency
// limit issues like HLS). Browsers render MJPEG natively in <img>.
// Detection boxes are baked into the video by DeepStream's nvdsosd.

import { useEffect, useRef, useState } from 'react';
import type { Stream } from '../types';
import { CameraIcon } from './icons';

export interface StreamThumbnailProps {
  stream: Stream;
  onClick?: (streamId: string) => void;
  className?: string;
  /** MJPEG stream URL (e.g. http://host:8090/mjpeg/<streamId>). */
  mjpegUrl?: string | null;
}

const STATUS_COLORS: Record<string, string> = {
  running: 'var(--color-success, #22c55e)',
  connecting: 'var(--color-info, #3b82f6)',
  degraded: 'var(--color-warning, #f59e0b)',
  reconnecting: 'var(--color-warning, #f59e0b)',
  error: 'var(--color-error, #ef4444)',
  stopped: 'var(--muted-foreground, #888)',
};

export function StreamThumbnail({
  stream,
  onClick,
  className,
  mjpegUrl,
}: StreamThumbnailProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [intersecting, setIntersecting] = useState(true);
  const [imgError, setImgError] = useState(false);
  const [retryKey, setRetryKey] = useState(0);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const io = new IntersectionObserver(
      (entries) => {
        const e = entries[0];
        if (e) setIntersecting(e.isIntersecting);
      },
      { threshold: 0.05 },
    );
    io.observe(el);
    return () => io.disconnect();
  }, []);

  // Reset error state when URL changes.
  useEffect(() => {
    setImgError(false);
  }, [mjpegUrl]);

  // Auto-retry on error: after 3s, clear error and force a new <img> element.
  // This handles the case where the stream isn't ready yet when the tile
  // first renders (pipeline still starting up).
  useEffect(() => {
    if (!imgError) return;
    const timer = setTimeout(() => {
      setImgError(false);
      setRetryKey((k) => k + 1);
    }, 3000);
    return () => clearTimeout(timer);
  }, [imgError, retryKey]);

  const statusColor = STATUS_COLORS[stream.status] ?? STATUS_COLORS.stopped;
  const showVideo = intersecting && mjpegUrl && !imgError;

  return (
    <div
      ref={containerRef}
      className={`ds-thumb ${className ?? ''}`}
      onClick={onClick ? () => onClick(stream.stream_id) : undefined}
      role={onClick ? 'button' : undefined}
      tabIndex={onClick ? 0 : undefined}
      aria-label={onClick ? `Open stream ${stream.stream_id}` : undefined}
    >
      <div className="ds-thumb__video-wrap">
        {showVideo ? (
          <img
            key={retryKey}
            src={mjpegUrl!}
            className="ds-thumb__img"
            alt={stream.stream_id}
            onError={() => setImgError(true)}
          />
        ) : (
          <div className="ds-thumb__placeholder"><CameraIcon /></div>
        )}
      </div>
      <div className="ds-thumb__badge">
        <span className="ds-thumb__badge-dot" style={{ background: statusColor }} />
        {stream.status === 'running' ? '● Live' : stream.status}
      </div>
      <div className="ds-thumb__overlay">
        <div className="ds-thumb__id">{stream.stream_id}</div>
        <div className="ds-thumb__sub">{stream.model}</div>
      </div>

      <style>{`
        .ds-thumb {
          position: relative;
          border-radius: var(--radius-md, 8px);
          overflow: hidden;
          background: #000;
          cursor: ${onClick ? 'pointer' : 'default'};
        }
        .ds-thumb__video-wrap {
          position: relative;
          width: 100%;
          aspect-ratio: 16 / 9;
          overflow: hidden;
          background: #000;
        }
        .ds-thumb__img {
          display: block;
          width: 100%;
          height: 100%;
          object-fit: cover;
        }
        .ds-thumb__placeholder {
          position: absolute; inset: 0;
          display: flex; align-items: center; justify-content: center;
          background: color-mix(in srgb, var(--muted-foreground, #888) 12%, #000);
        }
        .ds-thumb__placeholder svg { width: 28px; height: 28px; opacity: 0.4; color: var(--muted-foreground, #888); }
        .ds-thumb__badge {
          position: absolute; top: 6px; right: 6px;
          display: flex; align-items: center; gap: 4px;
          padding: 2px 7px;
          border-radius: 9999px;
          background: rgba(0,0,0,0.6);
          backdrop-filter: blur(6px);
          font-size: 9px; font-weight: 600;
          text-transform: uppercase; letter-spacing: 0.03em;
          color: #fff; z-index: 2;
        }
        .ds-thumb__badge-dot { width: 5px; height: 5px; border-radius: 50%; }
        .ds-thumb__overlay {
          position: absolute; bottom: 0; left: 0; right: 0;
          padding: 20px 10px 7px;
          background: linear-gradient(to top, rgba(0,0,0,0.8), transparent);
          z-index: 1; pointer-events: none;
        }
        .ds-thumb__id { font-size: 12px; font-weight: 600; color: #fff; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
        .ds-thumb__sub { font-size: 9px; color: rgba(255,255,255,0.65); margin-top: 1px; }
      `}</style>
    </div>
  );
}

export default StreamThumbnail;
