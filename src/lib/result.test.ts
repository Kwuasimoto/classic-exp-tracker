import { describe, expect, it } from "vitest";
import { err, ok } from "./result";

describe("result", () => {
  it("builds success and failure values", () => {
    expect(ok(3)).toEqual({ success: true, data: 3 });
    expect(err("x")).toEqual({ success: false, error: "x" });
  });
});
