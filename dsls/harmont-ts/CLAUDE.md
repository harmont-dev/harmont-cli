# harmont (TypeScript DSL)

TypeScript pipeline DSL — equivalent of `dsls/harmont-py/`.

## Commands

- `npm test` — run Vitest test suite
- `npm run build` — compile TypeScript to `dist/`

## Architecture

- `src/step.ts` — Step class (immutable chain primitive)
- `src/cache.ts` — Cache policy discriminated unions
- `src/triggers.ts` — Trigger factory functions
- `src/keys.ts` — Step key resolution (slug/hash)
- `src/pipeline.ts` — Lowering pass (step chains → petgraph IR)
- `src/target.ts` — Memoized reusable targets
- `src/envelope.ts` — Envelope rendering (schema_version:1)
- `src/toolchains/` — Language toolchain abstractions
- `src/index.ts` — Public API barrel export

## IR Compatibility

Output must match the v0 IR that `crates/hm-pipeline-ir/` deserializes.
The Rust `CommandStep` accepts: key, cmd, label?, image?, env?, timeout_seconds?, cache?, runner?, runner_args?.
The Rust `Cache` accepts: policy, key?.
Edge kinds: `builds_in`, `depends_on`.
Envelope: `{ schema_version: "1", pipelines: [...] }`.
