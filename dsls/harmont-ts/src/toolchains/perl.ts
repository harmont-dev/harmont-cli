import type { Step, StepOptions } from "../step.js";
import { forever, onChange } from "../cache.js";
import { makeInstallChain } from "./shared.js";

const APT_PACKAGES = ["perl", "cpanminus", "build-essential"] as const;

export interface PerlOptions {
  readonly path?: string;
  readonly image?: string;
  readonly base?: Step;
}

type ActionOptions = Omit<StepOptions, "cwd">;

export class PerlProject {
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
    return this._installed.sh(`cd ${this.path} && prove -lv t/`, {
      label: ":perl: test",
      ...opts,
    });
  }

  lint(opts?: ActionOptions): Step {
    return this._installed.sh(`cd ${this.path} && perlcritic lib/`, {
      label: ":perl: lint",
      ...opts,
    });
  }
}

export function perl(opts?: PerlOptions): PerlProject {
  const path = opts?.path ?? ".";

  const cpanmInstalled = makeInstallChain({
    aptPackages: [...APT_PACKAGES],
    installCmd: "cpanm --notest --quiet Perl::Critic && perl --version",
    installCache: forever(),
    langTag: "perl",
    installTag: "cpanm",
    image: opts?.image,
    base: opts?.base,
  });

  const deps = cpanmInstalled.sh(
    `cd ${path} && cpanm --installdeps --notest .`,
    {
      label: ":perl: deps",
      cache: onChange(`${path}/cpanfile`),
    },
  );

  return new PerlProject(path, deps);
}
