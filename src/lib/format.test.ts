import { describe, expect, it } from "vitest";
import { formatExp, formatHours, formatPct, statusText } from "./format";

describe("format", () => {
  it("formats values", () => {
    expect(formatPct(10)).toBe("0.10%");
    expect(formatHours(1.5)).toBe("1h 30m");
    expect(formatHours(0.25)).toBe("15m");
    expect(formatHours(-1)).toBe("—");
    expect(formatExp(1_234_567)).toBe("1.23M");
  });

  it("describes statuses", () => {
    expect(statusText({ kind: "ok" })).toBe("Reading OK");
    expect(statusText({ kind: "storageFailed", detail: "disk full" })).toContain("disk full");
  });
});
