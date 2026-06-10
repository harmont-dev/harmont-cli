// tests/duration.test.ts
import { describe, expect, it } from "vitest";

import { parseDuration } from "../src/duration.js";

describe("parseDuration", () => {
  it.each([
    ["30s", 30],
    ["5m", 300],
    ["1h", 3600],
    ["1h30m", 5400],
    ["2h15m30s", 8130],
    [45, 45],
  ])("parses %s -> %d", (value, expected) => {
    expect(parseDuration(value as string | number)).toBe(expected);
  });

  it.each(["", "30", "30 s", "1d", "m", "-5s", "0s", 0, -3, NaN])(
    "rejects %s",
    (bad) => {
      expect(() => parseDuration(bad as string | number)).toThrow();
    },
  );
});
