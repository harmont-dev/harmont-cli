import type { Step, StepOptions } from "../step.js";
import { forever, onChange } from "../cache.js";
import { makeInstallChain } from "./shared.js";

const APT_PACKAGES = [
  "curl",
  "ca-certificates",
  "build-essential",
  "libgmp-dev",
  "libffi-dev",
  "libncurses-dev",
  "zlib1g-dev",
] as const;
const GHC_RE = /^[a-zA-Z0-9.-]+$/;

export interface HaskellOptions {
  readonly ghc: string;
  readonly cabal?: string;
  readonly path?: string;
  readonly image?: string;
  readonly base?: Step;
}

type ActionOptions = Omit<StepOptions, "cwd">;

export class HaskellToolchain {
  private readonly _installed: Step;

  constructor(installed: Step) {
    this._installed = installed;
  }

  install(): Step {
    return this._installed;
  }

  cabal(path: string = ".", opts?: { cachePaths?: readonly string[] }): HaskellPackage {
    const cachePaths = opts?.cachePaths ?? [`${path}/*.cabal`];
    const depsStep = this._installed.sh(
      `cabal update && cd ${path} && cabal build all --only-dependencies`,
      {
        label: `:haskell: ${path} deps`,
        cache: onChange(...cachePaths),
      },
    );
    return new HaskellPackage(path, depsStep);
  }
}

export class HaskellPackage {
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
    return this._installed.sh(`cd ${this.path} && cabal build all`, {
      label: `:haskell: ${this.path} build`,
      ...opts,
    });
  }

  test(opts?: ActionOptions): Step {
    return this._installed.sh(`cd ${this.path} && cabal test all`, {
      label: `:haskell: ${this.path} test`,
      ...opts,
    });
  }

  lint(opts?: ActionOptions): Step {
    return this._installed.sh(
      `cd ${this.path} && cabal build all --flag werror`,
      { label: `:haskell: ${this.path} lint`, ...opts },
    );
  }

  hlint(opts?: ActionOptions): Step {
    return this._installed.sh(`hlint ${this.path}`, {
      label: `:haskell: ${this.path} hlint`,
      ...opts,
    });
  }

  fmt(opts?: ActionOptions): Step {
    return this._installed.sh(`fourmolu --mode check ${this.path}`, {
      label: `:haskell: ${this.path} fmt`,
      ...opts,
    });
  }
}

export function haskell(opts: HaskellOptions & { path: string }): HaskellPackage;
export function haskell(opts: HaskellOptions): HaskellToolchain;
export function haskell(opts: HaskellOptions): HaskellToolchain | HaskellPackage {
  const ghc = opts.ghc;
  const cabalVersion = opts.cabal ?? "latest";

  if (!GHC_RE.test(ghc)) {
    throw new Error(
      `hm.haskell: invalid ghc version "${ghc}"\n  → use a version like "9.6.7"`,
    );
  }

  const installCmd = [
    "curl -fsSL https://downloads.haskell.org/~ghcup/x86_64-linux-ghcup -o /usr/local/bin/ghcup",
    "chmod +x /usr/local/bin/ghcup",
    `ghcup install ghc ${ghc} && ghcup install cabal ${cabalVersion}`,
    `ghcup set ghc ${ghc} && ghcup set cabal ${cabalVersion}`,
    "ln -sf /root/.ghcup/bin/* /usr/local/bin/",
    "curl -fsSL https://github.com/fourmolu/fourmolu/releases/download/v0.18.0.0/fourmolu-0.18.0.0-linux-x86_64 -o /usr/local/bin/fourmolu",
    "chmod +x /usr/local/bin/fourmolu",
  ].join(" && ");

  const installed = makeInstallChain({
    aptPackages: [...APT_PACKAGES],
    installCmd,
    installCache: forever(),
    langTag: "haskell",
    installTag: "ghcup",
    image: opts.image,
    base: opts.base,
  });

  const toolchain = new HaskellToolchain(installed);
  return opts.path != null ? toolchain.cabal(opts.path) : toolchain;
}
