<p>
  <h1>Harmont</h1>
  <a href="https://github.com/harmont-dev/harmont-cli/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/harmont-dev/harmont-cli/ci.yml?branch=main&logo=github" alt="CI"></a>
  <a href="https://crates.io/crates/harmont-cli"><img src="https://img.shields.io/crates/v/harmont-cli?logo=rust" alt="crates.io"></a>
  <a href="https://docs.harmont.dev"><img src="https://img.shields.io/badge/docs-read-blue" alt="Docs"></a>
  <a href="https://discord.gg/hm-dev"><img src="https://img.shields.io/discord/1503184719578136576?logo=discord&label=discord" alt="Discord"></a>
  <a href="https://join.slack.com/t/harmont-dev/shared_invite/zt-3yt0tiv7r-qHm1O0p0nVh2GU~KKhUk9A"><img src="https://img.shields.io/badge/slack-join-brightgreen?logo=slack" alt="Slack"></a>
  <a href="#license"><img src="https://img.shields.io/badge/license-MIT%2FApache--2.0-blue" alt="License"></a>
</p>

<p>
  <a href="https://harmont.dev">Website</a> · <a href="https://app.harmont.dev">Harmont Cloud</a> · <a href="https://docs.harmont.dev">Docs</a> · <a href="https://join.slack.com/t/harmont-dev/shared_invite/zt-3yt0tiv7r-qHm1O0p0nVh2GU~KKhUk9A">Slack</a>
</p>

<p>
  <b>CI/CD as real code. Write your pipelines in Python or TypeScript, then run the exact same pipeline locally in Docker or on managed runners in <a href="https://app.harmont.dev">Harmont Cloud</a> — with layer caching and DAG parallelism built in.</b>
</p>

## What is Harmont?

