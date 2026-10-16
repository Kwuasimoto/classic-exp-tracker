import { useCallback, useEffect, useState } from "react";
import { RateChart } from "../components/RateChart";
import { formatExp, formatHours, formatPct, statusText } from "../lib/format";
import { getLiveChart, getLiveSnapshot, onMinute, onTick, type AppError, type Bar, type Tick } from "../lib/ipc";
import { liveRates } from "../lib/live";

type WindowMinutes = 120 | 180;

export function LiveView({ onCalibrate }: { onCalibrate: () => void }) {
  const [tick, setTick] = useState<Tick | null>(null);
  const [bars, setBars] = useState<Bar[]>([]);
  const [windowMinutes, setWindowMinutes] = useState<WindowMinutes>(120);
  const [error, setError] = useState<AppError | null>(null);

  const refreshChart = useCallback(async () => {
    const res = await getLiveChart(windowMinutes);
    if (res.success) {
      setBars(res.data);
    } else {
      setError(res.error);
    }
  }, [windowMinutes]);

  useEffect(() => {
    void refreshChart();
  }, [refreshChart]);

  useEffect(() => {
    let disposed = false;
    const unlisteners: Array<() => void> = [];
    const keep = (res: Awaited<ReturnType<typeof onMinute>>) => {
      if (!res.success) return setError(res.error);
      if (disposed) res.data();
      else unlisteners.push(res.data);
    };
    void getLiveSnapshot().then((res) => {
      if (disposed) return;
      if (res.success) setTick(res.data);
      else setError(res.error);
    });
    void onTick((res) => (res.success ? setTick(res.data) : setError(res.error))).then(keep);
    void onMinute(() => void refreshChart()).then(keep);
    return () => {
      disposed = true;
      unlisteners.forEach((u) => u());
    };
  }, [refreshChart]);

  const rates = liveRates(bars, tick);
  const status = tick?.status ?? null;

  return (
    <>
      <div className="card">
        <div className="row" style={{ justifyContent: "space-between" }}>
          <h2 style={{ margin: 0 }}>{tick?.mapName ?? "Waiting for map…"}</h2>
          <span className={status?.kind === "ok" ? "status" : "status warn"}>
            {status ? statusText(status) : "Starting…"}
            {status?.kind === "notCalibrated" && (
              <>
                {" "}
                <button className="primary" onClick={onCalibrate}>Calibrate</button>
              </>
            )}
          </span>
        </div>
        {error && <p className="error">{error.message}</p>}
        <div className="stats" style={{ marginTop: 12 }}>
          <Stat label="EXP/hr (last 60 min)" value={rates.expPerHour === null ? "—" : formatExp(rates.expPerHour)} />
          <Stat label="%/hr" value={rates.pctPerHour === null ? "—" : `${rates.pctPerHour.toFixed(2)}%`} />
          <Stat label="Time to level" value={rates.hoursToLevel === null ? "—" : formatHours(rates.hoursToLevel)} />
          <Stat label="Current EXP" value={tick?.exp == null || tick.pctBp == null ? "—" : `${formatExp(tick.exp)} (${formatPct(tick.pctBp)})`} />
        </div>
      </div>
      <div className="card">
        <div className="row" style={{ justifyContent: "flex-end", marginBottom: 8 }}>
          {([120, 180] as const).map((m) => (
            <button key={m} className={m === windowMinutes ? "primary" : ""} onClick={() => setWindowMinutes(m)}>
              {m / 60}h
            </button>
          ))}
        </div>
        <RateChart bars={bars} />
      </div>
    </>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="stat">
      <div className="label">{label}</div>
      <div className="value">{value}</div>
    </div>
  );
}
