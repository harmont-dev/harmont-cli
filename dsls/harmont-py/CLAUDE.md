# cidsl/py — Python chain DSL for Harmont pipelines

> Read `PRINCIPLES.md` at the repo root before editing. Validation
> errors raised in `__post_init__` and from the lowering pass are
> user-facing to pipeline authors — keep them precise and fix-directed
> per § 5.

A Python package that emits the v0 IR JSON for Harmont CI pipelines.
Runtime deps: `croniter` (HAR-9 schedule trigger validation).

## How It Works

`Step` is a frozen dataclass. `scratch()` returns a root `Step`;
`Step.sh(cmd, **kw)` returns a child carrying one shell command (use
`cwd="path"` to prepend `cd <path> && ` to the command);
`Step.fork(label=None)` returns a passthrough used to brand a branch.
`hm.sh(cmd, **kw)` is shorthand for `scratch().sh(cmd, **kw)` — start a
chain in one call. The `pipeline(*leaves, env=None)` factory walks back
from each leaf via `parent`, topo-sorts, and emits the v0 IR as a
Python dict.  `pipeline_to_json(p)` serializes that dict (resolving
cache keys first via `harmont.keygen`) to the wire-format JSON string.

## Build & Test

```bash
python3 -m venv .venv && source .venv/bin/activate
pip install -e '.[dev]'

pytest                     # all tests
pytest -v --tb=short
```

## Public surface

`hm.pipeline` is **polymorphic**. When called with positional
`Step` arguments it is the factory — returns the v0 IR dict. When
called with no positionals (or a string slug) it is the HAR-9
**decorator**: it registers the wrapped function as a CI pipeline.

Decorator form:

```python
import harmont as hm


@hm.pipeline("default")
def default() -> hm.Step:
    return hm.sh("echo hi", label="hi")
```

Factory form (used inside a pipeline definition that builds the dict
imperatively, and in unit tests):

```python
hm.pipeline(hm.sh("echo hi"), default_image="alpine:3.20")
```

Stage 1 of rendering (`hm.dump_registry_json`) walks every
`.harmont/*.py`, imports each (which has the side effect of running
the decorators), assembles each registered pipeline via the factory,
and emits a `schema_version="1"` envelope keyed by slug, with each
pipeline's resolved v0 IR carried in the `definition` field.

The full surface (all reachable through `hm.`):

```python
pipeline(slug=None, *, name=None, triggers=(), allow_manual=True,
         env=None, default_image=None)              # decorator
pipeline(*leaves, env=None, default_image=None)     # factory (v0 IR dict)
pipeline_to_json(p, **kw)                           # -> str (wire JSON)
dump_registry_json()                                # -> str (envelope JSON)

target()                                            # decorator: memoized building block

sh(cmd, *, cwd=None, label=None, ...)               # -> Step (= scratch().sh(cmd, ...))
scratch()                                           # -> Step (root)
Step.sh(cmd, *, cwd=None, ...)                      # -> Step
Step.fork(label=None)                               # -> Step
wait(*, continue_on_failure=False)                  # -> Step

# trigger constructors (passed via `triggers=` on the decorator)
push(branch=..., tag=...)
pull_request(branches=..., types=...)
schedule(cron=...)

# cache helpers
ttl(duration) | on_change(*paths) | forever(env_keys=()) | compose(*policies)

# language toolchains (call to construct; bare-form actions also work)
haskell(ghc=..., cabal="latest")                  # -> HaskellToolchain (cabal package via .package(path))
rust(path=..., version="stable")                  # -> RustToolchain
npm(path=..., version="20")                       # -> NpmProject
elm(path=..., elm_version="0.19.1")               # -> ElmProject
python(path=..., uv_version="latest")             # -> PythonToolchain  (uv-based)
go(path=..., version="1.23.2")                    # -> GoToolchain
gradle(path=..., jdk="21", kotlin=False)          # -> GradleProject  (Java + Kotlin)
cmake(path=..., lang="c"|"cpp")                   # -> CMakeProject
dotnet(path=..., channel="8.0")                   # -> DotnetProject
ruby(path=..., version="default")                 # -> RubyProject
ocaml(path=..., compiler="5.1.1")                 # -> OCamlProject
zig(version="0.13.0")                             # -> ZigToolchain (zig project via .project(path))
zig(path=..., version="0.13.0")                   # -> ZigProject (one-shot)
perl(path=...)                                    # -> PerlProject
composer(path=..., laravel=False)                 # -> ComposerProject  (PHP + Laravel)
```

`Step` is opaque — pipeline authors do not read its attributes.

### Reusable targets (HAR-28)

`@hm.target()` decorates a parameterless function and memoizes its
return value per envelope render. Targets are the composition unit:

```python
@hm.target()
def apt_base() -> hm.Step:
    return hm.sh("apt-get update").sh("apt-get install -y python3 python3-venv")

@hm.target()
def api():
    return hm.haskell(ghc="9.6.7").cabal(path="api")

@hm.pipeline("ci")
def ci() -> tuple[hm.Step, ...]:
    return (apt_base().sh("./run-smoke"), api())
```

