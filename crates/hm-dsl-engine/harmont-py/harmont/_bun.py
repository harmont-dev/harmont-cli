"""Bun project abstraction."""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any

from ._toolchain import bun_install_cmd, make_install_chain
from .cache import CacheForever, CacheOnChange

if TYPE_CHECKING:
    from ._step import Step

APT_PACKAGES = ("curl", "ca-certificates", "unzip")

_ACTION_KWARGS = frozenset(("cache", "env", "label", "key"))

_VERSION_RE = re.compile(r"^[0-9]+\.[0-9]+(\.[0-9]+)?$")


@dataclass(frozen=True)
class BunProject:
    path: str
    installed: Step  # the `bun install` step

    def _emit(self, cmd: str, default_label: str, **kw: Any) -> Step:
        if kw.get("label") is None:
            kw["label"] = default_label
        return self.installed.sh(cmd, **kw)

    def install(self) -> Step:
        return self.installed

    def run(self, script: str, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && bun run {script}",
            f":bun: {script}",
            **kw,
        )

    def test(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && bun test",
            ":bun: test",
            **kw,
        )

    def lint(self, **kw: Any) -> Step:
        return self.run("lint", **kw)

    def fmt(self, **kw: Any) -> Step:
        return self.run("fmt", **kw)


def _make_bun(
    *,
    path: str = ".",
    version: str | None = None,
    image: str | None = None,
    base: Step | None = None,
) -> BunProject:
    if version is not None and not _VERSION_RE.match(version):
        msg = (
            f'hm.bun: invalid version {version!r}\n  → use a semver version like "1.2.0" or "1.2"'
        )
        raise ValueError(msg)
    bun_installed = make_install_chain(
        apt_packages=APT_PACKAGES,
        install_cmd=bun_install_cmd(version),
        install_cache=CacheForever(env_keys=()),
        lang_tag="bun",
        install_tag="install",
        image=image,
        base=base,
    )
    bun_deps = bun_installed.sh(
        f"cd {path} && bun install --frozen-lockfile",
        label=":bun: deps",
        cache=CacheOnChange(paths=(f"{path}/bun.lock",)),
    )
    return BunProject(path=path, installed=bun_deps)


class _BunEntry:
    def __call__(
        self,
        *,
        path: str = ".",
        version: str | None = None,
        image: str | None = None,
        base: Step | None = None,
    ) -> BunProject:
        return _make_bun(path=path, version=version, image=image, base=base)

    def install(self, **kw: Any) -> Step:
        return self(**kw).install()

    def run(self, script: str, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).run(script, **action_kw)

    def test(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).test(**action_kw)

    def lint(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).lint(**action_kw)

    def fmt(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).fmt(**action_kw)


bun = _BunEntry()
