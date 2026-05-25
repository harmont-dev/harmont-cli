import type { Step, StepOptions } from "../step.js";
import { type CachePolicy, forever, onChange } from "../cache.js";
import { makeInstallChain } from "./shared.js";

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

  build(opts?: ActionOptions & { release?: boolean }): Step {
    const cmd = opts?.release ? "cargo build --release" : "cargo build";
    return this._cargo(cmd, ":rust: build", opts);
  }

  test(opts?: ActionOptions & { release?: boolean }): Step {
    const cmd = opts?.release ? "cargo test --release" : "cargo test";
    return this._cargo(cmd, ":rust: test", opts);
  }

  clippy(opts?: ActionOptions): Step {
    return this._cargo(
      "cargo clippy --all-targets -- -D warnings",
      ":rust: clippy",
      opts,
    );
  }

  fmt(opts?: ActionOptions): Step {
    return this._cargo("cargo fmt --check", ":rust: fmt", opts);
  }

  doc(opts?: ActionOptions): Step {
    return this._cargo("cargo doc --no-deps", ":rust: doc", opts);
  }

  warmup(opts?: ActionOptions): Step {
    return this._cargo(
      "cargo build --workspace --tests --locked",
      ":rust: warmup",
      opts,
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

  test(opts?: { flags?: readonly string[] } & ActionOptions): Step {
    const extra = opts?.flags?.length ? " " + opts.flags.join(" ") : "";
    return this.warmup.sh(
      `. $HOME/.cargo/env && cd ${this.toolchain.path} && cargo test --workspace --locked${extra}`,
      { label: ":rust: test", ...opts },
    );
  }

  clippy(opts?: { flags?: readonly string[] } & ActionOptions): Step {
    const extra = opts?.flags?.length ? " " + opts.flags.join(" ") : "";
    return this.warmup.sh(
      `. $HOME/.cargo/env && cd ${this.toolchain.path} && cargo clippy --workspace --tests --locked${extra} -- -D warnings`,
      { label: ":rust: clippy", ...opts },
    );
  }

  fmt(opts?: { flags?: readonly string[] } & ActionOptions): Step {
    const extra = opts?.flags?.length ? " " + opts.flags.join(" ") : "";
    return this.toolchain._cargo(
      `cargo fmt --check${extra}`,
      ":rust: fmt",
      opts,
    );
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
  const warmupCache = opts?.cache ?? onChange(lockPath);

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
