import type { Step, StepOptions } from "../step.js";
import { type CachePolicy, forever, onChange } from "../cache.js";
import { makeInstallChain } from "./shared.js";
import { type CargoOpts, cargoFlags, shQuote } from "./cargo.js";

const APT_PACKAGES = [
  "curl",
  "ca-certificates",
  "build-essential",
  "pkg-config",
  "libssl-dev",
] as const;
const VERSION_RE = /^[a-z0-9.-]+$/;

export interface RustToolchainOptions {
  readonly path?: string;
  readonly version?: string;
  readonly image?: string;
  readonly components?: readonly string[];
  readonly base?: Step;
}

export interface RustProjectOptions extends RustToolchainOptions {
  readonly cache?: CachePolicy;
}

type ActionOptions = Omit<StepOptions, "cwd">;
type CargoActionOptions = CargoOpts & ActionOptions;

export interface FeaturePowersetOptions extends ActionOptions {
  readonly subcommand?: string;
  readonly depth?: number;
  readonly eachFeature?: boolean;
  readonly noDevDeps?: boolean;
  readonly skip?: readonly string[];
  readonly includeFeatures?: readonly string[];
  readonly keepGoing?: boolean;
  readonly flags?: readonly string[];
}

// --- pure command builders (shared by both classes) ---

function splitCargo(opts: CargoActionOptions | undefined): {
  cargo: CargoOpts;
  step: ActionOptions;
} {
  const o = (opts ?? {}) as CargoActionOptions;
  const {
    workspace,
    packages,
    exclude,
    allFeatures,
    noDefaultFeatures,
    features,
    target,
    allTargets,
    release,
    profile,
    locked,
    flags,
    ...step
  } = o;
  return {
    cargo: {
      workspace,
      packages,
      exclude,
      allFeatures,
      noDefaultFeatures,
      features,
      target,
      allTargets,
      release,
      profile,
      locked,
      flags,
    },
    step: step as ActionOptions,
  };
}

function buildCmd(c: CargoOpts): string {
  return `cargo build${cargoFlags(c)}`;
}
function testCmd(c: CargoOpts, nextest: boolean): string {
  return `${nextest ? "cargo nextest run" : "cargo test"}${cargoFlags(c)}`;
}
function doctestCmd(c: CargoOpts): string {
  return `cargo test${cargoFlags(c)} --doc`;
}
function clippyCmd(
  c: CargoOpts,
  denyWarnings: boolean,
  extraLints: readonly string[],
): string {
  const mid = cargoFlags(c);
  const trail = [...(denyWarnings ? ["-D warnings"] : []), ...extraLints];
  return `cargo clippy${mid}${trail.length ? " -- " + trail.join(" ") : ""}`;
}
function fmtCmd(all: boolean, check: boolean, flags: readonly string[]): string {
  const toks = ["cargo fmt"];
  if (all) toks.push("--all");
  if (check) toks.push("--check");
  toks.push(...flags);
  return toks.join(" ");
}
function docCmd(c: CargoOpts, noDeps: boolean, privateItems: boolean): string {
  const pre: string[] = [];
  if (noDeps) pre.push("--no-deps");
  if (privateItems) pre.push("--document-private-items");
  return `cargo doc${pre.length ? " " + pre.join(" ") : ""}${cargoFlags(c)}`;
}
function hackCmd(o: FeaturePowersetOptions): string {
  const toks = ["cargo hack", o.subcommand ?? "check"];
  if (o.eachFeature) toks.push("--each-feature");
  else toks.push("--feature-powerset", "--depth", String(o.depth ?? 2));
  if (o.noDevDeps ?? true) toks.push("--no-dev-deps");
  if (o.skip?.length) toks.push("--skip " + o.skip.map(shQuote).join(","));
  if (o.includeFeatures?.length)
    toks.push("--include-features " + o.includeFeatures.map(shQuote).join(","));
  if (o.keepGoing) toks.push("--keep-going");
  toks.push(...(o.flags ?? []));
  return toks.join(" ");
}
function denyDocEnv(step: ActionOptions, deny: boolean): ActionOptions {
  if (!deny) return step;
  return { ...step, env: { RUSTDOCFLAGS: "-D warnings", ...(step.env ?? {}) } };
}

function withTargetAdd(cmd: string, target: string | undefined, addTarget: boolean): string {
  return target !== undefined && addTarget
    ? `rustup target add ${shQuote(target)} && ${cmd}`
    : cmd;
}

// --- classes ---

export class RustToolchain {
  readonly path: string;
  private readonly _installed: Step;

  constructor(path: string, installed: Step) {
    this.path = path;
    this._installed = installed;
  }

  install(): Step {
    return this._installed;
  }

