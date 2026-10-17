import { useEffect, useState } from "react";
import { getLiveSnapshot, onTick, type AppError, type Tick } from "./ipc";
import type { Result } from "./result";

/** The latest `tracker-tick`: the snapshot at mount, then every event. */
export function useTick(): { tick: Tick | null; error: AppError | null } {
  const [tick, setTick] = useState<Tick | null>(null);
  const [error, setError] = useState<AppError | null>(null);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    // Each good update clears an earlier error, so a one-off failure doesn't stick.
    const take = (res: Result<Tick, AppError>) => {
      if (res.success) {
        setTick(res.data);
        setError(null);
      } else {
        setError(res.error);
      }
    };
    void getLiveSnapshot().then((res) => {
      if (!disposed) take(res);
    });
    void onTick(take).then((res) => {
      if (!res.success) return setError(res.error);
      if (disposed) res.data();
      else unlisten = res.data;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  return { tick, error };
}
