The `cli/` directory is a Cargo workspace.

- `crates/hm/` — the `hm` binary (today's CLI body).
- `crates/hm-pipeline-ir/` — pipeline IR schema (serde structs only, no runtime).
- `crates/hm-plugin-protocol/` — wire types (serde structs only).
- `crates/hm-plugin-sdk/` — authoring SDK for plugin writers.
- `crates/hm-fixtures/` — test-only WASM plugins; compiled to
  `target/wasm32-wasip1/debug/` by the test harness.

Run `cargo build` from the workspace root. Plugin fixtures need the
`wasm32-wasip1` target; install with `rustup target add wasm32-wasip1`.

For cross-cutting doctrine see [PRINCIPLES.md](../PRINCIPLES.md).
