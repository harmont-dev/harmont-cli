The `cli/` directory is a Cargo workspace.

- `crates/hm/` — the `hm` binary (today's CLI body).
- `crates/hm-pipeline-ir/` — pipeline IR schema (serde structs only, no runtime).
- `crates/hm-util/` — shared OS and filesystem utilities.
- `crates/hm-plugin-protocol/` — wire types (serde structs only).
- `crates/hm-plugin-sdk/` — authoring SDK for plugin writers.
Run `cargo build` from the workspace root. Build requires esbuild
(`npm ci` in `dsls/harmont-ts/`).

For cross-cutting doctrine see [PRINCIPLES.md](../PRINCIPLES.md).

## Python DSL

`dsls/harmont-py/` — the `harmont` Python package (pipeline DSL).
See `dsls/harmont-py/CLAUDE.md` for DSL-specific context.
