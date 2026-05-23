# harmont-py

[![license](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Python DSL for defining [Harmont](https://harmont.dev) CI pipelines.

Pipelines are chains of shell commands, branched with `.fork()`, synchronized with `hm.wait()`, registered with a decorator, and rendered to a JSON IR. The companion [`harmont-cli`](https://github.com/harmont-dev/harmont-cli) consumes that IR and runs the pipeline locally in Docker or on the hosted Harmont cloud.

The package installs as `harmont` and you import it as `harmont`:

```python
import harmont as hm
```

## Quick start

### 1. Write a pipeline

A pipeline file lives at `.harmont/<slug>.py` in your repo:

```python
import harmont as hm


@hm.pipeline("hello")
def hello() -> hm.Step:
    return (
        hm.sh("echo 'hello from harmont'", label="hello")
            .sh("uname -a", label="env")
    )
```

### 2. Install

Not yet on PyPI. Install from source (Python 3.11+):

```sh
git clone https://github.com/harmont-dev/harmont-py
cd harmont-py
pip install -e .
```

If you arrived here from the [`harmont-cli`](https://github.com/harmont-dev/harmont-cli) Quick start, you already did this — skip to Step 3.

Development extras (pytest, mypy, ruff):

```sh
pip install -e '.[dev]'
```

### 3. Run

Use the [Harmont CLI](https://github.com/harmont-dev/harmont-cli):

```sh
hm run hello
```

`hm run` walks `.harmont/*.py`, imports each file (triggering the decorators), renders the registered pipeline to JSON, and executes it (locally in Docker by default, or against the cloud via `hm cloud run`).

## DSL surface

| Primitive | Returns | What it does |
|---|---|---|
| `hm.sh(cmd, cwd=..., label=...)` | `Step` | Start a chain in one call (= `hm.scratch().sh(cmd, ...)`) |
| `hm.scratch()` | `Step` | Empty root; chain with `.sh(...)` for an explicit start |
| `Step.sh(cmd, cwd=..., ...)` | `Step` | Run a shell command; chained `.sh` shares container state |
| `Step.fork(label=...)` | `Step` | Branch a shared base into parallel work |
| `hm.wait()` | `Step` | Explicit synchronization barrier |
| `@hm.target()` | decorator | Reusable, memoized building block |
| `@hm.pipeline("slug")` | decorator | Register a pipeline (multiple per file are fine) |
| `hm.pipeline(*leaves, env=..., default_image=...)` | `dict` | Factory form — build the v0 IR dict directly (used in tests) |

Cache policies (`hm.ttl`, `hm.on_change`, `hm.forever`, `hm.compose`), triggers (`hm.push`, `hm.pull_request`, `hm.schedule`), and matrix axes are documented in the module docstrings; start at `harmont/__init__.py`.

## Language toolchains

`harmont` ships first-class wrappers for the common toolchains. Each exposes the actions that make sense for that ecosystem (e.g. `.build()`, `.test()`, `.clippy()`, `.fmt()` for Rust; `.test()`, `.lint()`, `.fmt()`, `.typecheck()` for Python):

| Call | Project type |
|---|---|
| `hm.rust(path=..., version="stable")` | cargo + clippy + rustfmt |
| `hm.haskell(ghc="9.6.7", cabal="latest")` | cabal (call `.cabal(path)` to build a package) |
| `hm.python(path=..., uv_version="latest")` | uv-based Python project |
| `hm.go(path=..., version="1.23.2")` | go build/test/vet/fmt |
| `hm.npm(path=..., version="20")` | npm + arbitrary scripts |
| `hm.gradle(path=..., jdk="21", kotlin=False)` | Java or Kotlin via Gradle |
| `hm.cmake(path=..., lang="c"\|"cpp")` | C/C++ via CMake + CTest |
| `hm.dotnet(path=..., channel="8.0")` | .NET via dotnet CLI |
| `hm.ruby(path=..., version="default")` | Bundler + Rake |
| `hm.ocaml(path=..., compiler="5.1.1")` | opam + Dune |
| `hm.zig(path=..., version="0.13.0")` | zig build/test/fmt |
| `hm.perl(path=...)` | cpanm + prove |
| `hm.composer(path=..., laravel=False)` | PHP / Laravel via Composer |
| `hm.elm(path=..., elm_version="0.19.1")` | Elm |

Working examples for each toolchain live in [`harmont-cli/examples/`](https://github.com/harmont-dev/harmont-cli/tree/main/examples).

## Composing with targets

For larger pipelines, factor toolchain setup into `@hm.target()` and let pipelines depend on them by parameter name. `Target[T]` and `Annotated[Step, BaseImage("...")]` are typed markers that unwrap cleanly under mypy and pyright.

```python
from typing import Annotated

import harmont as hm
from harmont.haskell import HaskellPackage, HaskellToolchain


@hm.target()
def apt_base(base: Annotated[hm.Step, hm.BaseImage("ubuntu-24.04")]) -> hm.Step:
    return base.sh("apt-get update").sh("apt-get install -y python3")


@hm.target()
def ghc() -> HaskellToolchain:
    return hm.haskell(ghc="9.6.7")


@hm.target()
def api(ghc: hm.Target[HaskellToolchain]) -> HaskellPackage:
    return ghc.cabal(path="api")


@hm.pipeline("ci")
def ci(
    apt_base: hm.Target[hm.Step],
    api: hm.Target[HaskellPackage],
) -> tuple[hm.Step, ...]:
    return (apt_base.sh("./run-smoke"), api)
```

Every fixture parameter must carry a marker or default value; unmarked parameters raise at decoration time. Memoization scope is one `dump_registry_json` render, so two targets that depend on the same `apt_base` share a single step.

<details>
<summary>How rendering works</summary>

`hm.sh(...).sh(...)` builds a chain of frozen `Step` dataclasses. Each `.sh()` returns a new `Step` carrying the parent reference. The `hm.pipeline()` factory walks back from each leaf, topo-sorts, and emits a `version: "0"` IR dict matching the schema in `harmont-pipeline` (Haskell side).

When used as a decorator, `@hm.pipeline("slug")` registers the wrapped function with a module-level registry. `hm.dump_registry_json()` walks every `.harmont/*.py`, imports each (which triggers the decorators), and returns the full envelope.

A chain edge — `parent.sh(cmd, ...)` — emits `builds_in: "<parent key>"` in the v0 IR JSON. The edge encodes synchronisation and state inheritance: the local executor reuses the parent's container; the cloud planner boots from its snapshot. A step rooted at `scratch()` has `builds_in: null` and boots from `image="..."` (or the pipeline's `default_image`) locally; the cloud planner ignores `image` (it always boots from the Freestyle base).

The JSON wire format and cache-key algorithm are stable; see module docstrings under `harmont/` for the contract.

</details>

## Build & test

```sh
python3 -m venv .venv && source .venv/bin/activate
pip install -e '.[dev]'

pytest                                  # all tests
pytest -v --tb=short
mypy --strict harmont
ruff check .
```

`pytest` is configured to treat warnings as errors (`filterwarnings = ["error"]`).

## See also

- [`harmont-cli`](https://github.com/harmont-dev/harmont-cli) — the CLI that runs pipelines defined with this package (`hm run`).

## License

MIT. See [`LICENSE`](LICENSE).
