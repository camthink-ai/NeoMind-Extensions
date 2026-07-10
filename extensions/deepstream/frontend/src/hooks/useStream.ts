import { useState, useEffect, useCallback } from 'react';
import type { Stream } from '../types';
import { dsCommands } from '../api';

export function useStream(streamId: string | null | undefined, pollMs: number = 3000) {
  const [stream, setStream] = useState<Stream | null>(null);
  const [serverHost, setServerHost] = useState<string>('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!streamId) return;
    setLoading(true);
    try {
      const r = await dsCommands.getStreamInfo(streamId);
      // get_stream_info returns the Stream projection directly at the top level
      // (see lib.rs cmd_get_stream_info) — there is no `{ stream: ... }`
      // wrapper. r.data IS the stream, and it carries `server_host` too.
      if (r.success && r.data) {
        setStream(r.data);
        setServerHost((r.data as any).server_host ?? '');
        setError(null);
      } else setError(r.error ?? 'get_stream_info failed');
    } catch (e: any) {
      setError(e.message ?? String(e));
    } finally {
      setLoading(false);
    }
  }, [streamId]);

  useEffect(() => {
    if (!streamId) return;
    refresh();
    let id: number | undefined;
    const start = () => {
      if (document.visibilityState === 'visible') {
        id = window.setInterval(refresh, pollMs);
      }
    };
    const stop = () => { if (id) { clearInterval(id); id = undefined; } };
    start();
    const onVis = () => { document.visibilityState === 'visible' ? start() : stop(); };
    document.addEventListener('visibilitychange', onVis);
    return () => {
      document.removeEventListener('visibilitychange', onVis);
      stop();
    };
  }, [refresh, pollMs, streamId]);

  return { stream, serverHost, loading, error, refresh };
}
