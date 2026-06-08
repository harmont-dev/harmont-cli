import type { Step, StepOptions } from "../step.js";
import { forever, onChange, type CachePolicy } from "../cache.js";
import { makeInstallChain } from "./shared.js";

const COMPILER_RE = /^(gcc|clang)(-\d+)?$/;

const SANITIZER_FLAGS: Record<string, string> = {
  asan: "address,undefined",
  tsan: "thread",
  ubsan: "undefined",
  msan: "memory",
};

type ActionOptions = Omit<StepOptions, "cwd">;

export interface CMakeToolchainOptions {
  readonly compiler?: string;
  readonly generator?: "ninja" | "make";
  readonly ccache?: boolean;
  readonly extras?: readonly string[];
  readonly image?: string;
  readonly base?: Step;
}

export interface CMakeProjectOptions {
  readonly path?: string;
  readonly buildType?: string;
  readonly preset?: string;
  readonly defines?: Record<string, string>;
  readonly shared?: boolean;
  readonly std?: number;
  readonly deps?: "auto" | "vcpkg" | null;
  readonly target?: string;
  readonly cache?: CachePolicy;
}

export type CMakeOptions = CMakeToolchainOptions & CMakeProjectOptions;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function aptPackages(
  compiler: string | undefined,
  ccache: boolean,
  generator: string,
): string[] {
  const pkgs: string[] = ["cmake", "build-essential", "pkg-config"];
  if (generator === "ninja") pkgs.push("ninja-build");
  if (ccache) pkgs.push("ccache");
  pkgs.push("clang-format");
  pkgs.push("clang-tidy");
  if (compiler != null) {
    const m = COMPILER_RE.exec(compiler);
    if (m == null) {
      throw new Error(
        `hm.cmake: invalid compiler "${compiler}"\n  → use "gcc", "gcc-14", "clang", or "clang-18"`,
      );
    }
    const family = m[1];
    const suffix = m[2] ?? "";
    if (family === "gcc") {
      pkgs.push(`gcc${suffix}`, `g++${suffix}`);
    } else {
      pkgs.push(`clang${suffix}`, `lld${suffix}`);
    }
  }
  return pkgs;
}

function verifyCmd(
  compiler: string | undefined,
  ccache: boolean,
  generator: string,
): string {
  const parts: string[] = ["cmake --version"];
  if (generator === "ninja") parts.push("ninja --version");
  if (ccache) parts.push("ccache --version");
  if (compiler != null) {
    const m = COMPILER_RE.exec(compiler);
    if (m) {
      const family = m[1];
      const suffix = m[2] ?? "";
      if (family === "gcc") {
        parts.push(`gcc${suffix} --version`);
      } else {
        parts.push(`clang${suffix} --version`);
      }
    }
  }
  return parts.join(" && ");
}

function configureCmd(opts: {
  path: string;
  buildType: string;
  preset: string | undefined;
  defines: Record<string, string> | undefined;
  shared: boolean | undefined;
  std: number | undefined;
  compiler: string | undefined;
  ccache: boolean;
  generator: string;
  buildDir?: string;
}): string {
  if (opts.preset != null) {
    return `cd ${opts.path} && cmake --preset ${opts.preset}`;
  }

  const buildDir = opts.buildDir ?? "build";
  const genFlag = opts.generator === "ninja" ? "Ninja" : "Unix Makefiles";
  const parts: string[] = [
    `cd ${opts.path} && cmake -S . -B ${buildDir}`,
    `-G ${genFlag}`,
    `-DCMAKE_BUILD_TYPE=${opts.buildType}`,
    "-DCMAKE_EXPORT_COMPILE_COMMANDS=ON",
  ];

  if (opts.ccache) {
    parts.push("-DCMAKE_C_COMPILER_LAUNCHER=ccache");
    parts.push("-DCMAKE_CXX_COMPILER_LAUNCHER=ccache");
  }

  if (opts.compiler != null) {
    const m = COMPILER_RE.exec(opts.compiler);
    if (m) {
      const family = m[1];
      const suffix = m[2] ?? "";
      if (family === "gcc") {
        parts.push(`-DCMAKE_C_COMPILER=gcc${suffix}`);
        parts.push(`-DCMAKE_CXX_COMPILER=g++${suffix}`);
      } else {
        parts.push(`-DCMAKE_C_COMPILER=clang${suffix}`);
        parts.push(`-DCMAKE_CXX_COMPILER=clang++${suffix}`);
      }
    }
  }

  if (opts.shared != null) {
    parts.push(`-DBUILD_SHARED_LIBS=${opts.shared ? "ON" : "OFF"}`);
  }

  if (opts.std != null) {
    parts.push(`-DCMAKE_CXX_STANDARD=${opts.std}`);
  }

  if (opts.defines) {
    for (const [k, v] of Object.entries(opts.defines)) {
      parts.push(`-D${k}=${v}`);
    }
  }

  return parts.join(" ");
}