`@hm.target()` functions may return `Step`, `tuple[Step, ...]`,
`HaskellPackage`, `ElmProject`, `NpmProject`, or `RustToolchain`.
When such a value reaches the pipeline assembler it is unwrapped to
its default leaf:

| Type | Default leaf |
|------|--------------|
| `HaskellPackage` | `.build()` |
| `RustToolchain` | `.build()` |
| `NpmProject` | `.install()` (npm-ci leaf) |
| `ElmProject` | `.make("src/Main.elm")` |

Authors who want a different default call the explicit action
(`.test()`, `.lint()`, etc.) themselves.

#### Fixture-style dependencies (typed markers)

A target's parameters are typed annotations that tell the decorator
how to inject the value. Two markers are public:

**`Target[T]`** — declares a dependency on another `@hm.target` by
parameter name. Static type-checkers see the parameter as `T`.

**`Annotated[Step, BaseImage("X")]`** — declares a scratch-rooted
`Step` in image `"X"`. The first `.sh()` call on the parameter
inherits `image="X"`, so the first emitted IR step carries it.

```python
from typing import Annotated

import harmont as hm
from harmont.haskell import HaskellPackage, HaskellToolchain

@hm.target()
def apt_base(base: Annotated[hm.Step, hm.BaseImage("ubuntu-24.04")]) -> hm.Step:
    return base.sh("apt-get update").sh("apt-get install -y python3")

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

Rules:

- Every fixture parameter **must** carry a marker (`Target[T]` or
  `Annotated[Step, BaseImage("...")]`) OR a default value. Unmarked
  params raise at decoration time.
- `*args` / `**kwargs` / positional-only parameters are rejected.
- Duplicate target names raise at decoration time. Use
  `@hm.target(name="...")` to disambiguate.
- Cycles raise `RuntimeError` listing the path.

Both markers unwrap cleanly under mypy and pyright via PEP 593
(`Annotated`); `assert_type(apt_base, Step)` and the like pass
without suppressions.

Memoization scope is one `dump_registry_json` render. Two targets
that both depend on `apt_base` share the same `Step`, so the v0 IR
contains one apt-base step with N children — not N copies.

## Deployments — `@hm.deploy` and `hm.dev`

`@hm.deploy` is a driver-agnostic decorator that registers a function
as a long-lived service. The function returns a `Deployment` value
produced by a driver-specific factory; v1 ships only the local Docker
driver via `hm.dev.deploy(...)`. Future cloud drivers (`hm.aws.deploy`,
`hm.fly.deploy`) plug in without touching the top-level decorator.

```python
import harmont as hm

@hm.deploy("hello")
def hello() -> hm.Deployment:
    return hm.dev.deploy(
        image="python:3.12-alpine",
        cmd=["python", "-m", "http.server", "5678"],
        port_mapping={5678: hm.dev.port()},
    )

@hm.deploy("greeter")
def greeter(hello: hm.Dep[hm.Deployment]) -> hm.Deployment:
    return hm.dev.deploy(
        image="python:3.12-alpine",
        cmd=["python", "-m", "http.server", "5678"],
        port_mapping={5678: hm.dev.port()},
        env={"HELLO_HOST": hello.name},
    )
```

Public surface:

```python
hm.deploy(slug=None, *, name=None)                 # decorator
hm.Dep[T]                                           # PEP-593 fixture marker
hm.Deployment                                       # abstract dataclass

hm.dev.deploy(*, image=None, from_=None, cmd=None,
              port_mapping=None, env=None,
              volumes=None, workdir=None)           # -> LocalDeployment
hm.dev.port()                                       # OS-assigned host port sentinel
hm.dev.LocalDeployment                              # concrete subclass
hm.dev.dump_registry_json(*, worktree_root)         # -> v0 JSON
```

`hm.dev.port()` is only valid as a value in `port_mapping`. The host
port is assigned by Docker (via `-p :<container_port>`) at `hm dev up`
time; query it from another terminal with `hm dev port-of <slug>
<container_port>`. Ports are fresh on every `hm dev up`.

The Rust CLI (`hm dev up`) shells out to `python -m harmont.dev
--dump-registry` to obtain the registry JSON. Schema is at
`docs/superpowers/specs/2026-05-21-hm-dev-deploy-design.md` § 1.

## Cache keys

`harmont.keygen.resolve_pipeline_keys` ports the algorithm previously
implemented in Scheme. `pipeline_to_json` calls it before
serialization, so every step whose policy is not `none` has a
deterministic `cache.key` baked into the wire-format JSON.

## Snapshot lineage: `builds_in`, `image`, `default_image`

A chain edge — `parent.sh(cmd, ...)` — emits `builds_in: "<parent
key>"` in the v0 IR JSON. The edge encodes both synchronisation (the
planner waits for the parent) and state inheritance (the local
executor reuses the parent's container; the cloud planner boots from
its snapshot).

A step rooted at `scratch()` has `builds_in: null`. It boots from
`image="..."` locally (or the pipeline's `default_image`). The cloud
planner ignores `image`/`default_image` (it always boots from the
Freestyle base image).
