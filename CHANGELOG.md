# Changelog

## [Unreleased]

### Changed

- **Breaking:** **CLI:** Replace legacy Docker runner with `hm-vm` crate providing a backend-abstracted VM executor (`--backend` flag, default `docker`) ([#79][pr79])
- **Breaking:** **CLI:** Rename pipeline directory from `.harmont/` to `.hm/` and adopt hierarchical TOML config with figment (user -> project -> env layering) ([#73][pr73])
- **Breaking:** **DSL:** Replace separate `hm.npm()` and `hm.bun()` toolchain factories with unified `hm.js.project()` in both TypeScript and Python DSLs, accepting `runtime` (node/bun/deno) and `pm` (npm/pnpm/yarn-classic/yarn-berry/bun) axes ([#58][pr58], [#67][pr67]) (versecafe)
- **Breaking:** **DSL:** Change `pipeline()` to accept an array of steps instead of variadic arguments in both Python and TypeScript DSLs ([#64][pr64])
- **Breaking:** **DSL:** Remove convenience methods (test/build/lint/fmt/typecheck) from TypeScript `JsProject` class in favor of the uniform `run("script")` method ([#67][pr67]) (versecafe)
- **Breaking:** **DSL:** Simplify cmake toolchain module with three-tier abstraction (`CMakeToolchain`/`CMakeProject`), generic `defines` dict, compiler/ccache/preset support, drop overspecific parameters ([#56][pr56])
- **Breaking:** **SDK:** Rename TypeScript package from `harmont` to `@harmont/hm` (update imports to `@harmont/hm` and `@harmont/hm/toolchains`) ([#77][pr77])
- **DSL:** Use corepack for pnpm and yarn bootstrap instead of `npm install -g pnpm` ([#67][pr67]) (versecafe)
- **CLI:** Switch Linux release artifacts to musl (static) binaries and drop glibc builds ([#78][pr78]) (Tadhg Dowdall)

### Added

- **Breaking:** **DSL:** Add step-level and pipeline-level timeout support via `hm.timeout(duration, step)` wrapper and pipeline `timeout_seconds` field, replacing the old `timeoutSeconds` step option ([#76][pr76])
- **DSL:** Add Deno runtime and Yarn (classic + berry) package manager support to the JS/TS toolchain in both DSLs ([#58][pr58], [#67][pr67]) (versecafe)
- **DSL:** Add Elixir/OTP toolchain (`hm.ex`) with Mix project support, dependency caching, and example projects for both DSLs ([#55][pr55])
- **DSL:** Add Bun toolchain with `BunProject`, shared install helpers, and example project for both Python and TypeScript DSLs ([`089cee0`][c089cee0])
- **DSL:** Add auto-detection of JS runtime and package manager from `package.json` engines/packageManager fields and lockfiles ([#74][pr74])
- **DSL:** Accept named `pipelines` export as alternative to default export in TypeScript DSL ([#61][pr61])
- **CLI:** Add `hm init` onboarding wizard with 7 project templates (CMake, Elixir, Next.js, JS/TS, Rust, Zig, Python) ([#71][pr71])
- **CLI:** Add `hm pipelines` and `hm render` commands for machine-readable pipeline discovery and IR output ([#33][pr33])
- **CLI:** Add `install.sh` one-line installer with SHA-256 verification, versioned alongside the CLI ([#59][pr59])
- **SDK:** Publish harmont SDK packages to npm and PyPI with CI release workflow and PEP 561 `py.typed` marker ([#75][pr75])
- **SDK:** Add deterministic cache key resolution to TS SDK (file-content hashing, TTL buckets, env-var scoping) ([#68][pr68])

### Removed

- **Breaking:** **DSL:** Remove Elm, Haskell, OCaml, .NET, Composer, Perl, and Gradle toolchains from both DSLs ([#51][pr51])
- **Breaking:** **DSL:** Remove schedule trigger and croniter dependency from Python and TypeScript DSLs ([#63][pr63])

### Fixed

- **DSL:** Fix example Python pipelines to use current API (`hm.js.project()` instead of removed `hm.npm()`/`hm.bun()`) ([#77][pr77])
- **DSL:** Use correct Zig download URL for >= 0.14.1 and bump default to 0.14.1 ([`1bf727e`][c1bf727e])
- **DSL:** Resolve ruff lint failures in Python detect module ([`99b7b03`][c99b7b03])
- **CLI:** Return empty registry for pipeline-less repos and prefer Python DSL in discovery commands ([#34][pr34])

[pr33]: https://github.com/harmont-dev/harmont-cli/pull/33
[pr34]: https://github.com/harmont-dev/harmont-cli/pull/34
[pr51]: https://github.com/harmont-dev/harmont-cli/pull/51
[pr55]: https://github.com/harmont-dev/harmont-cli/pull/55
[pr56]: https://github.com/harmont-dev/harmont-cli/pull/56
[pr58]: https://github.com/harmont-dev/harmont-cli/pull/58
[pr59]: https://github.com/harmont-dev/harmont-cli/pull/59
[pr61]: https://github.com/harmont-dev/harmont-cli/pull/61
[pr63]: https://github.com/harmont-dev/harmont-cli/pull/63
[pr64]: https://github.com/harmont-dev/harmont-cli/pull/64
[pr67]: https://github.com/harmont-dev/harmont-cli/pull/67
[pr68]: https://github.com/harmont-dev/harmont-cli/pull/68
[pr71]: https://github.com/harmont-dev/harmont-cli/pull/71
[pr73]: https://github.com/harmont-dev/harmont-cli/pull/73
[pr74]: https://github.com/harmont-dev/harmont-cli/pull/74
[pr75]: https://github.com/harmont-dev/harmont-cli/pull/75
[pr76]: https://github.com/harmont-dev/harmont-cli/pull/76
[pr77]: https://github.com/harmont-dev/harmont-cli/pull/77
[pr78]: https://github.com/harmont-dev/harmont-cli/pull/78
[pr79]: https://github.com/harmont-dev/harmont-cli/pull/79
[c089cee0]: https://github.com/harmont-dev/harmont-cli/commit/089cee0
[c1bf727e]: https://github.com/harmont-dev/harmont-cli/commit/1bf727e
[c99b7b03]: https://github.com/harmont-dev/harmont-cli/commit/99b7b03
