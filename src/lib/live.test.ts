import { describe, expect, it } from "vitest";
import { liveRates } from "./live";
import type { Tick } from "./ipc";

const tick: Tick = { status: { kind: "ok" }, mapName: "A", exp: 250_000, pctBp: 2500, totalEst: 1_000_000 };

describe("liveRates", () => {
  it("derives %/hr and time to level from the latest bar", () => {
    const r = liveRates([{ start: 0, expGained: 1, ratePerHour: 100_000 }], tick);
    expect(r).toEqual({ expPerHour: 100_000, pctPerHour: 10, hoursToLevel: 7.5 });
  });

  it("returns nulls without a rate", () => {
    expect(liveRates([], tick)).toEqual({ expPerHour: null, pctPerHour: null, hoursToLevel: null });
    expect(liveRates([{ start: 0, expGained: 0, ratePerHour: null }], null)).toEqual({ expPerHour: null, pctPerHour: null, hoursToLevel: null });
  });
});
