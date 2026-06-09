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

> [!WARNING]
> Harmont is in **early alpha**.
>
> Today it's a powerful task runner -- like `make` or `just`, but with DAG-based
> parallel execution, Docker isolation, layer caching, and typed toolchain
> presets for many languages.
>
> The cloud CI/CD platform at [harmont.dev](https://harmont.dev) is under
> active development. APIs will change. We'd love your feedback -- [join the
> community](#community).
>
> The performance of the `hm` CLI is not as good as I'd like it to be. I'm
> actively working on cross-run caching. The code quality is similar -- needs
> improving and is a work in progress.
>
> **`hm` will always remain open-source, and pluggable into any CI/CD
> provider.**

## What is Harmont?

Harmont lets you define CI/CD workflows in TypeScript or Python and run them
instantly on your machine in Docker containers. **No YAML.** No `commit -m "run
ci" --allow-empty` spam. Each pipeline step runs in an isolated container with
built-in caching, parallel execution, and consistent environments.



https://github.com/user-attachments/assets/114bc825-2889-4654-91d5-f830c3631b4c




**Features:**

- **Pipelines as real code** - Python or TypeScript, not YAML.
- **Instant local runs** - `hm run` executes in Docker on your machine.
- **DAG-based parallelism** - independent chains run concurrently.
- **Layer caching** - Docker snapshots are reused across runs; only changed steps
                      re-execute.
- **Typed toolchains** - first-class presets for Rust, Go, Python, Java, C++,
                         React, and more.


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

Save this as `.hm/pipeline.py` (or `.hm/pipeline.ts`):

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

### 2. Run it

```sh
hm run ci
```

If the repo declares only one pipeline, the slug is optional - just `hm run`.

Browse the [example projects](./examples) for idiomatic pipelines in Rust,
Go, Python, Java, C++, React, Next.js, and more.

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
