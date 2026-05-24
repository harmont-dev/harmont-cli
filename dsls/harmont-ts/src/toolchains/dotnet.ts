import type { Step, StepOptions } from "../step.js";
import { forever } from "../cache.js";
import { makeInstallChain } from "./shared.js";

const APT_PACKAGES = ["curl", "ca-certificates", "libicu-dev"] as const;
const CHANNEL_RE = /^([0-9]+\.[0-9]+|LTS|STS)$/;

export interface DotnetOptions {
  readonly path?: string;
  readonly channel?: string;
  readonly image?: string;
  readonly base?: Step;
}

type ActionOptions = Omit<StepOptions, "cwd">;

export class DotnetProject {
  readonly path: string;
  private readonly _installed: Step;

  constructor(path: string, installed: Step) {
    this.path = path;
    this._installed = installed;
  }

  install(): Step {
    return this._installed;
  }

  build(opts?: ActionOptions): Step {
    return this._installed.sh(`cd ${this.path} && dotnet build`, {
      label: ":dotnet: build",
      ...opts,
    });
  }

  test(opts?: ActionOptions): Step {
    return this._installed.sh(`cd ${this.path} && dotnet test`, {
      label: ":dotnet: test",
      ...opts,
    });
  }

  fmt(opts?: ActionOptions): Step {
    return this._installed.sh(
      `cd ${this.path} && dotnet format --verify-no-changes`,
      { label: ":dotnet: fmt", ...opts },
    );
  }
}

export function dotnet(opts?: DotnetOptions): DotnetProject {
  const path = opts?.path ?? ".";
  const channel = opts?.channel ?? "8.0";

  if (!CHANNEL_RE.test(channel)) {
    throw new Error(
      `hm.dotnet: invalid channel "${channel}"\n  → use "8.0", "LTS", or "STS"`,
    );
  }

  const installCmd = [
    "curl -fsSL https://dot.net/v1/dotnet-install.sh -o /tmp/dotnet-install.sh",
    "chmod +x /tmp/dotnet-install.sh",
    `/tmp/dotnet-install.sh --channel ${channel} --install-dir /usr/local/dotnet`,
    "ln -sf /usr/local/dotnet/dotnet /usr/local/bin/dotnet",
    "dotnet --info",
  ].join(" && ");

  const installed = makeInstallChain({
    aptPackages: [...APT_PACKAGES],
    installCmd,
    installCache: forever(),
    langTag: "dotnet",
    installTag: "install",
    image: opts?.image,
    base: opts?.base,
  });

  return new DotnetProject(path, installed);
}
