import { useCallback, useEffect, useState } from "react";
import { RateChart } from "./RateChart";
import { formatClock, formatDateTime, formatExp, formatHours } from "../lib/format";
import { deleteSnapshot, getSnapshotChart, getSnapshots, renameSnapshot, type AppError, type Bar, type Snapshot } from "../lib/ipc";

/** Snapshots saved by the Live view's reset: rename, delete, and a chart of the selected one. */
export function SnapshotsPanel({ onError }: { onError: (e: AppError) => void }) {
  const [snapshots, setSnapshots] = useState<Snapshot[]>([]);
  const [selected, setSelected] = useState<number | null>(null);
  const [bars, setBars] = useState<Bar[]>([]);
  const [editing, setEditing] = useState<{ id: number; name: string } | null>(null);
  const [deleting, setDeleting] = useState<number | null>(null);

  const load = useCallback(async () => {
    const res = await getSnapshots();
    if (res.success) setSnapshots(res.data);
    else onError(res.error);
  }, [onError]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (selected === null) return setBars([]);
    // Drop a response superseded by a newer selection.
    let stale = false;
    void getSnapshotChart(selected).then((res) => {
      if (stale) return;
      if (res.success) setBars(res.data);
      else onError(res.error);
    });
    return () => {
      stale = true;
    };
  }, [selected, onError]);

  const saveRename = async () => {
    if (!editing) return;
    const res = await renameSnapshot(editing.id, editing.name);
    if (!res.success) return onError(res.error);
    setEditing(null);
    await load();
  };

  const confirmDelete = async (id: number) => {
    const res = await deleteSnapshot(id);
    setDeleting(null);
    if (!res.success) return onError(res.error);
    if (selected === id) setSelected(null);
    await load();
  };

  const stop = (e: { stopPropagation: () => void }) => e.stopPropagation();

  return (
    <>
      <section className="panel">
        <h2>Snapshots</h2>
        {snapshots.length === 0 ? (
          <p className="caption">No snapshots yet. The reset button on the Live chart saves the last 2 hours here.</p>
        ) : (
          <table>
            <thead>
              <tr><th>Name</th><th>Map</th><th>Level</th><th>Period</th><th>Training</th><th>EXP</th><th>EXP/hr</th><th /></tr>
            </thead>
            <tbody>
              {snapshots.map((s) => (
                <tr key={s.id} className={`selectable ${s.id === selected ? "selected" : ""}`} onClick={() => setSelected(s.id === selected ? null : s.id)}>
                  <td>
                    {editing?.id === s.id ? (
                      <input
                        value={editing.name}
                        autoFocus
                        aria-label="Snapshot name"
                        onClick={stop}
                        onChange={(e) => setEditing({ id: s.id, name: e.target.value })}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") void saveRename();
                          if (e.key === "Escape") setEditing(null);
                        }}
                      />
                    ) : (
                      s.name
                    )}
                  </td>
                  <td>{s.mapName ?? "—"}</td>
                  <td>{s.level ?? "—"}</td>
                  <td>{s.from === null || s.to === null ? "—" : `${formatDateTime(s.from)} – ${formatClock(s.to)}`}</td>
                  <td>{formatHours(s.trainingSecs / 3600)}</td>
                  <td>{formatExp(s.expGained)}</td>
                  <td>{s.trainingSecs > 0 ? formatExp((s.expGained * 3600) / s.trainingSecs) : "—"}</td>
                  <td onClick={stop}>
                    <div className="row" style={{ flexWrap: "nowrap", justifyContent: "flex-end" }}>
                      {editing?.id === s.id ? (
                        <>
                          <button className="btn primary" onClick={() => void saveRename()}>Save</button>
                          <button className="btn" onClick={() => setEditing(null)}>Cancel</button>
                        </>
                      ) : deleting === s.id ? (
                        <>
                          <button className="btn danger" onClick={() => void confirmDelete(s.id)}>Delete</button>
                          <button className="btn" onClick={() => setDeleting(null)}>Keep</button>
                        </>
                      ) : (
                        <>
                          <button className="btn" onClick={() => setEditing({ id: s.id, name: s.name })}>Rename</button>
                          <button className="btn" onClick={() => setDeleting(s.id)}>Delete</button>
                        </>
                      )}
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>
      {selected !== null && <RateChart bars={bars} />}
    </>
  );
}
