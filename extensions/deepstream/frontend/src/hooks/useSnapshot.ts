import { useState, useEffect, useRef, useCallback } from 'react';

export function useSnapshot(streamId: string | null | undefined, intervalMs: number = 1000) {
  const [tick, setTick] = useState(0);
  const pausedRef = useRef(false);

  useEffect(() => {
    if (!streamId) return;
    setTick(0);
    const id = setInterval(() => {
      if (pausedRef.current) return;
      if (document.hidden) return;
      setTick(t => t + 1);
    }, intervalMs);
    return () => clearInterval(id);
  }, [streamId, intervalMs]);

  const pause = useCallback(() => { pausedRef.current = true; }, []);
  const resume = useCallback(() => { pausedRef.current = false; }, []);

  return { tick, pause, resume };
}
