## Orchestrator

`cli/crates/hm/src/orchestrator/` is the entry point for local builds.
`hm run` calls into `orchestrator::run()`, which:

- Builds a wire-typed `Graph` (`graph.rs`) from the parsed `Pipeline`
  and partitions it into chains for scheduling.
- Receives a `RunnerRegistry` (containing `DockerRunner` and any
  future runners) and resolves each step's `runner` field to a
  registered runner in `scheduler.rs`.
- Publishes `BuildEvent`s on a `tokio::sync::broadcast` (`events.rs`);
  the `output_subscriber` task drains the bus and invokes the selected
  `OutputRenderer` (human or JSON, both in `src/output/`).
- Reads the workspace archive once into memory (`archive.rs` +
  `source.rs`), and drives the Docker daemon via the Bollard wrapper
  (`docker_client.rs`).
- The `DockerRunner` (`src/runner/docker.rs`) executes steps directly
  via `DockerClient` — no FFI, no WASM, no host functions.
- Owns run-wide cancellation (`tokio_util::sync::CancellationToken`)
  via `signal.rs`.

## Runner system (static DI)

`src/runner/mod.rs` defines `StepRunner` (async trait), `OutputRenderer`,
`RunContext`, and `RunnerRegistry`. `DockerRunner` is the sole executor.
The registry is constructed in `commands/run/local.rs` and passed to
`scheduler::run` — no global state, no plugin loading.

## Cloud functionality

`hm cloud` subcommands are implemented in the `hm-plugin-cloud` library
crate (direct dependency, no FFI). HTTP goes through `reqwest`,
credentials are file-backed at `~/.harmont/credentials.toml`, and
organization state lives in `~/.harmont/cloud-state.json`.

## Feature flags

- `embedded-python` — wasmtime + CPython WASI for Python DSL evaluation (no system python3 needed)
- `embedded-typescript` — wasmtime + QuickJS WASI + oxc for TS DSL evaluation (no system node needed)
- `py-env` — test-only: assumes `harmont` Python package is on PATH

Without embedded features, falls back to system interpreters (python3/node subprocess).

Build-time asset fetching: when `embedded-python` or `embedded-typescript` features are enabled, `build.rs` downloads pinned assets (QuickJS WASM, Python vendor packages) from the internet on first build. Subsequent builds use the cached copies in `$OUT_DIR`.
