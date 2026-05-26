"""Shared helpers for language toolchain abstractions (HAR-15).

Each language module (rust.py, haskell.py, npm.py, elm.py) builds its
toolchain chain via :func:`make_install_chain`. The chain is:

    scratch (no Step) -> apt-base -> tool-install -> (action leaves)

When ``base`` is provided the apt-base step is skipped and the chain
forks off ``base`` directly. This is the explicit composition primitive
that lets toolchains stack (``hm.elm(base=node.installed)``) or share a
content-producing parent (``hm.npm(base=spec)``).
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
    """Create a standalone apt-base step sharable across toolchains via ``base=``."""
    return scratch().sh(
        apt_install_cmd(packages),
        label=label,
        image=image,
        cache=CacheTTL(duration=APT_TTL),
    )
