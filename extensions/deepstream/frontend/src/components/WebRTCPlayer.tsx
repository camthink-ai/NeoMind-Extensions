// Presentational WebRTC video player. Driven by the useWebRTC hook.
//
// Attaches the negotiated MediaStream to a <video> element and reports the
// hook's status upward so the parent (DetailDrawer) can decide whether to
// show this player, a "Connecting…" chip, or fall back to snapshot polling.

import { forwardRef, useEffect, useRef } from 'react';
import { useWebRTC, type WebRTCStatus } from '../hooks/useWebRTC';

export interface WebRTCPlayerProps {
  webrtcUrl: string | null | undefined;
  className?: string;
  /** Called whenever status changes. Parent uses it to flip to fallback. */
  onStatusChange?: (status: WebRTCStatus) => void;
}

export const WebRTCPlayer = forwardRef<HTMLVideoElement, WebRTCPlayerProps>(
  function WebRTCPlayer(props, ref) {
    const { webrtcUrl, className } = props;
    const { status, stream } = useWebRTC(webrtcUrl);
    const videoRef = useRef<HTMLVideoElement | null>(null);

    // Expose the inner video element to the parent via the forwarded ref.
    useEffect(() => {
      if (typeof ref === 'function') ref(videoRef.current);
      else if (ref) (ref as React.MutableRefObject<HTMLVideoElement | null>).current = videoRef.current;
    });

    useEffect(() => {
      const v = videoRef.current;
      if (!v) return;
      if (stream && status === 'live') {
        v.srcObject = stream;
        v.play().catch(() => { /* autoplay may need user gesture; ignore */ });
      } else {
        v.srcObject = null;
      }
    }, [stream, status]);

    useEffect(() => {
      props.onStatusChange?.(status);
    }, [status, props]);

    return (
      <video
        ref={videoRef}
        className={className}
        autoPlay
        playsInline
        muted
        controls={false}
      />
    );
  },
);

WebRTCPlayer.displayName = 'WebRTCPlayer';
export default { WebRTCPlayer };
