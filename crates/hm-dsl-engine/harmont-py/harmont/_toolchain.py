"""Shared helpers for language toolchain abstractions (HAR-15).

Each language module builds its toolchain chain via
``make_install_chain``. The chain is:

    scratch (no Step) -> apt-base -> tool-install -> (action leaves)

When ``base`` is provided the apt-base step is skipped and the chain
forks off ``base`` directly. This is the explicit composition primitive
that lets toolchains stack or share a content-producing parent
(``hm.npm(base=spec)``).
"""

from __future__ import annotations

from datetime import timedelta
from typing import TYPE_CHECKING

from ._step import scratch
from .cache import CacheTTL

if TYPE_CHECKING:
    from ._step import Step
    from .cache import CachePolicy


APT_TTL = timedelta(days=1)


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
