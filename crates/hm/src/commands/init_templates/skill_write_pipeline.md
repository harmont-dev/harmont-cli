---
name: write-pipeline
description: Write or modify Harmont CI pipelines. Use when creating new pipelines, adding/removing steps, switching toolchains, customizing caching/triggers, or when the user asks to set up CI with Harmont. Fetches live documentation from docs.harmont.dev for up-to-date API reference.
---

Write, modify, or extend Harmont CI pipelines defined in `.hm/pipeline.py` (Python) or `.hm/pipeline.ts` (TypeScript). Pipelines are real programs that import the `harmont` SDK and declaratively define build steps, triggers, and caching.

## When to use

- The user asks to "set up CI", "add a pipeline", "write a pipeline", or "configure Harmont"
- The user wants to add, remove, or modify pipeline steps
- The user asks to add triggers, caching, or environment variables to a pipeline
- The user wants to switch toolchains or add a new toolchain to their pipeline
- The user asks to create a multi-pipeline setup
- The user is migrating from another CI system and wants a Harmont pipeline

## When NOT to use

- The user only wants to run an existing pipeline — use the `validate-ci` skill instead
- The user is debugging a build failure caused by their application code, not the pipeline definition
- The user asks about Harmont Cloud features (login, org management, billing)

## Before you start

1. Fetch the patterns guide — the single most important reference:
   ```
   WebFetch https://docs.harmont.dev/pipeline-sdk/patterns.md
   ```
   Read it carefully. It covers correct vs. incorrect approaches, when to use toolchains vs. raw shell, and common anti-patterns.

2. If you need the full API reference for a specific toolchain or feature, fetch the relevant page (append `.md` to any docs.harmont.dev URL for raw Markdown):
   - Toolchain reference: `https://docs.harmont.dev/pipeline-sdk/reference/toolchains/<name>.md` (rust, python, npm, go, cmake, zig, elixir, ruby, etc.)
   - Chains and steps: `https://docs.harmont.dev/pipeline-sdk/reference/chains.md`
   - Triggers: `https://docs.harmont.dev/pipeline-sdk/reference/triggers.md`
   - Caching: `https://docs.harmont.dev/pipeline-sdk/reference/cache.md`
   - Pipeline decorator/factory: `https://docs.harmont.dev/pipeline-sdk/reference/pipeline.md`
   - Full page index: `https://docs.harmont.dev/llms.txt`

3. If you need a complete working example for a language or framework:
   ```
   WebFetch https://docs.harmont.dev/examples/<language>.md
   ```
   Available examples: rust, go, cmake, zig, nextjs, python-uv, ruby, elixir

## Procedure

1. **Identify the project's language and build system.** Look at the project root for `Cargo.toml` (Rust), `package.json` (JS/TS), `pyproject.toml` or `setup.py` (Python), `go.mod` (Go), `CMakeLists.txt` (C/C++), `mix.exs` (Elixir), `build.zig` (Zig), `Gemfile` (Ruby).

2. **Check for an existing pipeline.** Look for `.hm/pipeline.py` or `.hm/pipeline.ts`. If none exists, pick the DSL that matches the project's ecosystem before asking the user to confirm:
   - **TypeScript DSL** if the project already has `package.json`, `tsconfig.json`, or is primarily TypeScript/JavaScript (the team is already comfortable with the TS toolchain).
   - **Python DSL** for everything else — Rust, Go, C/C++, Elixir, Zig, Ruby, Python, or mixed-language projects (Python is the simpler, more universal choice).
   - Present your recommendation and rationale, then let the user override if they prefer the other DSL.
   Then either run `hm init --template <kind>` to scaffold or write the pipeline file directly.

3. **Fetch the relevant documentation** (see "Before you start" above). Always fetch the patterns guide first. Then fetch the toolchain reference for the detected language.

4. **Write or modify the pipeline.** Follow the patterns guide strictly:
   - Prefer toolchains over raw `sh()` calls when a toolchain exists for the language.
   - Use `.fork()` for steps that can run in parallel.
   - Set triggers (`push`, `pull_request`, `schedule`) appropriate to the project.
   - Use `default_image: "ubuntu:24.04"` unless the project needs something specific.
   - Set `env: {"CI": "true"}` on the pipeline.

5. **Validate the pipeline renders correctly:**
   ```bash
   hm render <pipeline-slug>
   ```
   This outputs the v0 IR JSON. If it fails, the pipeline has a syntax or import error — fix it before proceeding.

6. **Run the pipeline locally:**
   ```bash
   hm run
   ```
   - Exit code 0 = pass. Inform the user.
   - Exit code non-zero = fail. Read the output, diagnose, and fix. Re-run until green.
   - Exit code 130 = user cancelled (Ctrl-C). Do not treat as failure.
   - If Docker is not running, tell the user to start Docker.

7. **If `hm run` fails due to what appears to be a bug in Harmont** (not in the user's code or pipeline definition), and the `gh` CLI is available, ask the user for permission and then file an issue:
   ```bash
   gh issue create --repo harmont-dev/harmont-cli \
     --title "Bug: <concise description>" \
     --body "## Environment
   $(hm --version)

   ## What happened
   <description of the unexpected behavior>

   ## Steps to reproduce
   <pipeline snippet or steps that triggered the bug>

   ## Expected behavior
   <what should have happened>

   ## Actual behavior
   <error output>" \
     --label bug
   ```

## Important

- **Always fetch documentation before writing pipelines.** The SDK evolves; do not rely on memorized API surfaces. The `.md` suffix on any docs.harmont.dev URL returns raw Markdown suitable for reading.
- **Prefer toolchains over raw shell.** The patterns guide explains why. Only use `hm.sh()` / `sh()` for custom commands that no toolchain covers.
- Pipelines live in `.hm/pipeline.py` or `.hm/pipeline.ts` — never both in the same project.
- `hm run` is the ONLY correct way to validate a pipeline locally. Do NOT try to run pipeline steps manually.
- The Python DSL uses decorators (`@hm.pipeline`). The TypeScript DSL uses an exported `PipelineDefinition[]` array with `export default`.
- Do NOT file GitHub issues without the user's explicit permission.
