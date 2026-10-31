import { useState, type ReactNode } from "react";
import { SnapshotsPanel } from "../components/SnapshotsPanel";
import { useMessages } from "../lib/i18n";
import type { AppError } from "../lib/ipc";

/** Snapshots are the only history the app keeps; see SnapshotsPanel. */
export function HistoryView({ status }: { status: ReactNode }) {
  const m = useMessages();
  const [error, setError] = useState<AppError | null>(null);

  return (
    <>
      <section className="panel page-head">
        <div className="page-head-main">
          <h1 className="page-title">{m.history.title}</h1>
        </div>
        {status}
      </section>
      {error && (
        <p className="panel row error">
          <span style={{ flex: 1 }}>{error.message}</span>
          <button className="btn" onClick={() => setError(null)}>{m.common.dismiss}</button>
        </p>
      )}
      <SnapshotsPanel onError={setError} />
    </>
  );
}
