// tests/timeout.test.ts
import { describe, expect, it } from "vitest";

import { sh, timeout, wait } from "../src/index.js";

describe("timeout", () => {
  it("sets the timeout in seconds on a step", () => {
    const step = timeout("30s", sh("echo foo"));
    expect(step._timeoutSeconds).toBe(30);
    expect(step._cmd).toBe("echo foo");
  });

  it("accepts a number of seconds", () => {
    expect(timeout(45, sh("x"))._timeoutSeconds).toBe(45);
  });

  it("does not mutate the original and last-wins on re-wrap", () => {
    const base = sh("x");
    const wrapped = timeout("5m", base);
    expect(base._timeoutSeconds).toBeUndefined();
    expect(timeout("1m", wrapped)._timeoutSeconds).toBe(60);
  });

  it("rejects wrapping a wait barrier", () => {
    expect(() => timeout("30s", wait())).toThrow(/wait/);
  });
});
