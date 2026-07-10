// useDelayedDetection — buffer detection events and surface the one from
// HLS_LATENCY_MS ago, so bounding boxes approximately match the delayed
// video frame currently on screen.
//
// HLS typically adds 2-4s of latency. We default to 3000ms. The hook keeps
// a rolling buffer of {ts, objects} pairs and re-evaluates on a timer.

import { useEffect, useRef, useState } from 'react';
import type { SidecarEvent } from '../types';
import type { DetectedObject } from '../components/DetectionOverlay';
import { useEvents } from './useEvents';

const HLS_LATENCY_MS = 3000;
const TICK_MS = 200;
const MAX_BUFFER = 300; // ~30s at 10 events/s

interface BufferedDetection {
  ts: number;
  objects: DetectedObject[];
}

export function useDelayedDetection(streamId: string): DetectedObject[] {
  const { events } = useEvents(streamId);
  const bufferRef = useRef<BufferedDetection[]>([]);
  const [delayed, setDelayed] = useState<DetectedObject[]>([]);

  // Append new detections to the ring buffer.
  useEffect(() => {
    if (events.length === 0) return;
    const buf = bufferRef.current;
    const lastTs = buf.length > 0 ? buf[buf.length - 1].ts : 0;
    for (const ev of events) {
      if (ev.type !== 'detection') continue;
      const ts = (ev as any).ts as number;
      if (ts <= lastTs) continue;
      buf.push({ ts, objects: (ev as any).objects as DetectedObject[] });
    }
    if (buf.length > MAX_BUFFER) buf.splice(0, buf.length - MAX_BUFFER);
  }, [events]);

  // Tick: find the detection closest to (now - HLS_LATENCY_MS).
  useEffect(() => {
    const id = setInterval(() => {
      const buf = bufferRef.current;
      if (buf.length === 0) return;
      const target = Date.now() - HLS_LATENCY_MS;

      // Binary search for closest ts.
      let lo = 0, hi = buf.length - 1;
      while (lo < hi) {
        const mid = (lo + hi) >> 1;
        if (buf[mid].ts < target) lo = mid + 1;
        else hi = mid;
      }
      // Pick the entry just before lo if it's closer.
      let best = lo;
      if (lo > 0 && Math.abs(buf[lo - 1].ts - target) < Math.abs(buf[lo].ts - target)) {
        best = lo - 1;
      }

      setDelayed(buf[best].objects);

      // Trim old entries (older than 10s before target — no longer useful).
      const cutoff = target - 10000;
      while (bufferRef.current.length > 1 && bufferRef.current[0].ts < cutoff) {
        bufferRef.current.shift();
      }
    }, TICK_MS);
    return () => clearInterval(id);
  }, []);

  return delayed;
}
