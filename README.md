<p>
  <h1>Harmont</h1>
  <a href="https://github.com/harmont-dev/harmont-cli/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/harmont-dev/harmont-cli/ci.yml?branch=main&logo=github" alt="CI"></a>
  <a href="https://crates.io/crates/harmont-cli"><img src="https://img.shields.io/crates/v/harmont-cli?logo=rust" alt="crates.io"></a>
  <a href="https://discord.gg/hm-dev"><img src="https://img.shields.io/discord/1503184719578136576?logo=discord&label=discord" alt="Discord"></a>
  <a href="https://join.slack.com/t/harmont-dev/shared_invite/zt-3yt0tiv7r-qHm1O0p0nVh2GU~KKhUk9A"><img src="https://img.shields.io/badge/slack-join-brightgreen?logo=slack" alt="Slack"></a>
  <a href="#license"><img src="https://img.shields.io/badge/license-MIT%2FApache--2.0-blue" alt="License"></a>
</p>

<p>
  <a href="https://harmont.dev">Website</a> · <a href="https://docs.harmont.dev">Docs</a> · <a href="https://join.slack.com/t/harmont-dev/shared_invite/zt-3yt0tiv7r-qHm1O0p0nVh2GU~KKhUk9A">Slack</a>
</p>

<p>
  <b>CI/CD you can run locally. Pipelines in real Python or TypeScript — no YAML. Each step runs in an isolated Docker container with automatic layer caching and DAG parallelism.</b>
</p>

