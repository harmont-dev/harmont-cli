import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

type Runtime = "node" | "bun" | "deno";
type Pm = "npm" | "pnpm" | "bun";

export interface DetectedToolchain {
  runtime?: Runtime;
  pm?: Pm;
}

export function detectFromPackageJson(
  packageJson: Record<string, unknown>,
): DetectedToolchain {
  const result: DetectedToolchain = {};

  const engines = packageJson.engines;
  if (engines != null && typeof engines === "object") {
    const eng = engines as Record<string, unknown>;
    if ("bun" in eng) {
      result.runtime = "bun";
      result.pm = "bun";
    } else if ("deno" in eng) {
      result.runtime = "deno";
    } else if ("node" in eng) {
      result.runtime = "node";
    }
  }

  if (result.pm == null) {
    const pmField = packageJson.packageManager;
    if (typeof pmField === "string") {
      const name = pmField.split("@")[0];
      if (name === "pnpm") result.pm = "pnpm";
      else if (name === "bun") result.pm = "bun";
      else if (name === "npm") result.pm = "npm";
    }
  }

  return result;
}

export function detectFromLockfiles(
  files: readonly string[],
): DetectedToolchain {
  const set = new Set(files);

  if (set.has("bun.lock") || set.has("bun.lockb")) {
    return { pm: "bun", runtime: "bun" };
  }
  if (set.has("pnpm-lock.yaml")) {
    return { pm: "pnpm" };
  }
  if (set.has("deno.lock")) {
    return { runtime: "deno" };
  }
  if (set.has("package-lock.json")) {
    return { pm: "npm" };
  }

  return {};
}

export function detect(path: string): DetectedToolchain {
  let fromPkg: DetectedToolchain = {};
  try {
    const raw = readFileSync(join(path, "package.json"), "utf8");
    fromPkg = detectFromPackageJson(JSON.parse(raw));
  } catch {
    // no package.json or invalid JSON
  }

  let fromLock: DetectedToolchain = {};
  try {
    fromLock = detectFromLockfiles(readdirSync(path));
  } catch {
    // directory unreadable
  }

  const result: DetectedToolchain = {};
  const runtime = fromPkg.runtime ?? fromLock.runtime;
  const pm = fromPkg.pm ?? fromLock.pm;
  if (runtime != null) result.runtime = runtime;
  if (pm != null) result.pm = pm;
  return result;
}
