"""Proving grounds — exercise harmont's DSL against real-world OSS repos.

Each repo is cloned at a pinned SHA into /opt/<name> and built/tested/linted
using harmont's toolchain abstractions. All repos run as parallel chains
from a shared apt base.

Repos are audited individually — each uses the exact build/test/lint
commands from its real CI, not generic defaults.
"""
from __future__ import annotations

from datetime import timedelta
from typing import Annotated

import harmont as hm

# (name, url, sha)
REPOS = {
    "ripgrep":   ("https://github.com/BurntSushi/ripgrep",   "82313cf95849"),
    "clap":      ("https://github.com/clap-rs/clap",         "8387c812c4b9"),
    "tokio":     ("https://github.com/tokio-rs/tokio",       "778e9d97d91c"),
    "starship":  ("https://github.com/starship/starship",    "f595899021ca"),
    "ruff":      ("https://github.com/astral-sh/ruff",       "8cf0c77eb2be"),
    "flask":     ("https://github.com/pallets/flask",         "36e4a824f340"),
    "fastapi":   ("https://github.com/fastapi/fastapi",      "5cdf820c8046"),
    "express":   ("https://github.com/expressjs/express",     "dae209ae6559"),
    "gin":       ("https://github.com/gin-gonic/gin",         "d75fcd4c9ab2"),
    "terraform": ("https://github.com/hashicorp/terraform",   "d038b9e6cd86"),
    "jekyll":    ("https://github.com/jekyll/jekyll",         "202df571314b"),
}


@hm.target()
def apt(root: Annotated[hm.Step, hm.BaseImage("ubuntu:24.04")]) -> hm.Step:
    return root.sh(
        "apt-get update && "
        "apt-get install -y --no-install-recommends "
        "git ca-certificates curl build-essential pkg-config "
        "libdbus-1-dev",
        label=":apt: base",
        cache=hm.ttl(timedelta(days=1)),
    )


def _clone(base: hm.Step, name: str) -> hm.Step:
    url, sha = REPOS[name]
    return base.fork(name).sh(
        f"git init /opt/{name} && "
        f"git -C /opt/{name} fetch --depth 1 {url} {sha} && "
        f"git -C /opt/{name} checkout FETCH_HEAD",
        label=f":git: {name}",
    )


def _rust(base: hm.Step, name: str, **kw: str) -> hm.Step:
    """Install Rust toolchain on top of a cloned repo."""
    version = kw.get("version", "stable")
    cloned = _clone(base, name)
    return hm.rust.toolchain(
        path=f"/opt/{name}", base=cloned, version=version,
    ).installed


# ── Rust: ripgrep ────────────────────────────────────────────────────
# No clippy in their CI. Simple cargo build/test/fmt.
def _ripgrep(apt: hm.Step) -> list[hm.Step]:
    tc = _rust(apt, "ripgrep")
    cd = f". $HOME/.cargo/env && cd /opt/ripgrep"
    return [
        tc.sh(f"{cd} && cargo build --workspace", label=":ripgrep: build"),
        tc.sh(f"{cd} && cargo test --workspace", label=":ripgrep: test"),
        tc.sh(f"{cd} && cargo fmt --all --check", label=":ripgrep: fmt"),
    ]


# ── Rust: clap ───────────────────────────────────────────────────────
# Needs feature flags and -A deprecated for clippy.
def _clap(apt: hm.Step) -> list[hm.Step]:
    tc = _rust(apt, "clap")
    cd = f". $HOME/.cargo/env && cd /opt/clap"
    features = "deprecated derive cargo env unicode string wrap_help unstable-ext"
    return [
        tc.sh(f"{cd} && cargo build --workspace --features \"{features}\" --all-targets",
               label=":clap: build"),
        tc.sh(f"{cd} && cargo test --workspace --features \"{features}\"",
               label=":clap: test"),
        tc.sh(f"{cd} && cargo clippy --workspace --all-targets "
               f"--features \"{features}\" -- -D warnings -A deprecated",
               label=":clap: clippy"),
        tc.sh(f"{cd} && cargo fmt --all --check", label=":clap: fmt"),
    ]


# ── Rust: tokio ──────────────────────────────────────────────────────
# Needs --features full,test-util. Uses rustfmt directly (not cargo fmt).
def _tokio(apt: hm.Step) -> list[hm.Step]:
    tc = _rust(apt, "tokio")
    cd = f". $HOME/.cargo/env && cd /opt/tokio"
    env = "RUSTFLAGS='-Dwarnings'"
    return [
        tc.sh(f"{cd} && {env} cargo build --workspace --features full,test-util",
               label=":tokio: build"),
        tc.sh(f"{cd} && {env} cargo test --workspace --features full,test-util",
               label=":tokio: test"),
        tc.sh(f"{cd} && cargo clippy --workspace --tests --no-deps "
               f"--features full,test-util -- -D warnings",
               label=":tokio: clippy"),
        tc.sh(f"{cd} && rustfmt --check --edition 2021 $(git ls-files '*.rs')",
               label=":tokio: fmt"),
    ]


