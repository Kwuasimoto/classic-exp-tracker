import { useCallback, useEffect, useRef, useState } from "react";
import { RateChart } from "../components/RateChart";
import { formatDateTime, formatHours, formatPct } from "../lib/format";
import { getLiveChart, resetSession, type AppError, type Bar, type Tick } from "../lib/ipc";
import { expToLevel, liveRates } from "../lib/live";

type WindowMinutes = 120 | 180;
const whole = (n: number) => Math.round(n).toLocaleString("en-US");

export function LiveView({ tick, onCalibrate }: { tick: Tick | null; onCalibrate: () => void }) {
  const [bars, setBars] = useState<Bar[]>([]);
  const [windowMinutes, setWindowMinutes] = useState<WindowMinutes>(120);
  const [error, setError] = useState<AppError | null>(null);
  const [confirmingReset, setConfirmingReset] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
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

  const reset = async () => {
    setConfirmingReset(false);
    const name = `${tick?.mapName ?? "Session"} · ${formatDateTime(Date.now() / 1000)}`;
    const res = await resetSession(name);
    if (!res.success) return setError(res.error);
    setNotice(res.data === null ? "Session reset. There was nothing to save." : `Session reset. Saved "${name}" to History.`);
    void refreshChart();
  };

  // Reload with every tick (once a second): the chart includes the minute in progress.
  useEffect(() => {
    void refreshChart();
  }, [tick, refreshChart]);

  const rates = liveRates(bars, tick);
  const toLevel = expToLevel(tick);
  const level = tick?.level ?? null;

  return (
    <>
      <section className="panel row" style={{ flexWrap: "nowrap", gap: 16 }}>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div className="label">Current map</div>
          <h1>{tick?.mapName ?? "Waiting for map…"}</h1>
        </div>
        {tick?.status.kind === "notCalibrated" && <button className="btn primary" onClick={onCalibrate}>Calibrate</button>}
        {level !== null && <div className="badge">LV. {level}</div>}
      </section>
      {error && <p className="panel error">{error.message}</p>}
      {notice && (
        <p className="panel row caption" style={{ fontSize: 14 }}>
          <span style={{ flex: 1 }}>{notice}</span>
          <button className="btn" onClick={() => setNotice(null)}>OK</button>
        </p>
      )}

      <div className="stats">
        <Stat label="EXP / hr" accent value={rates.expPerHour === null ? "—" : whole(rates.expPerHour)} caption="last 60 min of training" />
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
            {confirmingReset ? (
              <div className="row" style={{ flexWrap: "nowrap" }}>
                <span className="caption">Save the last 2 h and start a new session?</span>
                <button className="btn primary" onClick={() => void reset()}>Save &amp; reset</button>
                <button className="btn" onClick={() => setConfirmingReset(false)}>Cancel</button>
              </div>
            ) : (
              <button className="icon-btn" aria-label="Reset session" title="Save the last 2 hours as a snapshot and start a new session" onClick={() => setConfirmingReset(true)}>
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                  <path d="M3 12a9 9 0 1 0 3-6.7" />
                  <polyline points="3 3 3 9 9 9" />
                </svg>
              </button>
            )}
            <div className="segmented" role="group" aria-label="Chart window">
              {([120, 180] as const).map((m) => (
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
