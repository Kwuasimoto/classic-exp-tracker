import { useEffect, useState } from "react";
import { getLiveSnapshot, onTick, type AppError, type Tick } from "./ipc";

/** The latest `tracker-tick`: the snapshot at mount, then every event. */
export function useTick(): { tick: Tick | null; error: AppError | null } {
  const [tick, setTick] = useState<Tick | null>(null);
  const [error, setError] = useState<AppError | null>(null);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void getLiveSnapshot().then((res) => {
      if (disposed) return;
      if (res.success) setTick(res.data);
      else setError(res.error);
    });
    void onTick((res) => (res.success ? setTick(res.data) : setError(res.error))).then((res) => {
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
