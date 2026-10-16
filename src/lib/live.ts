import type { Bar, Tick } from "./ipc";

export interface LiveRates {
  expPerHour: number | null;
  /** Percent of the current level per hour (2.5 = 2.5%/hr). */
  pctPerHour: number | null;
  hoursToLevel: number | null;
}

export function liveRates(bars: Bar[], tick: Tick | null): LiveRates {
  const rate = bars[bars.length - 1]?.ratePerHour ?? null;
  if (rate === null || rate <= 0) return { expPerHour: null, pctPerHour: null, hoursToLevel: null };
  const total = tick?.totalEst ?? null;
  const exp = tick?.exp ?? null;
  return {
    expPerHour: rate,
    pctPerHour: total ? (rate / total) * 100 : null,
    hoursToLevel: total !== null && exp !== null ? Math.max(0, total - exp) / rate : null,
  };
}
