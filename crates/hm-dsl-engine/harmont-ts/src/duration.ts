// src/duration.ts
/**
 * Parse a human duration to a positive integer number of seconds.
 *
 * Accepts a Go-style string ("30s", "5m", "1h30m"; units h, m, s) or a
 * number of seconds. Used by `timeout()` and `pipeline({ timeout })`.
 */
const DURATION_RE = /^(?:\d+[hms])+$/;
const SEGMENT_RE = /(\d+)([hms])/g;
const UNIT_SECONDS: Record<string, number> = { h: 3600, m: 60, s: 1 };

export function parseDuration(value: string | number): number {
  let seconds: number;
  if (typeof value === "number") {
    if (!Number.isInteger(value)) {
      throw new Error(
        `hm: timeout duration must be a whole number of seconds — got ${value}`,
      );
    }
    seconds = value;
  } else if (typeof value === "string") {
    seconds = parseStr(value);
  } else {
    throw new Error(
      `hm: timeout duration must be a string or number — got ${typeof value}`,
    );
  }

  if (seconds <= 0) {
    throw new Error(
      `hm: timeout duration must be positive — got ${JSON.stringify(value)}\n` +
        `  → use a value like "30s" or "5m"`,
    );
  }
  return seconds;
}

function parseStr(text: string): number {
  const stripped = text.trim();
  if (!DURATION_RE.test(stripped)) {
    throw new Error(
      `hm: invalid timeout duration ${JSON.stringify(text)}\n` +
        `  → use a Go-style duration like "30s", "5m", or "1h30m" (units: h, m, s)`,
    );
  }
  let total = 0;
  for (const m of stripped.matchAll(SEGMENT_RE)) {
    total += Number(m[1]) * UNIT_SECONDS[m[2]];
  }
  return total;
}
