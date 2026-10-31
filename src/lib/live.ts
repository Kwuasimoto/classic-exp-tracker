import type { LedgerLine } from "../components/Ledger";
import { formatClock, formatWhole } from "./format";
import type { Bar, Loot, LootStatus, Tick } from "./ipc";

export interface LiveRates {
  expPerHour: number | null;
  /** Percent of the current level per hour (2.5 = 2.5%/hr). */
  pctPerHour: number | null;
  hoursToLevel: number | null;
}

/** The map lock, as the ledgers show it: which map is counted, and whether the player is
 *  somewhere else (another map, or a shop), where nothing counts. */
export interface Lock {
  name: string;
  away: boolean;
}

export function lockOf(tick: Tick | null): Lock | null {
  if (tick?.lockedMap == null) return null;
  return { name: tick.lockedName ?? "the locked map", away: tick.shopping || tick.mapId !== tick.lockedMap };
}

/** EXP left in the level; exact once the level is known and the counter is on screen, else
 *  an estimate (past 1,000,000 EXP the game shows only the percent). */
export function expToLevel(tick: Tick | null): { exp: number; exact: boolean } | null {
  if (tick?.totalEst == null || tick.exp == null) return null;
  return { exp: Math.max(0, tick.totalEst - tick.exp), exact: tick.level !== null && tick.expExact };
}

/** Mesos per hour of training over the last 60 minutes; profit is what's made after potions. */
export interface MesoRates {
  profit: number;
  meso: number;
  etc: number;
  potions: number;
}

export function mesoRates(bars: Bar[]): MesoRates | null {
  const r = bars[bars.length - 1]?.lootPerHour ?? null;
  if (r === null) return null;
  return { profit: r.meso + r.etcValue - r.potionCost, meso: r.meso, etc: r.etcValue, potions: r.potionCost };
}

/** The inventory unseen this long (s) and the loot rates count as held: a frame or two
 *  without it (the cursor over it) isn't worth mentioning. */
const HELD_AFTER_SECS = 90;

/** While the inventory is closed the loot rates hold at the last time it was seen
 *  (rates::bars_range): that time, or null when they're current. */
export function lootHeldSince(loot: LootStatus | null): number | null {
  if (!loot?.enabled || loot.seenAt === null) return null;
  return Date.now() / 1000 - loot.seenAt > HELD_AFTER_SECS ? loot.seenAt : null;
}

/** The Profit/hr headline's caption: "after potions", or since when it's held. */
export const profitCaption = (held: number | null): string => (held === null ? "after potions" : `held since ${formatClock(held)}`);

/** The ledger's line while the loot rates are held. */
export const HELD_WARNING = "Inventory closed: Profit/hr holds until you open it on the Etc tab";

/** Which potions count as used: the main HP and MP items, named. */
export function potionsCounted(loot: LootStatus | null): string {
  const chosen = [loot?.mainHp, loot?.mainMp].filter((p) => p != null);
  if (chosen.length === 0) return "choose your pots under Live › Active pots";
  return chosen.map((p) => p.name ?? "unnamed item").join(" · ");
}

/** The ledger lines of mesos per hour of training (Live, Mesos). */
export function mesoLines(rates: MesoRates | null, loot: LootStatus | null): LedgerLine[] {
  // A sign only on something: nothing burned reads "0", not "−0".
  const perHour = (n: number | undefined, sign: "+" | "−") => (n === undefined ? "—" : Math.round(n) === 0 ? "0" : `${sign}${formatWhole(n)}`);
  return [
    { label: "Picked up", value: perHour(rates?.meso, "+") },
    { label: "Etc at NPC prices", value: perHour(rates?.etc, "+") },
    { label: "Potions burned", note: potionsCounted(loot), value: perHour(rates?.potions, "−"), tone: "spent" },
  ];
}

/** What a bucket or snapshot made after potions (can be negative). */
export const profitOf = (loot: Loot) => loot.meso + loot.etcValue - loot.potionCost;

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
