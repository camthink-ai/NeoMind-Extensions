// HLS playback hook using hls.js.
//
// mediamtx serves HLS at http://<host>:8888/<path>/index.m3u8
// HLS works over plain HTTP — no ICE/UDP/mDNS issues like WebRTC.
// Latency with LL-HLS parts (~200ms segments) is typically 2-4s.
//
// Safari uses native HLS support (<video src="...m3u8">).
// Chrome/Firefox use hls.js loaded dynamically.

import { useEffect, useRef, useState } from 'react';

export type HlsStatus = 'loading' | 'live' | 'error';

export interface UseHlsResult {
  videoRef: React.RefObject<HTMLVideoElement>;
  status: HlsStatus;
  error: string | null;
}

export function useHls(hlsUrl: string | null | undefined): UseHlsResult {
  const videoRef = useRef<HTMLVideoElement>(null);
  const [status, setStatus] = useState<HlsStatus>('loading');
  const [error, setError] = useState<string | null>(null);
  const hlsRef = useRef<any>(null);

  useEffect(() => {
    const video = videoRef.current;
    if (!video || !hlsUrl) {
      if (!hlsUrl) {
        setStatus('error');
        setError('no hls url');
      }
      return;
    }

    setStatus('loading');
    setError(null);

    let cancelled = false;

    // Safari has native HLS support.
    if (video.canPlayType('application/vnd.apple.mpegurl')) {
      video.src = hlsUrl;
      video.addEventListener('loadedmetadata', () => {
        if (!cancelled) setStatus('live');
      }, { once: true });
      video.addEventListener('error', () => {
        if (!cancelled) { setStatus('error'); setError('native hls error'); }
      }, { once: true });
      return () => {
        cancelled = true;
        video.removeAttribute('src');
        video.load();
      };
    }

    // Chrome/Firefox: load hls.js from CDN (dynamic import doesn't work in
    // the extension UMD environment — no module resolver at runtime).
    // Use a global promise to prevent multiple simultaneous card instances
    // from injecting multiple <script> tags.
    const HLS_CDN = 'https://cdn.jsdelivr.net/npm/hls.js@1.5.17/dist/hls.min.js';

    const loadHls = (): Promise<any> => {
      if ((window as any).Hls) return Promise.resolve((window as any).Hls);
      if ((window as any).__hlsPromise) return (window as any).__hlsPromise;
      (window as any).__hlsPromise = new Promise((resolve, reject) => {
        const s = document.createElement('script');
        s.src = HLS_CDN;
        s.onload = () => {
          if ((window as any).Hls) resolve((window as any).Hls);
          else reject(new Error('Hls not found after script load'));
        };
        s.onerror = () => reject(new Error('failed to load hls.js script'));
        document.head.appendChild(s);
      });
      return (window as any).__hlsPromise;
    };

    let hls: any;
    loadHls().then((Hls) => {
      if (cancelled || !videoRef.current) return;
      if (!Hls.isSupported()) {
        setStatus('error');
        setError('hls.js not supported');
        return;
      }
      hls = new Hls({
        liveDurationInfinity: true,
        lowLatencyMode: false,
        backBufferLength: 60,
        liveSyncDurationCount: 3,
        maxBufferLength: 30,
        maxMaxBufferLength: 60,
        enableWorker: true,
      });
      hlsRef.current = hls;
      hls.loadSource(hlsUrl);
      hls.attachMedia(video);
      hls.on(Hls.Events.MANIFEST_PARSED, () => {
        if (!cancelled) setStatus('live');
        video.play().catch(() => {});
      });
      hls.on(Hls.Events.ERROR, (_evt: any, data: any) => {
        if (cancelled) return;
        if (data.fatal) {
          switch (data.type) {
            case Hls.ErrorTypes.NETWORK_ERROR:
              // Network error — try recovering by reloading.
              hls.startLoad();
              break;
            case Hls.ErrorTypes.MEDIA_ERROR:
              // Media error — try recovering by seeking to live edge.
              hls.recoverMediaError();
              break;
            default:
              // Unrecoverable — destroy.
              setStatus('error');
              setError(data.details || 'hls fatal error');
              break;
          }
        }
      });
    }).catch(() => {
      if (!cancelled) { setStatus('error'); setError('failed to load hls.js'); }
    });

    return () => {
      cancelled = true;
      if (hls) {
        hls.destroy();
        hlsRef.current = null;
      }
    };
  }, [hlsUrl]);

  return { videoRef, status, error };
}
