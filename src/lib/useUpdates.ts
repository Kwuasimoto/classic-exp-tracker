import { useEffect, useState } from "react";
import { onUpdateStatus, updateStatus, type AppError, type UpdateStatus } from "./ipc";
import type { Result } from "./result";

/** The latest update status: asked for at mount, then every `update-status` event. */
export function useUpdates(): { updates: UpdateStatus | null; error: AppError | null; take: (res: Result<UpdateStatus, AppError>) => void } {
  const [updates, setUpdates] = useState<UpdateStatus | null>(null);
  const [error, setError] = useState<AppError | null>(null);

  const take = (res: Result<UpdateStatus, AppError>) => {
    if (res.success) {
      setUpdates(res.data);
      setError(null);
    } else {
      setError(res.error);
    }
  };

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void updateStatus().then((res) => {
      if (!disposed) take(res);
    });
    void onUpdateStatus(take).then((res) => {
      if (!res.success) return setError(res.error);
      if (disposed) res.data();
      else unlisten = res.data;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  return { updates, error, take };
}