  _cargo(cmd: string, label: string, opts?: ActionOptions): Step {
    return this._installed.sh(
      `. $HOME/.cargo/env && cd ${this.path} && ${cmd}`,
      { label, ...opts },
    );
  }

  build(opts?: CargoActionOptions & { addTarget?: boolean }): Step {
    const { addTarget, ...rest } = opts ?? {};
    const { cargo, step } = splitCargo(rest);
    const cmd = withTargetAdd(buildCmd(cargo), cargo.target, addTarget ?? true);
    return this._cargo(cmd, ":rust: build", step);
  }

  test(opts?: CargoActionOptions & { nextest?: boolean; addTarget?: boolean }): Step {
    const { nextest, addTarget, ...rest } = opts ?? {};
    const { cargo, step } = splitCargo(rest);
    const cmd = withTargetAdd(testCmd(cargo, nextest ?? false), cargo.target, addTarget ?? true);
    return this._cargo(cmd, ":rust: test", step);
  }

  doctest(opts?: CargoActionOptions & { addTarget?: boolean }): Step {
    const { addTarget, ...rest } = opts ?? {};
    const { cargo, step } = splitCargo(rest);
    const cmd = withTargetAdd(doctestCmd(cargo), cargo.target, addTarget ?? true);
    return this._cargo(cmd, ":rust: doctest", step);
  }

  clippy(
    opts?: CargoActionOptions & {
      denyWarnings?: boolean;
      extraLints?: readonly string[];
      addTarget?: boolean;
    },
  ): Step {
    const { denyWarnings, extraLints, addTarget, ...rest } = opts ?? {};
    const { cargo, step } = splitCargo({ allTargets: true, ...rest });
    const cmd = withTargetAdd(clippyCmd(cargo, denyWarnings ?? true, extraLints ?? []), cargo.target, addTarget ?? true);
    return this._cargo(cmd, ":rust: clippy", step);
  }

  fmt(
    opts?: ActionOptions & {
      all?: boolean;
      check?: boolean;
      flags?: readonly string[];
    },
  ): Step {
    const { all, check, flags, ...step } = opts ?? {};
    return this._cargo(
      fmtCmd(all ?? true, check ?? true, flags ?? []),
      ":rust: fmt",
      step,
    );
  }

  doc(
    opts?: CargoActionOptions & {
      noDeps?: boolean;
      documentPrivateItems?: boolean;
      denyWarnings?: boolean;
      addTarget?: boolean;
    },
  ): Step {
    const { noDeps, documentPrivateItems, denyWarnings, addTarget, ...rest } = opts ?? {};
    const { cargo, step } = splitCargo(rest);
    const cmd = withTargetAdd(docCmd(cargo, noDeps ?? true, documentPrivateItems ?? false), cargo.target, addTarget ?? true);
    return this._cargo(cmd, ":rust: doc", denyDocEnv(step, denyWarnings ?? true));
  }

  warmup(opts?: ActionOptions): Step {
    return this._cargo(
      "cargo build --workspace --tests --locked",
      ":rust: warmup",
      opts,
    );
  }

  featurePowerset(opts?: FeaturePowersetOptions): Step {
    const {
      subcommand,
      depth,
      eachFeature,
      noDevDeps,
      skip,
      includeFeatures,
      keepGoing,
      flags,
      ...step
    } = opts ?? {};
    // Global install — no crate dir, so don't cd; keeps the forever-cache key
    // identical across toolchains regardless of path.
    const installedHack = this._installed.sh(
      ". $HOME/.cargo/env && cargo install cargo-hack --locked",
      { label: ":rust: install cargo-hack", cache: forever() },
    );
    return installedHack.sh(
      `. $HOME/.cargo/env && cd ${this.path} && ${hackCmd({ subcommand, depth, eachFeature, noDevDeps, skip, includeFeatures, keepGoing, flags })}`,
      { label: ":rust: feature-powerset", ...step },
    );
  }
}

export class RustProject {
  readonly toolchain: RustToolchain;
  readonly warmup: Step;

  constructor(toolchain: RustToolchain, warmup: Step) {
    this.toolchain = toolchain;
    this.warmup = warmup;
  }

  private _emit(cmd: string, label: string, step: ActionOptions): Step {
    return this.warmup.sh(
      `. $HOME/.cargo/env && cd ${this.toolchain.path} && ${cmd}`,
      { label, ...step },
    );
  }

  build(opts?: CargoActionOptions & { addTarget?: boolean }): Step {
    const { addTarget, ...rest } = opts ?? {};
    const { cargo, step } = splitCargo({ workspace: true, ...rest });
    const cmd = withTargetAdd(buildCmd(cargo), cargo.target, addTarget ?? true);
    return this._emit(cmd, ":rust: build", step);
  }

