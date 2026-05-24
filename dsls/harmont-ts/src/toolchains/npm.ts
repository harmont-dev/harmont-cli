import type { Step, StepOptions } from "../step.js";
import { forever, onChange } from "../cache.js";
import { makeInstallChain, nodeInstallCmd } from "./shared.js";

const APT_PACKAGES = ["curl", "ca-certificates"] as const;
const VERSION_RE = /^[0-9]+(\.x)?$/;

export interface NpmOptions {
  readonly path?: string;
  readonly version?: string;
  readonly image?: string;
  readonly base?: Step;
}

type ActionOptions = Omit<StepOptions, "cwd">;

export class NpmProject {
  readonly path: string;
  private readonly _installed: Step;

  constructor(path: string, installed: Step) {
    this.path = path;
    this._installed = installed;
  }

  install(): Step {
    return this._installed;
  }

  run(script: string, opts?: ActionOptions): Step {
    return this._installed.sh(`cd ${this.path} && npm run ${script}`, {
      label: `:node: ${script}`,
      ...opts,
    });
  }

  test(opts?: ActionOptions): Step {
    return this._installed.sh(`cd ${this.path} && npm test`, {
      label: ":node: test",
      ...opts,
    });
  }

  lint(opts?: ActionOptions): Step {
    return this.run("lint", opts);
  }

  fmt(opts?: ActionOptions): Step {
    return this.run("fmt", opts);
  }
}

export function npm(opts?: NpmOptions): NpmProject {
  const path = opts?.path ?? ".";
  const version = opts?.version ?? "20";

  if (!VERSION_RE.test(version)) {
    throw new Error(
      `hm.npm: invalid version "${version}"\n  → use a Node major version like "20" or "20.x"`,
    );
  }

  const nodeInstalled = makeInstallChain({
    aptPackages: [...APT_PACKAGES],
    installCmd: nodeInstallCmd(version),
    installCache: forever(),
    langTag: "node",
    installTag: "install",
    image: opts?.image,
    base: opts?.base,
  });

  const npmCi = nodeInstalled.sh(`cd ${path} && npm ci`, {
    label: ":node: deps",
    cache: onChange(`${path}/package-lock.json`),
  });

  return new NpmProject(path, npmCi);
}
