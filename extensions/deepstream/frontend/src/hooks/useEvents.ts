import { useState, useEffect, useRef } from 'react';
import type { SidecarEvent } from '../types';

// Singleton WS — shared across all useEvents() callers
let sharedWs: WebSocket | null = null;
let refCount = 0;
let sharedListeners: Set<(ev: SidecarEvent) => void> = new Set();
let sharedStatus: 'connecting' | 'open' | 'closed' = 'closed';

function getWsUrl(): string {
  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  // NOTE: exact path may need adjustment once NeoMind EventBus WS endpoint is confirmed
  // Open Question §12 Q5 in the design spec
  return `${proto}//${window.location.host}/ws/events`;
}

function ensureWs() {
  if (sharedWs) return;
  sharedStatus = 'connecting';
  try {
    const ws = new WebSocket(getWsUrl());
    sharedWs = ws;
    ws.onopen = () => { sharedStatus = 'open'; };
    ws.onclose = () => {
      sharedStatus = 'closed';
      sharedWs = null;
      // Auto-reconnect with backoff if any listeners remain
      if (sharedListeners.size > 0) {
        setTimeout(ensureWs, 2000);
      }
    };
    ws.onerror = () => { /* let onclose handle reconnect */ };
    ws.onmessage = (msg) => {
      try {
        const ev = JSON.parse(msg.data) as SidecarEvent;
        sharedListeners.forEach(fn => fn(ev));
      } catch {
        // ignore non-JSON / malformed frames
      }
    };
  } catch {
    sharedWs = null;
    sharedStatus = 'closed';
  }
}

function releaseWs() {
  if (sharedWs && sharedListeners.size === 0) {
    try { sharedWs.close(); } catch {}
    sharedWs = null;
  }
}

export function useEvents(streamId?: string) {
  const [events, setEvents] = useState<SidecarEvent[]>([]);
  const [status, setStatus] = useState<'connecting' | 'open' | 'closed'>(sharedStatus);
  const eventsRef = useRef<SidecarEvent[]>([]);
  eventsRef.current = events;

  useEffect(() => {
    const listener = (ev: SidecarEvent) => {
      // Filter by stream_id if provided
      if (streamId) {
        const sid = (ev as any).stream_id;
        if (sid && sid !== streamId) return;
      }
      // Cap queue to prevent unbounded growth
      const next = [...eventsRef.current, ev];
      if (next.length > 200) next.splice(0, next.length - 200);
      setEvents(next);
    };

    sharedListeners.add(listener);
    refCount++;
    ensureWs();

    // Status poll — cheap, runs 1/sec
    const statusId = setInterval(() => setStatus(sharedStatus), 1000);

    return () => {
      sharedListeners.delete(listener);
      refCount--;
      if (refCount <= 0) releaseWs();
      clearInterval(statusId);
    };
  }, [streamId]);

  const clear = () => setEvents([]);
  return { events, status, clear };
}
