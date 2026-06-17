import type { Step, StepOptions } from "../step.js";
import { forever, onChange } from "../cache.js";
import {
  makeInstallChain,
  nodeInstallCmd,
  bunInstallCmd,
  denoInstallCmd,
} from "./shared.js";
import { detect } from "./detect.js";

// Runtimes execute JS/TS; package managers install dependencies. `deno` is a
// runtime only — its dependency management is intrinsic, so it is not a `pm`
// value (see makeProject). yarn's classic/berry split is two pm values rather
// than a version axis because the lockfile install flag differs between them.
type Runtime = "node" | "bun" | "deno";
type PackageManager = "npm" | "pnpm" | "yarn-classic" | "yarn-berry" | "bun" | "deno";
type ActionOptions = Omit<StepOptions, "cwd">;

export interface JsOptions {
  readonly path?: string;
  readonly pm?: PackageManager;
  readonly runtime?: Runtime;
  /** Runtime version — Node major ("22"/"22.x") or Bun/Deno semver ("1.2.3").
   *  PM versions are pinned by the project's `packageManager` field. */
  readonly version?: string;
  readonly image?: string;
  readonly base?: Step;
}

const NODE_VERSION_RE = /^[0-9]+(\.x)?$/;
const SEMVER_RE = /^[0-9]+\.[0-9]+(\.[0-9]+)?$/;

const LOCKFILES: Record<PackageManager, string> = {
  npm: "package-lock.json",
  pnpm: "pnpm-lock.yaml",
  "yarn-classic": "yarn.lock",
  "yarn-berry": "yarn.lock",
  bun: "bun.lock",
  deno: "deno.lock",
};

const DEPS_CMD: Record<PackageManager, string> = {
  npm: "npm ci",
  pnpm: "pnpm install --frozen-lockfile",
  "yarn-classic": "yarn install --frozen-lockfile",
  "yarn-berry": "yarn install --immutable",
  bun: "bun install --frozen-lockfile",
  deno: "deno install",
};

const RUN_PREFIX: Record<PackageManager, string> = {
  npm: "npm run",
  pnpm: "pnpm run",
  "yarn-classic": "yarn run",
  "yarn-berry": "yarn run",
  bun: "bun run",
  deno: "deno task",
};

/** Command to bring `pm` onto the runtime image, or null when the PM already
 *  ships with the runtime (npm with node, bun with the bun runtime). */
function pmBootstrap(pm: PackageManager, runtime: Runtime, version?: string): string | null {
  switch (pm) {
    case "npm":
      return null; // bundled with node
    case "bun":
      return runtime === "bun" ? null : bunInstallCmd();
    case "deno":
      return null; // bundled with deno
    case "pnpm":
      return version != null
        ? `corepack enable pnpm && corepack install -g pnpm@${version}`
        : "corepack enable pnpm";
    case "yarn-classic":
    case "yarn-berry":
      return version != null
        ? `corepack enable yarn && corepack install -g yarn@${version}`
        : "corepack enable";
  }
}

export class JsProject {
  readonly path: string;
  private readonly _installed: Step;
  private readonly _runPrefix: string;
  private readonly _tag: string;
  private readonly _pm: PackageManager;

  constructor(path: string, installed: Step, pm: PackageManager, tag: string) {
    this.path = path;
    this._installed = installed;
    this._runPrefix = RUN_PREFIX[pm];
    this._tag = tag;
    this._pm = pm;
  }

  /** The dependency-install step (`npm ci`, `bun install`, `deno install`, …).
   *  Every action attaches to it so installation is shared across CI jobs. */
  install(): Step {
    return this._installed;
  }

  /** Append a post-install command and return an advanced project; chainable.
   *  For prep steps the toolchain's actions must depend on but the SDK does not
   *  model natively (codegen, fixtures, extra tooling). Action methods on the
   *  returned object fork from this step.
   *  @example hm.js.project({ path: "web" }).setup("npm run codegen").run("build") */
  setup(cmd: string, opts?: StepOptions): JsProject {
    return new JsProject(this.path, this._installed.sh(cmd, opts), this._pm, this._tag);
  }

  /** Run a package.json script / deno.json task by name.
   *  This is the uniform action across all PMs — for native tooling
   *  (`deno test`, `bun test`) define a script or drop to `.sh()`. */
  run(script: string, opts?: ActionOptions): Step {
    return this._installed.sh(
      `cd ${this.path} && ${this._runPrefix} ${script}`,
      {
        label: `:${this._tag}: ${script}`,
        ...opts,
      },
    );
  }
}

function validateVersion(runtime: Runtime, version: string): void {
  if (runtime === "node") {
    if (!NODE_VERSION_RE.test(version)) {
      throw new Error(
        `js.project: invalid version "${version}"\n  → use a Node major version like "22" or "22.x"`,
      );
    }
  } else if (!SEMVER_RE.test(version)) {
    throw new Error(
      `js.project: invalid version "${version}"\n  → use a semver version like "1.2" or "1.2.0"`,
    );
  }
}

function makeProject(opts?: JsOptions): JsProject {
  const path = opts?.path ?? ".";
  const detected =
    opts?.runtime == null && opts?.pm == null ? detect(path) : {};
  const runtime = opts?.runtime ?? detected.runtime ?? "node";

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
    const depsInstalled = runtimeInstalled.sh(`cd ${path} && ${DEPS_CMD.deno}`, {
      label: ":deno: deps",
      cache: onChange(`${path}/${LOCKFILES.deno}`),
    });
    return new JsProject(path, depsInstalled, "deno", "deno");
  }

  // --- Node / Bun runtime ---
  const pm: PackageManager = opts?.pm ?? detected.pm ?? (runtime === "bun" ? "bun" : "npm");
  const pmVersion = detected.pmVersion;

  if (pm === "deno") {
    throw new Error(
      'js.project: pm="deno" is not valid — use runtime="deno" instead',
    );
  }

  if (runtime === "bun" && pm !== "bun") {
    throw new Error(`js.project: runtime="bun" only supports pm="bun"`);
  }

  const aptPkgs: string[] = ["curl", "ca-certificates"];
  if (runtime === "bun" || pm === "bun") {
    aptPkgs.push("unzip"); // bun's installer needs unzip
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

  // Layer the package manager onto the runtime image when it isn't bundled.
  const bootstrap = pmBootstrap(pm, runtime, pmVersion);
  const bootstrapCache = pmVersion != null ? onChange(`${path}/package.json`) : forever();
  const pmReady =
    bootstrap == null
      ? runtimeInstalled
      : runtimeInstalled.sh(bootstrap, {
        label: `:${langTag}: ${pm}`,
        cache: bootstrapCache,
      });

  const depsInstalled = pmReady.sh(`cd ${path} && ${DEPS_CMD[pm]}`, {
    label: `:${langTag}: deps`,
    cache: onChange(`${path}/${LOCKFILES[pm]}`),
  });

  return new JsProject(path, depsInstalled, pm, langTag);
}

export const js = { project: makeProject };
export const ts = js;
