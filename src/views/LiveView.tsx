import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { ActivePots } from "../components/ActivePots";
import { Ledger, type Headline, type LedgerGroup } from "../components/Ledger";
import { Pickups } from "../components/Pickups";
import { expSeries, RateChart } from "../components/RateChart";
import { formatDateTime, formatDay, formatHours, formatPct, formatWhole as whole } from "../lib/format";
import { useMessages } from "../lib/i18n";
import { getLiveChart, lockMap, renameMap, resetSession, resetTotals, saveSnapshot, unlockMap, WINDOWS_MINUTES, type AppError, type Bar, type Tick, type WindowMinutes } from "../lib/ipc";
import { expToLevel, liveRates, lockOf, lootHeldSince, mesoLines, mesoRates, profitCaption, profitOf } from "../lib/live";

const TOAST_MS = 3000;

export function LiveView({ tick, status }: { tick: Tick | null; status: ReactNode }) {
  const m = useMessages();
  const l = m.live;
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
    const name = l.snapshotName(tick?.mapName ?? l.session, formatDateTime(Date.now() / 1000));
    const res = await saveSnapshot(name, windowMinutes);
    if (!res.success) return setError(res.error);
    setToast(res.data === null ? l.nothingToSave : l.saved);
  };

  const saveMapName = async () => {
    if (!naming) return;
    const res = await renameMap(naming.mapId, naming.name);
    if (!res.success) return setError(res.error);
    setNaming(null);
    setToast(l.mapRenamed);
  };

  const toggleLock = async () => {
    if (!tick) return;
    const res = tick.lockedMap !== null ? await unlockMap() : tick.mapId !== null ? await lockMap(tick.mapId) : null;
    if (res === null) return;
    if (!res.success) return setError(res.error);
    setToast(tick.lockedMap !== null ? l.unlocked : l.locked);
    void refreshChart();
  };

  const reset = async () => {
    const res = await resetSession();
    if (!res.success) return setError(res.error);
    setToast(l.sessionReset);
    void refreshChart();
  };

  const clearTotals = async () => {
    const res = await resetTotals();
    setConfirmingTotals(false);
    if (!res.success) return setError(res.error);
    setToast(l.totalsReset);
  };

  // Reload with every tick (once a second): the chart includes the minute in progress.
  useEffect(() => {
    void refreshChart();
  }, [tick, refreshChart]);

  const rates = liveRates(bars, tick);
  const toLevel = expToLevel(tick);
  const level = tick?.level ?? null;
  const lock = lockOf(tick, m);
  const locked = lock !== null;
  const lockedName = lock?.name ?? l.theLockedMap;
  // With meso tracking on, the ledger and chart show the mesos too.
  const tracking = tick?.loot.enabled === true;
  const mesos = mesoRates(bars);
  const headlines: Headline[] = [
    { title: l.expPerHour, tone: "exp", value: rates.expPerHour === null ? "—" : whole(rates.expPerHour), caption: locked ? l.last60On(lockedName) : l.last60 },
  ];
  const held = lootHeldSince(tick?.loot ?? null);
  if (tracking) headlines.push({ title: l.profitPerHour, tone: "meso", value: mesos === null ? "—" : whole(mesos.profit), caption: profitCaption(held, m) });
  const groups: LedgerGroup[] = [
    {
      title: l.exp,
      lines: [
        { label: l.pctPerHour, note: level === null ? undefined : l.ofLevel(level), value: rates.pctPerHour === null ? "—" : `${rates.pctPerHour.toFixed(2)}%` },
        { label: l.timeToLevel, value: rates.hoursToLevel === null ? "—" : formatHours(rates.hoursToLevel) },
        { label: l.expToLevel, value: toLevel === null ? "—" : `${toLevel.exact ? "" : "≈ "}${whole(toLevel.exp)}` },
        {
          label: l.expNow,
          note: tick?.pctBp == null ? undefined : formatPct(tick.pctBp),
          value: tick?.exp == null ? "—" : `${tick.expExact ? "" : "≈ "}${whole(tick.exp)}`,
        },
      ],
    },
  ];
  if (tracking) groups.push({ title: m.mesos.perHour, lines: mesoLines(mesos, tick?.loot ?? null, m) });
  const totals = tick?.totals ?? null;
  groups.push({
    title: l.totals,
    note: totals?.since == null ? undefined : l.since(formatDay(totals.since)),
    action: confirmingTotals ? (
      <>
        <button className="btn small danger" onClick={() => void clearTotals()}>{l.resetTotals}</button>
        <button className="btn small" onClick={() => setConfirmingTotals(false)}>{m.common.keep}</button>
      </>
    ) : (
      <button className="icon-btn" aria-label={l.resetTotals} title={l.resetTotalsTitle} disabled={totals === null} onClick={() => setConfirmingTotals(true)}>
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="M3 12a9 9 0 1 0 3-6.7" />
          <polyline points="3 3 3 9 9 9" />
        </svg>
      </button>
    ),
    lines: [
      { label: l.expGained, value: totals === null ? "—" : whole(totals.exp) },
      ...(tracking ? [{ label: l.profit, note: l.afterPotions, value: totals === null ? "—" : whole(profitOf(totals.loot)) }] : []),
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
                aria-label={l.mapName}
                style={{ flex: 1, minWidth: 0 }}
                onChange={(e) => setNaming({ mapId: naming.mapId, name: e.target.value })}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void saveMapName();
                  if (e.key === "Escape") setNaming(null);
                }}
              />
              <button className="btn primary" onClick={() => void saveMapName()}>{m.common.save}</button>
              <button className="btn" onClick={() => setNaming(null)}>{m.common.cancel}</button>
            </div>
          ) : (
            <div className="row" style={{ flexWrap: "nowrap", gap: 8 }}>
              <h1 className="page-title">{tick?.shopping ? l.shopping : tick?.mapId == null ? l.waitingForMap : (tick.mapName ?? l.unrecognisedMap)}</h1>
              {tick?.mapId != null && !tick.shopping && (
                <button
                  className="icon-btn"
                  aria-label={l.renameMap}
                  title={tick.mapName === null ? l.nameMapTitle : l.renameMapTitle}
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
                  aria-label={locked ? l.unlock : l.lock}
                  aria-pressed={locked}
                  title={locked ? l.unlockTitle(lockedName) : l.lockTitle}
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
        <div className={level === null ? "badge pending" : "badge"} title={level === null ? l.levelPendingTitle : undefined}>
          {level === null ? l.levelPending : l.level(level)}
        </div>
      </section>
      {error && <p className="panel error">{error.message}</p>}
      {toast && <div className="toast" role="status">{toast}</div>}

      <div className="page-body chart-only">
        <Ledger headlines={headlines} warning={lock?.away ? l.lockedAway(lockedName) : held !== null ? m.mesos.heldWarning : null} groups={groups}>
          {tracking && tick && (
            <>
              <ActivePots loot={tick.loot} onError={setError} onDone={setToast} />
              <Pickups pickedUp={tick.loot.pickedUp} />
            </>
          )}
        </Ledger>
        <div className="page-main">
      <RateChart
        series={expSeries(bars, m, tracking)}
        fill
        actions={
          <div className="row" style={{ gap: 8, flexWrap: "nowrap" }}>
            <button className="icon-btn" aria-label={l.saveToHistory} title={l.saveTitle(windowMinutes / 60)} onClick={() => void save()}>
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z" />
                <polyline points="17 21 17 13 7 13 7 21" />
                <polyline points="7 3 7 8 15 8" />
              </svg>
            </button>
            <button className="icon-btn" aria-label={l.resetSession} title={l.resetTitle} onClick={() => void reset()}>
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <path d="M3 12a9 9 0 1 0 3-6.7" />
                <polyline points="3 3 3 9 9 9" />
              </svg>
            </button>
            <div className="segmented" role="group" aria-label={m.common.chartWindow}>
              {WINDOWS_MINUTES.map((w) => (
                <button key={w} className={w === windowMinutes ? "current" : ""} aria-pressed={w === windowMinutes} onClick={() => setWindowMinutes(w)}>
                  {m.time.windowHours(w / 60)}
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
