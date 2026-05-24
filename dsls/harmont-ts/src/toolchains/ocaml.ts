import type { Step, StepOptions } from "../step.js";
import { forever } from "../cache.js";
import { makeInstallChain } from "./shared.js";

const APT_PACKAGES = [
  "opam",
  "build-essential",
  "git",
  "m4",
  "unzip",
  "bubblewrap",
] as const;
const COMPILER_RE = /^[0-9]+\.[0-9]+\.[0-9]+$/;

export interface OCamlOptions {
  readonly path?: string;
  readonly compiler?: string;
  readonly image?: string;
  readonly base?: Step;
}

type ActionOptions = Omit<StepOptions, "cwd">;

export class OCamlProject {
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
    return this._installed.sh(
      `cd ${this.path} && opam exec -- dune build`,
      { label: ":ocaml: build", ...opts },
    );
  }

  test(opts?: ActionOptions): Step {
    return this._installed.sh(
      `cd ${this.path} && opam exec -- dune runtest`,
      { label: ":ocaml: test", ...opts },
    );
  }

  fmt(opts?: ActionOptions): Step {
    return this._installed.sh(
      `cd ${this.path} && opam exec -- dune build @fmt`,
      { label: ":ocaml: fmt", ...opts },
    );
  }
}

export function ocaml(opts?: OCamlOptions): OCamlProject {
  const path = opts?.path ?? ".";
  const compiler = opts?.compiler ?? "5.1.1";

  if (!COMPILER_RE.test(compiler)) {
    throw new Error(
      `hm.ocaml: invalid compiler "${compiler}"\n  → use a semver like "5.1.1"`,
    );
  }

  const opamInitCmd = [
    "opam init -y --disable-sandboxing --bare",
    `opam switch create ${compiler} ${compiler}`,
    "eval $(opam env)",
    "opam install -y dune ocamlformat",
  ].join(" && ");

  const opamInstalled = makeInstallChain({
    aptPackages: [...APT_PACKAGES],
    installCmd: opamInitCmd,
    installCache: forever(),
    langTag: "ocaml",
    installTag: "opam",
    image: opts?.image,
    base: opts?.base,
  });

  const depsCmd = [
    `cd ${path}`,
    'if ls *.opam >/dev/null 2>&1; then opam install -y . --deps-only --with-test; else echo "no .opam files; skipping deps"; fi',
  ].join(" && ");

  const deps = opamInstalled.sh(depsCmd, {
    label: ":ocaml: deps",
    cache: forever(),
  });

  return new OCamlProject(path, deps);
}
