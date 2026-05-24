import type { Step, StepOptions } from "../step.js";
import { forever } from "../cache.js";
import { makeInstallChain } from "./shared.js";

const APT_PACKAGES = [
  "build-essential",
  "cmake",
  "ninja-build",
  "clang-format",
] as const;

export interface CMakeOptions {
  readonly path?: string;
  readonly lang?: "c" | "cpp";
  readonly image?: string;
  readonly base?: Step;
}

type ActionOptions = Omit<StepOptions, "cwd">;

export class CMakeProject {
  readonly path: string;
  private readonly _installed: Step;
  private readonly _tag: string;

  constructor(path: string, installed: Step, tag: string) {
    this.path = path;
    this._installed = installed;
    this._tag = tag;
  }

  install(): Step {
    return this._installed;
  }

  configure(opts?: ActionOptions): Step {
    return this._installed.sh(`cd ${this.path} && cmake -S . -B build`, {
      label: `:${this._tag}: configure`,
      ...opts,
    });
  }

  build(opts?: ActionOptions): Step {
    return this._installed.sh(
      `cd ${this.path} && cmake -S . -B build && cmake --build build`,
      { label: `:${this._tag}: build`, ...opts },
    );
  }

  test(opts?: ActionOptions): Step {
    return this._installed.sh(
      `cd ${this.path} && cmake -S . -B build && cmake --build build && ctest --test-dir build --output-on-failure`,
      { label: `:${this._tag}: test`, ...opts },
    );
  }

  fmt(opts?: ActionOptions): Step {
    return this._installed.sh(
      `cd ${this.path} && find src tests -name '*.[ch]' -o -name '*.cpp' -o -name '*.hpp' | xargs clang-format --dry-run --Werror`,
      { label: `:${this._tag}: fmt`, ...opts },
    );
  }
}

export function cmake(opts?: CMakeOptions): CMakeProject {
  const path = opts?.path ?? ".";
  const lang = opts?.lang ?? "c";

  if (lang !== "c" && lang !== "cpp") {
    throw new Error(
      `hm.cmake: invalid lang "${lang}"\n  → use "c" or "cpp"`,
    );
  }

  const installed = makeInstallChain({
    aptPackages: [...APT_PACKAGES],
    installCmd: "cmake --version && clang-format --version",
    installCache: forever(),
    langTag: lang,
    installTag: "cmake-verify",
    image: opts?.image,
    base: opts?.base,
  });

  return new CMakeProject(path, installed, lang);
}
