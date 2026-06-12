The `cli/` directory is a Cargo workspace.

- `crates/hm/` — the `hm` binary (today's CLI body).
- `crates/hm-exec/` — the `ExecutionBackend` trait + `LocalDockerBackend` (in-process Docker DAG scheduler) + `CloudBackend` (submit+watch over the SDK). The `hm` binary renders the emitted `BuildEvent` stream (via `hm-render`) and owns Ctrl-C; auth is injected (the crate takes a built `HarmontClient`).
- `crates/hm-render/` — `drive_stream`: consumes an `EventStream` and writes terminal/JSON output. No I/O beyond stdout.
- `crates/hm-pipeline-ir/` — pipeline IR schema (serde structs only, no runtime).
- `crates/hm-util/` — shared OS and filesystem utilities.
- `crates/hm-plugin-protocol/` — wire types (serde structs only).
- `crates/hm-plugin-sdk/` — authoring SDK for plugin writers.
Run `cargo build` from the workspace root. Build requires esbuild
(`npm ci` in `crates/hm-dsl-engine/harmont-ts/`).

For cross-cutting doctrine see [PRINCIPLES.md](../PRINCIPLES.md).

## DSLs

Both DSLs live inside `crates/hm-dsl-engine/` so they ship with the crate:

- `crates/hm-dsl-engine/harmont-py/` — the `harmont` Python package (pipeline DSL).
- `crates/hm-dsl-engine/harmont-ts/` — the `harmont` TypeScript package (pipeline DSL).

## Keep the SDK, `hm init` templates, and docs in sync

The toolchain helpers in `crates/hm-dsl-engine/` (e.g.
`harmont-py/harmont/_rust.py`, `harmont-ts/src/toolchains/rust.ts`) are the
**public authoring SDK**. They have two downstream surfaces that drift silently
unless you update them in the same change. **A toolchain change is not done until
all three agree:**

1. **`hm init` templates** — `crates/hm/src/commands/init_templates/<lang>.py`,
   embedded into the binary via `include_str!` in `crates/hm/src/commands/init.rs`.
   When you change a toolchain's recommended entrypoint (e.g. Rust →
   `rust.project().ci()`), update the matching template so scaffolded projects use
   the current API. Roundtrip tests: `crates/hm/tests/cmd_init.rs`.

2. **Pipeline-SDK reference docs** —
   `docs-site/content/docs/pipeline-sdk/reference/toolchains/<lang>.mdx` are
   **auto-generated from the Python docstrings** in `harmont-py` (griffe →
   `docs-site/scripts/extract-dsl-api.py` → `generate-dsl-docs.ts`); they carry a
   "do not edit" header. So: (a) write/refresh the docstring on any method you add
   or change, then (b) regenerate from the simci repo root with `make docs-generate`
   (DSL-only: rebuild `docs-site/dsl-api.json` from `harmont-py`, then
   `cd docs-site && npx tsx scripts/generate-dsl-docs.ts && npx tsx scripts/check-dsl-pages.ts`),
   and (c) commit the regenerated `*.mdx` in the **simci (parent) repo** alongside
   the gitlink bump. `check-dsl-pages.ts` guards that the committed pages match the
   docstrings.
