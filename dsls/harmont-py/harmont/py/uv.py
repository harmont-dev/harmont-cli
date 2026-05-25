"""uv-managed Python project toolchain (``hm.py.uv``).

Public surface lives on the module-level singleton :data:`uv`. Call
it to construct a :class:`UvProject`, or use the bare-form action
methods (``uv.test()``, ``uv.lint()``, etc.) for a one-shot leaf.

The chain is:

    scratch -> apt-base -> uv-install -> uv-sync -> action leaves

The ``uv-install`` step is cached forever (keyed on the uv version baked
into the command). The ``uv-sync`` step is cached on the project's
``uv.lock`` and ``pyproject.toml``.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any

from harmont._toolchain import make_install_chain
from harmont.cache import CacheForever, CacheOnChange

if TYPE_CHECKING:
    from harmont._step import Step

APT_PACKAGES = ("curl", "ca-certificates", "python3", "python3-venv")

_ACTION_KWARGS = frozenset(("cache", "env", "timeout_seconds", "label", "key"))

_VERSION_RE = re.compile(r"^([0-9]+\.[0-9]+\.[0-9]+|latest)$")


def _resolve_paths(paths: str | list[str] | None) -> str:
    if paths is None:
        return "."
    if isinstance(paths, str):
        return paths
    return " ".join(paths)


def _uv_install_cmd(version: str) -> str:
    pin = "" if version == "latest" else f"UV_VERSION={version} "
    return (
        f"{pin}curl -LsSf https://astral.sh/uv/install.sh | sh && "
        "ln -sf /root/.local/bin/uv /usr/local/bin/uv && uv --version"
    )


@dataclass(frozen=True)
class UvProject:
    path: str
    installed: Step  # uv-sync Step

    def _emit(self, cmd: str, default_label: str, **kw: Any) -> Step:
        if kw.get("label") is None:
            kw["label"] = default_label
        return self.installed.sh(cmd, **kw)

    def test(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && uv run pytest",
            ":python: test",
            **kw,
        )

    def lint(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && uv run ruff check .",
            ":python: lint",
            **kw,
        )

    def fmt(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && uv run ruff format --check .",
            ":python: fmt",
            **kw,
        )

    def typecheck(self, *, paths: str | list[str] | None = None, **kw: Any) -> Step:
        target = _resolve_paths(paths)
        return self._emit(
            f"cd {self.path} && uv run ty check {target}",
            ":python: typecheck",
            **kw,
        )

    def run(self, cmd: str, **kw: Any) -> Step:
        first_word = cmd.split()[0] if cmd.split() else "run"
        return self._emit(
            f"cd {self.path} && uv run {cmd}",
            f":python: {first_word}",
            **kw,
        )

    def build(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && uv build",
            ":python: build",
            **kw,
        )

    def lock_check(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && uv lock --check",
            ":python: lock-check",
            **kw,
        )

    def publish(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && uv publish",
            ":python: publish",
            **kw,
        )


def _make_uv(
    *,
    path: str = ".",
    version: str = "latest",
    image: str | None = None,
    base: Step | None = None,
) -> UvProject:
    if not _VERSION_RE.match(version):
        msg = (
            f"py.uv: invalid version {version!r}\n"
            '  → use "latest" or a pinned version like "0.4.18"'
        )
        raise ValueError(msg)
    uv_installed = make_install_chain(
        apt_packages=APT_PACKAGES,
        install_cmd=_uv_install_cmd(version),
        install_cache=CacheForever(env_keys=()),
        lang_tag="python",
        install_tag="uv-install",
        image=image,
        base=base,
    )
    synced = uv_installed.sh(
        f"cd {path} && uv sync --all-extras",
        label=":python: uv-sync",
        cache=CacheOnChange(paths=(f"{path}/uv.lock", f"{path}/pyproject.toml")),
    )
    return UvProject(path=path, installed=synced)


class _UvEntry:
    def __call__(
        self,
        *,
        path: str = ".",
        version: str = "latest",
        image: str | None = None,
        base: Step | None = None,
    ) -> UvProject:
        return _make_uv(path=path, version=version, image=image, base=base)

    def test(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).test(**action_kw)

    def lint(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).lint(**action_kw)

    def fmt(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).fmt(**action_kw)

    def typecheck(self, *, paths: str | list[str] | None = None, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).typecheck(paths=paths, **action_kw)

    def run(self, cmd: str, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).run(cmd, **action_kw)

    def build(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).build(**action_kw)

    def lock_check(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).lock_check(**action_kw)

    def publish(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).publish(**action_kw)


uv = _UvEntry()