> [!WARNING]
> Harmont is in **early alpha**. APIs will change.
>
> Today `hm` is a fast, local-first task runner — think `make` or `just`, but
> with DAG-based parallel execution, Docker isolation, layer caching, and typed
> toolchain presets for many languages. The hosted CI/CD platform at
> [harmont.dev](https://harmont.dev) is under active development.
>
> Cross-run caching and code-quality polish are in progress. We'd love your
> feedback — [join the community](#community).
>
> **`hm` will always remain open-source, and pluggable into any CI/CD
> provider.**

## What is Harmont?

Harmont lets you define CI/CD pipelines in **TypeScript or Python** and run them
instantly on your machine in Docker containers. **No YAML.** No
`git commit -m "fix ci" --allow-empty` spam to debug a pipeline. Each step runs
in an isolated container with built-in caching, DAG parallelism, and consistent
environments — the *same* pipeline runs locally and in the cloud.



https://github.com/user-attachments/assets/114bc825-2889-4654-91d5-f830c3631b4c




**Why teams switch:**

- **Run CI locally** — `hm run` executes your real pipeline in Docker on your
  machine. No push-and-pray.
- **Pipelines are real code** — Python or TypeScript with autocomplete and
  types, not YAML.
- **DAG-based parallelism** — independent steps run concurrently; `hm` figures
  out the dependency graph.
- **Automatic layer caching** — Docker snapshots are reused across runs; only
  changed steps re-execute. No `actions/cache` boilerplate.
- **Typed toolchains** — first-class presets for Rust, Go, Python, JavaScript/
  TypeScript, C/C++, Ruby, Zig, and Elixir — each handles setup, build, test,
  lint, and format for you.
- **Local *and* cloud** — the same pipeline runs with `hm run` or
  `hm run --cloud`, byte-for-byte.
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

`hm init` detects your project, scaffolds a working `.hm/pipeline.{py,ts}`, and
offers to install Claude Code skills that can write and maintain your pipeline
for you. Pick a template explicitly with `-t`:

```sh
hm init -t rust      # cmake · elixir · nextjs · js · rust · zig · python
```

Then run it:

```sh
hm run
```

If the repo declares only one pipeline, the slug is optional. Otherwise name it:
`hm run ci`.

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
      [
        project.test(),
        project.lint(),
        project.fmt(),
        project.typecheck(),
      ],
      { defaultImage: "ubuntu:24.04" },
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
Python, Ruby, Elixir, Zig, C/C++, TypeScript, React, and Next.js.

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
- `runs-on` → `default_image`

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

## Cloud (`hm run --cloud`)

`hm run --cloud` runs your **local working tree** in Harmont Cloud without
committing or pushing first. The CLI renders the pipeline locally (fast DSL
failure before any upload), archives the worktree (respects `.gitignore`,
strips `.git`), uploads the tarball, and streams live job logs.

```sh
# One-time login (opens a browser tab; token stored in ~/.harmont/credentials.toml)
hm cloud login

# Run the current worktree against the "acme" org in the cloud
hm run --cloud --org acme

# Submit and exit without waiting for logs
hm run --cloud --org acme --no-watch

# Machine-readable NDJSON event stream to stdout (for scripting / CI wrappers).
# Emits the same `BuildEvent` line stream as a local `hm run --format json`.
hm run --cloud --org acme --format json
```

With `--format json`, cloud runs emit the unified `BuildEvent` JSON stream
(one event per line on stdout) — identical to a local `hm run --format json`,
so the same wrappers parse both paths. The progress spinner is suppressed in
JSON mode even on a TTY.

**Flags added by `--cloud`:**

| Flag | Description |
|------|-------------|
| `--cloud` | Run in Harmont Cloud instead of locally. |
| `--org <ORG>` | Cloud organization slug. Defaults to `default_org` in `~/.harmont/config.toml`. |

The shared flags `--branch`, `--message`, `--env KEY=VALUE`, `--dir`,
`--no-watch`, and `--format` all apply to cloud runs.

### Authentication

**Browser login (default):**

```sh
hm cloud login
```

Binds a loopback listener, opens `app.harmont.dev/cli-login`, and polls for
the token. On success, stores it in `~/.harmont/credentials.toml` (mode 0600).

**Paste-code flow (no browser):**

```sh
hm cloud login --paste
```

Prints a URL; you open it, copy the short code, paste it back.

**Token via env (CI):**

```sh
export HARMONT_API_TOKEN=hm_live_...
hm run --cloud --org acme
```

`HARMONT_API_TOKEN` takes precedence over the credentials file.

### Config files

All config lives under `~/.harmont/`:

| File | Mode | Contents |
|------|------|----------|
| `config.toml` | 0644 | `api_url`, `default_org`, `default_pipeline` |
| `credentials.toml` | 0600 | Bearer tokens keyed by API base URL |

**Env overrides:**

| Env var | Overrides |
|---------|-----------|
| `HARMONT_API_URL` | `api_url` in `config.toml` |
| `HARMONT_API_TOKEN` | Token in `credentials.toml` |

Set `default_org` to avoid typing `--org` every time:

```sh
hm cloud org switch acme   # writes default_org = "acme" into config.toml
```

### Other cloud commands

```sh
hm cloud whoami                              # show authenticated user
hm cloud logout                              # remove stored credentials
hm cloud pipeline list                       # list pipelines for the active org
hm cloud build list --pipeline ci            # list builds
hm cloud build watch --pipeline ci 42        # tail logs for build #42
hm cloud job log --pipeline ci --build 42 <job-id>
hm cloud billing balance                     # credit balance
```

### Example session

```sh
# 1. Authenticate
hm cloud login
# → Logged in as alice (alice@example.com)

# 2. Set a default org so you don't need --org every time
hm cloud org switch acme

# 3. Run your local tree in the cloud
hm run --cloud
# ⠹ uploading worktree…
# ✓ Build #17 submitted (acme/ci on https://api.harmont.dev)
# [step 1/3] test  …  ✓ passed
# [step 2/3] lint  …  ✓ passed
# [step 3/3] fmt   …  ✓ passed
# Build #17 passed.
```

## GitHub Actions

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
Container Registry so subsequent runs skip unchanged steps. No `actions/cache`
configuration required.

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

## Documentation

For the full pipeline reference, richer examples, and more - see the
[docs](https://docs.harmont.dev).

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
