import { useEffect, useRef, useState, type ReactNode } from "react";
import { Bar as BarMark, CartesianGrid, ComposedChart, Line, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import type { Bar } from "../lib/ipc";
import { formatClock, formatExp } from "../lib/format";
import { useMessages } from "../lib/i18n";
import { profitOf } from "../lib/live";
import type { Messages } from "../locales/en";

const tick = (v: unknown) => (typeof v === "number" ? formatExp(v) : String(v));
const axisText = { fill: "var(--muted)", fontSize: 12 };

// Time-axis label spacing in minutes: the smallest that keeps the labels at the most that
// fit, one per TICK_ROOM px of plot (never more than MAX_TICKS), so a narrow chart's labels
// don't run into each other.
const TICK_STEPS = [10, 20, 30, 60, 120, 180, 360, 720];
const MAX_TICKS = 7;
const TICK_ROOM = 90;

/** Bar starts to label: one per step, on round clock times (12:00, 12:20, ...).
 *  A bar is labelled when it holds the step boundary, so time zones with a
 *  non-10-minute offset still get evenly spaced labels. */
function clockTicks(starts: number[], maxTicks: number): number[] {
  let ticks = starts;
  for (const step of TICK_STEPS) {
    ticks = starts.filter((s) => {
      const d = new Date(s * 1000);
      return Math.floor((d.getHours() * 60 + d.getMinutes()) / 10) % (step / 10) === 0;
    });
    if (ticks.length <= maxTicks) break;
  }
  return ticks;
}

/** What the chart plots: an amount per 10-minute bar and a 60-minute hourly rate. */
export interface Series {
  /** Which chart this is, so the parts hidden on it are remembered in any language. */
  id: "exp" | "exp-profit" | "meso";
  barLabel: string;
  lineLabel: string;
  /** The rate line's colour: orange for EXP/hr, meso gold for mesos (docs/STYLE.md). */
  colour: string;
  points: { start: number; bar: number; line: number | null }[];
  /** A second rate on its own right axis, dashed (Live: Profit/hr beside EXP/hr). */
  second?: { label: string; colour: string; values: (number | null)[] };
}

/** Profit per hour of training over the bar's 60 minutes, or null without training. */
const profitPerHour = (b: Bar) => b.lootPerHour && b.lootPerHour.meso + b.lootPerHour.etcValue - b.lootPerHour.potionCost;

/** EXP, with Profit/hr as a second line when meso tracking is on. */
export const expSeries = (bars: Bar[], m: Messages, withProfit = false): Series => ({
  id: withProfit ? "exp-profit" : "exp",
  // With two rate lines the legend is tight: the keys get shorter (the ledger says "last 60 min").
  barLabel: withProfit ? m.chart.expPer10Short : m.chart.expPer10,
  lineLabel: withProfit ? m.chart.expPerHour : m.chart.expPerHourAvg,
  colour: "var(--accent)",
  points: bars.map((b) => ({ start: b.start, bar: b.expGained, line: b.ratePerHour })),
  second: withProfit ? { label: m.chart.profitPerHour, colour: "var(--meso)", values: bars.map(profitPerHour) } : undefined,
});

/** Profit: mesos and Etc value picked up, less potions used. */
export const mesoSeries = (bars: Bar[], m: Messages): Series => ({
  id: "meso",
  barLabel: m.chart.mesosPer10,
  lineLabel: m.chart.profitPerHourAvg,
  colour: "var(--meso)",
  points: bars.map((b) => ({
    start: b.start,
    bar: profitOf(b.loot),
    line: profitPerHour(b),
  })),
});

type Part = "bar" | "line" | "second";
const HIDDEN_KEY = "chart-hidden-parts";

/** The chart parts the viewer switched off, as "series:part" (a display preference; it
 *  only lives in this browser's storage, and a failed read shows everything). */
function savedHidden(): Set<string> {
  try {
    const raw = localStorage.getItem(HIDDEN_KEY);
    const parsed: unknown = raw ? JSON.parse(raw) : [];
    return new Set(Array.isArray(parsed) ? parsed.filter((v): v is string => typeof v === "string") : []);
  } catch {
    return new Set();
  }
}

function saveHidden(hidden: Set<string>) {
  try {
    localStorage.setItem(HIDDEN_KEY, JSON.stringify([...hidden]));
  } catch {
    // Storage blocked: the choice lasts until the page reloads.
  }
}

/** Amount per 10 min (bars, left axis) and the 60-min hourly rate (orange line, right axis).
 *  No animation: the live chart reloads every second.
 *  `fill` makes the panel take the remaining height; `actions` sit at the legend's right. */
export function RateChart({ series, fill = false, actions }: { series: Series; fill?: boolean; actions?: ReactNode }) {
  const m = useMessages();
  const data = series.points.map((p, i) => ({ start: p.start, exp: p.bar, rate: p.line, second: series.second?.values[i] ?? null }));
  const empty = series.points.every((p) => p.bar === 0 && p.line === null);
  const bars = series.points;
  // Legend keys switch their part of the chart on and off.
  const [hidden, setHidden] = useState(savedHidden);
  const keyOf = (part: Part) => `${series.id}:${part}`;
  const shown = (part: Part) => !hidden.has(keyOf(part));
  const toggle = (part: Part) => {
    const next = new Set(hidden);
    if (next.has(keyOf(part))) next.delete(keyOf(part));
    else next.add(keyOf(part));
    setHidden(next);
    saveHidden(next);
  };
  const key = (part: Part, colour: string | undefined, swatch: ReactNode, label: string) => (
    <button
      className={shown(part) ? "legend-key label" : "legend-key label off"}
      style={colour ? { color: colour } : undefined}
      aria-pressed={shown(part)}
      title={shown(part) ? m.chart.hide(label) : m.chart.show(label)}
      onClick={() => toggle(part)}
    >
      {swatch}
      {label}
    </button>
  );
  // The plot's width decides how many time labels fit. A plot folded away (compressed mode)
  // isn't drawn at all, rather than redrawn into no room every second.
  const plot = useRef<HTMLDivElement>(null);
  const [maxTicks, setMaxTicks] = useState(MAX_TICKS);
  const [hasRoom, setHasRoom] = useState(true);
  useEffect(() => {
    const el = plot.current;
    if (!el) return;
    const observer = new ResizeObserver(() => {
      setMaxTicks(Math.max(2, Math.min(MAX_TICKS, Math.floor(el.clientWidth / TICK_ROOM))));
      setHasRoom(el.clientWidth > 0 && el.clientHeight > 0);
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  return (
    <section className={fill ? "panel chart fill" : "panel chart"}>
      <div className="legend">
        <div className="legend-keys">
          {key("bar", undefined, <span className="swatch" />, series.barLabel)}
          {key("line", series.colour, <span className="swatch line" style={{ background: series.colour }} />, series.lineLabel)}
          {series.second && key("second", series.second.colour, <span className="swatch line dashed" style={{ borderColor: series.second.colour }} />, series.second.label)}
        </div>
        {actions}
      </div>
      <div className="chart-plot" ref={plot}>
        {empty && <p className="chart-empty caption">{m.chart.empty}</p>}
        {hasRoom && (
          <ResponsiveContainer width="100%" height="100%">
            <ComposedChart
              data={data}
              margin={{ top: 8, right: 4, bottom: 0, left: 4 }}
              title={m.chart.title([series.barLabel, series.lineLabel, series.second?.label].filter(Boolean).join(", "))}
              desc={m.chart.desc}
            >
              <CartesianGrid vertical={false} stroke="var(--line)" />
              <XAxis
                dataKey="start"
                ticks={clockTicks(bars.map((b) => b.start), maxTicks)}
                interval={0}
                tickFormatter={(v: number) => formatClock(v)}
                tick={axisText}
                tickLine={false}
                axisLine={false}
                tickMargin={8}
              />
              <YAxis yAxisId="exp" hide={!shown("bar")} width={40} tick={axisText} tickLine={false} axisLine={false} tickFormatter={tick} />
              <YAxis yAxisId="rate" hide={!shown("line")} orientation="right" width={44} tick={{ ...axisText, fill: series.colour }} tickLine={false} axisLine={false} tickFormatter={tick} />
              {series.second && (
                <YAxis yAxisId="second" hide={!shown("second")} orientation="right" width={44} tick={{ ...axisText, fill: series.second.colour }} tickLine={false} axisLine={false} tickFormatter={tick} />
              )}
              <Tooltip
                formatter={tick}
                labelFormatter={(v) => (typeof v === "number" ? formatClock(v) : String(v))}
                cursor={{ fill: "rgba(255, 255, 255, 0.05)" }}
                contentStyle={{ background: "#141416", border: 0, borderRadius: 10, boxShadow: "var(--shadow)" }}
                labelStyle={{ color: "var(--muted)" }}
              />
              <BarMark yAxisId="exp" hide={!shown("bar")} dataKey="exp" name={series.barLabel} fill="var(--bar)" radius={[4, 4, 0, 0]} isAnimationActive={false} />
              <Line
                yAxisId="rate"
                hide={!shown("line")}
                dataKey="rate"
                name={series.lineLabel}
                stroke={series.colour}
                strokeWidth={3}
                dot={{ r: 4, fill: series.colour, strokeWidth: 0 }}
                activeDot={{ r: 5 }}
                connectNulls={false}
                isAnimationActive={false}
              />
              {series.second && (
                <Line
                  yAxisId="second"
                  hide={!shown("second")}
                  dataKey="second"
                  name={series.second.label}
                  stroke={series.second.colour}
                  strokeWidth={2}
                  strokeDasharray="6 4"
                  dot={{ r: 3, fill: series.second.colour, strokeWidth: 0 }}
                  activeDot={{ r: 4 }}
                  connectNulls={false}
                  isAnimationActive={false}
                />
              )}
            </ComposedChart>
          </ResponsiveContainer>
        )}
      </div>
    </section>
  );
}
