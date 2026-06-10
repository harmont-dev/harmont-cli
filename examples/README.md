# Harmont examples

Minimal idiomatic starter projects, each wired up to a Harmont CI pipeline. Every example lives in its own subdirectory with a `.hm/pipeline.py` you can read, copy, and run via `hm run <slug>`.

| Example | Toolchain | Pipeline |
|---|---|---|
| [react](./react) | npm + Vite + Vitest + ESLint | `hm.js.project(...)` |
| [nextjs](./nextjs) | npm + Jest + ESLint | `hm.js.project(...)` |
| [typescript](./typescript) | tsc + Vitest + ESLint | `hm.js.project(...)` |
| [bun](./bun) | Bun + tsc + bun:test + ESLint | `hm.js.project(runtime="bun")` |
| [rust](./rust) | cargo + clippy + rustfmt | `hm.rust(...)` |
| [python-uv](./python-uv) | uv + pytest + ruff + mypy | `hm.python(...)` |
| [go](./go) | go build/test/vet/fmt | `hm.go(...)` |
| [c](./c) | CMake + CTest + clang-format | `hm.cmake(lang="c")` |
| [cpp](./cpp) | CMake + CTest + clang-format | `hm.cmake(lang="cpp")` |
| [zig](./zig) | zig build/test/fmt | `hm.zig(version="0.13.0")` |

## How to run an example locally

1. Install the Harmont CLI (`cli/` in this repo, or `cargo install harmont-cli` once published).
2. `cd examples/<lang>` and run `hm run ci`. The CLI uses the project's `.hm/pipeline.py` and executes each step in a local Docker container (the default backend), sharing caches across runs. Use `--backend cloud` to submit the run to Harmont Cloud instead.

Every pipeline uses `default_image="ubuntu:24.04"` and the apt-base / language-install steps are cached forever — only the action leaves (`test`, `lint`, etc.) re-run after a code change.

## What to copy

The shape every pipeline shares: a single `@hm.target()` builds the toolchain object once; the `@hm.pipeline("ci")` body returns a tuple of leaves (`build`, `test`, `lint`). Each leaf forks off the shared install step, so adding a fifth check costs you the action — never the install.
