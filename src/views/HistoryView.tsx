import { useState } from "react";
import { SnapshotsPanel } from "../components/SnapshotsPanel";
import type { AppError } from "../lib/ipc";

/** Snapshots are the only history the app keeps; see SnapshotsPanel. */
export function HistoryView() {
  const [error, setError] = useState<AppError | null>(null);

  return (
    <>
      {error && (
        <p className="panel row error">
          <span style={{ flex: 1 }}>{error.message}</span>
          <button className="btn" onClick={() => setError(null)}>Dismiss</button>
        </p>
      )}
      <SnapshotsPanel onError={setError} />
    </>
  );
}
