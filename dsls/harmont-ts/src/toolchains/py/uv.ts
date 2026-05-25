import type { Step, StepOptions } from "../../step.js";
import { forever, onChange } from "../../cache.js";
import { makeInstallChain } from "../shared.js";

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

export interface UvOptions {
  readonly path?: string;
  readonly version?: string;
  readonly image?: string;
  readonly base?: Step;
}

type ActionOptions = Omit<StepOptions, "cwd">;

export class UvProject {
  readonly path: string;
  private readonly _installed: Step;

  constructor(path: string, installed: Step) {
    this.path = path;
    this._installed = installed;
  }

  install(): Step {
    return this._installed;
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

  run(cmd: string, opts?: ActionOptions): Step {
    const firstWord = cmd.split(/\s+/)[0] ?? "run";
    return this._installed.sh(`cd ${this.path} && uv run ${cmd}`, {
      label: `:python: ${firstWord}`,
      ...opts,
    });
  }

  build(opts?: ActionOptions): Step {
    return this._installed.sh(`cd ${this.path} && uv build`, {
      label: ":python: build",
      ...opts,
    });
  }

  lockCheck(opts?: ActionOptions): Step {
    return this._installed.sh(`cd ${this.path} && uv lock --check`, {
      label: ":python: lock-check",
      ...opts,
    });
  }

  publish(opts?: ActionOptions): Step {
    return this._installed.sh(`cd ${this.path} && uv publish`, {
      label: ":python: publish",
      ...opts,
    });
  }
}

export function uv(opts?: UvOptions): UvProject {
  const path = opts?.path ?? ".";
  const version = opts?.version ?? "latest";

  if (!VERSION_RE.test(version)) {
    throw new Error(
      `py.uv: invalid version "${version}"\n  → use "latest" or a semver like "0.4.18"`,
    );
  }

  const uvEnvPrefix =
    version === "latest" ? "" : `UV_VERSION=${version} `;
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

  return new UvProject(path, synced);
}
