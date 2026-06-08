# Dagger Mirror of the Dogfood CI Pipeline — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Hand-build a Dagger (Python SDK) pipeline in a new top-level `comparison/` module that reproduces, step-for-step, the dogfood pipeline in `.harmont/ci.py`, so we can compare Dagger's authoring ergonomics against the harmont DSL.

**Architecture:** A single Dagger module (`comparison/`) exposes one `@function` per harmont step. A `shared_base` container (ubuntu:24.04 + apt) is forked by a Rust chain (`rustup → warmup → test/clippy/fmt`) and a Python chain (`uv install → uv sync → lint/fmt/typecheck/test`), exactly as `.harmont/ci.py` forks `shared_base` into `rust_project` and `py_project`. A top-level `ci` function runs all seven leaves concurrently. Every shell command is copied **verbatim** from what the harmont toolchains emit, so the only variable under comparison is the authoring surface — not the work done.

**Tech Stack:** Dagger v0.20.3 (already installed), Dagger Python SDK, Docker (already installed), `anyio` (ships with the SDK).

---

## Background: what we are mirroring

`.harmont/ci.py` is ~60 lines and produces this DAG (commands below are the *actual* strings emitted by the harmont toolchains — verified by reading `crates/hm-dsl-engine/harmont-py/harmont/{_toolchain,rust,py/uv}.py`):

**shared_base** — `apt_base(packages=...)` on `default_image="ubuntu:24.04"`, env `CI=true`:
```
apt-get update && apt-get install -y curl ca-certificates build-essential pkg-config libssl-dev python3 python3-venv
```

**rust_project** = `hm.rust.project(path=".", base=shared_base)`:
- install (rustup): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal --component clippy,rustfmt && . $HOME/.cargo/env && rustc --version && cargo --version`
- warmup: `. $HOME/.cargo/env && cd . && cargo build --workspace --tests --locked`
- `test(flags=("--lib",), packages=("harmont-cli",))` (forks warmup): `. $HOME/.cargo/env && cd . && cargo test -p harmont-cli --locked --lib`
- `clippy()` (forks warmup): `. $HOME/.cargo/env && cd . && cargo clippy --workspace --tests --locked -- -D warnings`
- `fmt()` (forks the toolchain, **not** warmup — no compile needed): `. $HOME/.cargo/env && cd . && cargo fmt --check`

**py_project** = `hm.py.uv(path="dsls/harmont-py", base=shared_base)`:
- install (uv): `curl -LsSf https://astral.sh/uv/install.sh | sh && ln -sf /root/.local/bin/uv /usr/local/bin/uv && uv --version`
- sync: `cd <PY_PATH> && uv sync --all-extras`
- `lint()`: `cd <PY_PATH> && uv run ruff check .`
- `fmt()`: `cd <PY_PATH> && uv run ruff format --check .`
- `typecheck(paths="harmont")`: `cd <PY_PATH> && uv run ty check harmont`
- `run("pytest -v --deselect tests/test_gradle.py --deselect tests/test_haskell.py")`: `cd <PY_PATH> && uv run pytest -v --deselect tests/test_gradle.py --deselect tests/test_haskell.py`

