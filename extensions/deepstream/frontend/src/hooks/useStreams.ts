import { useState, useEffect, useCallback } from 'react';
import type { Stream } from '../types';
import { dsCommands } from '../api';

export function useStreams(pollMs: number = 3000) {
  const [streams, setStreams] = useState<Stream[]>([]);
  const [serverHost, setServerHost] = useState<string>('');
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const r = await dsCommands.listStreams();
      if (r.success && r.data) {
        setStreams(r.data.streams ?? []);
        setServerHost(r.data.server_host ?? '');
      } else {
        setError(r.error ?? 'list_streams failed');
      }
    } catch (e: any) {
      setError(e.message ?? String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, pollMs);
    return () => clearInterval(id);
  }, [refresh, pollMs]);

  return { streams, serverHost, loading, error, refresh };
}
