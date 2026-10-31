import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useLayoutEffect, useRef, useState, type MouseEvent } from "react";
import { formatDay, formatHours, formatWhole, statusShort } from "../lib/format";
import { getLiveChart, type Bar } from "../lib/ipc";
import { liveRates, mesoRates, profitOf } from "../lib/live";
import { useTick } from "../lib/useTick";
import { fitWidth, placeAndShow } from "./place";

/** The window's height (logical px); its width follows the content (fitWidth). */
const HEIGHT = 96;
const RING = 80;
const STROKE = 6;

const clock = (secs: number) => {
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
};

/** The outer ring is the level: white up to where the player is, then orange for one hour more
 *  at the current rate. The thin inner ring laps once per hour of grinding time. */
function Ring({ pct, pctPerHour, grindSecs }: { pct: number | null; pctPerHour: number | null; grindSecs: number }) {
  const mid = RING / 2;
  const r = (RING - STROKE) / 2 - 1;
  const c = 2 * Math.PI * r;
  const done = pct === null ? 0 : (c * pct) / 100;
  const hour = pct === null || pctPerHour === null ? 0 : (c * Math.min(pctPerHour, 100 - pct)) / 100;
  // The lap ring sits just inside the level ring, leaving the centre for the clock.
  const ri = r - STROKE / 2 - 3.5;
  const ci = 2 * Math.PI * ri;
  const lap = (ci * (grindSecs % 3600)) / 3600;
  const turn = `rotate(-90 ${mid} ${mid})`;
  return (
    <svg className="ring" width={RING} height={RING} viewBox={`0 0 ${RING} ${RING}`} aria-hidden="true">
      <circle cx={mid} cy={mid} r={r} className="track" strokeWidth={STROKE} />
      <circle cx={mid} cy={mid} r={r} className="done" strokeWidth={STROKE} strokeDasharray={`${done} ${c}`} transform={turn} />
      <circle cx={mid} cy={mid} r={r} className="hour" strokeWidth={STROKE} strokeDasharray={`0 ${done} ${hour} ${c}`} transform={turn} />
      <circle cx={mid} cy={mid} r={ri} className="lap-track" strokeWidth={3} />
      <circle cx={mid} cy={mid} r={ri} className="lap" strokeWidth={3} strokeDasharray={`${lap} ${ci}`} transform={turn} />
      <text x={mid} y={mid + 4} className={grindSecs >= 36_000 ? "clock long" : "clock"}>{clock(grindSecs)}</text>
      <text x={mid} y={mid + 15} className="clock-label">session</text>
    </svg>
  );
}

export function Popout() {
  const { tick, error } = useTick();
  const [bars, setBars] = useState<Bar[]>([]);
  const [problem, setProblem] = useState<string | null>(null);
  const root = useRef<HTMLDivElement>(null);
  // Chart requests go out every second and can finish out of order: keep only the newest.
  const latestRequest = useRef(0);

  useEffect(() => {
    void placeAndShow().then((res) => setProblem(res.success ? null : `window: ${res.error}`));
  }, []);

  useEffect(() => {
    if (tick === null) return;
    const request = ++latestRequest.current;
    void getLiveChart(120).then((res) => {
      if (request !== latestRequest.current) return;
      if (res.success) {
        setBars(res.data);
        setProblem(null);
      } else {
        setProblem(res.error.message);
      }
    });
  }, [tick]);

  const rates = liveRates(bars, tick);
  const mesos = tick?.loot.enabled ? mesoRates(bars) : null;
  const ok = error === null && problem === null && tick?.status.kind === "ok";
  const warning = error ? error.message : problem ?? (tick && tick.status.kind !== "ok" ? statusShort(tick.status) : null);
  const nextLevel = tick?.level != null ? `Lv ${tick.level + 1}` : "Level up";
  const toLevel = rates.hoursToLevel === null ? "—" : formatHours(rates.hoursToLevel);
  // The totals stay out of the way: hovering the Σ shows them.
  const totals = tick?.totals ?? null;
  const totalsText =
    totals === null || totals.since === null
      ? "Totals: nothing counted yet"
      : [
          `Totals since ${formatDay(totals.since)}`,
          `EXP gained: ${formatWhole(totals.exp)}`,
          ...(tick?.loot.enabled ? [`Profit: ${formatWhole(profitOf(totals.loot))}`] : []),
        ].join("\n");

  // The rates' width changes with their digits: keep the window as wide as the content.
  const width = useRef(0);
  useLayoutEffect(() => {
    const w = Math.ceil(root.current?.getBoundingClientRect().width ?? 0);
    if (w === 0 || w === width.current) return;
    width.current = w;
    void fitWidth(w, HEIGHT).then((res) => {
      if (!res.success) setProblem(`window: ${res.error}`);
    });
  });

  const drag = (e: MouseEvent) => {
    if (e.button !== 0 || (e.target as HTMLElement).closest("button")) return;
    void getCurrentWindow().startDragging().catch((e: unknown) => setProblem(`window: ${String(e)}`));
  };

  return (
    <div ref={root} className={ok ? "popout" : "popout stale"} onMouseDown={drag}>
      <span className="dot" title={ok ? "Reading OK" : (warning ?? "Starting…")} />
      <Ring pct={tick?.pctBp == null ? null : tick.pctBp / 100} pctPerHour={rates.pctPerHour} grindSecs={tick?.grindSecs ?? 0} />
      <div className="side">
        <dl className="rates">
          <div>
            <dt>EXP/h</dt>
            <dd className="exp">{rates.expPerHour === null ? "—" : formatWhole(rates.expPerHour)}</dd>
          </div>
          {tick?.loot.enabled && (
            <div>
              <dt>Meso/h</dt>
              <dd className="meso">{mesos === null ? "—" : formatWhole(mesos.profit)}</dd>
            </div>
          )}
        </dl>
        {ok || warning === null ? (
          <div className="foot">
            {nextLevel} in <b>{toLevel}</b>
            <span className="totals" title={totalsText} aria-label={totalsText}>Σ</span>
          </div>
        ) : (
          <div className="foot warning">{warning}</div>
        )}
      </div>
      <button className="close" aria-label="Close the popout" title="Close" onClick={() => void getCurrentWindow().close().catch((e: unknown) => setProblem(`window: ${String(e)}`))}>
        ✕
      </button>
    </div>
  );
}
