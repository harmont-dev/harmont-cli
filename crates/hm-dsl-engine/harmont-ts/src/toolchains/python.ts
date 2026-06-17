import type { Step, StepOptions } from "../step.js";
import { forever, onChange } from "../cache.js";
import { makeInstallChain } from "./shared.js";

const APT_PACKAGES = [
  "curl",
  "ca-certificates",
  "python3",
  "python3-venv",
] as const;
const VERSION_RE = /^([0-9]+\.[0-9]+\.[0-9]+|latest)$/;

function resolvePaths(paths?: string | string[]): string {
  if (paths == null) return ".";
  return Array.isArray(paths) ? paths.join(" ") : paths;
}

export interface PythonOptions {
  readonly path?: string;
  readonly uvVersion?: string;
  readonly image?: string;
  readonly base?: Step;
}

type ActionOptions = Omit<StepOptions, "cwd">;

export class PythonToolchain {
  readonly path: string;
  private readonly _installed: Step;

  constructor(path: string, installed: Step) {
    this.path = path;
    this._installed = installed;
  }

  install(): Step {
    return this._installed;
  }

  /** Append a post-install command and return an advanced toolchain; chainable.
   *  For prep steps the toolchain's actions must depend on but the SDK does not
   *  model natively (codegen, fixtures, extra tooling). Action methods on the
   *  returned object fork from this step.
   *  @example hm.python({ path: "." }).setup("uv run python gen.py").test() */
  setup(cmd: string, opts?: StepOptions): PythonToolchain {
    return new PythonToolchain(this.path, this._installed.sh(cmd, opts));
  }

  test(opts?: ActionOptions): Step {
    return this._installed.sh(`cd ${this.path} && uv run pytest`, {
      label: ":python: test",
      ...opts,
    });
  }

  lint(opts?: ActionOptions): Step {
    return this._installed.sh(`cd ${this.path} && uv run ruff check .`, {
      label: ":python: lint",
      ...opts,
    });
  }

  fmt(opts?: ActionOptions): Step {
    return this._installed.sh(
      `cd ${this.path} && uv run ruff format --check .`,
      { label: ":python: fmt", ...opts },
    );
  }

  typecheck(opts?: ActionOptions & { paths?: string | string[] }): Step {
    const target = resolvePaths(opts?.paths);
    const { paths: _, ...rest } = opts ?? {};
    return this._installed.sh(`cd ${this.path} && uv run ty check ${target}`, {
      label: ":python: typecheck",
      ...rest,
    });
  }
}

export function python(opts?: PythonOptions): PythonToolchain {
  const path = opts?.path ?? ".";
  const uvVersion = opts?.uvVersion ?? "latest";

  if (!VERSION_RE.test(uvVersion)) {
    throw new Error(
      `hm.python: invalid uv version "${uvVersion}"\n  → use "latest" or a semver like "0.2.0"`,
    );
  }

  const uvEnvPrefix =
    uvVersion === "latest" ? "" : `UV_VERSION=${uvVersion} `;
  const uvInstallCmd = [
    `${uvEnvPrefix}curl -LsSf https://astral.sh/uv/install.sh | sh`,
    "ln -sf /root/.local/bin/uv /usr/local/bin/uv",
    "uv --version",
  ].join(" && ");

  const uvInstalled = makeInstallChain({
    aptPackages: [...APT_PACKAGES],
    installCmd: uvInstallCmd,
    installCache: forever(),
    langTag: "python",
    installTag: "uv-install",
    image: opts?.image,
    base: opts?.base,
  });

  const synced = uvInstalled.sh(`cd ${path} && uv sync --all-extras`, {
    label: ":python: uv-sync",
    cache: onChange(`${path}/uv.lock`, `${path}/pyproject.toml`),
  });

  return new PythonToolchain(path, synced);
}
