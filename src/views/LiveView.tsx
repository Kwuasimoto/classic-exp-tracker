import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { ActivePots } from "../components/ActivePots";
import { Ledger, type Headline, type LedgerGroup } from "../components/Ledger";
import { Pickups } from "../components/Pickups";
import { expSeries, RateChart } from "../components/RateChart";
import { formatDateTime, formatDay, formatHours, formatPct, formatWhole as whole } from "../lib/format";
import { getLiveChart, lockMap, renameMap, resetSession, resetTotals, saveSnapshot, unlockMap, WINDOWS_MINUTES, type AppError, type Bar, type Tick, type WindowMinutes } from "../lib/ipc";
import { expToLevel, HELD_WARNING, liveRates, lockOf, lootHeldSince, mesoLines, mesoRates, profitCaption, profitOf } from "../lib/live";

const TOAST_MS = 3000;

export function LiveView({ tick, status }: { tick: Tick | null; status: ReactNode }) {
  const [bars, setBars] = useState<Bar[]>([]);
  const [windowMinutes, setWindowMinutes] = useState<WindowMinutes>(WINDOWS_MINUTES[0]);
  const [error, setError] = useState<AppError | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  // The map being renamed: only ever opened by the user's click, never by the app.
  const [naming, setNaming] = useState<{ mapId: number; name: string } | null>(null);
  // Resetting the totals throws away what may be weeks of counting: it asks first.
  const [confirmingTotals, setConfirmingTotals] = useState(false);
  // Chart requests go out every second and can finish out of order: keep only the newest.
  const latestRequest = useRef(0);

  const refreshChart = useCallback(async () => {
    const request = ++latestRequest.current;
    const res = await getLiveChart(windowMinutes);
    if (request !== latestRequest.current) return;
    if (res.success) {
      setBars(res.data);
      setError(null);
    } else {
      setError(res.error);
    }
  }, [windowMinutes]);

  // The toast fades by itself.
  useEffect(() => {
    if (toast === null) return;
    const timer = setTimeout(() => setToast(null), TOAST_MS);
    return () => clearTimeout(timer);
  }, [toast]);

  const save = async () => {
    const name = `${tick?.mapName ?? "Session"} · ${formatDateTime(Date.now() / 1000)}`;
    const res = await saveSnapshot(name, windowMinutes);
    if (!res.success) return setError(res.error);
    setToast(res.data === null ? "Nothing to save yet" : "Saved to History");
  };

  const saveMapName = async () => {
    if (!naming) return;
    const res = await renameMap(naming.mapId, naming.name);
    if (!res.success) return setError(res.error);
    setNaming(null);
    setToast("Map renamed");
  };

  const toggleLock = async () => {
    if (!tick) return;
    const res = tick.lockedMap !== null ? await unlockMap() : tick.mapId !== null ? await lockMap(tick.mapId) : null;
    if (res === null) return;
    if (!res.success) return setError(res.error);
    setToast(tick.lockedMap !== null ? "Unlocked: every map counts" : "Locked: only this map counts");
    void refreshChart();
  };

  const reset = async () => {
    const res = await resetSession();
    if (!res.success) return setError(res.error);
    setToast("Session reset");
    void refreshChart();
  };

  const clearTotals = async () => {
    const res = await resetTotals();
    setConfirmingTotals(false);
    if (!res.success) return setError(res.error);
    setToast("Totals reset");
  };

  // Reload with every tick (once a second): the chart includes the minute in progress.
  useEffect(() => {
    void refreshChart();
  }, [tick, refreshChart]);

  const rates = liveRates(bars, tick);
  const toLevel = expToLevel(tick);
  const level = tick?.level ?? null;
  const lock = lockOf(tick);
  const locked = lock !== null;
  const lockedName = lock?.name ?? "the locked map";
  // With meso tracking on, the ledger and chart show the mesos too.
  const tracking = tick?.loot.enabled === true;
  const mesos = mesoRates(bars);
  const headlines: Headline[] = [
    { title: "EXP / hr", tone: "exp", value: rates.expPerHour === null ? "—" : whole(rates.expPerHour), caption: locked ? `last 60 min · ${lockedName}` : "last 60 min" },
  ];
  const held = lootHeldSince(tick?.loot ?? null);
  if (tracking) headlines.push({ title: "Profit / hr", tone: "meso", value: mesos === null ? "—" : whole(mesos.profit), caption: profitCaption(held) });
  const groups: LedgerGroup[] = [
    {
      title: "EXP",
      lines: [
        { label: "% / hr", note: level === null ? undefined : `of level ${level}`, value: rates.pctPerHour === null ? "—" : `${rates.pctPerHour.toFixed(2)}%` },
        { label: "Time to level", value: rates.hoursToLevel === null ? "—" : formatHours(rates.hoursToLevel) },
        { label: "EXP to level", value: toLevel === null ? "—" : `${toLevel.exact ? "" : "≈ "}${whole(toLevel.exp)}` },
        {
          label: "EXP now",
          note: tick?.pctBp == null ? undefined : formatPct(tick.pctBp),
          value: tick?.exp == null ? "—" : `${tick.expExact ? "" : "≈ "}${whole(tick.exp)}`,
        },
      ],
    },
  ];
  if (tracking) groups.push({ title: "Mesos per hour", lines: mesoLines(mesos, tick?.loot ?? null) });
  const totals = tick?.totals ?? null;
  groups.push({
    title: "Totals",
    note: totals?.since == null ? undefined : `since ${formatDay(totals.since)}`,
    action: confirmingTotals ? (
      <>
        <button className="btn small danger" onClick={() => void clearTotals()}>Reset totals</button>
        <button className="btn small" onClick={() => setConfirmingTotals(false)}>Keep</button>
      </>
    ) : (
      <button className="icon-btn" aria-label="Reset totals" title="Start the totals again from 0 (the session reset leaves them alone)" disabled={totals === null} onClick={() => setConfirmingTotals(true)}>
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="M3 12a9 9 0 1 0 3-6.7" />
          <polyline points="3 3 3 9 9 9" />
        </svg>
      </button>
    ),
    lines: [
      { label: "EXP gained", value: totals === null ? "—" : whole(totals.exp) },
      ...(tracking ? [{ label: "Profit", note: "after potions", value: totals === null ? "—" : whole(profitOf(totals.loot)) }] : []),
    ],
  });

  return (
    <>
      <section className="panel page-head">
        <div className="page-head-main">
          {naming ? (
            <div className="row" style={{ flexWrap: "nowrap", gap: 8 }}>
              <input
                value={naming.name}
                autoFocus
                aria-label="Map name"
                style={{ flex: 1, minWidth: 0 }}
                onChange={(e) => setNaming({ mapId: naming.mapId, name: e.target.value })}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void saveMapName();
                  if (e.key === "Escape") setNaming(null);
                }}
              />
              <button className="btn primary" onClick={() => void saveMapName()}>Save</button>
              <button className="btn" onClick={() => setNaming(null)}>Cancel</button>
            </div>
          ) : (
            <div className="row" style={{ flexWrap: "nowrap", gap: 8 }}>
              <h1 className="page-title">{tick?.shopping ? "Shopping" : tick?.mapId == null ? "Waiting for map…" : (tick.mapName ?? "Unrecognised map")}</h1>
              {tick?.mapId != null && !tick.shopping && (
                <button
                  className="icon-btn"
                  aria-label="Rename map"
                  title={tick.mapName === null ? "This map isn't on the map list: name it" : "Rename this map"}
                  onClick={() => setNaming({ mapId: tick.mapId as number, name: tick.mapName ?? "" })}
                >
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                    <path d="M12 20h9" />
                    <path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z" />
                  </svg>
                </button>
              )}
              {(locked || (tick?.mapId != null && !tick.shopping)) && (
                <button
                  className={locked ? "icon-btn current" : "icon-btn"}
                  aria-label={locked ? "Unlock map" : "Lock to this map"}
                  aria-pressed={locked}
                  title={locked ? `Locked to ${lockedName}: only its EXP counts. Click to count every map.` : "Lock to this map: count only its EXP and time"}
                  onClick={() => void toggleLock()}
                >
                  <svg width="14" height="14" viewBox="0 0 24 24" fill={locked ? "currentColor" : "none"} stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                    <rect x="5" y="11" width="14" height="10" rx="2" />
                    <path d={locked ? "M8 11V7a4 4 0 0 1 8 0v4" : "M8 11V7a4 4 0 0 1 7.5-2"} />
                  </svg>
                </button>
              )}
            </div>
          )}
        </div>
        {status}
        <div className={level === null ? "badge pending" : "badge"} title={level === null ? "Narrowing down your level from the EXP readings" : undefined}>
          LV. {level ?? "Determining…"}
        </div>
      </section>
      {error && <p className="panel error">{error.message}</p>}
      {toast && <div className="toast" role="status">{toast}</div>}

      <div className="page-body chart-only">
        <Ledger headlines={headlines} warning={lock?.away ? `Locked to ${lockedName}: nothing here is counted` : held !== null ? HELD_WARNING : null} groups={groups}>
          {tracking && tick && (
            <>
              <ActivePots loot={tick.loot} onError={setError} onDone={setToast} />
              <Pickups pickedUp={tick.loot.pickedUp} />
            </>
          )}
        </Ledger>
        <div className="page-main">
      <RateChart
        series={expSeries(bars, tracking)}
        fill
        actions={
          <div className="row" style={{ gap: 8, flexWrap: "nowrap" }}>
            <button className="icon-btn" aria-label="Save to History" title={`Save the last ${windowMinutes / 60} hours to History`} onClick={() => void save()}>
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z" />
                <polyline points="17 21 17 13 7 13 7 21" />
                <polyline points="7 3 7 8 15 8" />
              </svg>
            </button>
            <button className="icon-btn" aria-label="Reset session" title="Start a new session (saves nothing)" onClick={() => void reset()}>
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <path d="M3 12a9 9 0 1 0 3-6.7" />
                <polyline points="3 3 3 9 9 9" />
              </svg>
            </button>
            <div className="segmented" role="group" aria-label="Chart window">
              {WINDOWS_MINUTES.map((m) => (
                <button key={m} className={m === windowMinutes ? "current" : ""} aria-pressed={m === windowMinutes} onClick={() => setWindowMinutes(m)}>
                  {m / 60}H
                </button>
              ))}
            </div>
          </div>
        }
      />
        </div>
      </div>
    </>
  );
}