  test(opts?: CargoActionOptions & { nextest?: boolean; addTarget?: boolean }): Step {
    const { nextest, addTarget, ...rest } = opts ?? {};
    const { cargo, step } = splitCargo({ workspace: true, ...rest });
    const cmd = withTargetAdd(testCmd(cargo, nextest ?? false), cargo.target, addTarget ?? true);
    return this._emit(cmd, ":rust: test", step);
  }

  doctest(opts?: CargoActionOptions & { addTarget?: boolean }): Step {
    const { addTarget, ...rest } = opts ?? {};
    const { cargo, step } = splitCargo({ workspace: true, ...rest });
    const cmd = withTargetAdd(doctestCmd(cargo), cargo.target, addTarget ?? true);
    return this._emit(cmd, ":rust: doctest", step);
  }

  clippy(
    opts?: CargoActionOptions & {
      denyWarnings?: boolean;
      extraLints?: readonly string[];
      addTarget?: boolean;
    },
  ): Step {
    const { denyWarnings, extraLints, addTarget, ...rest } = opts ?? {};
    const { cargo, step } = splitCargo({ workspace: true, allTargets: true, ...rest });
    const cmd = withTargetAdd(clippyCmd(cargo, denyWarnings ?? true, extraLints ?? []), cargo.target, addTarget ?? true);
    return this._emit(cmd, ":rust: clippy", step);
  }

  fmt(
    opts?: ActionOptions & {
      all?: boolean;
      check?: boolean;
      flags?: readonly string[];
    },
  ): Step {
    // fmt has no warmup dependency; chain off install like the toolchain does.
    return this.toolchain.fmt(opts);
  }

  doc(
    opts?: CargoActionOptions & {
      noDeps?: boolean;
      documentPrivateItems?: boolean;
      denyWarnings?: boolean;
      addTarget?: boolean;
    },
  ): Step {
    const { noDeps, documentPrivateItems, denyWarnings, addTarget, ...rest } = opts ?? {};
    const { cargo, step } = splitCargo({ workspace: true, ...rest });
    const cmd = withTargetAdd(docCmd(cargo, noDeps ?? true, documentPrivateItems ?? false), cargo.target, addTarget ?? true);
    return this._emit(cmd, ":rust: doc", denyDocEnv(step, denyWarnings ?? true));
  }

  featurePowerset(opts?: FeaturePowersetOptions): Step {
    return this.toolchain.featurePowerset(opts);
  }

  ci(opts?: { nextest?: boolean; doc?: boolean }): Step[] {
    const nextest = opts?.nextest ?? false;
    const steps: Step[] = [this.test({ nextest })];
    if (nextest) steps.push(this.doctest());
    steps.push(this.clippy());
    steps.push(this.fmt());
    if (opts?.doc) steps.push(this.doc());
    return steps;
  }
}

function makeToolchain(opts?: RustToolchainOptions): RustToolchain {
  const path = opts?.path ?? ".";
  const version = opts?.version ?? "stable";
  const components = opts?.components ?? ["clippy", "rustfmt"];

  if (!VERSION_RE.test(version)) {
    throw new Error(
      `rust.toolchain: invalid version "${version}"\n  → use "stable", "nightly", or a semver like "1.81.0"`,
    );
  }

  const componentFlag =
    components.length > 0 ? ` --component ${components.join(",")}` : "";
  const installCmd = [
    `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain ${version} --profile minimal${componentFlag}`,
    `. $HOME/.cargo/env && rustc --version && cargo --version`,
  ].join(" && ");

  const installed = makeInstallChain({
    aptPackages: [...APT_PACKAGES],
    installCmd,
    installCache: forever(),
    langTag: "rust",
    installTag: "rustup",
    image: opts?.image,
    base: opts?.base,
  });

  return new RustToolchain(path, installed);
}

function makeProject(opts?: RustProjectOptions): RustProject {
  const path = opts?.path ?? ".";
  const tc = makeToolchain(opts);

  const lockPath = path !== "." ? `${path}/Cargo.lock` : "Cargo.lock";
  const tomlGlob = path !== "." ? `${path}/**/Cargo.toml` : "**/Cargo.toml";
  const rsGlob = path !== "." ? `${path}/**/*.rs` : "**/*.rs";
  const warmupCache = opts?.cache ?? onChange(lockPath, tomlGlob, rsGlob);

  const warm = tc._cargo(
    "cargo build --workspace --tests --locked",
    ":rust: warmup",
    { cache: warmupCache },
  );

  return new RustProject(tc, warm);
}

export const rust = {
  toolchain: makeToolchain,
  project: makeProject,
};
