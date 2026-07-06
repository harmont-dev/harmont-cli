# Contributing to Harmont

Thank you for your interest in making Harmont better — we're glad you're here.
Contributions of every size are valued, whether that's a typo fix, a bug
report with a great reproduction, or a new feature. This document explains how
we work together, how to get your environment running, and how to get a change
merged.

If you just want to chat or ask a question, join us on
[Discord](https://discord.gg/hm-dev) or
[Slack](https://join.slack.com/t/harmont-dev/shared_invite/zt-3yt0tiv7r-qHm1O0p0nVh2GU~KKhUk9A).

## Finding something to work on

- Issues labeled [`good first issue`](https://github.com/harmont-dev/harmont-cli/labels/good%20first%20issue)
  are scoped for newcomers.
- Issues labeled [`help wanted`](https://github.com/harmont-dev/harmont-cli/labels/help%20wanted)
  are ready for anyone to pick up.
- Found a bug? [File an issue](#filing-issues) — a good bug report is a
  first-class contribution on its own.

## Before you write code: open an issue

**Every pull request must reference an issue that a maintainer has agreed
should be fixed or built.** This applies to small fixes too — an issue takes
two minutes to file and it means nobody's work gets wasted on a change we
can't merge or that someone else is already making.

The flow is:

1. Open an issue (or comment on an existing one) describing the bug or the
   feature and how you'd like to approach it.
2. Wait for a maintainer to confirm the direction. For bug fixes this is
   usually quick; for features expect some design discussion first — a new
   feature is a long-term maintenance commitment, so we want consensus before
   anyone invests time in code.
3. Comment that you're taking it, then go build it.

Pull requests for features that were never discussed in an issue will
usually be closed with a pointer back to this section. It isn't personal —
it's how we protect both your time and ours.

You can expect a first response on issues and PRs within a week. If you
haven't heard anything by then, feel free to ping the thread.

## Use of AI

AI-assisted contributions are welcome — much of Harmont is built with AI in
the loop. The requirement is that **you** understand and stand behind what
you submit: you can explain the change, you've run the tests, and the PR
description is written (or at minimum, curated) by you. Please keep
descriptions concise and specific; a giant auto-generated summary suggests
the author doesn't know what the change does, and PRs that show no human
understanding will be closed.

## Setting up your environment

Prerequisites:

- **Rust** — latest stable via [rustup](https://rustup.rs). The workspace
  uses edition 2024, so you need a recent stable (1.85+). Formatting is
  pinned to **rustfmt 1.96** in CI; if your local `cargo fmt` produces
  unrelated diffs, update your toolchain (`rustup update stable`).
- **Docker** — a running daemon. The local execution backend runs every
  pipeline step in a container, and the integration tests do too. On Linux,
  the layer-caching snapshotter also needs FUSE's `user_allow_other` enabled
  in `/etc/fuse.conf`.
- **Python 3.11+** and [uv](https://docs.astral.sh/uv/) — for the `harmont`
  pipeline DSL that lives in `crates/hm-dsl-engine/harmont-py/`. Linting is
  pinned to **ruff 0.15** in CI.

Build everything from the workspace root:

```sh
cargo build
```

Run your freshly built CLI against a real project — the `examples/`
directory has fourteen runnable projects to try it on:

```sh
cargo run -p harmont-cli -- run ci --backend docker --dir examples/rust
```

## Running the checks

CI runs the same checks you can run locally. Before pushing, this block
should pass from the workspace root:

```sh
cargo test --workspace --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

And for the Python DSL, from `crates/hm-dsl-engine/harmont-py/`:

```sh
uv sync --all-extras
uv run pytest -v
uv run ruff format --check .
uv run ruff check .
uv run ty check harmont
```

Two things that surprise people:

- **Clippy is strict.** The workspace denies `unwrap()`, `expect()`,
  `panic!`, `todo!`, `dbg!`, and direct `print!`/`eprintln!` in library
  code, and warns on the full pedantic and nursery sets. Write error
  handling with `?` and real error types.
- **pytest treats warnings as errors.** A deprecation warning from a
  dependency is a test failure; don't suppress it without a comment.

You can also run the repo's own CI pipeline exactly as CI does — Harmont
dogfoods itself via `.hm/ci.py`:

```sh
hm run ci --backend docker                     # locally, in Docker
hm cloud login && hm run ci --org <your-org>   # or on Harmont Cloud
```

These lines use an installed `hm`; if you only have the freshly built
workspace, substitute `cargo run -p harmont-cli --` for `hm`. The repo's
`.hm/config.toml` pins the maintainer's cloud org, so pass `--org` to
submit cloud runs to your own.

Signing up at [app.harmont.dev](https://app.harmont.dev) gets you cloud runs
of this same pipeline, which is the fastest way to reproduce a CI result.

### Running a single test

```sh
cargo test -p harmont-cli --test cmd_init             # one Rust integration test file
cd crates/hm-dsl-engine/harmont-py && uv run pytest tests/test_rust.py -k <name>
```

The Docker-backed integration tests (for example `crates/hm/tests/keep_going.rs`)
are marked `#[ignore]`; run them with a live Docker daemon via
`cargo test -p harmont-cli --test keep_going -- --ignored`.

### Snapshot tests

`hm-pipeline-ir` uses [insta](https://insta.rs) JSON snapshots. If you
change the pipeline IR schema, review and accept the new snapshots with:

```sh
cargo insta review
```

## How the workspace fits together

| Crate | What it is |
|---|---|
| `crates/hm` | The `hm` binary (package `harmont-cli`) — command-line client for the Harmont CI platform. |
| `crates/hm-exec` | Pluggable CI execution backends: local and cloud. |
| `crates/hm-vm` | Local Docker backends that run pipeline steps on your machine. |
| `crates/hm-render` | Build-event renderers (human, with progress bars, and JSON). |
| `crates/hm-dsl-engine` | Evaluates Python pipeline definitions; contains the `harmont` Python package (`harmont-py/`). |
| `crates/hm-pipeline-ir` | The pipeline IR wire-format schema. |
| `crates/hm-config` | Layered configuration and credential storage. |
| `crates/hm-plugin-cloud` | Cloud client library. |
| `crates/hm-plugin-protocol` | Wire types shared between `hm` and plugins. |
| `crates/hm-util` | Shared OS and filesystem utilities. |

A useful mental model: the DSL engine evaluates your `.hm/*.py` pipeline
into IR, an execution backend (local Docker or Harmont Cloud) schedules it
as a DAG, and the binary renders the resulting event stream in your
terminal.

One sync rule to know about: the Python SDK in
`crates/hm-dsl-engine/harmont-py/`, the `hm init` templates in
`crates/hm/src/commands/init_templates/`, and the generated reference docs
on docs.harmont.dev must stay in agreement. If you change a toolchain
helper, update the matching template (roundtrip-tested by
`crates/hm/tests/cmd_init.rs`) and refresh the docstring it is generated
from; a maintainer will regenerate the published docs during review.

## Debugging

Every `hm` command accepts `-v`/`--verbose` for debug-level logging. For
finer control, set `RUST_LOG` with standard tracing filter directives
(for example `RUST_LOG=hm_exec=trace`); when set, it takes precedence over
the flag. To see the raw `BuildEvent` stream that the renderers consume,
run with `hm run --format json`.

When chasing a local-execution bug, the fastest loop is usually a minimal
pipeline in one of the `examples/` projects plus `--verbose`.
For cloud-side behavior, `hm cloud login` followed by
`hm run ci --org <your-org>` against your own
[app.harmont.dev](https://app.harmont.dev) org reproduces what CI sees;
the `--org` flag overrides the org pinned in `.hm/config.toml`.

## Submitting a pull request

- Branch from `main` in your fork.
- Keep the PR scoped to its issue — resist drive-by refactors; file another
  issue instead.
- PRs are **squash-merged**, so your PR title becomes the commit that lands.
  Write it as a [Conventional Commit](https://www.conventionalcommits.org):
  `fix(dsl): reject empty pipeline names`, `feat(render): add JSON event
  output`. Use `!` for breaking changes. Individual commits within the PR
  can be messy; the title cannot.
- Fill in the PR description with two sections: **Summary** (what and why,
  linking the issue) and **Test plan** (what you ran and what you saw).
- Mark the PR as draft while you're still iterating; when review comments
  are addressed, re-request review rather than waiting silently.

## Filing issues

For bugs, include:

- `hm --version` output and your OS,
- what you ran, what you expected, and what happened instead,
- the smallest pipeline or command that reproduces it (the `examples/`
  projects make good starting points).

For feature requests, describe the problem you're trying to solve before
the solution you have in mind — the discussion usually starts there.

Security issues: please email [marko@harmont.dev](mailto:marko@harmont.dev)
instead of opening a public issue.

## License

Harmont is dual-licensed under [MIT](LICENSE-MIT) and
[Apache 2.0](LICENSE-APACHE). Unless you explicitly state otherwise, any
contribution you intentionally submit for inclusion in the work, as defined
in the Apache-2.0 license, shall be dual-licensed as above, without any
additional terms or conditions. (The `harmont` Python package carries its
own MIT license; contributions to it are MIT-licensed.)

## Code of Conduct

This project follows the
[Contributor Covenant Code of Conduct](CODE_OF_CONDUCT.md). By
participating, you agree to abide by its terms. Conduct concerns go to
[marko@harmont.dev](mailto:marko@harmont.dev).