Harmont lets you define CI/CD pipelines in **TypeScript or Python** and run them
two ways from a single definition: instantly on your own machine in Docker, or on
managed runners in [Harmont Cloud](https://app.harmont.dev). It's the same
pipeline either way — the run you debug locally is byte-for-byte the run that
ships in CI, so you stop pushing throwaway commits just to find out what breaks.
Each step runs in an isolated container with built-in caching, DAG parallelism,
and consistent environments.

Run it all locally, or [sign up for Harmont Cloud](https://app.harmont.dev) and
push your pipelines to managed runners with a single `--cloud` flag.



https://github.com/user-attachments/assets/114bc825-2889-4654-91d5-f830c3631b4c




**Why teams switch:**

- **Pipelines are real code** — Python or TypeScript, with the autocomplete,
  types, and abstractions your editor already gives you.
- **Run it locally** — `hm run` executes your real pipeline in Docker on your
  machine, so you catch failures before you push.
- **…or run it in the cloud** — the same pipeline runs on Harmont Cloud's
  managed runners with `hm run --cloud`, byte-for-byte identical to your local
  run. [Sign up](https://app.harmont.dev) to get started.
- **DAG-based parallelism** — independent steps run concurrently; `hm` figures
  out the dependency graph for you.
- **Automatic layer caching** — Docker snapshots are reused across runs, so only
  changed steps re-execute. Caching works out of the box.
- **Typed toolchains** — first-class presets for Rust, Go, Python, JavaScript/
  TypeScript, C/C++, Zig, and Elixir — each handles setup, build, test, lint,
  and format for you.
- **Claude writes it for you** — `hm init` installs Claude Code skills that
  author your pipeline and migrate your GitHub Actions (see below).


## Quick Start

### Install `hm`

```sh
curl -fsSL https://get.harmont.dev/install.sh | sh
```

Or via Cargo:

```sh
cargo install harmont-cli
```

### The 30-second path: `hm init`

```sh
hm init
```

`hm init` scaffolds a working `.hm/pipeline.{py,ts}` from a template and offers
to install Claude Code skills that write and maintain your pipeline. Run it and
pick your stack from the menu, or name a template up front with `-t`:

```sh
hm init -t rust      # cmake · elixir · nextjs · js · rust · zig · python
```

Then run it:

```sh
hm run
```

If the repo declares only one pipeline, the slug is optional. Otherwise name it:
`hm run ci`.

Want it to run in CI instead of on your laptop? [Sign up for Harmont
Cloud](https://app.harmont.dev), then `hm cloud login` and `hm run --cloud` — the
same pipeline, on managed runners. See [Cloud](#cloud) below.

### Or write it by hand

A pipeline is just code. Save this as `.hm/pipeline.py` (or `.hm/pipeline.ts`):

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
import { pipeline, push, type PipelineDefinition } from "@harmont/hm";
import { python } from "@harmont/hm/toolchains";

const project = python({ path: "." });

const pipelines: PipelineDefinition[] = [
  {
    slug: "ci",
    triggers: [push({ branch: "main" })],
    pipeline: pipeline(
      [
        project.test(),
        project.lint(),
        project.fmt(),
        project.typecheck(),
      ],
    ),
  },
];

export default pipelines;
```

</details>

```sh
hm run ci
```

Browse the [example projects](./examples) for idiomatic pipelines in Rust, Go,
Python, Elixir, Zig, C/C++, TypeScript, React, and Next.js.

## Let Claude set up your CI

`hm init` can install three [Claude Code](https://claude.com/claude-code) skills
into your repo. They turn pipeline authoring and migration into a conversation:

| Skill | What it does |
|-------|--------------|
| **write-pipeline** | Ask Claude to "set up CI" and it detects your stack, reads the live Harmont docs, and writes a correct `.hm/pipeline`. |
| **convert-gha** | Point Claude at your `.github/workflows/*.yml` and it migrates them to a Harmont pipeline — dropping the `actions/cache`, `actions/checkout`, and `actions/setup-*` boilerplate Harmont handles for you. |
| **validate-ci** | Before you push, Claude runs the whole pipeline locally (`hm run -k --logs`) and only gives the green light when it actually passes. |

```sh
hm init          # detects .github/workflows and offers convert-gha
```

Already have a pipeline and just want the skills? Re-run `hm init` — it skips
the template and installs the skills.

### Coming from GitHub Actions?

Migration is the easy part. The `convert-gha` skill reads every workflow and
maps it over for you:

- `actions/checkout` → not needed (your source is always in the container)
- `actions/setup-*` → replaced by a typed toolchain
- `actions/cache` → not needed (Harmont caches Docker layers automatically)
- `jobs.*.needs` → the DAG `hm` derives from your code
- `runs-on` → per-step `image=` (the default base is `ubuntu:24.04`)

The result is a pipeline you can run **locally** before it ever hits CI.

## How it works

**Automatic layer caching.** Every step's result is committed as a Docker
snapshot, keyed deterministically from the step and its inputs. Re-run a
pipeline and only the steps whose inputs changed actually execute — everything
else is restored from cache. You can tune this per step in the DSL:

```python
hm.forever()                 # cache until inputs change
hm.ttl(timedelta(hours=6))   # cache for a window
hm.on_change("src/")         # rebuild when these paths change
```

**DAG parallelism.** `hm` builds a dependency graph from your pipeline and runs
independent chains concurrently. Use `.fork()` to branch and `hm.wait()` to
join. Control concurrency with `--parallelism N` (defaults to your CPU count).

**Run everything, even after a failure.** Pass `-k` / `--keep-going` and
independent chains keep running after one step fails, so you see *all* failures
in a single run instead of one at a time.

```sh
hm run ci -k
```

**Timeouts.** Bound a single step or the whole pipeline:

```python
hm.timeout("5m", project.test())          # per-step
@hm.pipeline("ci", timeout="30m")          # whole pipeline
```

**Machine-readable output.** `--format json` emits one `BuildEvent` per line
(NDJSON) on stdout — identical whether the build runs locally or in the cloud —
so the same wrapper script parses both:

```sh
hm run ci --format json
```

Prefer raw logs over progress bars? Add `--logs`.

## Cloud

[Harmont Cloud](https://app.harmont.dev) runs your pipelines on managed runners —
no executors to provision or babysit. `hm run --cloud` submits your **local
working tree** without committing or pushing first: the CLI renders the pipeline
locally (so a broken DSL fails fast, before any upload), archives the worktree
(respecting `.gitignore`, stripping `.git`), uploads it, and streams live job
logs.

[Create an account](https://app.harmont.dev), then:

```sh
hm cloud login                 # one-time browser login (or --paste for no browser)
hm cloud org switch acme       # set a default org so you can skip --org
hm run --cloud                 # run the current tree in the cloud
```

Everything you can do locally works in the cloud — same flags, same
`--format json` event stream:

```sh
hm run --cloud --no-watch          # submit and exit without tailing logs
hm run --cloud --org acme          # pick the org explicitly
hm run --cloud --format json       # NDJSON BuildEvent stream for scripting
```

### Authentication

`hm cloud login` binds a loopback listener, opens `app.harmont.dev/cli-login`,
and stores the token in `~/.config/hm/credentials.toml` (mode 0600). No browser?
Use `hm cloud login --paste`. In CI, set a token instead:

```sh
export HM_API_TOKEN=hm_live_...   # takes precedence over the file
hm run --cloud --org acme
```

### Config

| File | Mode | Contents |
|------|------|----------|
| `~/.config/hm/config.toml` | 0644 | `backend`, `[cloud]` (`org`, `api_url`), `[preferences]` (`format`, `auto_watch`) |
| `~/.config/hm/credentials.toml` | 0600 | bearer tokens keyed by API base URL |

Settings layer **defaults → user config → project `.hm/config.toml` → env**, so
you can commit per-repo defaults and still override them locally. Env overrides:
`HM_API_URL`, `HM_API_TOKEN`.

### Managing builds from the CLI

```sh
hm cloud whoami                              # who am I
hm cloud pipeline list                       # pipelines in the active org
hm cloud build list   --pipeline ci          # builds for a pipeline
hm cloud build watch  --pipeline ci 42       # tail build #42
hm cloud build cancel --pipeline ci 42       # cancel build #42
hm cloud job log --pipeline ci --build 42 <job-id>
hm cloud billing balance                     # credit balance
hm cloud billing topup 20                    # add $20 via Stripe
```

## GitHub Actions

Not ready to leave GitHub Actions? Run your Harmont pipelines *inside* GHA and
get automatic Docker image caching for free. (Ready to leave? See
[convert-gha](#coming-from-github-actions) above.)

Use [`harmont-dev/actions-hm`](https://github.com/harmont-dev/actions-hm) to run
your pipelines in GitHub Actions with automatic Docker image caching:

```yaml
name: CI

on: [push, pull_request]

permissions:
  contents: read
  packages: write        # needed for Docker image caching via GHCR

jobs:
  ci:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: harmont-dev/actions-hm@main
        with:
          pipeline: ci
```

The action installs `hm`, runs your pipeline, and caches Docker images in GitHub
Container Registry so subsequent runs skip unchanged steps — the caching is wired
up for you.

<details>
<summary><b>Multiple pipelines</b></summary>

```yaml
jobs:
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: harmont-dev/actions-hm@main
        with:
          pipeline: lint

  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: harmont-dev/actions-hm@main
        with:
          pipeline: test
          parallelism: 4
```

</details>

<details>
<summary><b>Without caching</b></summary>

```yaml
- uses: harmont-dev/actions-hm@main
  with:
    pipeline: ci
    cache: 'false'
```

</details>

See the [action repo](https://github.com/harmont-dev/actions-hm) for the full
input reference, sub-actions, and caching details.

## Examples

The [`examples/`](./examples) directory has a complete, runnable pipeline for
each stack — every one shipped in **both** Python and TypeScript:

| | | |
|---|---|---|
| [Rust](./examples/rust) | [Go](./examples/go) | [Python (uv)](./examples/python-uv) |
| [Elixir](./examples/elixir) · [Phoenix](./examples/elixir-phoenix) | [Zig](./examples/zig) | [C](./examples/c) · [C++](./examples/cpp) |
| [TypeScript](./examples/typescript) · [Bun](./examples/bun) | [React](./examples/react) | [Next.js](./examples/nextjs) |

Don't see your stack? Toolchains compose from raw steps (`hm.sh(...)`), so you
can build a pipeline for anything that runs in a container.

## Documentation

For the full pipeline reference, richer examples, and more - see the
[docs](https://docs.harmont.dev).

## Community

Harmont is built in the open and we want your feedback while the APIs are still
moving.

- **Discord** — [discord.gg/hm-dev](https://discord.gg/hm-dev)
- **Slack** — [join the workspace](https://join.slack.com/t/harmont-dev/shared_invite/zt-3yt0tiv7r-qHm1O0p0nVh2GU~KKhUk9A)
- **Issues** — [github.com/harmont-dev/harmont-cli/issues](https://github.com/harmont-dev/harmont-cli/issues)

File bugs, request toolchains, or tell us what made you bounce — all of it helps.

## License

The CLI is dual-licensed under either of

- Apache License, Version 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE))
- MIT license ([`LICENSE-MIT`](LICENSE-MIT))

## Motivation

>
> The reason I started this project is because every other CI/CD tool I've used
> in my life has sucked.
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
and that's what [Harmont](https://harmont.dev) wants to be -- a CI/CD that
sucks a lot less.