function buildCmd(
  path: string,
  target: string | undefined,
  buildDir: string = "build",
): string {
  let cmd = `cmake --build ${path}/${buildDir} --parallel $(nproc)`;
  if (target != null) {
    cmd += ` --target ${target}`;
  }
  return cmd;
}

// ---------------------------------------------------------------------------
// CMakeToolchain
// ---------------------------------------------------------------------------

export class CMakeToolchain {
  private readonly _installed: Step;
  readonly compiler: string | undefined;
  readonly ccache: boolean;
  readonly generator: string;

  constructor(
    installed: Step,
    compiler: string | undefined,
    ccache: boolean,
    generator: string,
  ) {
    this._installed = installed;
    this.compiler = compiler;
    this.ccache = ccache;
    this.generator = generator;
  }

  install(): Step {
    return this._installed;
  }

  project(opts?: CMakeProjectOptions): CMakeProject {
    const path = opts?.path ?? ".";
    const buildType = opts?.buildType ?? "Release";
    const preset = opts?.preset;
    const defines = opts?.defines;
    const shared = opts?.shared;
    const std = opts?.std;
    const deps = opts?.deps;
    const target = opts?.target;
    const cache = opts?.cache;

    const configure = configureCmd({
      path,
      buildType,
      preset,
      defines,
      shared,
      std,
      compiler: this.compiler,
      ccache: this.ccache,
      generator: this.generator,
    });
    const build = buildCmd(path, target);
    const warmupCmd = `${configure} && ${build}`;

    // Determine warmup cache policy
    let warmupCache: CachePolicy;
    if (cache != null) {
      warmupCache = cache;
    } else if (deps === "vcpkg") {
      warmupCache = onChange("vcpkg.json");
    } else {
      const cmakelists =
        path !== "." ? `${path}/CMakeLists.txt` : "CMakeLists.txt";
      warmupCache = onChange(cmakelists);
    }

    // Determine the parent for the warmup step
    let warmupParent: Step;
    if (deps === "vcpkg") {
      const vcpkgCmd = [
        "git clone https://github.com/microsoft/vcpkg.git /opt/vcpkg",
        "/opt/vcpkg/bootstrap-vcpkg.sh",
        `cd ${path} && /opt/vcpkg/vcpkg install`,
      ].join(" && ");
      warmupParent = this._installed.sh(vcpkgCmd, {
        label: ":cmake: vcpkg",
        cache: onChange("vcpkg.json"),
      });
    } else {
      warmupParent = this._installed;
    }

    const built = warmupParent.sh(warmupCmd, {
      label: ":cmake: build",
      cache: warmupCache,
    });

    return new CMakeProject(this, built, path);
  }
}

// ---------------------------------------------------------------------------
// CMakeProject
// ---------------------------------------------------------------------------

export class CMakeProject {
  readonly toolchain: CMakeToolchain;
  private readonly _built: Step;
  readonly path: string;

  constructor(toolchain: CMakeToolchain, built: Step, path: string) {
    this.toolchain = toolchain;
    this._built = built;
    this.path = path;
  }

  build(opts?: ActionOptions): Step {
    if (opts) {
      return this._built.sh(
        buildCmd(this.path, undefined),
        { label: ":cmake: build", ...opts },
      );
    }
    return this._built;
  }

  test(opts?: ActionOptions): Step {
    const cmd = [
      buildCmd(this.path, undefined),
      `ctest --test-dir ${this.path}/build --output-on-failure --parallel $(nproc)`,
    ].join(" && ");
    return this._built.sh(cmd, { label: ":cmake: test", ...opts });
  }

  install(opts?: ActionOptions): Step {
    const cmd = `cmake --install ${this.path}/build`;
    return this._built.sh(cmd, { label: ":cmake: install", ...opts });
  }

  fmt(opts?: ActionOptions): Step {
    const cmd = [
      `cd ${this.path} && find . -name '*.c' -o -name '*.h'`,
      `-o -name '*.cpp' -o -name '*.hpp' -o -name '*.cc' -o -name '*.cxx' |`,
      `xargs clang-format --dry-run --Werror`,
    ].join(" ");
    return this.toolchain.install().sh(cmd, { label: ":cmake: fmt", ...opts });
  }

  lint(opts?: ActionOptions): Step {
    const cmd = `cd ${this.path} && run-clang-tidy -p build`;
    return this._built.sh(cmd, { label: ":cmake: lint", ...opts });
  }