# ── Rust: starship ───────────────────────────────────────────────────
# Needs --locked. libdbus-1-dev already in apt base.
def _starship(apt: hm.Step) -> list[hm.Step]:
    tc = _rust(apt, "starship")
    cd = f". $HOME/.cargo/env && cd /opt/starship"
    return [
        tc.sh(f"{cd} && cargo build --workspace --locked", label=":starship: build"),
        tc.sh(f"{cd} && cargo test --workspace --locked", label=":starship: test"),
        tc.sh(f"{cd} && cargo clippy --workspace --locked -- -D warnings",
               label=":starship: clippy"),
        tc.sh(f"{cd} && cargo fmt --all -- --check", label=":starship: fmt"),
    ]


# ── Rust: ruff ───────────────────────────────────────────────────────
# Pinned to Rust 1.96 via rust-toolchain.toml. Needs --all-features --locked.
def _ruff(apt: hm.Step) -> list[hm.Step]:
    tc = _rust(apt, "ruff", version="1.96.0")
    cd = f". $HOME/.cargo/env && cd /opt/ruff"
    return [
        tc.sh(f"{cd} && cargo build --workspace --locked", label=":ruff: build"),
        tc.sh(f"{cd} && cargo test --workspace --all-features --locked",
               label=":ruff: test"),
        tc.sh(f"{cd} && cargo clippy --workspace --all-targets --all-features "
               f"--locked -- -D warnings",
               label=":ruff: clippy"),
        tc.sh(f"{cd} && cargo fmt --all --check", label=":ruff: fmt"),
    ]


# ── Python: flask ────────────────────────────────────────────────────
# Native uv project. Uses ruff + ty. Standard hm.python() works.
def _flask(apt: hm.Step) -> list[hm.Step]:
    cloned = _clone(apt, "flask")
    tc = hm.python(path="/opt/flask", base=cloned)
    return [tc.test(), tc.lint(), tc.fmt(), tc.typecheck()]


# ── Python: fastapi ──────────────────────────────────────────────────
# Uses uv but needs custom sync flags and test invocation.
def _fastapi(apt: hm.Step) -> list[hm.Step]:
    cloned = _clone(apt, "fastapi")
    tc = hm.python(path="/opt/fastapi", base=cloned)
    cd = "cd /opt/fastapi"
    return [
        tc.installed.sh(
            f"{cd} && PYTHONPATH=./docs_src uv run pytest tests/ -x -q",
            label=":fastapi: test",
        ),
        tc.lint(),
        tc.fmt(),
        tc.installed.sh(
            f"{cd} && uv run ty check fastapi",
            label=":fastapi: typecheck",
        ),
    ]


# ── npm: express ─────────────────────────────────────────────────────
# Plain npm. No build script. Just test + lint.
def _express(apt: hm.Step) -> list[hm.Step]:
    cloned = _clone(apt, "express")
    project = hm.npm(path="/opt/express", base=cloned)
    return [project.test(), project.lint()]


# ── Go: gin ──────────────────────────────────────────────────────────
# Simple Go project. Standard commands work.
def _gin(apt: hm.Step) -> list[hm.Step]:
    cloned = _clone(apt, "gin")
    tc = hm.go(path="/opt/gin", base=cloned)
    return [tc.build(), tc.test(), tc.vet(), tc.fmt()]


# ── Go: terraform ────────────────────────────────────────────────────
# Multi-module workspace. Test all sub-modules via loop.
def _terraform(apt: hm.Step) -> list[hm.Step]:
    cloned = _clone(apt, "terraform")
    tc = hm.go(path="/opt/terraform", base=cloned)
    cd = "cd /opt/terraform && export PATH=$HOME/go/bin:$PATH"
    return [
        tc.build(),
        tc.installed.sh(
            f"{cd} && "
            "for dir in $(go list -m -f '{{{{.Dir}}}}' github.com/hashicorp/terraform/...); do "
            "(cd $dir && go test -short -count=1 ./...) || exit 1; done",
            label=":terraform: test",
        ),
        tc.vet(),
        tc.fmt(),
    ]


# ── Ruby: jekyll ─────────────────────────────────────────────────────
# Bundler project. Tests need TZ=UTC. Lint via rubocop.
def _jekyll(apt: hm.Step) -> list[hm.Step]:
    cloned = _clone(apt, "jekyll")
    project = hm.ruby(path="/opt/jekyll", base=cloned)
    cd = "cd /opt/jekyll"
    return [
        project.installed.sh(
            f"{cd} && TZ=UTC bundle exec rake test",
            label=":jekyll: test",
        ),
        project.installed.sh(
            f"{cd} && bundle exec rubocop -D --disable-pending-cops",
            label=":jekyll: lint",
        ),
    ]


@hm.pipeline(
    "ci",
    env={"CI": "true"},
    default_image="ubuntu:24.04",
)
def ci(apt: hm.Target[hm.Step]) -> tuple[hm.Step, ...]:
    leaves: list[hm.Step] = []

    leaves.extend(_ripgrep(apt))
    leaves.extend(_clap(apt))
    leaves.extend(_tokio(apt))
    leaves.extend(_starship(apt))
    leaves.extend(_ruff(apt))
    leaves.extend(_flask(apt))
    leaves.extend(_fastapi(apt))
    leaves.extend(_express(apt))
    leaves.extend(_gin(apt))
    leaves.extend(_terraform(apt))
    leaves.extend(_jekyll(apt))

    return tuple(leaves)
