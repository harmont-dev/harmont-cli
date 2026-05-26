<p align="center">
  <img src=".github/res/github-banner.png" alt="harmont — CI/CD that sucks less." width="100%">
</p>

<p align="center">
  <a href="https://github.com/harmont-dev/harmont-cli/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/harmont-dev/harmont-cli/ci.yml?branch=main&logo=github" alt="CI"></a>
  <a href="https://crates.io/crates/harmont-cli"><img src="https://img.shields.io/crates/v/harmont-cli?logo=rust" alt="crates.io"></a>
  <a href="https://discord.gg/hm-dev"><img src="https://img.shields.io/discord/1503184719578136576?logo=discord&label=discord" alt="Discord"></a>
  <a href="https://join.slack.com/t/harmont-dev/shared_invite/zt-3yt0tiv7r-qHm1O0p0nVh2GU~KKhUk9A"><img src="https://img.shields.io/badge/slack-join-brightgreen?logo=slack" alt="Slack"></a>
  <a href="#license"><img src="https://img.shields.io/badge/license-MIT%2FApache--2.0-blue" alt="License"></a>
</p>

<p align="center">
  <a href="https://harmont.dev">Website</a> · <a href="https://harmont.dev/docs">Docs</a> · <a href="https://join.slack.com/t/harmont-dev/shared_invite/zt-3yt0tiv7r-qHm1O0p0nVh2GU~KKhUk9A">Slack</a>
</p>

> [!WARNING]
> Harmont is in **early alpha**. Today it's a powerful local task runner —
> think `make` or `just`, but with DAG-based parallel execution, Docker
> isolation, layer caching, and typed toolchain presets for 16+ languages.
> The cloud CI/CD platform at [harmont.dev](https://harmont.dev) is under
> active development. APIs will change. We'd love your feedback — [join the
> community](#community).

## What is Harmont?

Harmont lets you define CI/CD pipelines in TypeScript or Python and run them
instantly on your machine in Docker containers. No YAML. No waiting for CI. No
`commit -m "run ci" --allow-empty` spam. Each pipeline step runs in an isolated
container with built-in caching, parallel execution, and consistent environments.

**Features:**

- **Pipelines as real code** — Python or TypeScript, not YAML
- **Instant local runs** — `hm run` executes in Docker on your machine
- **DAG-based parallelism** — independent chains run concurrently, automatically
- **Layer caching** — Docker snapshots are reused across runs; only changed steps re-execute
- **Typed toolchains** — first-class presets for Rust, Go, Python, Java, C++, React, and more

<!-- TODO(marko): add asciinema -->

## Quick Start

### 0. Install `hm`

```sh
curl -fsSL https://get.harmont.dev/install.sh | sh
```

Or via Cargo:

```sh
cargo install harmont-cli
```

### 1. Create a pipeline

Save this as `.harmont/pipeline.py` (or `.harmont/pipeline.ts`):

<details open>
<summary><b>Python</b></summary>

```python
import harmont as hm

@hm.pipeline("ci")
def ci() -> hm.Step:
    return (
        hm.sh("echo 'hello from harmont'", label="hello")
          .sh("uname -a", label="env")
    )
```

</details>

<details>
<summary><b>TypeScript</b></summary>

```typescript
import { sh, pipeline, type PipelineDefinition } from "harmont";

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    pipeline: pipeline(
      sh("echo 'hello from harmont'", { label: "hello" })
        .sh("uname -a", { label: "env" }),
    ),
  },
];

export default pipelines;
```

</details>

### 2. Run it

```sh
hm run ci
```

If the repo declares only one pipeline, the slug is optional — just `hm run`.

### Real-world example

For production pipelines, use typed toolchains — they generate test, lint, and
format steps from your project layout:

<details open>
<summary><b>Python</b></summary>

```python
import harmont as hm
from harmont.python import PythonToolchain

@hm.target()
def project() -> PythonToolchain:
    return hm.python(path=".")

@hm.pipeline(
    "ci",
    default_image="ubuntu:24.04",
    triggers=[hm.push(branch="main")],
)
def ci(project: hm.Target[PythonToolchain]) -> tuple[hm.Step, ...]:
    return (
        project.test(),
        project.lint(),
        project.fmt(),
        project.typecheck(),
    )
```

</details>

<details>
<summary><b>TypeScript</b></summary>

```typescript
import { pipeline, push, type PipelineDefinition } from "harmont";
import { python } from "harmont/toolchains";

const project = python({ path: "." });

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" })],
    pipeline: pipeline(
      project.test(),
      project.lint(),
      project.fmt(),
      project.typecheck(),
      { defaultImage: "ubuntu:24.04" },
    ),
  },
];

export default pipelines;
```

</details>

Browse the [example projects](./examples) for idiomatic pipelines in Rust,
Go, Python, Java, C++, React, Next.js, and more.

## Community

- **Discord** — [discord.gg/hm-dev](https://discord.gg/hm-dev)
- **Slack** — [harmont-dev.slack.com](https://join.slack.com/t/harmont-dev/shared_invite/zt-3yt0tiv7r-qHm1O0p0nVh2GU~KKhUk9A)
- **Website** — [harmont.dev](https://harmont.dev)

## Documentation

For the full pipeline reference, rich examples, and more — see the
[docs](https://harmont.dev/docs).

## License

The CLI is dual-licensed under either of

- Apache License, Version 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE))
- MIT license ([`LICENSE-MIT`](LICENSE-MIT))

## Motivation

>
> The reason [I](https://github.com/markovejnovic) started this project is
> because every other CI/CD tool I've used in my life has sucked.
>
> I've worked at [Tesla](https://tesla.com), [Bun](https://bun.com),
> [Mesa](https://mesa.dev) and never did I find a CI/CD system that was easy to
> use and was also fast.
>
> At Tesla, we used [Jenkins](https://www.jenkins.io/) -- executors are finite,
> so your builds are stuck in queues.
>
> At Bun, we used [Buildkite](https://buildkite.com/) -- large shell pipelines,
> and really pricy service, and a TS SDK that's only slightly better than
> YAMLs.
>
> At Mesa, I migrated everyone to use [BuildBuddy](https://www.buildbuddy.io/)
> and Buildkite. [Bazel](https://bazel.build/) is awesome, but the mental
> overhead required to use it is way too high. We, sadly, ended up reverting
> to plain Buildkite.
>
> I asked myself a couple questions:
>
> - **Why can't I run my CI/CD pipelines locally?**
>   [act](https://github.com/nektos/act) is an awesome project, but it's
>   surprisingly slow (not to the author's fault -- but rather GHA's model).
> - **Why is my CI/CD system not just a `Makefile`?** Why is there no `hm run`
>   command that is shared between local dev and CI/CD?
> - **Why can't I get preview environments for Haskell, Rust, Zig or
>   whatever?** Vercel does an awesome job with `next.js` preview environments,
>   but there is no good way to do this for arbitrary environments.
> - **Why do we have to write YAMLs for our pipelines?** All my pipelines end
>   up being [YAML documents from
>   hell](https://ruuda.nl/2023/the-yaml-document-from-hell). I think we can do
>   better.
> - **Why do I need `artifacts-upload` and `artifacts-download` everywhere?**
>   I don't need it locally, so why do I need it in CI/CD? In other words, why
>   aren't our CI/CD systems stateful? If my build scripts can write an
>   `openapi.json` in the local directory, why do I need some magic to transfer
>   it between individual steps?

Harmont's goal is to make all these questions obsolete. CI/CD _can_ be better,
and that's what [Harmont](https://harmont.dev) is -- a CI/CD that wants to suck
a lot less.