**Path note.** `.harmont/ci.py` passes `path="dsls/harmont-py"`. In this working tree the Python package lives at `crates/hm-dsl-engine/harmont-py` (that's the path `.github/workflows/ci.yml` uses, lines 55–67), and `dsls/harmont-py` does not resolve here. The Dagger mirror uses `PY_PATH = "crates/hm-dsl-engine/harmont-py"` so the Python leaves actually run; the string differs from `ci.py` only because the mirror has to point at a directory that exists on disk. If `dsls/harmont-py` is expected to resolve in the harmont run context, set `PY_PATH` to match it instead — it's a single constant.

**⚠️ Source upload / `Ignore` (discovered during execution — applies to every `source` param).** Dagger uploads the host directory passed to a `Directory` argument **verbatim and does not honor `.gitignore`**. This repo's `target/` dir is **33 GB**; an unguarded `--source=.` streams all of it into the engine and exhausts RAM. The fix is a shared annotated type used by every source-taking function:

```python
from typing import Annotated
from dagger import DefaultPath, Ignore

Source = Annotated[
    dagger.Directory,
    DefaultPath(".."),  # resolved relative to the module dir (comparison/) -> repo root
    Ignore(
        ["target", ".git", "comparison", "**/node_modules", "**/__pycache__", "**/.venv"]
    ),
]
```

All functions below take `source: Source` (NOT a bare `dagger.Directory`). Notes:
- `DefaultPath` is relative to the **module** directory (`comparison/`), so `".."` is the repo root and lets callers omit `--source`.
- `**/node_modules` **is** ignored (corrected during Task 3). harmont's container has no node and its git-tree snapshot omits the gitignored `node_modules`, so `hm-dsl-engine`'s `build.rs` finds no esbuild and writes **stub** TS bundles. Mounting the host `node_modules` would instead ship macOS esbuild binaries into a Linux container and break `cargo build`. Excluding it is therefore both faithful (stubs on both sides) and correct.
- This explicit-exclude requirement is itself a comparison finding: harmont snapshots the git tree (artifacts excluded implicitly); Dagger needs the `Ignore` annotation.
- Static type-checkers (Pyright) flag the `Source` alias with "Variable not allowed in type expression" and can't resolve `dagger`/`anyio` in the editor env. Both are false positives — Dagger resolves the annotation at runtime (verified: the source context correctly shows the exclude list). Leave them.

**Caching note (the interesting comparison axis).** Harmont caches the apt-base / install / warmup steps forever (or on lockfile change) and re-runs only the action leaves. Dagger gets the equivalent for free from its content-addressed layer cache **provided we mount the source directory only in the steps that need it, after the installs**. The plan is written so that `shared_base`, `rust_installed`, and the uv-install step never see the source — so a source change re-runs only the leaves, mirroring harmont.

---

## Conventions for this plan

- All `dagger` commands are run **from the repo root** (`/Users/marko/Desktop/harmont-cli`) using `-m comparison` so the `--source=.` argument resolves to the repo root.
- These pipelines do real work (compiling the whole Rust workspace, syncing uv, running pytest). Leaf calls can take **minutes** on a cold cache; that is expected and is part of what we're comparing. The warmup layer compiles once and is shared by `rust-test` and `rust-clippy`.
- "Verify it fails" for infra code = the function isn't exposed yet / the call errors. "Verify it passes" = the `dagger call` exits 0 and prints the leaf's stdout.
- Commit after every task. Branch is already `feat/dagger-comparison`.

---

## Task 0: Prerequisites & scaffold the module

**Files:**
- Create: `comparison/dagger.json` (generated)
- Create: `comparison/src/...` (generated SDK skeleton — exact path depends on the SDK scaffold)

**Step 1: Confirm tooling**

Run:
```bash
dagger version && docker info >/dev/null 2>&1 && echo "docker ok"
```
Expected: prints `dagger v0.20.3 ...` and `docker ok`. If docker is not running, start Docker Desktop first.

**Step 2: Initialize the Dagger module**

Run:
```bash
cd /Users/marko/Desktop/harmont-cli
dagger init --sdk=python --name=harmont-dagger ./comparison
```
Expected: creates `comparison/dagger.json` and a Python SDK skeleton. The generated `@object_type` class will be named `HarmontDagger` (derived from the module name).

**Step 3: Generate SDK bindings**

Run:
```bash
cd /Users/marko/Desktop/harmont-cli/comparison && dagger develop && cd ..
```
Expected: SDK client code is generated; a Python module file appears. Locate it:
```bash
find comparison -name '*.py' -not -path '*/sdk/*' -not -path '*/.venv/*'
```
Note the path of the file containing `class HarmontDagger` — this is the file you edit in every subsequent task. (Commonly `comparison/src/harmont_dagger/main.py` or `comparison/src/main/__init__.py`; use whatever `find` reports.)

**Step 4: Verify the empty module loads**

Run:
```bash
dagger -m comparison functions
```
Expected: lists the scaffold's default function(s) with no errors. This proves the module loads before we add anything.

**Step 5: Commit**

```bash
git add comparison/dagger.json comparison/src
git commit -m "chore(comparison): scaffold dagger python module"
```

---

## Task 1: `shared_base` — the apt base container

**Files:**
- Modify: the module file located in Task 0 (referred to below as `MAIN`)

**Step 1: Replace the module file with the base container**

Replace the entire contents of `MAIN` with the header + `shared_base` (keep the class name that `dagger init` generated):

```python
"""Dagger mirror of the dogfood CI pipeline in .harmont/ci.py.

Hand-wired equivalent of the harmont pipeline, for comparing Dagger's
authoring ergonomics against the harmont DSL. Every shell command below is
copied verbatim from what the harmont toolchains emit
(harmont.rust / harmont.py.uv / harmont._toolchain).
"""

import anyio
import dagger
from dagger import dag, function, object_type

UBUNTU = "ubuntu:24.04"

# Packages from .harmont/ci.py shared_base().
APT_PACKAGES = (
    "curl ca-certificates build-essential pkg-config libssl-dev "
    "python3 python3-venv"
)


@object_type
class HarmontDagger:
    @function
    def shared_base(self) -> dagger.Container:
        """ubuntu:24.04 + apt packages + CI=true (mirrors hm.apt_base)."""
        return (
            dag.container()
            .from_(UBUNTU)
            .with_env_variable("CI", "true")
            .with_exec(
                [
                    "sh",
                    "-c",
                    f"apt-get update && apt-get install -y {APT_PACKAGES}",
                ]
            )
        )
```

**Step 2: Verify the function is exposed**

Run:
```bash
dagger -m comparison functions
```
Expected: `shared-base` appears in the list.

**Step 3: Run it to verify it builds green**

Run:
```bash
dagger -m comparison call shared-base with-exec --args="sh,-c,echo base-ok" stdout
```
Expected: prints `base-ok` (the apt install layer ran, then our echo). First run pulls ubuntu + runs apt; subsequent runs are cached.

**Step 4: Commit**

```bash
git add comparison
git commit -m "feat(comparison): dagger shared_base container"
```

---

## Task 2: `rust_installed` + `rust_fmt` (cheapest Rust leaf first)

`fmt` forks the toolchain (no warmup build), so it's the fastest Rust leaf to validate the chain end-to-end.

**Files:**
- Modify: `MAIN`

**Step 1: Add the rustup constant**

Add below `APT_PACKAGES`:
```python
# rustup install — verbatim from harmont.rust._rustup_cmd("stable", ("clippy", "rustfmt")).
RUSTUP = (
    "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | "
    "sh -s -- -y --default-toolchain stable --profile minimal "
    "--component clippy,rustfmt && . $HOME/.cargo/env && "
    "rustc --version && cargo --version"
)
```

**Step 2: Add `rust_installed` and `rust_fmt` methods**

Add inside the class:
```python
    # ---- Rust: mirrors hm.rust.project(path=".", base=shared_base) ----

    @function
    def rust_installed(self) -> dagger.Container:
        """shared_base + rustup stable with clippy & rustfmt. No source mounted."""
        return self.shared_base().with_exec(["sh", "-c", RUSTUP])

    @function
    async def rust_fmt(self, source: Source) -> str:
        """cargo fmt --check. Forks the toolchain (no warmup build), as harmont does."""
        return await (
            self.rust_installed()
            .with_directory("/src", source)
            .with_workdir("/src")
            .with_exec(
                ["sh", "-c", ". $HOME/.cargo/env && cd . && cargo fmt --check"]
            )
            .stdout()
        )
```

**Step 3: Verify functions exposed**

Run: `dagger -m comparison functions`
Expected: `rust-installed` and `rust-fmt` listed.

**Step 4: Run `rust-fmt` to verify green**

Run:
```bash
dagger -m comparison call rust-fmt --source=.
```
Expected: exits 0 (the repo is rustfmt-clean on this branch). Installs rustup on first run, then runs `cargo fmt --check`.

**Step 5: Commit**

```bash
git add comparison
git commit -m "feat(comparison): dagger rust toolchain + fmt leaf"
```

---

## Task 3: `rust_warmup` + `rust_test` + `rust_clippy`

`test` and `clippy` both fork the warmup container, so the workspace compiles **once** and is reused — mirroring harmont's warmup snapshot fork.

**Files:**
- Modify: `MAIN`

**Step 1: Add `rust_warmup`, `rust_test`, `rust_clippy`**

Add inside the class, after `rust_installed`:
```python
    @function
    def rust_warmup(self, source: Source) -> dagger.Container:
        """rust_installed + source + the warmup build that test/clippy fork from."""
        return (
            self.rust_installed()
            .with_directory("/src", source)
            .with_workdir("/src")
            .with_exec(
                [
                    "sh",
                    "-c",
                    ". $HOME/.cargo/env && cd . && "
                    "cargo build --workspace --tests --locked",
                ]
            )
        )

    @function
    async def rust_test(self, source: Source) -> str:
        """cargo test -p harmont-cli --locked --lib (forks warmup)."""
        return await (
            self.rust_warmup(source)
            .with_exec(
                [
                    "sh",
                    "-c",
                    ". $HOME/.cargo/env && cd . && "
                    "cargo test -p harmont-cli --locked --lib",
                ]
            )
            .stdout()
        )

    @function
    async def rust_clippy(self, source: Source) -> str:
        """cargo clippy --workspace --tests --locked -- -D warnings (forks warmup)."""
        return await (
            self.rust_warmup(source)
            .with_exec(
                [
                    "sh",
                    "-c",
                    ". $HOME/.cargo/env && cd . && "
                    "cargo clippy --workspace --tests --locked -- -D warnings",
                ]
            )
            .stdout()
        )
```

**Step 2: Verify functions exposed**

Run: `dagger -m comparison functions`
Expected: `rust-warmup`, `rust-test`, `rust-clippy` listed.

**Step 3: Run `rust-test` (compiles workspace — may take several minutes)**

Run:
```bash
dagger -m comparison call rust-test --source=.
```
Expected: exits 0, prints `cargo test` output (test results for `harmont-cli` lib tests). Note: requires `npm ci` in `crates/hm-dsl-engine/harmont-ts/` for the build — see CLAUDE.md. If the build fails on the esbuild bundle, that is a real workspace prerequisite, not a Dagger bug; the source mount includes the whole repo so the build script runs as it does locally. If it fails for this reason, note it and continue (clippy/test parity with harmont still holds — harmont hits the same prerequisite).

**Step 4: Run `rust-clippy` to confirm warmup is reused**

Run:
```bash
dagger -m comparison call rust-clippy --source=.
```
Expected: exits 0. Should start fast because the warmup layer is cached from Step 3.

**Step 5: Commit**

```bash
git add comparison
git commit -m "feat(comparison): dagger rust warmup + test + clippy leaves"
```

---

## Task 4: `py_synced` + the four Python leaves

**Files:**
- Modify: `MAIN`

**Step 1: Add the uv + path constants**

Add below the `RUSTUP` constant:
```python
# uv install — verbatim from harmont.py.uv._uv_install_cmd("latest").
UV_INSTALL = (
    "curl -LsSf https://astral.sh/uv/install.sh | sh && "
    "ln -sf /root/.local/bin/uv /usr/local/bin/uv && uv --version"
)

# .harmont/ci.py passes path="dsls/harmont-py"; in this tree the package is at
# crates/hm-dsl-engine/harmont-py (the path .github/workflows/ci.yml uses). Point
# at the directory that exists on disk so the Python leaves run.
PY_PATH = "crates/hm-dsl-engine/harmont-py"
```

**Step 2: Add `py_synced` and the leaves**

Add inside the class:
```python
    # ---- Python uv: mirrors hm.py.uv(path=PY_PATH, base=shared_base) ----

    @function
    def py_synced(self, source: Source) -> dagger.Container:
        """shared_base + uv install + uv sync --all-extras."""
        return (
            self.shared_base()
            .with_exec(["sh", "-c", UV_INSTALL])
            .with_directory("/src", source)
            .with_workdir("/src")
            .with_exec(["sh", "-c", f"cd {PY_PATH} && uv sync --all-extras"])
        )

    @function
    async def py_lint(self, source: Source) -> str:
        return await (
            self.py_synced(source)
            .with_exec(["sh", "-c", f"cd {PY_PATH} && uv run ruff check ."])
            .stdout()
        )

    @function
    async def py_fmt(self, source: Source) -> str:
        return await (
            self.py_synced(source)
            .with_exec(
                ["sh", "-c", f"cd {PY_PATH} && uv run ruff format --check ."]
            )
            .stdout()
        )

    @function
    async def py_typecheck(self, source: Source) -> str:
        return await (
            self.py_synced(source)
            .with_exec(["sh", "-c", f"cd {PY_PATH} && uv run ty check harmont"])
            .stdout()
        )

    @function
    async def py_test(self, source: Source) -> str:
        return await (
            self.py_synced(source)
            .with_exec(
                [
                    "sh",
                    "-c",
                    f"cd {PY_PATH} && uv run pytest -v "
                    "--deselect tests/test_gradle.py "
                    "--deselect tests/test_haskell.py",
                ]
            )
            .stdout()
        )
```

**Step 3: Verify functions exposed**

Run: `dagger -m comparison functions`
Expected: `py-synced`, `py-lint`, `py-fmt`, `py-typecheck`, `py-test` listed.

**Step 4: Run the cheapest leaf first (`py-lint`)**

Run:
```bash
dagger -m comparison call py-lint --source=.
```
Expected: exits 0. Installs uv, runs `uv sync --all-extras`, then `ruff check .` in `crates/hm-dsl-engine/harmont-py`.

**Step 5: Run the remaining Python leaves**

Run each; each should exit 0 (sync layer reused from Step 4):
```bash
dagger -m comparison call py-fmt --source=.
dagger -m comparison call py-typecheck --source=.
dagger -m comparison call py-test --source=.
```
Expected: all exit 0. `py-test` prints pytest output.

**Step 6: Commit**

```bash
git add comparison
git commit -m "feat(comparison): dagger python uv sync + lint/fmt/typecheck/test leaves"
```

---

## Task 5: `ci` — aggregate all leaves concurrently

This mirrors `@hm.pipeline("ci")` returning `[rust_project, py_project]`: all leaves run, the pipeline fails if any leaf fails. Dagger dedupes the shared warmup/sync work, so `rust-test` and `rust-clippy` share one workspace compile and the four Python leaves share one `uv sync`.

**Files:**
- Modify: `MAIN`

**Step 1: Add the `ci` function**

Add inside the class (last method):
```python
    # ---- Aggregate: mirrors @hm.pipeline("ci") -> [rust_project, py_project] ----

    @function
    async def ci(self, source: Source) -> str:
        """Run every leaf concurrently; fail if any leaf fails."""
        results: dict[str, str] = {}

        async def run(name: str, coro) -> None:
            results[name] = await coro

        async with anyio.create_task_group() as tg:
            tg.start_soon(run, "rust_test", self.rust_test(source))
            tg.start_soon(run, "rust_clippy", self.rust_clippy(source))
            tg.start_soon(run, "rust_fmt", self.rust_fmt(source))
            tg.start_soon(run, "py_lint", self.py_lint(source))
            tg.start_soon(run, "py_fmt", self.py_fmt(source))
            tg.start_soon(run, "py_typecheck", self.py_typecheck(source))
            tg.start_soon(run, "py_test", self.py_test(source))

        return "\n".join(
            f"=== {name} ===\n{out}" for name, out in sorted(results.items())
        )
```

**Step 2: Verify `ci` is exposed**

Run: `dagger -m comparison functions`
Expected: `ci` listed alongside the seven leaves.

**Step 3: Run the full pipeline**

Run:
```bash
dagger -m comparison call ci --source=.
```
Expected: exits 0; prints a `=== <leaf> ===` section per leaf. If a leaf fails, the task group propagates the failure and `ci` exits non-zero — matching harmont's all-or-nothing semantics. (Compare wall-clock against `hm run ci` mentally: both compile once and fan out.)

**Step 4: Commit**

```bash
git add comparison
git commit -m "feat(comparison): dagger ci aggregate running all leaves concurrently"
```

---

## Task 6: Final parity check

**Step 1: Read `.harmont/ci.py` and the Dagger `MAIN` side by side**

Confirm every harmont step has a Dagger counterpart with an identical command string:

| harmont (`.harmont/ci.py`) | Dagger function | command identical? |
|---|---|---|
| `apt_base(...)` | `shared_base` | ✅ |
| `rust.project(...).warmup` | `rust_warmup` | ✅ |
| `rust.project.test(--lib, -p harmont-cli)` | `rust_test` | ✅ |
| `rust.project.clippy()` | `rust_clippy` | ✅ |
| `rust.project.fmt()` | `rust_fmt` | ✅ |
| `py.uv(...).lint()` | `py_lint` | ✅ |
| `py.uv(...).fmt()` | `py_fmt` | ✅ |
| `py.uv(...).typecheck(paths="harmont")` | `py_typecheck` | ✅ |
| `py.uv(...).run("pytest ...")` | `py_test` | ✅ |

**Step 2: Confirm the run commands for the comparison are documented in the module docstring**

The harmont invocation is `hm run ci`. The Dagger invocation is `dagger -m comparison call ci --source=.`. Both are single commands. (No separate comparison doc per the agreed scope — the side-by-side is the two source files plus this table.)

**Step 3: Final commit (if anything changed)**

```bash
git add comparison docs/plans/2026-05-28-dagger-comparison-pipeline.md
git commit -m "docs(comparison): dagger/harmont parity plan + checklist"
```

---

## Notes for the executor

- **Class name:** Use whatever name `dagger init` generated for the `@object_type` class; the plan assumes `HarmontDagger`. If it differs, keep init's name and adjust the snippets' `class` line accordingly — nothing else changes.
- **Generated SDK file location varies by SDK version.** Always edit the file `find` reported in Task 0, Step 3.
- **Do not add a `.harmont`-style trigger concept to Dagger.** Dagger has no triggers; `push`/`pr` in harmont have no Dagger equivalent — that *absence* is itself a comparison finding, not a gap to fill.
- **Cold-cache runs are slow by design.** Don't shorten commands to make them faster; the whole point is that the work is identical and only the authoring differs.
- **If the Rust build needs `npm ci`** in `crates/hm-dsl-engine/harmont-ts/` (per CLAUDE.md), that prerequisite applies equally to harmont and Dagger; note it but don't treat it as a Dagger-specific failure.
