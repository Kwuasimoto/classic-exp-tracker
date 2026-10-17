import type { Bar, Tick } from "./ipc";

export interface LiveRates {
  expPerHour: number | null;
  /** Percent of the current level per hour (2.5 = 2.5%/hr). */
  pctPerHour: number | null;
  hoursToLevel: number | null;
}

/** EXP left in the level; exact once the level is known, else an estimate. */
export function expToLevel(tick: Tick | null): { exp: number; exact: boolean } | null {
  if (tick?.totalEst == null || tick.exp == null) return null;
  return { exp: Math.max(0, tick.totalEst - tick.exp), exact: tick.level !== null };
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
