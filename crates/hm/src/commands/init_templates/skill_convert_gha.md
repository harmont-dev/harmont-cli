---
name: convert-gha
description: Convert GitHub Actions workflows to Harmont pipelines. Use when the user has existing `.github/workflows/` YAML files and wants to migrate their CI to Harmont. Reads each workflow, maps GHA concepts to Harmont equivalents, explains differences, and delegates to the write-pipeline skill for the actual pipeline creation.
---

Convert existing GitHub Actions workflows (`.github/workflows/*.yml` / `*.yaml`) into Harmont CI pipelines. This skill reads your GHA configuration, explains what maps directly and what changes, then uses the `write-pipeline` skill to produce the Harmont pipeline.

## When to use

- The user asks to "convert", "migrate", or "port" their GitHub Actions to Harmont
- The user says "I have GHA workflows" and wants Harmont equivalents
- The user asks "how do I replace GitHub Actions with Harmont"
- `hm init` told the user about this skill after detecting `.github/workflows/`

## When NOT to use

- The user wants to write a Harmont pipeline from scratch — use the `write-pipeline` skill directly
- The user wants to keep using GitHub Actions alongside Harmont (dual CI setup)
- The user is debugging an existing Harmont pipeline — use the `validate-ci` skill

## Before you start

1. **Read every workflow file** in `.github/workflows/`:
   ```bash
   find .github/workflows -name '*.yml' -o -name '*.yaml' | sort
   ```
   Read each file. Understand what each workflow does, what triggers it, and what its jobs and steps accomplish.

2. **Fetch the Harmont patterns guide** — required before writing anything:
   ```
   WebFetch https://docs.harmont.dev/pipeline-sdk/patterns.md
   ```

3. **Fetch the GHA migration example** if available:
   ```
   WebFetch https://docs.harmont.dev/examples/github-actions.md
   ```

## Procedure

1. **Inventory the GHA workflows.** For each `.yml` file, note:
   - Workflow name and trigger events (`on: push`, `on: pull_request`, `on: schedule`, etc.)
   - Each job: its name, `runs-on`, and what it does
   - Dependencies between jobs (`needs:`)
   - Services used (`services:`)
   - Secrets and environment variables referenced
   - Caching steps (`actions/cache`, `actions/setup-*` with built-in caching)
   - Artifacts (`actions/upload-artifact`, `actions/download-artifact`)
   - Matrix strategies

2. **Map GHA concepts to Harmont.** Present the user with a summary table covering what they have and how it translates:

   | GHA concept | Harmont equivalent | Notes |
   |---|---|---|
   | `on: push` / `on: pull_request` | `push` / `pull_request` triggers | Direct mapping — same semantics |
   | `on: schedule` (cron) | `schedule` trigger | Direct mapping |
   | `jobs.<id>.steps` | Chain of toolchain calls or `sh()` | Each meaningful step becomes a Harmont step |
   | `jobs.<id>.needs` | `.fork()` for parallel, sequential chain for dependencies | Harmont DAG is implicit from chain structure |
   | `actions/cache` | **Not needed — caching is implicit in Harmont** | Harmont automatically caches build artifacts, dependency installs, and toolchain outputs between runs. Remove all cache steps. |
   | `actions/setup-*` (setup-node, setup-python, etc.) | Harmont toolchains (`hm.npm`, `hm.python`, etc.) | Toolchains handle installation. Specify version via toolchain config. |
   | `actions/checkout` | **Not needed — source is always available** | Harmont automatically provides the source code to every step. |
   | `runs-on: ubuntu-latest` | `default_image: "ubuntu:24.04"` | Harmont runs steps in Docker containers |
   | `services:` (e.g., postgres) | Service containers in step config | Check docs for service container syntax |
   | `matrix:` | Multiple pipelines or parameterized steps | No direct matrix — may need separate pipeline definitions or `.fork()` |
   | `env:` / `secrets.*` | `env: {}` on pipeline or step | Secrets must be passed as environment variables |
   | `actions/upload-artifact` / `actions/download-artifact` | Step outputs and DAG dependencies | Harmont passes outputs between steps via the DAG |
   | `if:` conditionals | Pipeline-level logic (Python/TS) | Use the DSL's native control flow |

3. **Be honest about differences.** After presenting the mapping, explain:
   - **What's simpler:** Caching is implicit — no `actions/cache` boilerplate. No `actions/checkout` needed. Toolchains replace `actions/setup-*` with cleaner configuration.
   - **What's different:** Matrix strategies don't have a direct equivalent — you may need multiple pipeline definitions or `.fork()`. Service containers have different syntax. Complex `if:` conditionals become DSL-level control flow.
   - **What's a real gap:** Only mention a gap if functionality genuinely cannot be replicated. Do NOT invent problems — most GHA workflows map cleanly. Common real gaps: GHA marketplace actions that have no Harmont toolchain equivalent (use `sh()` with the underlying commands instead), GitHub-specific features like `github.event` context or `GITHUB_TOKEN` permissions.

4. **Delegate to the `write-pipeline` skill.** Once the user understands the mapping, invoke the `write-pipeline` skill to create the actual Harmont pipeline. Tell it:
   - What language/build system the project uses (detected from the GHA workflow)
   - The trigger configuration (mapped from GHA `on:`)
   - The step structure (mapped from GHA jobs and steps)
   - Any environment variables or services needed

5. **Validate the converted pipeline:**
   ```bash
   hm render <pipeline-slug>
   ```
   Then:
   ```bash
   hm run
   ```

6. **Summarize what changed.** After the pipeline works, tell the user:
   - Which GHA workflows were converted and what the Harmont pipeline covers
   - What was simplified (cache removal, checkout removal)
   - Any GHA features that were intentionally dropped and why
   - Remind them they can safely delete `.github/workflows/` once they're satisfied with the Harmont pipeline (but suggest keeping it until they've verified on a real push)

## Important

- **Read ALL GHA workflow files before starting.** Don't skip workflows — the user expects a complete migration.
- **Do NOT fabricate differences.** Only flag a gap when something genuinely can't be done in Harmont. `actions/cache` removal is a simplification, not a gap.
- **Delegate pipeline writing to the `write-pipeline` skill.** This skill handles analysis and mapping; `write-pipeline` handles the actual Harmont SDK usage and documentation fetching.
- **One Harmont pipeline can replace multiple GHA workflows** if they share the same trigger. Consolidation is usually the right call.
- The user's GHA workflows are the source of truth for what their CI does. Don't assume — read them.
