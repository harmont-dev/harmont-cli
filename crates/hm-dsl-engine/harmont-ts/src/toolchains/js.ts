import type { Step, StepOptions } from "../step.js";
import { forever, onChange } from "../cache.js";
import {
  makeInstallChain,
  nodeInstallCmd,
  bunInstallCmd,
  denoInstallCmd,
} from "./shared.js";

type Pm = "npm" | "pnpm" | "bun";
type Runtime = "node" | "bun" | "deno";
type ActionOptions = Omit<StepOptions, "cwd">;

export interface JsOptions {
  readonly path?: string;
  readonly pm?: Pm;
  readonly runtime?: Runtime;
  readonly version?: string;
  readonly image?: string;
  readonly base?: Step;
}

const NODE_VERSION_RE = /^[0-9]+(\.x)?$/;
const SEMVER_RE = /^[0-9]+\.[0-9]+(\.[0-9]+)?$/;

const LOCKFILES: Record<Pm, string> = {
  npm: "package-lock.json",
  pnpm: "pnpm-lock.yaml",
  bun: "bun.lock",
};

function depsCmd(pm: Pm, path: string): string {
  switch (pm) {
    case "npm":
      return `cd ${path} && npm ci`;
    case "pnpm":
      return `cd ${path} && pnpm install --frozen-lockfile`;
    case "bun":
      return `cd ${path} && bun install --frozen-lockfile`;
  }
}

function runPrefixFor(pm: Pm | "deno"): string {
  switch (pm) {
    case "npm":
      return "npm run";
    case "pnpm":
      return "pnpm run";
    case "bun":
      return "bun run";
    case "deno":
      return "deno task";
  }
}

export class JsProject {
  readonly path: string;
  private readonly _installed: Step;
  private readonly _runPrefix: string;
  private readonly _tag: string;

  constructor(path: string, installed: Step, pm: Pm | "deno", tag: string) {
    this.path = path;
    this._installed = installed;
    this._runPrefix = runPrefixFor(pm);
    this._tag = tag;
  }

  install(): Step {
    return this._installed;
  }

  run(script: string, opts?: ActionOptions): Step {
    return this._installed.sh(
      `cd ${this.path} && ${this._runPrefix} ${script}`,
      {
        label: `:${this._tag}: ${script}`,
        ...opts,
      },
    );
  }

  test(opts?: ActionOptions): Step {
    return this.run("test", opts);
  }

  build(opts?: ActionOptions): Step {
    return this.run("build", opts);
  }

  lint(opts?: ActionOptions): Step {
    return this.run("lint", opts);
  }

  fmt(opts?: ActionOptions): Step {
    return this.run("fmt", opts);
  }

  typecheck(opts?: ActionOptions): Step {
    return this.run("typecheck", opts);
  }
}

function validateVersion(runtime: Runtime, version: string): void {
  if (runtime === "node") {
    if (!NODE_VERSION_RE.test(version)) {
      throw new Error(
        `js.project: invalid version "${version}"\n  → use a Node major version like "22" or "22.x"`,
      );
    }
  } else {
    if (!SEMVER_RE.test(version)) {
      throw new Error(
        `js.project: invalid version "${version}"\n  → use a semver version like "1.2" or "1.2.0"`,
      );
    }
  }
}

function makeProject(opts?: JsOptions): JsProject {
  const path = opts?.path ?? ".";
  const runtime = opts?.runtime ?? "node";

  if (opts?.version != null) {
    validateVersion(runtime, opts.version);
  }

  // --- Deno: built-in PM, no pm option ---
  if (runtime === "deno") {
    if (opts?.pm != null) {
      throw new Error(
        `js.project: runtime="deno" manages its own dependencies — do not set pm`,
      );
    }
    const runtimeInstalled = makeInstallChain({
      aptPackages: ["curl", "ca-certificates", "unzip"],
      installCmd: denoInstallCmd(opts?.version),
      installCache: forever(),
      langTag: "deno",
      installTag: "install",
      image: opts?.image,
      base: opts?.base,
    });
    const depsInstalled = runtimeInstalled.sh(`cd ${path} && deno install`, {
      label: ":deno: deps",
      cache: onChange(`${path}/deno.lock`),
    });
    return new JsProject(path, depsInstalled, "deno", "deno");
  }

  // --- Node / Bun runtime ---
  const pm: Pm = opts?.pm ?? (runtime === "bun" ? "bun" : "npm");

  if ((pm === "npm" || pm === "pnpm") && runtime !== "node") {
    throw new Error(`js.project: pm="${pm}" requires runtime="node"`);
  }

  const aptPkgs: string[] = ["curl", "ca-certificates"];
  if (runtime === "bun" || pm === "bun") {
    aptPkgs.push("unzip");
  }

  const langTag = runtime === "bun" ? "bun" : "node";
  const runtimeCmd =
    runtime === "bun"
      ? bunInstallCmd(opts?.version)
      : nodeInstallCmd(opts?.version ?? "22");

  const runtimeInstalled = makeInstallChain({
    aptPackages: aptPkgs,
    installCmd: runtimeCmd,
    installCache: forever(),
    langTag,
    installTag: "install",
    image: opts?.image,
    base: opts?.base,
  });

  let pmReady: Step;
  if (runtime === "node" && pm === "npm") {
    pmReady = runtimeInstalled;
  } else if (runtime === "bun" && pm === "bun") {
    pmReady = runtimeInstalled;
  } else if (pm === "pnpm") {
    pmReady = runtimeInstalled.sh("npm install -g pnpm", {
      label: `:${langTag}: pnpm`,
      cache: forever(),
    });
  } else if (pm === "bun" && runtime === "node") {
    pmReady = runtimeInstalled.sh(bunInstallCmd(), {
      label: `:${langTag}: bun`,
      cache: forever(),
    });
  } else {
    pmReady = runtimeInstalled;
  }

  const lockfile = LOCKFILES[pm];
  const depsInstalled = pmReady.sh(depsCmd(pm, path), {
    label: `:${langTag}: deps`,
    cache: onChange(`${path}/${lockfile}`),
  });

  return new JsProject(path, depsInstalled, pm, langTag);
}

export const js = { project: makeProject };
export const ts = js;
