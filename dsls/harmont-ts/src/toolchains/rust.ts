import type { Step, StepOptions } from "../step.js";
import { forever } from "../cache.js";
import { makeInstallChain } from "./shared.js";

const APT_PACKAGES = [
  "curl",
  "ca-certificates",
  "build-essential",
  "pkg-config",
  "libssl-dev",
] as const;
const VERSION_RE = /^[a-z0-9.-]+$/;

export interface RustOptions {
  readonly path?: string;
  readonly version?: string;
  readonly image?: string;
  readonly components?: readonly string[];
  readonly base?: Step;
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

  private _cargo(cmd: string, label: string, opts?: ActionOptions): Step {
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
}

export function rust(opts?: RustOptions): RustToolchain {
  const path = opts?.path ?? ".";
  const version = opts?.version ?? "stable";
  const components = opts?.components ?? ["clippy", "rustfmt"];

  if (!VERSION_RE.test(version)) {
    throw new Error(
      `hm.rust: invalid version "${version}"\n  → use "stable", "nightly", or a semver like "1.81.0"`,
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
