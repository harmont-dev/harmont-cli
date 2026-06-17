"""Shared helpers for language toolchain abstractions (HAR-15).

Each language module builds its toolchain chain via
``make_install_chain``. The chain is:

    scratch (no Step) -> apt-base -> tool-install -> (action leaves)

When ``base`` is provided the apt-base step is skipped and the chain
forks off ``base`` directly. This is the explicit composition primitive
that lets toolchains stack or share a content-producing parent
(``hm.js.project(base=spec)``).
"""

from __future__ import annotations

import dataclasses
from datetime import timedelta
from typing import TYPE_CHECKING, Protocol, TypeVar

from ._step import scratch
from .cache import CacheTTL

if TYPE_CHECKING:
    from ._step import Step
    from .cache import CachePolicy


APT_TTL = timedelta(days=1)


class _HasInstalled(Protocol):
    # Read-only member (property form) so frozen-dataclass toolchains, whose
    # `installed` field is read-only, satisfy the protocol. A bare
    # `installed: Step` annotation declares a *writable* member, which frozen
    # instances do not match.
    @property
    def installed(self) -> Step: ...


_ProjectT = TypeVar("_ProjectT", bound="_HasInstalled")


def advance_install(
    project: _ProjectT,
    cmd: str,
    *,
    cwd: str | None = None,
    label: str | None = None,
    cache: CachePolicy | None = None,
    env: dict[str, str] | None = None,
) -> _ProjectT:
    """Return a copy of a toolchain object with one command appended to its
    install chain. Every action method emitted from the returned object forks
    from the new step. Shared implementation behind each toolchain's ``setup()``.
    """
    new_installed = project.installed.sh(cmd, cwd=cwd, label=label, cache=cache, env=env)
    # All callers are frozen dataclasses carrying an `installed: Step` field, but
    # the Protocol bound cannot express "is a dataclass", so the replace below
    # cannot satisfy its DataclassInstance upper bound — hence the narrow ignore.
    return dataclasses.replace(project, installed=new_installed)  # ty: ignore[invalid-argument-type]


def apt_install_cmd(packages: tuple[str, ...]) -> str:
    """Single shell string: ``apt-get update && apt-get install -y <pkgs>``."""
    pkgs = " ".join(packages)
    return f"apt-get update && apt-get install -y {pkgs}"


def node_install_cmd(version: str) -> str:
    """NodeSource node-install command for a given major Node version.

    Used by both the npm toolchain and the elm toolchain (whose
    tooling runs under npx).
    """
    major = version.removesuffix(".x")
    return (
        f"curl -fsSL https://deb.nodesource.com/setup_{major}.x | bash - && "
        "apt-get install -y nodejs"
    )


def bun_install_cmd(version: str | None = None) -> str:
    """Bun install command. Installs to /usr/local/bin for PATH availability."""
    version_arg = f' -s "bun-v{version}"' if version is not None else ""
    return f"curl -fsSL https://bun.sh/install | BUN_INSTALL=/usr/local bash{version_arg}"


def deno_install_cmd(version: str | None = None) -> str:
    """Deno install command. Symlinks into /usr/local/bin for PATH availability."""
    version_arg = f' -s "v{version}"' if version is not None else ""
    return (
        f"curl -fsSL https://deno.land/install.sh | sh{version_arg} && "
        "ln -sf $HOME/.deno/bin/deno /usr/local/bin/deno"
    )


def make_install_chain(
    *,
    apt_packages: tuple[str, ...],
    install_cmd: str,
    install_cache: CachePolicy,
    lang_tag: str,
    install_tag: str,
    image: str | None,
    base: Step | None,
) -> Step:
    """Build apt-base + tool-install chain. Return the tool-install Step.

    ``base=None`` (default) emits ``scratch -> apt-base -> tool-install``.
    ``base=<Step>`` emits ``base -> tool-install`` — both ``apt_packages``
    and ``image`` are ignored; the caller asserts that ``base`` already
    provides the system prerequisites the tool install needs.
    """
    if base is None:
        parent = scratch().sh(
            apt_install_cmd(apt_packages),
            label=f":{lang_tag}: apt-base",
            image=image,
            cache=CacheTTL(duration=APT_TTL),
        )
    else:
        parent = base
    return parent.sh(
        install_cmd,
        label=f":{lang_tag}: {install_tag}",
        cache=install_cache,
    )


def apt_base(
    *,
    packages: tuple[str, ...],
    image: str | None = None,
    label: str = ":apt: base",
) -> Step:
    """Create a standalone apt-base step sharable across toolchains.

    Emits ``apt-get update && apt-get install -y <packages>`` with a
    daily TTL cache. Pass the returned step as ``base=`` to any toolchain
    constructor to share one apt-base across multiple toolchains.

    Args:
        packages: apt package names to install.
        image: Local-mode Docker base image override for this step.
        label: Human-facing label shown in the UI.

    Returns:
        A ``Step`` that installs the given apt packages.

    Examples:
        >>> import harmont as hm
        >>> base = hm.apt_base(packages=("git", "curl"))
        >>> tc = hm.rust.toolchain(base=base)
    """
    return scratch().sh(
        apt_install_cmd(packages),
        label=label,
        image=image,
        cache=CacheTTL(duration=APT_TTL),
    )
