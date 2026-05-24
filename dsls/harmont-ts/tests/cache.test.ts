import { describe, expect, it } from "vitest";
import { forever, ttl, onChange, compose, type CachePolicy } from "../src/cache.js";

describe("forever", () => {
  it("creates a forever policy with no env keys", () => {
    const p = forever();
    expect(p).toEqual({ kind: "forever", envKeys: [] });
  });

  it("accepts env keys", () => {
    const p = forever({ envKeys: ["NODE_ENV"] });
    expect(p.envKeys).toEqual(["NODE_ENV"]);
  });
});

describe("ttl", () => {
  it("creates a ttl policy with duration in seconds", () => {
    const p = ttl(3600);
    expect(p).toEqual({ kind: "ttl", durationSeconds: 3600, envKeys: [] });
  });

  it("accepts env keys", () => {
    const p = ttl(86400, { envKeys: ["CI"] });
    expect(p.envKeys).toEqual(["CI"]);
  });
});

describe("onChange", () => {
  it("creates an on_change policy with paths", () => {
    const p = onChange("src/", "package.json");
    expect(p).toEqual({ kind: "on_change", paths: ["src/", "package.json"] });
  });
});

describe("compose", () => {
  it("composes multiple policies", () => {
    const p = compose(ttl(86400), onChange("src/"));
    expect(p.kind).toBe("compose");
    expect(p.policies).toHaveLength(2);
    expect(p.policies[0].kind).toBe("ttl");
    expect(p.policies[1].kind).toBe("on_change");
  });
});

describe("type discrimination", () => {
  it("kind field enables type narrowing", () => {
    const p: CachePolicy = forever();
    switch (p.kind) {
      case "forever":
        expect(p.envKeys).toEqual([]);
        break;
      default:
        throw new Error("unexpected kind");
    }
  });
});
