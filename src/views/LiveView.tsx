import { useCallback, useEffect, useState } from "react";
import { RateChart } from "../components/RateChart";
import { formatHours, formatPct } from "../lib/format";
import { getLiveChart, type AppError, type Bar, type Tick } from "../lib/ipc";
import { expToLevel, liveRates } from "../lib/live";

type WindowMinutes = 120 | 180;
const whole = (n: number) => Math.round(n).toLocaleString("en-US");

export function LiveView({ tick, onCalibrate }: { tick: Tick | null; onCalibrate: () => void }) {
  const [bars, setBars] = useState<Bar[]>([]);
  const [windowMinutes, setWindowMinutes] = useState<WindowMinutes>(120);
  const [error, setError] = useState<AppError | null>(null);

  const refreshChart = useCallback(async () => {
    const res = await getLiveChart(windowMinutes);
    if (res.success) setBars(res.data);
    else setError(res.error);
  }, [windowMinutes]);

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
          <div className="segmented" role="group" aria-label="Chart window">
            {([120, 180] as const).map((m) => (
              <button key={m} className={m === windowMinutes ? "current" : ""} aria-pressed={m === windowMinutes} onClick={() => setWindowMinutes(m)}>
                {m / 60}H
              </button>
            ))}
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