  coverage(opts?: ActionOptions): Step {
    const covConfigure = configureCmd({
      path: this.path,
      buildType: "Debug",
      preset: undefined,
      defines: undefined,
      shared: undefined,
      std: undefined,
      compiler: this.toolchain.compiler,
      ccache: this.toolchain.ccache,
      generator: this.toolchain.generator,
      buildDir: "build-cov",
    });
    const covFlags = "--coverage -fno-omit-frame-pointer";
    const covBuild = buildCmd(this.path, undefined, "build-cov");
    const cmd = [
      `${covConfigure} -DCMAKE_C_FLAGS='${covFlags}' -DCMAKE_CXX_FLAGS='${covFlags}'`,
      covBuild,
      `ctest --test-dir ${this.path}/build-cov --output-on-failure`,
      `lcov --capture --directory ${this.path}/build-cov --output-file coverage.info`,
      `lcov --remove coverage.info '/usr/*' --output-file coverage.info`,
    ].join(" && ");
    return this.toolchain.install().sh(cmd, {
      label: ":cmake: coverage",
      ...opts,
    });
  }

  sanitize(kind: string = "asan", opts?: ActionOptions): Step {
    if (!(kind in SANITIZER_FLAGS)) {
      throw new Error(
        `hm.cmake: unknown sanitizer kind "${kind}"\n  → use one of: ${Object.keys(SANITIZER_FLAGS).sort().join(", ")}`,
      );
    }
    const sanFlags = `-fsanitize=${SANITIZER_FLAGS[kind]} -fno-omit-frame-pointer -g`;
    const buildDir = `build-${kind}`;
    const sanConfigure = configureCmd({
      path: this.path,
      buildType: "Debug",
      preset: undefined,
      defines: undefined,
      shared: undefined,
      std: undefined,
      compiler: this.toolchain.compiler,
      ccache: this.toolchain.ccache,
      generator: this.toolchain.generator,
      buildDir,
    });
    const sanBuild = buildCmd(this.path, undefined, buildDir);
    const cmd = [
      `${sanConfigure} -DCMAKE_C_FLAGS='${sanFlags}' -DCMAKE_CXX_FLAGS='${sanFlags}'`,
      sanBuild,
      `ctest --test-dir ${this.path}/${buildDir} --output-on-failure`,
    ].join(" && ");
    return this.toolchain.install().sh(cmd, {
      label: `:cmake: ${kind}`,
      ...opts,
    });
  }

  package(opts?: ActionOptions): Step {
    const cmd = `cd ${this.path}/build && cpack`;
    return this._built.sh(cmd, { label: ":cmake: package", ...opts });
  }
}

// ---------------------------------------------------------------------------
// Factory function (overloaded)
// ---------------------------------------------------------------------------

export function cmake(opts: CMakeOptions & { path: string }): CMakeProject;
export function cmake(opts?: CMakeToolchainOptions): CMakeToolchain;
export function cmake(opts?: CMakeOptions): CMakeToolchain | CMakeProject {
  const compiler = opts?.compiler;
  const generator = opts?.generator ?? "ninja";
  const ccache = opts?.ccache ?? true;

  if (generator !== "ninja" && generator !== "make") {
    throw new Error(
      `hm.cmake: invalid generator "${generator}"\n  → use "ninja" or "make"`,
    );
  }

  if (compiler != null && !COMPILER_RE.test(compiler)) {
    throw new Error(
      `hm.cmake: invalid compiler "${compiler}"\n  → use "gcc", "gcc-14", "clang", or "clang-18"`,
    );
  }

  const pkgs = aptPackages(compiler, ccache, generator);
  const verify = verifyCmd(compiler, ccache, generator);

  const installed = makeInstallChain({
    aptPackages: pkgs,
    installCmd: verify,
    installCache: forever(),
    langTag: "cmake",
    installTag: "verify",
    image: opts?.image,
    base: opts?.base,
  });

  const toolchain = new CMakeToolchain(installed, compiler, ccache, generator);

  if (opts?.path != null) {
    return toolchain.project({
      path: opts.path,
      buildType: (opts as CMakeProjectOptions).buildType,
      preset: (opts as CMakeProjectOptions).preset,
      defines: (opts as CMakeProjectOptions).defines,
      shared: (opts as CMakeProjectOptions).shared,
      std: (opts as CMakeProjectOptions).std,
      deps: (opts as CMakeProjectOptions).deps,
      target: (opts as CMakeProjectOptions).target,
      cache: (opts as CMakeProjectOptions).cache,
    });
  }

  return toolchain;
}
