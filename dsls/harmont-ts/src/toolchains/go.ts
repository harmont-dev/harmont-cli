import type { Step, StepOptions } from "../step.js";
import { forever } from "../cache.js";
import { makeInstallChain } from "./shared.js";

const APT_PACKAGES = ["curl", "ca-certificates", "git"] as const;
const VERSION_RE = /^[0-9]+\.[0-9]+(\.[0-9]+)?$/;

export interface GoOptions {
  readonly path?: string;
  readonly version?: string;
  readonly image?: string;
  readonly base?: Step;
}

type ActionOptions = Omit<StepOptions, "cwd">;

export class GoToolchain {
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
    return this._installed.sh(`cd ${this.path} && go build ./...`, {
      label: ":go: build",
      ...opts,
    });
  }

  test(opts?: ActionOptions): Step {
    return this._installed.sh(`cd ${this.path} && go test ./...`, {
      label: ":go: test",
      ...opts,
    });
  }

  vet(opts?: ActionOptions): Step {
    return this._installed.sh(`cd ${this.path} && go vet ./...`, {
      label: ":go: vet",
      ...opts,
    });
  }

  fmt(opts?: ActionOptions): Step {
    return this._installed.sh(
      `cd ${this.path} && test -z "$(gofmt -l .)"`,
      { label: ":go: fmt", ...opts },
    );
  }
}

export function go(opts?: GoOptions): GoToolchain {
  const path = opts?.path ?? ".";
  const version = opts?.version ?? "1.23.2";

  if (!VERSION_RE.test(version)) {
    throw new Error(
      `hm.go: invalid version "${version}"\n  → use a semver like "1.23" or "1.23.2"`,
    );
  }

  const installCmd = [
    `curl -fsSL https://go.dev/dl/go${version}.linux-amd64.tar.gz -o /tmp/go.tgz`,
    "rm -rf /usr/local/go && tar -C /usr/local -xzf /tmp/go.tgz",
    "ln -sf /usr/local/go/bin/go /usr/local/bin/go",
    "ln -sf /usr/local/go/bin/gofmt /usr/local/bin/gofmt",
    "go version",
  ].join(" && ");

  const installed = makeInstallChain({
    aptPackages: [...APT_PACKAGES],
    installCmd,
    installCache: forever(),
    langTag: "go",
    installTag: "install",
    image: opts?.image,
    base: opts?.base,
  });

  return new GoToolchain(path, installed);
}
