import type { Step, StepOptions } from "../step.js";
import { forever } from "../cache.js";
import { makeInstallChain, nodeInstallCmd } from "./shared.js";

const APT_PACKAGES = ["curl", "ca-certificates"] as const;
const ELM_VERSION_RE = /^[0-9]+(\.[0-9]+)+$/;
const NODE_VERSION_RE = /^[0-9]+(\.x)?$/;

export interface ElmOptions {
  readonly path?: string;
  readonly elmVersion?: string;
  readonly nodeVersion?: string;
  readonly image?: string;
  readonly base?: Step;
}

type ActionOptions = Omit<StepOptions, "cwd">;

export class ElmProject {
  readonly path: string;
  private readonly _installed: Step;

  constructor(path: string, installed: Step) {
    this.path = path;
    this._installed = installed;
  }

  install(): Step {
    return this._installed;
  }

  make(target: string, opts?: ActionOptions & { output?: string }): Step {
    const outputFlag = opts?.output != null ? ` --output=${opts.output}` : "";
    return this._installed.sh(
      `cd ${this.path} && elm make ${target}${outputFlag}`,
      { label: `:elm: make ${target}`, ...opts },
    );
  }

  test(opts?: ActionOptions): Step {
    return this._installed.sh(`cd ${this.path} && npx --yes elm-test`, {
      label: ":elm: test",
      ...opts,
    });
  }

  review(opts?: ActionOptions): Step {
    return this._installed.sh(`cd ${this.path} && npx --yes elm-review`, {
      label: ":elm: review",
      ...opts,
    });
  }

  fmt(opts?: ActionOptions): Step {
    return this._installed.sh(
      `cd ${this.path} && npx --yes elm-format --validate .`,
      { label: ":elm: fmt", ...opts },
    );
  }
}

export function elm(opts?: ElmOptions): ElmProject {
  const path = opts?.path ?? ".";
  const elmVersion = opts?.elmVersion ?? "0.19.1";
  const nodeVersion = opts?.nodeVersion ?? "20";

  if (!ELM_VERSION_RE.test(elmVersion)) {
    throw new Error(
      `hm.elm: invalid elm version "${elmVersion}"\n  → use a semver like "0.19.1"`,
    );
  }

  if (!NODE_VERSION_RE.test(nodeVersion)) {
    throw new Error(
      `hm.elm: invalid node version "${nodeVersion}"\n  → use a major version like "20"`,
    );
  }

  const nodeInstalled = makeInstallChain({
    aptPackages: [...APT_PACKAGES],
    installCmd: nodeInstallCmd(nodeVersion),
    installCache: forever(),
    langTag: "elm",
    installTag: "node",
    image: opts?.image,
    base: opts?.base,
  });

  const elmInstallCmd = [
    `curl -fsSL https://github.com/elm/compiler/releases/download/${elmVersion}/binary-for-linux-64-bit.gz -o /tmp/elm.gz`,
    "gunzip /tmp/elm.gz",
    "chmod +x /tmp/elm",
    "mv /tmp/elm /usr/local/bin/elm",
  ].join(" && ");

  const elmInstalled = nodeInstalled.sh(elmInstallCmd, {
    label: ":elm: install",
    cache: forever(),
  });

  return new ElmProject(path, elmInstalled);
}
