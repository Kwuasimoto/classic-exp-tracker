import { Bar as BarMark, ComposedChart, Legend, Line, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import type { Bar } from "../lib/ipc";
import { formatClock, formatExp } from "../lib/format";

export function RateChart({ bars }: { bars: Bar[] }) {
  const data = bars.map((b) => ({ label: formatClock(b.start), exp: b.expGained, rate: b.ratePerHour }));
  const tick = (v: unknown) => (typeof v === "number" ? formatExp(v) : String(v));
  return (
    <ResponsiveContainer width="100%" height={300}>
      <ComposedChart data={data} margin={{ top: 8, right: 8, bottom: 0, left: 0 }}>
        <XAxis dataKey="label" stroke="var(--muted)" />
        <YAxis yAxisId="exp" stroke="var(--muted)" tickFormatter={tick} />
        <YAxis yAxisId="rate" orientation="right" stroke="var(--muted)" tickFormatter={tick} />
        <Tooltip formatter={tick} contentStyle={{ background: "var(--surface)", borderColor: "var(--border)" }} />
        <Legend />
        <BarMark yAxisId="exp" dataKey="exp" name="EXP per 10 min" fill="var(--bar)" />
        <Line yAxisId="rate" dataKey="rate" name="EXP/hr (60-min avg)" stroke="var(--line)" strokeWidth={2} dot={false} connectNulls={false} />
      </ComposedChart>
    </ResponsiveContainer>
  );
}
