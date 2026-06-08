"""Npm project abstraction (HAR-15)."""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any

from ._toolchain import make_install_chain, node_install_cmd
from .cache import CacheForever, CacheOnChange

if TYPE_CHECKING:
    from ._step import Step

APT_PACKAGES = ("curl", "ca-certificates")

_ACTION_KWARGS = frozenset(("cache", "env", "timeout_seconds", "label", "key"))

_VERSION_RE = re.compile(r"^[0-9]+(\.x)?$")


@dataclass(frozen=True)
class NpmProject:
    """npm project install chain — constructed via ``hm.npm()``.

    ``installed`` is the ``npm ci`` step. Action methods (``run``,
    ``test``, ``lint``, ``fmt``) attach leaves to ``installed`` so
    dependency installation is shared across CI actions.
    """

    path: str
    installed: Step  # the `npm ci` step

    def _emit(self, cmd: str, default_label: str, **kw: Any) -> Step:
        if kw.get("label") is None:
            kw["label"] = default_label
        return self.installed.sh(cmd, **kw)

    def install(self) -> Step:
        return self.installed

    def run(self, script: str, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && npm run {script}",
            f":node: {script}",
            **kw,
        )

    def test(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && npm test",
            ":node: test",
            **kw,
        )

    def lint(self, **kw: Any) -> Step:
        return self.run("lint", **kw)

    def fmt(self, **kw: Any) -> Step:
        return self.run("fmt", **kw)


def _make_npm(
    *,
    path: str = ".",
    version: str = "20",
    image: str | None = None,
    base: Step | None = None,
) -> NpmProject:
    if not _VERSION_RE.match(version):
        msg = (
            f"hm.npm: invalid version {version!r}\n"
            '  → use a Node major version like "20" or "20.x"'
        )
        raise ValueError(msg)
    node_installed = make_install_chain(
        apt_packages=APT_PACKAGES,
        install_cmd=node_install_cmd(version),
        install_cache=CacheForever(env_keys=()),
        lang_tag="node",
        install_tag="install",
        image=image,
        base=base,
    )
    npm_ci = node_installed.sh(
        f"cd {path} && npm ci",
        label=":node: deps",
        cache=CacheOnChange(paths=(f"{path}/package-lock.json",)),
    )
    return NpmProject(path=path, installed=npm_ci)


class NpmEntry:
    """Callable singleton for the npm toolchain — access as ``hm.npm``.

    Call directly to construct an ``NpmProject``, or use the bare-form
    action methods (``npm.test()``, ``npm.run(script)``, etc.) for a
    one-shot leaf.
    """

    def __call__(
        self,
        *,
        path: str = ".",
        version: str = "20",
        image: str | None = None,
        base: Step | None = None,
    ) -> NpmProject:
        """Install Node.js and run ``npm ci``, returning a project object.

        Args:
            path: Path to the npm project root (must contain a
                ``package-lock.json``).
            version: Node.js major version to install (e.g. ``"20"`` or
                ``"20.x"``).
            image: Local-mode Docker base image override.
            base: Existing ``Step`` to attach to instead of emitting a fresh
                apt-base step.

        Returns:
            An ``NpmProject`` whose ``installed`` step is ``npm ci``.

        Examples:
            >>> import harmont as hm
            >>> proj = hm.npm(path="frontend", version="20")
            >>> hm.pipeline([proj.test()])
        """
        return _make_npm(path=path, version=version, image=image, base=base)

    def install(self, **kw: Any) -> Step:
        # .install() returns the pre-existing npm-ci Step verbatim — it
        # doesn't emit a new action, so it doesn't accept action kwargs
        # (label/cache/env/...). Constructor kwargs only.
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


npm: NpmEntry = NpmEntry()
