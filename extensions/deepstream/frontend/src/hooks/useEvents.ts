import { useState, useEffect, useRef } from 'react';
import type { SidecarEvent } from '../types';
import { getApiOrigin, getAuthToken } from '../api';

// Singleton WS — shared across all useEvents() callers.
//
// Fixes vs the original implementation:
//  1. Path was `/ws/events` (a guess); the real NeoMind endpoint is `/api/events/ws`.
//  2. No Auth handshake — NeoMind's WS closes the socket unless the client
//     sends `{"type":"Auth","token":"<JWT>"}` after connect and waits for
//     `{"type":"Authenticated"}`.
//  3. No pong — NeoMind sends `{"type":"ping"}` every HEARTBEAT_INTERVAL_SECS
//     and disconnects on timeout unless the client replies `{"type":"pong"}`.
//  4. Wrong event-type match — extension publishes as `deepstream.stats`, not `stats`.
//  5. Wrong envelope — server wraps each event as
//     `{id,type,timestamp,source,data:<SidecarEvent-payload>}` and may send
//     `{batch:true,events:[...]}`. We unwrap `.data` so downstream listeners
//     see the bare SidecarEvent shape they were written against.

let sharedWs: WebSocket | null = null;
let refCount = 0;
let sharedListeners: Set<(ev: SidecarEvent) => void> = new Set();
let sharedStatus: 'connecting' | 'open' | 'closed' = 'closed';

function getWsUrl(): string {
  const origin = getApiOrigin();
  // Auth is done via Auth message after connect (NOT via ?token= query param —
  // that path is reserved for the chat WS). See neomind-api handlers/events.rs
  // `event_websocket_handler`.
  return `${origin.replace(/^http/, 'ws')}/api/events/ws`;
}

function rawSend(ws: WebSocket, obj: Record<string, unknown>) {
  try { ws.send(JSON.stringify(obj)); } catch { /* socket not ready */ }
}

/**
 * Extract the bare SidecarEvent payload from a NeoMind EventBus envelope.
 *
 * Two envelope shapes arrive on the WS:
 *
 * 1. Direct (e.g. ExtensionOutput): `{id, type, timestamp, source,
 *    data: {type:"stats", ts, ...}}` — the SidecarEvent sits in `data`
 *    with its own `type` field.
 *
 * 2. Custom (extension-published via `event_publish` capability): the host
 *    wraps deepstream events as `NeoMindEvent::Custom { event_type, data }`.
 *    The WS serialises Custom as
 *    `{id, type:"Custom", timestamp, source,
 *      data: {custom_type:"deepstream.stats", data: {type:"stats", ts, ...}}}`
 *    because `extract_event_data` flattens Custom events (see
 *    handlers/events.rs:298). We unwrap the inner `.data.data` so listeners
 *    see the bare SidecarEvent shape.
 *
 * Returns null for control frames (Authenticated/ping/pong/Error) — those
 * are filtered by `handleText` before this is called — and for unknown
 * envelope shapes.
 */
function unwrapEvent(frame: any): SidecarEvent | null {
  if (!frame || typeof frame !== 'object') return null;
  const data = frame.data;
  if (!data || typeof data !== 'object') return null;

  // Shape 1: direct SidecarEvent in `data`.
  if (typeof data.type === 'string') {
    return data as SidecarEvent;
  }

  // Shape 2: Custom wrap. Only unwrap deepstream events — leave other Custom
  // events for their own consumers.
  if (frame.type === 'Custom' && typeof data.custom_type === 'string') {
    if (data.custom_type.startsWith('deepstream.')) {
      const inner = data.data;
      if (inner && typeof inner === 'object' && typeof inner.type === 'string') {
        return inner as SidecarEvent;
      }
    }
  }

  return null;
}

function handleText(text: string) {
  let parsed: any;
  try { parsed = JSON.parse(text); } catch { return; }

  // Control frames.
  const t = parsed?.type;
  if (t === 'Authenticated') { sharedStatus = 'open'; return; }
  if (t === 'ping') {
    if (sharedWs && sharedWs.readyState === WebSocket.OPEN) rawSend(sharedWs, { type: 'pong' });
    return;
  }
  if (t === 'pong') { return; }          // server echoes pong too
  if (t === 'Error') { /* logged server-side */ return; }

  // Batch envelope: { batch:true, events:[<envelope>, ...] }
  if (parsed?.batch === true && Array.isArray(parsed.events)) {
    for (const env of parsed.events) {
      const ev = unwrapEvent(env);
      if (ev) sharedListeners.forEach((fn) => fn(ev));
    }
    return;
  }

  // Single-event envelope.
  const ev = unwrapEvent(parsed);
  if (ev) sharedListeners.forEach((fn) => fn(ev));
}

function ensureWs() {
  if (sharedWs) return;
  sharedStatus = 'connecting';
  try {
    const ws = new WebSocket(getWsUrl());
    sharedWs = ws;
    ws.onopen = () => {
      // Send Auth message immediately. Server won't push any events until it
      // has validated the token and replied Authenticated.
      const token = getAuthToken();
      if (token) {
        rawSend(ws, { type: 'Auth', token });
      } else {
        // No token in storage — still send an Auth with api_key fallback slot
        // empty; server will close with "Authentication required". We let
        // onclose schedule a reconnect so a login in another tab picks up.
        rawSend(ws, { type: 'Auth', token: '' });
      }
    };
    ws.onclose = () => {
      sharedStatus = 'closed';
      sharedWs = null;
      // Auto-reconnect with backoff if any listeners remain.
      if (sharedListeners.size > 0) {
        setTimeout(ensureWs, 2000);
      }
    };
    ws.onerror = () => { /* let onclose handle reconnect */ };
    ws.onmessage = (msg) => {
      // NeoMind WS only sends Text frames.
      if (typeof msg.data === 'string') handleText(msg.data);
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
      // Filter by stream_id if provided (Detection / LineCross / Stats all carry it).
      if (streamId) {
        const sid = (ev as any).stream_id;
        if (sid && sid !== streamId) return;
      }
      // Cap queue to prevent unbounded growth.
      const next = [...eventsRef.current, ev];
      if (next.length > 200) next.splice(0, next.length - 200);
      setEvents(next);
    };

    sharedListeners.add(listener);
    refCount++;
    ensureWs();

    // Status poll — cheap, runs 1/sec.
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
