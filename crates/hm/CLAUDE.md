## Orchestrator

`cli/crates/hm/src/orchestrator/` is the entry point for local builds.
`hm run` calls into `orchestrator::run()`, which:

- Builds a wire-typed `Graph` (`graph.rs`) from the parsed `Pipeline`
  and partitions it into chains for scheduling.
- Loads the plugin registry (the embedded docker plugin is baked in
  via `build.rs`) and resolves each step's `runner` field to a
  registered plugin in `scheduler.rs`.
- Publishes `BuildEvent`s on a `tokio::sync::broadcast` (`events.rs`);
  the `output_subscriber` task drains the bus and invokes the selected
  output plugin's `hm_output_on_event` per event (`hm-plugin-output-human`
  or `hm-plugin-output-json`, both embedded via `build.rs`). Default
  `--format` is `human`; `--format json` writes one JSON event per
  line on stdout.
- Streams cache decisions host-side (`cache.rs`), reads the workspace
  archive once into memory (`archive.rs` + `source.rs`), and drives
  the Docker daemon via the Bollard wrapper (`docker_client.rs`,
  exposed to step plugins through `docker_host_fns.rs`).
- Owns run-wide cancellation (`tokio_util::sync::CancellationToken`) and shared mutable state
  (`state.rs`) so step plugins can coordinate without reaching across
  module boundaries.

Plugin parallelism is bounded by `PluginPool`. Each `LoadedPlugin`
owns a pool sized to the run's `--parallelism`, so concurrent chains
don't serialise on the same Extism `Plugin` instance.

## Cloud functionality (plan 4)

Every cloud verb runs through the embedded `hm-plugin-cloud` plugin
under the `hm cloud` namespace: `hm cloud {login,logout,whoami,org,
pipeline,build,job,billing,run}`. Legacy cli/src/{client,credentials,
generated}.rs and the matching command modules are deleted.

The plugin uses extism-pdk's host-mediated HTTP, restricted by the
manifest's `allowed_hosts: ["api.harmont.dev", "*.harmont.dev"]`. The
host fns `hm_keyring_*` back token storage (file-backed at
`~/.harmont/credentials.toml`, mode 0o600 — no OS keyring / D-Bus);
`hm_kv_*` (KvScope::Plugin) backs persistent state (active org slug);
`hm_spawn_loopback` + `hm_loopback_recv` support the browser-loopback
OAuth flow.

`hm cloud run` is partial: it submits a pre-rendered plan JSON
(default path: `.harmont/plan.json`, override with `--plan-file`).
Source-archive upload to the cloud is plan-5 work. The legacy
`commands/run/remote.rs` source-tar logic is gone.

Known follow-ups for plan 5 or later:
- `hm_random_bytes(len) -> Vec<u8>` host fn so the cloud plugin's
  PKCE verifier uses real entropy.
- `hm_sleep_ms(ms)` host fn so `cloud build watch` doesn't busy-wait.
- `cloud run` source-archive upload.

Broadcast lag in `output_subscriber` surfaces a `tracing::warn!` plus
an `eprintln!` line; full lag-recovery (e.g., per-step backpressure)
is a future concern.
