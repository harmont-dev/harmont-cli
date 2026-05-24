import type { Step, StepOptions } from "../step.js";
import { forever, onChange } from "../cache.js";
import { makeInstallChain } from "./shared.js";

const APT_PACKAGES = ["ruby-full", "build-essential", "git"] as const;
const VERSION_RE = /^(default|[0-9]+\.[0-9]+(\.[0-9]+)?)$/;

export interface RubyOptions {
  readonly path?: string;
  readonly version?: string;
  readonly image?: string;
  readonly base?: Step;
}

type ActionOptions = Omit<StepOptions, "cwd">;

export class RubyProject {
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
    return this._installed.sh(`cd ${this.path} && bundle exec rspec`, {
      label: ":ruby: test",
      ...opts,
    });
  }

  lint(opts?: ActionOptions): Step {
    return this._installed.sh(`cd ${this.path} && bundle exec rubocop`, {
      label: ":ruby: lint",
      ...opts,
    });
  }
}

export function ruby(opts?: RubyOptions): RubyProject {
  const path = opts?.path ?? ".";
  const version = opts?.version ?? "default";

  if (!VERSION_RE.test(version)) {
    throw new Error(
      `hm.ruby: invalid version "${version}"\n  → use "default" or a semver like "3.3"`,
    );
  }

  if (version !== "default") {
    throw new Error(
      `hm.ruby: pinned Ruby versions are not yet implemented\n  → use version="default" (system apt package)`,
    );
  }

  const bundlerInstalled = makeInstallChain({
    aptPackages: [...APT_PACKAGES],
    installCmd: "gem install bundler && bundle --version",
    installCache: forever(),
    langTag: "ruby",
    installTag: "bundler",
    image: opts?.image,
    base: opts?.base,
  });

  const deps = bundlerInstalled.sh(`cd ${path} && bundle install`, {
    label: ":ruby: deps",
    cache: onChange(`${path}/Gemfile.lock`),
  });

  return new RubyProject(path, deps);
}
