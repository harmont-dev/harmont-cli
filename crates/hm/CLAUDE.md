## Execution backends (`hm-exec`)

Local and cloud execution both go through `crates/hm-exec/` — the
`ExecutionBackend` trait + two impls:

- `LocalDockerBackend` — in-process Docker DAG scheduler (formerly
  `orchestrator/` + `runner/` + `executor/` in this crate).
- `CloudBackend` — submits the build to Harmont cloud and watches it
  over the REST SDK, emitting the same `BuildEvent` stream.

`hm run` calls `ExecutionBackend::start(req) -> BackendHandle`, splits
the handle via `into_parts()` into an `EventStream` (handed to
`hm-render::drive_stream`) and a `Control` (Ctrl-C + `wait()`).
Auth is injected: this crate takes a pre-built `HarmontClient`; it never
reads credentials from disk.

## Cloud functionality

`hm cloud` subcommands (login, token, org) are in `src/commands/cloud/`.
HTTP goes through `reqwest` via the `harmont-cloud` SDK crate;
credentials are file-backed at `~/.harmont/credentials.toml`, and
organization state lives in `~/.harmont/cloud-state.json`.

## Feature flags

- `py-env` — test-only: assumes `harmont` Python package is on PATH

## DSL engine

The `hm-dsl-engine` crate evaluates pipeline definitions by shelling out
to system-installed runtimes:

- **Python pipelines:** `python3 -c "..."` subprocess with bundled `harmont`
  package extracted to temp dir via `PYTHONPATH`. Requires `croniter` and
  `python-dateutil` pip-installed.
- **TypeScript pipelines:** `bun run` or `node --experimental-strip-types`
  subprocess with bundled harmont-ts ESM bundles in a temp `node_modules/`.
  Prefers Bun, falls back to Node 22+.

DSL source code (harmont-py, harmont-ts bundles) is compiled into the binary
at build time. Build requires esbuild (`npm ci` in `crates/hm-dsl-engine/harmont-ts/`).
