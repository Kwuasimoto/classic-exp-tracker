import { useCallback, useEffect, useState } from "react";
import { usePages } from "./Pager";
import { expSeries, mesoSeries, RateChart } from "./RateChart";
import { formatDay, formatExp, formatHours, formatPeriod } from "../lib/format";
import { useMessages } from "../lib/i18n";
import { profitOf } from "../lib/live";
import { deleteSnapshot, getSnapshotChart, getSnapshots, keepSnapshot, renameSnapshot, type AppError, type Bar, type Snapshot } from "../lib/ipc";

/** Meso tracking was on for some of it. */
const hasLoot = (s: Snapshot) => s.loot.meso + s.loot.etcValue + s.loot.potionCost > 0;

/** How long an automatic snapshot is kept (grinds::KEEP_AUTO_SECS). */
const KEEP_AUTO_SECS = 30 * 86_400;

/** Snapshots, saved automatically at the end of each grind or from the Live chart: keep,
 *  rename, delete, and a chart of the selected one. */
export function SnapshotsPanel({ onError }: { onError: (e: AppError) => void }) {
  const m = useMessages();
  const h = m.history;
  const [snapshots, setSnapshots] = useState<Snapshot[]>([]);
  const [selected, setSelected] = useState<number | null>(null);
  const [bars, setBars] = useState<Bar[]>([]);
  const [editing, setEditing] = useState<{ id: number; name: string } | null>(null);
  const [deleting, setDeleting] = useState<number | null>(null);
  const [chart, setChart] = useState<"exp" | "meso">("exp");
  const pages = usePages(snapshots);

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

  const keep = async (id: number) => {
    const res = await keepSnapshot(id);
    if (!res.success) return onError(res.error);
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
  const selectedSnapshot = snapshots.find((s) => s.id === selected);
  const selectedHasLoot = selectedSnapshot !== undefined && hasLoot(selectedSnapshot);

  return (
    <>
      <section className="panel">
        <h2>{h.snapshots}</h2>
        {snapshots.length === 0 ? (
          <p className="caption">{h.empty}</p>
        ) : (
          <table className="snapshots">
            <thead>
              <tr>
                <th>{h.name}</th>
                <th className="opt" style={{ width: 40 }}>{h.level}</th>
                <th className="period" style={{ width: 236 }}>{h.period}</th>
                <th className="opt" style={{ width: 92 }}>{h.training}</th>
                <th className="num" style={{ width: 66 }}>{h.expPerHour}</th>
                <th className="num meso" style={{ width: 76 }}>{h.profitPerHour}</th>
                <th style={{ width: 104 }}><span className="sr-only">{h.actions}</span></th>
              </tr>
            </thead>
            <tbody>
              {pages.rows.map((s) => (
                <tr key={s.id} className={`selectable ${s.id === selected ? "selected" : ""}`} onClick={() => setSelected(s.id === selected ? null : s.id)}>
                  <td>
                    {editing?.id === s.id ? (
                      <input
                        value={editing.name}
                        autoFocus
                        aria-label={h.snapshotName}
                        onClick={stop}
                        onChange={(e) => setEditing({ id: s.id, name: e.target.value })}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") void saveRename();
                          if (e.key === "Escape") setEditing(null);
                        }}
                      />
                    ) : (
                      <button className="row-open" aria-pressed={s.id === selected} title={s.id === selected ? h.hideChart : h.showChart}>
                        <span className="cut" title={s.name}>{s.name}</span>
                        {s.auto ? (
                          <span className="cut caption">{h.autoUntil(formatDay(s.createdAt + KEEP_AUTO_SECS))}</span>
                        ) : (
                          <span className="cut caption" title={s.mapName ?? undefined}>{s.mapName ?? h.noMap}</span>
                        )}
                      </button>
                    )}
                  </td>
                  <td className="opt">{s.level ?? "—"}</td>
                  <td className="period">{s.from === null || s.to === null ? "—" : <span className="cut" title={formatPeriod(s.from, s.to)}>{formatPeriod(s.from, s.to)}</span>}</td>
                  <td className="opt"><span className="cut">{formatHours(s.trainingSecs / 3600)}</span></td>
                  <td className="num">{s.trainingSecs > 0 ? formatExp((s.expGained * 3600) / s.trainingSecs) : "—"}</td>
                  <td className="num">{s.trainingSecs > 0 && hasLoot(s) ? formatExp((profitOf(s.loot) * 3600) / s.trainingSecs) : "—"}</td>
                  <td onClick={stop}>
                    <div className="row-actions">
                      {editing?.id === s.id ? (
                        <>
                          <button className="btn small primary" onClick={() => void saveRename()}>{m.common.save}</button>
                          <button className="btn small" onClick={() => setEditing(null)}>{m.common.cancel}</button>
                        </>
                      ) : deleting === s.id ? (
                        <>
                          <button className="btn small danger" onClick={() => void confirmDelete(s.id)}>{m.common.delete}</button>
                          <button className="btn small" onClick={() => setDeleting(null)}>{m.common.keep}</button>
                        </>
                      ) : (
                        <>
                          {s.auto && (
                            <button className="icon-btn" aria-label={h.keepNamed(s.name)} title={h.keepTitle(formatDay(s.createdAt + KEEP_AUTO_SECS))} onClick={() => void keep(s.id)}>
                            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                              <path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z" />
                            </svg>
                            </button>
                          )}
                          <button className="icon-btn" aria-label={h.renameNamed(s.name)} title={m.common.rename} onClick={() => setEditing({ id: s.id, name: s.name })}>
                          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                            <path d="M12 20h9" />
                            <path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z" />
                          </svg>
                          </button>
                          <button className="icon-btn" aria-label={h.deleteNamed(s.name)} title={m.common.delete} onClick={() => setDeleting(s.id)}>
                          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                            <polyline points="3 6 5 6 21 6" />
                            <path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" />
                            <path d="M10 11v6M14 11v6M9 6V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2" />
                          </svg>
                          </button>
                        </>
                      )}
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        {pages.pager}
      </section>
      {selected !== null && (
        <RateChart
          series={chart === "meso" && selectedHasLoot ? mesoSeries(bars, m) : expSeries(bars, m)}
          actions={
            selectedHasLoot && (
              <div className="segmented" role="group" aria-label={h.chart}>
                {(["exp", "meso"] as const).map((c) => (
                  <button key={c} className={c === chart ? "current" : ""} aria-pressed={c === chart} onClick={() => setChart(c)}>
                    {c === "exp" ? h.exp : h.mesos}
                  </button>
                ))}
              </div>
            )
          }
        />
      )}
    </>
  );
}
