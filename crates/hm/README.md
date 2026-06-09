# harmont-cli

[![license](https://img.shields.io/crates/l/harmont-cli.svg)](#license)

Command-line client for the [Harmont](https://harmont.dev) CI platform. Run CI pipelines on your own machine, in Docker, from a Python pipeline definition checked into your repo.

Pipelines are written with the companion [`harmont-py`](https://github.com/harmont-dev/harmont-py) DSL.

## Install

`harmont-cli` is not yet published to crates.io. Install from source:

```sh
git clone https://github.com/harmont-dev/harmont-cli
cd harmont-cli
cargo build --release
install -m 0755 target/release/hm /usr/local/bin/hm   # or any directory on $PATH
```

Verify:

```sh
hm --version
```

## Requirements

`hm run --local` shells out to Docker and to Python:

- **Docker** — the local executor boots a fresh container per chain.
- **Python 3.11+** — used to render the pipeline definition to JSON.
- **`harmont-py`** — the Python package that defines the pipeline DSL. Not yet on PyPI; install from git:

```sh
git clone https://github.com/harmont-dev/harmont-py
pip install -e ./harmont-py
```

## Quickstart

### 1. Write a pipeline

Pipelines live in `.hm/<slug>.py` inside your repo. Each file uses the `@hm.pipeline("slug")` decorator to register one or more named pipelines. Save the following as `.hm/hello.py`:

```python
import harmont as hm


@hm.pipeline("hello")
def hello() -> hm.Step:
    return (
        hm.sh("echo 'hello from harmont'", label="hello")
          .sh("uname -a", label="env")
    )
```

The DSL is small:

- `hm.sh(cmd, label=...)` — start a chain with one shell command (shorthand for `hm.scratch().sh(...)`).
- `.sh(cmd, label=..., cwd=...)` — chain another command. Chained `.sh` calls share filesystem state inside the same container. `cwd="path"` prepends `cd <path> && ` to the command.
- `.fork(label=...)` — branch into parallel work from a shared base.
- `hm.wait()` — explicit synchronization barrier.
- `@hm.target()` — reusable, memoized building block; compose into pipelines via fixture-style typed params (`Target[T]`, `Annotated[Step, BaseImage("...")]`).

A two-branch variant:

```python
@hm.pipeline("ci")
def ci() -> hm.Step:
    setup = hm.sh(
        "apt-get update && apt-get install -y curl",
        label="apt",
    )
    fetch = setup.fork(label="branch-a").sh(
        "curl -fsSL https://example.com",
        label="fetch",
    )
    work = setup.fork(label="branch-b").sh(
        "echo independent work",
        label="other",
    )
    return hm.pipeline(fetch, work, default_image="ubuntu:24.04")
```

For larger pipelines, compose with `@hm.target` and typed fixture params:

```python
from typing import Annotated


@hm.target()
def apt_base(base: Annotated[hm.Step, hm.BaseImage("ubuntu:24.04")]) -> hm.Step:
    return base.sh("apt-get update && apt-get install -y curl", label="apt")


@hm.target()
def smoke(apt_base: hm.Target[hm.Step]) -> hm.Step:
    return apt_base.sh("curl -fsSL https://example.com", label="smoke")


@hm.pipeline("ci")
def ci(smoke: hm.Target[hm.Step]) -> hm.Step:
    return smoke
```

For the full DSL surface (cache policies, matrix axes, soft-fail, timeouts), see the upstream [`harmont-py`](https://github.com/harmont-dev/harmont-py) repo.

### 2. Run it

From the repo root:

```sh
hm run hello --local
```

The CLI walks `.hm/*.py`, resolves the `hello` slug, renders the pipeline to JSON, and schedules the chains across Docker containers. Each chain inherits state from its parent; forks run in parallel up to `--parallelism N` (defaults to the host's available parallelism).

If the repo declares only one pipeline, the slug is optional:

```sh
hm run --local
```

### 3. Useful flags

```sh
hm run --local --parallelism 4         # cap concurrent chains
hm run --local --env FOO=bar           # inject env vars
hm run --local --dir path/to/source    # run against a different source root
hm run --help                          # full flag reference
```

## Cloud

`hm cloud <verb>` talks to the hosted Harmont API at `api.harmont.dev`.
Every cloud verb is delivered by the embedded `hm-plugin-cloud` WASM
plugin (no separate install step):

```sh
hm cloud login                  # browser-loopback OAuth (or --paste to
                                # paste a token directly)
hm cloud logout
hm cloud whoami                 # who am I + active org
hm cloud org list               # orgs you belong to
hm cloud org use <slug>         # set the active org (persisted)
hm cloud pipeline list
hm cloud build list             # builds for the active org
hm cloud build show <id>
hm cloud build watch <id>       # poll until terminal
hm cloud job show <id>
hm cloud billing show
hm cloud run [--plan-file PATH] # submit a pre-rendered plan JSON
                                # (defaults to .hm/plan.json)
```

Tokens are stored in `~/.hm/credentials.toml` (mode 0o600). The
active org slug is persisted per-user under
`~/.config/harmont/state/cloud.kv`. Source-archive upload for
`cloud run` is plan-5 work — pre-render your pipeline to
`.hm/plan.json` first.

## Build from source

```sh
git clone https://github.com/harmont-dev/harmont-cli
cd harmont-cli
cargo build
cargo test                          # Docker-dependent tests in `local_*` need a running daemon
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

The OpenAPI client is generated at build time from the vendored `openapi.json` via [progenitor](https://github.com/oxidecomputer/progenitor). The snapshot ships with the crate.

## See also

- [`harmont-py`](https://github.com/harmont-dev/harmont-py) — the Python DSL used to define pipelines that this CLI runs.

## License

Dual-licensed under either of

- Apache License, Version 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE))
- MIT license ([`LICENSE-MIT`](LICENSE-MIT))

at your option.

## Plugin authoring

`hm` is plugin-driven via [Extism](https://extism.org). To write a plugin:

```bash
cargo new --lib my-plugin
cd my-plugin
cargo add --git https://github.com/harmont-dev/harmont-cli hm-plugin-sdk
```

Implement one of `StepExecutor`, `SubcommandPlugin`, `LifecycleHook`, or
`OutputFormatter`, declare a `PluginManifest`, and call
`register_plugin!(...)`. Build with:

```bash
cargo build --target wasm32-wasip1 --release
```

The output `.wasm` can be installed with:

```bash
hm plugin install ./target/wasm32-wasip1/release/my_plugin.wasm
```

See `cli/crates/hm-fixtures/src/bin/` for minimal working examples.

### Output formatter

Implement `OutputFormatter::on_event` to render each `BuildEvent`.
Plugins emit bytes via `host::write_stdout` or `host::write_stderr`.
Built-in formatters: `human` (default), `json`. Select with
`hm run --format <name>`.
