// WHEP (WebRTC-HTTP Egress Protocol) playback hook.
//
// mediamtx v1.9.0 exposes the inference-overlaid stream over WebRTC at
//   http://<host>:<webrtcPort>/ds/<streamId>/whep
// The browser POSTs an SDP offer (Content-Type: application/sdp), receives an
// SDP answer, sets it as the remote description, and plays the incoming track
// on a <video> element. Latency ~200-500ms vs the ~8s snapshot pipeline.
//
// Lifecycle:
//   status: 'connecting' -> 'live' | 'error'   (monotone except on reconnect())
// On unmount or webrtcUrl change the RTCPeerConnection is closed and tracks
// stopped. ICE timeout (4s) forces -> 'error'. Call reconnect() to retry.

import { useCallback, useEffect, useRef, useState } from 'react';

export type WebRTCStatus = 'connecting' | 'live' | 'error';

export interface UseWebRTCResult {
  status: WebRTCStatus;
  /** Attach this to a <video ref>. Set when status === 'live'. */
  stream: MediaStream | null;
  /** Force a fresh negotiation (e.g. after the user clicks Retry). */
  reconnect: () => void;
  /** Last error message, when status === 'error'. */
  error: string | null;
}

const ICE_TIMEOUT_MS = 4000;

export function useWebRTC(webrtcUrl: string | null | undefined): UseWebRTCResult {
  const [status, setStatus] = useState<WebRTCStatus>('connecting');
  const [stream, setStream] = useState<MediaStream | null>(null);
  const [error, setError] = useState<string | null>(null);

  const pcRef = useRef<RTCPeerConnection | null>(null);
  const iceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const closedRef = useRef(false);
  // Bumped to force a re-run of the negotiation effect.
  const [attempt, setAttempt] = useState(0);

  const teardown = useCallback(() => {
    if (iceTimerRef.current) {
      clearTimeout(iceTimerRef.current);
      iceTimerRef.current = null;
    }
    const pc = pcRef.current;
    if (pc) {
      try {
        pc.getTransceivers().forEach((t) => {
          try { if (t.stop) t.stop(); } catch { /* ignore */ }
        });
        pc.getSenders().forEach((s) => {
          try { if (s.track) s.track.stop(); } catch { /* ignore */ }
        });
      } catch { /* ignore */ }
      try { pc.close(); } catch { /* ignore */ }
      pcRef.current = null;
    }
  }, []);

  useEffect(() => {
    closedRef.current = false;
    if (!webrtcUrl) {
      setStatus('error');
      setError('no webrtc url');
      return;
    }
    setStatus('connecting');
    setError(null);
    setStream(null);

    let iceConnected = false;
    const pc = new RTCPeerConnection();
    pcRef.current = pc;

    // Receive video only.
    pc.addTransceiver('video', { direction: 'recvonly' });
    pc.addTransceiver('audio', { direction: 'recvonly' });

    const incoming = new MediaStream();
    pc.ontrack = (ev) => {
      try { incoming.addTrack(ev.track); } catch { /* ignore */ }
      if (!closedRef.current) setStream(incoming);
    };

    pc.oniceconnectionstatechange = () => {
      const st = pc.iceConnectionState;
      if ((st === 'connected' || st === 'completed') && !iceConnected) {
        iceConnected = true;
        if (iceTimerRef.current) { clearTimeout(iceTimerRef.current); iceTimerRef.current = null; }
        if (!closedRef.current) setStatus('live');
      }
      if (st === 'failed' || st === 'disconnected' || st === 'closed') {
        if (!iceConnected && !closedRef.current) {
          setStatus('error');
          setError(`ice ${st}`);
        }
      }
    };

    iceTimerRef.current = setTimeout(() => {
      if (!iceConnected && !closedRef.current) {
        setStatus('error');
        setError('ice timeout');
        teardown();
      }
    }, ICE_TIMEOUT_MS);

    let cancelled = false;

    (async () => {
      try {
        const offer = await pc.createOffer({ offerToReceiveVideo: true, offerToReceiveAudio: true });
        await pc.setLocalDescription(offer);
        // Wait for ICE gathering to complete (trickle off — full SDP in one round).
        await waitForIceGathering(pc, 2000);

        const resp = await fetch(webrtcUrl, {
          method: 'POST',
          headers: { 'Content-Type': 'application/sdp' },
          body: pc.localDescription!.sdp,
        });
        if (cancelled) return;
        if (!resp.ok) {
          setStatus('error');
          setError(`whep HTTP ${resp.status}`);
          teardown();
          return;
        }
        const answer = await resp.text();
        if (cancelled) return;
        await pc.setRemoteDescription({ type: 'answer', sdp: answer });
      } catch (e: any) {
        if (cancelled) return;
        setStatus('error');
        setError(e?.message ?? String(e));
        teardown();
      }
    })();

    return () => {
      cancelled = true;
      closedRef.current = true;
      teardown();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [webrtcUrl, attempt]);

  const reconnect = useCallback(() => {
    teardown();
    setAttempt((n) => n + 1);
  }, [teardown]);

  return { status, stream, reconnect, error };
}

/** Resolve once ICE gathering completes, or after `timeoutMs` (partial SDP). */
function waitForIceGathering(pc: RTCPeerConnection, timeoutMs: number): Promise<void> {
  if (pc.iceGatheringState === 'complete') return Promise.resolve();
  return new Promise((resolve) => {
    const t = setTimeout(() => {
      pc.removeEventListener('icegatheringstatechange', onChange);
      resolve();
    }, timeoutMs);
    const onChange = () => {
      if (pc.iceGatheringState === 'complete') {
        clearTimeout(t);
        pc.removeEventListener('icegatheringstatechange', onChange);
        resolve();
      }
    };
    pc.addEventListener('icegatheringstatechange', onChange);
  });
}
