import { describe, expect, it } from "vitest";
import { BarSchema, RegionPreviewSchema, TickSchema, toAppError } from "./ipc";

describe("ipc schemas", () => {
  it("parses a tick as Rust serializes it", () => {
    const tick = { status: { kind: "ok" }, mapName: "Victoria Road Perion", exp: 326, pctBp: 10, totalEst: 326000 };
    expect(TickSchema.parse(tick)).toEqual(tick);
    const failed = { ...tick, status: { kind: "captureFailed", detail: "no monitor" } };
    expect(TickSchema.parse(failed).status).toEqual({ kind: "captureFailed", detail: "no monitor" });
  });

  it("rejects unknown statuses and bad numbers", () => {
    expect(TickSchema.safeParse({ status: { kind: "nope" }, mapName: null, exp: null, pctBp: null, totalEst: null }).success).toBe(false);
    expect(BarSchema.safeParse({ start: 0, expGained: -1, ratePerHour: null }).success).toBe(false);
  });

  it("parses previews", () => {
    const p = { exp: { kind: "ok", value: { exp: 326, pctBp: 10 } }, map: { kind: "failed", reason: "region contains no text" } };
    expect(RegionPreviewSchema.parse(p)).toEqual(p);
  });

  it("maps rejections to AppError", () => {
    expect(toAppError({ kind: "notFound", message: "map 3" })).toEqual({ kind: "notFound", message: "map 3" });
    expect(toAppError(new Error("boom"))).toEqual({ kind: "ipc", message: "Error: boom" });
  });
});
