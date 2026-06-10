import { describe, expect, it, beforeEach, afterEach } from "vitest";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import {
  detectFromPackageJson,
  detectFromLockfiles,
  detect,
} from "../../src/toolchains/detect.js";

// ---------------------------------------------------------------------------
// detectFromPackageJson
// ---------------------------------------------------------------------------

describe("detectFromPackageJson", () => {
  it("returns empty for empty object", () => {
    expect(detectFromPackageJson({})).toEqual({});
  });

  it("detects runtime=node from engines.node", () => {
    expect(detectFromPackageJson({ engines: { node: ">=18" } })).toEqual({
      runtime: "node",
    });
  });

  it("detects runtime=bun and pm=bun from engines.bun", () => {
    expect(detectFromPackageJson({ engines: { bun: ">=1.0" } })).toEqual({
      runtime: "bun",
      pm: "bun",
    });
  });

  it("detects runtime=deno from engines.deno", () => {
    expect(detectFromPackageJson({ engines: { deno: ">=2.0" } })).toEqual({
      runtime: "deno",
    });
  });

  it("detects pm=pnpm from packageManager field", () => {
    expect(
      detectFromPackageJson({ packageManager: "pnpm@8.15.4" }),
    ).toEqual({ pm: "pnpm", pmVersion: "8.15.4" });
  });

  it("detects pm=bun from packageManager field", () => {
    expect(detectFromPackageJson({ packageManager: "bun@1.1.0" })).toEqual({
      pm: "bun",
      pmVersion: "1.1.0",
    });
  });

  it("detects pm=npm from packageManager field", () => {
    expect(
      detectFromPackageJson({ packageManager: "npm@10.2.4" }),
    ).toEqual({ pm: "npm", pmVersion: "10.2.4" });
  });

  it("detects yarn-classic from packageManager yarn@1.x", () => {
    expect(
      detectFromPackageJson({ packageManager: "yarn@1.22.22" }),
    ).toEqual({ pm: "yarn-classic", pmVersion: "1.22.22" });
  });

  it("detects yarn-berry from packageManager yarn@4.x", () => {
    expect(
      detectFromPackageJson({ packageManager: "yarn@4.0.0" }),
    ).toEqual({ pm: "yarn-berry", pmVersion: "4.0.0" });
  });

  it("engines.bun overrides packageManager for pm", () => {
    expect(
      detectFromPackageJson({
        engines: { bun: ">=1.0" },
        packageManager: "pnpm@8",
      }),
    ).toEqual({ runtime: "bun", pm: "bun" });
  });

  it("engines.node + packageManager=pnpm both contribute", () => {
    expect(
      detectFromPackageJson({
        engines: { node: ">=18" },
        packageManager: "pnpm@8",
      }),
    ).toEqual({ runtime: "node", pm: "pnpm", pmVersion: "8" });
  });

  it("omits pmVersion when packageManager has no @version", () => {
    expect(
      detectFromPackageJson({ packageManager: "pnpm" }),
    ).toEqual({ pm: "pnpm" });
  });
});

// ---------------------------------------------------------------------------
// detectFromLockfiles
// ---------------------------------------------------------------------------

describe("detectFromLockfiles", () => {
  it("returns empty for no files", () => {
    expect(detectFromLockfiles([])).toEqual({});
  });

  it("detects bun from bun.lock", () => {
    expect(detectFromLockfiles(["bun.lock"])).toEqual({
      pm: "bun",
      runtime: "bun",
    });
  });

  it("detects bun from bun.lockb (legacy binary format)", () => {
    expect(detectFromLockfiles(["bun.lockb"])).toEqual({
      pm: "bun",
      runtime: "bun",
    });
  });

  it("detects pnpm from pnpm-lock.yaml", () => {
    expect(detectFromLockfiles(["pnpm-lock.yaml"])).toEqual({ pm: "pnpm" });
  });

  it("detects deno from deno.lock", () => {
    expect(detectFromLockfiles(["deno.lock"])).toEqual({ runtime: "deno" });
  });

  it("detects npm from package-lock.json", () => {
    expect(detectFromLockfiles(["package-lock.json"])).toEqual({
      pm: "npm",
    });
  });

  it("detects yarn-classic from yarn.lock", () => {
    expect(detectFromLockfiles(["yarn.lock"])).toEqual({ pm: "yarn-classic" });
  });

  it("bun.lock takes priority over package-lock.json", () => {
    expect(
      detectFromLockfiles(["package-lock.json", "bun.lock"]),
    ).toEqual({ pm: "bun", runtime: "bun" });
  });
});

// ---------------------------------------------------------------------------
// detect (filesystem integration)
// ---------------------------------------------------------------------------

describe("detect", () => {
  let tmp: string;

  beforeEach(() => {
    tmp = mkdtempSync(join(tmpdir(), "hm-detect-"));
  });

  afterEach(() => {
    rmSync(tmp, { recursive: true, force: true });
  });

  it("returns empty for directory with no package.json or lockfiles", () => {
    expect(detect(tmp)).toEqual({});
  });

  it("detects from package.json engines", () => {
    writeFileSync(
      join(tmp, "package.json"),
      JSON.stringify({ engines: { bun: ">=1.0" } }),
    );
    expect(detect(tmp)).toEqual({ runtime: "bun", pm: "bun" });
  });

  it("detects from lockfile", () => {
    writeFileSync(join(tmp, "pnpm-lock.yaml"), "");
    expect(detect(tmp)).toEqual({ pm: "pnpm" });
  });

  it("package.json pm takes priority over lockfile pm", () => {
    writeFileSync(
      join(tmp, "package.json"),
      JSON.stringify({ packageManager: "pnpm@8" }),
    );
    writeFileSync(join(tmp, "bun.lock"), "");
    const result = detect(tmp);
    expect(result.pm).toBe("pnpm");
    expect(result.runtime).toBe("bun");
    expect(result.pmVersion).toBe("8");
  });

  it("merges package.json runtime with lockfile pm", () => {
    writeFileSync(
      join(tmp, "package.json"),
      JSON.stringify({ engines: { node: ">=18" } }),
    );
    writeFileSync(join(tmp, "pnpm-lock.yaml"), "");
    expect(detect(tmp)).toEqual({ runtime: "node", pm: "pnpm" });
  });

  it("returns empty for nonexistent path", () => {
    expect(detect(join(tmp, "does-not-exist"))).toEqual({});
  });

  it("detects yarn-berry from packageManager field", () => {
    writeFileSync(
      join(tmp, "package.json"),
      JSON.stringify({ packageManager: "yarn@4.5.0" }),
    );
    writeFileSync(join(tmp, "yarn.lock"), "");
    expect(detect(tmp)).toEqual({ pm: "yarn-berry", pmVersion: "4.5.0" });
  });

  it("passes pmVersion through from package.json", () => {
    writeFileSync(
      join(tmp, "package.json"),
      JSON.stringify({ packageManager: "pnpm@9.1.0" }),
    );
    const result = detect(tmp);
    expect(result.pmVersion).toBe("9.1.0");
  });

  it("omits pmVersion when no packageManager field", () => {
    writeFileSync(join(tmp, "pnpm-lock.yaml"), "");
    const result = detect(tmp);
    expect(result.pmVersion).toBeUndefined();
  });
});
