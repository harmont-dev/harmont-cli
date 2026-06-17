import type { Step, StepOptions } from "../step.js";
import { forever } from "../cache.js";
import { makeInstallChain } from "./shared.js";

const APT_PACKAGES = ["curl", "ca-certificates", "xz-utils"] as const;
const VERSION_RE = /^[0-9]+\.[0-9]+\.[0-9]+$/;
const NEW_URL_FORMAT_VERSION = [0, 14, 1] as const;

function versionAtLeast(
  version: string,
  min: readonly [number, number, number],
): boolean {
  const parts = version.split(".").map(Number);
  for (let i = 0; i < 3; i++) {
    if ((parts[i] ?? 0) !== min[i]) return (parts[i] ?? 0) > min[i];
  }
  return true;
}

export interface ZigOptions {
  readonly path?: string;
  readonly version?: string;
  readonly image?: string;
  readonly base?: Step;
}

type ActionOptions = Omit<StepOptions, "cwd">;

export class ZigToolchain {
  private readonly _installed: Step;

  constructor(installed: Step) {
    this._installed = installed;
  }

  install(): Step {
    return this._installed;
  }

  /** Append a post-install command and return an advanced toolchain; chainable.
   *  For prep steps the toolchain's actions must depend on but the SDK does not
   *  model natively (codegen, fixtures, extra tooling). Action methods on the
   *  returned object fork from this step.
   *  @example hm.zig().setup("zig build gen").project(".") */
  setup(cmd: string, opts?: StepOptions): ZigToolchain {
    return new ZigToolchain(this._installed.sh(cmd, opts));
  }

  project(path: string = "."): ZigProject {
    return new ZigProject(path, this._installed);
  }
}

export class ZigProject {
  readonly path: string;
  private readonly _installed: Step;

  constructor(path: string, installed: Step) {
    this.path = path;
    this._installed = installed;
  }

  install(): Step {
    return this._installed;
  }

  /** Append a post-install command and return an advanced project; chainable.
   *  For prep steps the toolchain's actions must depend on but the SDK does not
   *  model natively (codegen, fixtures, extra tooling). Action methods on the
   *  returned object fork from this step.
   *  @example hm.zig({ path: "." }).setup("zig build gen").build() */
  setup(cmd: string, opts?: StepOptions): ZigProject {
    return new ZigProject(this.path, this._installed.sh(cmd, opts));
  }

  build(opts?: ActionOptions): Step {
    return this._installed.sh(`cd ${this.path} && zig build`, {
      label: `:zig: ${this.path} build`,
      ...opts,
    });
  }

  test(opts?: ActionOptions): Step {
    return this._installed.sh(`cd ${this.path} && zig build test`, {
      label: `:zig: ${this.path} test`,
      ...opts,
    });
  }

  fmt(opts?: ActionOptions): Step {
    return this._installed.sh(`cd ${this.path} && zig fmt --check .`, {
      label: `:zig: ${this.path} fmt`,
      ...opts,
    });
  }
}

export function zig(opts: ZigOptions & { path: string }): ZigProject;
export function zig(opts?: ZigOptions): ZigToolchain;
export function zig(opts?: ZigOptions): ZigToolchain | ZigProject {
  const version = opts?.version ?? "0.14.1";

  if (!VERSION_RE.test(version)) {
    throw new Error(
      `hm.zig: invalid version "${version}"\n  → use a semver like "0.14.1"`,
    );
  }

  const tarball = versionAtLeast(version, NEW_URL_FORMAT_VERSION)
    ? `zig-x86_64-linux-${version}.tar.xz`
    : `zig-linux-x86_64-${version}.tar.xz`;

  const installCmd = [
    `curl -fsSL https://ziglang.org/download/${version}/${tarball} -o /tmp/zig.tar.xz`,
    "rm -rf /usr/local/zig && mkdir -p /usr/local/zig",
    "tar -xJf /tmp/zig.tar.xz -C /usr/local/zig --strip-components=1",
    "ln -sf /usr/local/zig/zig /usr/local/bin/zig",
    "zig version",
  ].join(" && ");

  const installed = makeInstallChain({
    aptPackages: [...APT_PACKAGES],
    installCmd,
    installCache: forever(),
    langTag: "zig",
    installTag: "install",
    image: opts?.image,
    base: opts?.base,
  });

  const toolchain = new ZigToolchain(installed);
  return opts?.path != null ? toolchain.project(opts.path) : toolchain;
}
