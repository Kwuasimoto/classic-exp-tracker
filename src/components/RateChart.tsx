import type { ReactNode } from "react";
import { Bar as BarMark, CartesianGrid, ComposedChart, Line, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import type { Bar } from "../lib/ipc";
import { formatClock, formatExp } from "../lib/format";

const tick = (v: unknown) => (typeof v === "number" ? formatExp(v) : String(v));
const axisText = { fill: "var(--muted)", fontSize: 12 };

/** EXP per 10 min (bars, left axis) and the 60-min EXP/hr (orange line, right axis).
 *  No animation: the live chart reloads every second.
 *  `fill` makes the panel take the remaining height; `actions` sit at the legend's right. */
export function RateChart({ bars, fill = false, actions }: { bars: Bar[]; fill?: boolean; actions?: ReactNode }) {
  const data = bars.map((b) => ({ label: formatClock(b.start), exp: b.expGained, rate: b.ratePerHour }));
  return (
    <section className={fill ? "panel chart fill" : "panel chart"}>
      <div className="legend">
        <span className="label"><span className="swatch" />EXP per 10 min</span>
        <span className="label accent"><span className="swatch line" />EXP/hr (60-min avg)</span>
        <span style={{ flex: 1 }} />
        {actions}
      </div>
      <div className="chart-plot">
        <ResponsiveContainer width="100%" height="100%">
          <ComposedChart data={data} margin={{ top: 8, right: 4, bottom: 0, left: 4 }}>
            <CartesianGrid vertical={false} stroke="var(--line)" />
            <XAxis dataKey="label" tick={axisText} tickLine={false} axisLine={false} tickMargin={8} minTickGap={12} />
            <YAxis yAxisId="exp" width={40} tick={axisText} tickLine={false} axisLine={false} tickFormatter={tick} />
            <YAxis yAxisId="rate" orientation="right" width={44} tick={{ ...axisText, fill: "var(--accent)" }} tickLine={false} axisLine={false} tickFormatter={tick} />
            <Tooltip
              formatter={tick}
              cursor={{ fill: "rgba(255, 255, 255, 0.05)" }}
              contentStyle={{ background: "#141416", border: 0, borderRadius: 10, boxShadow: "var(--shadow)" }}
              labelStyle={{ color: "var(--muted)" }}
            />
            <BarMark yAxisId="exp" dataKey="exp" name="EXP per 10 min" fill="var(--bar)" radius={[4, 4, 0, 0]} isAnimationActive={false} />
            <Line
              yAxisId="rate"
              dataKey="rate"
              name="EXP/hr (60-min avg)"
              stroke="var(--accent)"
              strokeWidth={3}
              dot={{ r: 4, fill: "var(--accent)", strokeWidth: 0 }}
              activeDot={{ r: 5 }}
              connectNulls={false}
              isAnimationActive={false}
            />
          </ComposedChart>
        </ResponsiveContainer>
      </div>
    </section>
  );
}
