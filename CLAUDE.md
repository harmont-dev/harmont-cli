The `cli/` directory is a Cargo workspace.

- `crates/hm/` — the `hm` binary (today's CLI body).
- `crates/hm-core/` — the shared core: `config` (layered project/user/env config + credential storage), `exec` (the `ExecutionBackend` trait + `LocalBackend` in-process Docker DAG scheduler + `CloudBackend` submit+watch over the SDK), and `sys_runtime` (the process-wide git/python/dirs/cwd runtime). The `hm` binary renders the emitted `BuildEvent` stream (via `hm-render`) and owns Ctrl-C; auth is injected (the backends take a built `HarmontClient`).
- `crates/hm-render/` — `drive_stream`: consumes an `EventStream` and writes terminal/JSON output. No I/O beyond stdout.
- `crates/hm-pipeline-ir/` — pipeline IR schema (serde structs only, no runtime).
- `crates/hm-common/` — shared utilities (OS/filesystem, formatting, and other cross-crate helpers). This is the source of truth for common code; prefer adding shared helpers here.
- `crates/hm-plugin-protocol/` — wire types (serde structs only).
Run `cargo build` from the workspace root.

For cross-cutting doctrine see [PRINCIPLES.md](../PRINCIPLES.md).

## Testing

When writing or running any Rust test, follow the
[`writing-rust-tests`](.claude/skills/writing-rust-tests/SKILL.md) skill:
`#[rstest]` over bare `#[test]`, parametrized `#[case]` over duplicated test
functions and hand-rolled loops, `proptest` for domain-wide properties. Run with
plain `cargo test -p <crate>` (no nextest/just wrapper).

## Documentation

When writing or editing any docblock, doc comment, or module header (`///`,
`//!`) — including on code you just changed — follow the
[`writing-interface-docblocks`](.claude/skills/writing-interface-docblocks/SKILL.md)
skill: a docblock is a contract, not a changelog. Terse, present-tense, no
prompt or diff leakage (`rather than`, `now returns`, `as requested`); document
the *when* of errors/panics, not the *why*; module docs name the domain, not the
one item currently inside them.

## DSL

The `harmont` Python package (pipeline DSL) lives inside `crates/hm-dsl-engine/harmont-py/` so it ships with the crate.

## Keep the SDK, `hm init` templates, and docs in sync

The toolchain helpers in `crates/hm-dsl-engine/` (e.g.
`harmont-py/harmont/_rust.py`) are the
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
