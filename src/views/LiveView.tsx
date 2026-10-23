import { useCallback, useEffect, useRef, useState } from "react";
import { RateChart } from "../components/RateChart";
import { formatDateTime, formatHours, formatPct } from "../lib/format";
import { getLiveChart, lockMap, renameMap, resetSession, saveSnapshot, unlockMap, WINDOWS_MINUTES, type AppError, type Bar, type Tick, type WindowMinutes } from "../lib/ipc";
import { expToLevel, liveRates } from "../lib/live";

const TOAST_MS = 3000;
const whole = (n: number) => Math.round(n).toLocaleString("en-US");

export function LiveView({ tick }: { tick: Tick | null }) {
  const [bars, setBars] = useState<Bar[]>([]);
  const [windowMinutes, setWindowMinutes] = useState<WindowMinutes>(WINDOWS_MINUTES[0]);
  const [error, setError] = useState<AppError | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  // The map being renamed: only ever opened by the user's click, never by the app.
  const [naming, setNaming] = useState<{ mapId: number; name: string } | null>(null);
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

  // Reload with every tick (once a second): the chart includes the minute in progress.
  useEffect(() => {
    void refreshChart();
  }, [tick, refreshChart]);

  const rates = liveRates(bars, tick);
  const toLevel = expToLevel(tick);
  const level = tick?.level ?? null;
  const locked = tick?.lockedMap != null;
  const lockedName = tick?.lockedName ?? "the locked map";
  // Locked, but somewhere else: the EXP here isn't counted.
  const away = locked && (tick.shopping || tick.mapId !== tick.lockedMap);

  return (
    <>
      <section className="panel row" style={{ flexWrap: "nowrap", gap: 16 }}>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div className="label">Current map</div>
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
              <h1>{tick?.shopping ? "Shopping" : tick?.mapId == null ? "Waiting for map…" : (tick.mapName ?? "Unrecognised map")}</h1>
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
          {away && <div className="caption">Locked to {lockedName}: EXP here isn't counted</div>}
        </div>
        <div className={level === null ? "badge pending" : "badge"} title={level === null ? "Narrowing down your level from the EXP readings" : undefined}>
          LV. {level ?? "Determining…"}
        </div>
      </section>
      {error && <p className="panel error">{error.message}</p>}
      {toast && <div className="toast" role="status">{toast}</div>}

      <div className="stats">
        <Stat label="EXP / hr" accent value={rates.expPerHour === null ? "—" : whole(rates.expPerHour)} caption={locked ? `last 60 min on ${lockedName}` : "last 60 min of training"} />
        <Stat label="% / hr" value={rates.pctPerHour === null ? "—" : `${rates.pctPerHour.toFixed(2)}%`} caption={level === null ? "of this level" : `of level ${level}`} />
        <Stat label="Time to level" value={rates.hoursToLevel === null ? "—" : formatHours(rates.hoursToLevel)} caption="at the current rate" />
        <Stat
          label="EXP to level"
          value={toLevel === null ? "—" : `${toLevel.exact ? "" : "≈ "}${whole(toLevel.exp)}`}
          caption={
            tick?.exp == null || tick.pctBp == null
              ? "waiting for EXP"
              : `${whole(tick.exp)}${tick.totalEst == null ? "" : ` / ${whole(tick.totalEst)}`} (${formatPct(tick.pctBp)})`
          }
        />
      </div>

      <RateChart
        bars={bars}
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
    </>
  );
}

function Stat({ label, value, caption, accent = false }: { label: string; value: string; caption: string; accent?: boolean }) {
  return (
    <section className="panel">
      <div className={accent ? "label accent" : "label"}>{label}</div>
      <div className="value">{value}</div>
      <div className="caption">{caption}</div>
    </section>
  );
}
