## Execution backends (`hm-exec`)

Local and cloud execution both go through `crates/hm-exec/` — the
`ExecutionBackend` trait + two impls:

- `LocalBackend` — runs the whole build in-process via a DAG scheduler,
  executing each step inside a lightweight VM. It composes the per-step
  `hm-vm` crate: it builds an `hm_vm::HmVm` (a `VmBackend` + snapshot
  `ImageRegistry`), registers a `VmRunner` as the default runner, and
  hands it to `scheduler::run`. The VM backend is injected (`docker` is
  the only one wired today). Snapshot caching is owned by `hm-vm`, not
  the scheduler.
- `CloudBackend` — submits the build to Harmont cloud and watches it
  over the REST SDK, emitting the same `BuildEvent` stream.

`hm run` resolves the backend by name (`--backend`, the deprecated
`--cloud` alias, or the `backend` config key — default `docker`):
`cloud` → `CloudBackend`; any other name → `LocalBackend` on that
`hm_vm::VmBackend`. It then calls `ExecutionBackend::start(req) ->
BackendHandle`, splits the handle via `into_parts()` into an
`EventStream` (handed to `hm-render::drive_stream`) and a `Control`
(Ctrl-C + `wait()`). Auth is injected: this crate takes a pre-built
`HarmontClient`; it never reads credentials from disk.

### Per-step mechanism (`hm-exec`'s `local` module + `hm-vm`)

The whole-build `ExecutionBackend` and the per-step `hm_vm::VmBackend`
are two separate traits. Inside `LocalBackend`, `crates/hm-exec/src/local/`:
- Builds the source archive once into memory (`source.rs` + `archive.rs`).
- Walks the DAG in `scheduler.rs`, resolving each step's `runner` field
  against a `RunnerRegistry` (default: `VmRunner`).
- `runner/vm.rs`'s `VmRunner` drives `hm_vm::HmVm::execute` per step,
  streaming stdout/stderr as `BuildEvent::StepLog` via an `OutputSink`
  that emits onto the build's `EventBus` (`events.rs`).
- Publishes `BuildEvent`s on a `tokio::sync::broadcast` (`events.rs`),
  forwarded to the caller's mpsc stream.
- Run-wide cancellation (`tokio_util::sync::CancellationToken`) is owned
  by the CLI and threaded into `scheduler::run`.

`runner/mod.rs` defines `StepRunner` (async trait), `StepContext`, and
`RunnerRegistry` — static DI, no global state, no plugin loading.

## Cloud functionality

`hm cloud` subcommands (login, token, org) are in `src/commands/cloud/`.
HTTP goes through `reqwest` via the `harmont-cloud` SDK crate;
credentials are file-backed at `~/.config/hm/credentials.toml`, and
organization state lives in `~/.config/hm/config.toml` (`[cloud] org`).

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
