import { useEffect, useState } from "react";

/**
 * Returns the current time in ms, re-rendering the caller on each tick.
 * Pass `enabled: false` to pause the tick when nothing on screen needs it.
 */
export const useNow = (intervalMs = 1000, enabled = true): number => {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!enabled) return undefined;
    const id = window.setInterval(() => setNow(Date.now()), intervalMs);
    return (): void => window.clearInterval(id);
  }, [intervalMs, enabled]);
  return now;
};
