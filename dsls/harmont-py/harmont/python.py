"""Python (uv) toolchain abstraction.

Public surface lives on the module-level singleton :data:`python`. Call
it to construct a :class:`PythonToolchain`, or use the bare-form action
methods (``python.test()``, ``python.lint()``, etc.) for a one-shot leaf.

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

from ._toolchain import make_install_chain
from .cache import CacheForever, CacheOnChange

if TYPE_CHECKING:
    from ._step import Step

APT_PACKAGES = ("curl", "ca-certificates", "python3", "python3-venv")

_ACTION_KWARGS = frozenset(("cache", "env", "timeout_seconds", "label", "key"))

_VERSION_RE = re.compile(r"^([0-9]+\.[0-9]+\.[0-9]+|latest)$")


def _resolve_paths(paths: str | list[str] | None) -> str:
    if paths is None:
        return "."
    if isinstance(paths, str):
        return paths
    return " ".join(paths)


def _uv_install_cmd(uv_version: str) -> str:
    pin = "" if uv_version == "latest" else f"UV_VERSION={uv_version} "
    return (
        f"{pin}curl -LsSf https://astral.sh/uv/install.sh | sh && "
        "ln -sf /root/.local/bin/uv /usr/local/bin/uv && uv --version"
    )


@dataclass(frozen=True)
class PythonToolchain:
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


def _make_python(
    *,
    path: str = ".",
    uv_version: str = "latest",
    image: str | None = None,
    base: Step | None = None,
) -> PythonToolchain:
    if not _VERSION_RE.match(uv_version):
        msg = (
            f"hm.python: invalid uv_version {uv_version!r}\n"
            '  → use "latest" or a pinned version like "0.4.18"'
        )
        raise ValueError(msg)
    uv_installed = make_install_chain(
        apt_packages=APT_PACKAGES,
        install_cmd=_uv_install_cmd(uv_version),
        install_cache=CacheForever(env_keys=()),
        lang_tag="python",
        install_tag="uv-install",
        image=image,
        base=base,
    )
    # `--all-extras` pulls every `[project.optional-dependencies]`
    # group declared in pyproject.toml. This matters because the
    # action surface (`.lint()`, `.fmt()`, `.typecheck()`, `.test()`)
    # depends on tools like `ruff`, `ty`, `pytest` that authors
    # almost always declare under an `[optional-dependencies] dev`
    # extra rather than as runtime deps. Without `--all-extras`,
    # `uv sync` only installs runtime deps and every action step
    # fails with `Failed to spawn: <tool>: No such file or directory`.
    synced = uv_installed.sh(
        f"cd {path} && uv sync --all-extras",
        label=":python: uv-sync",
        cache=CacheOnChange(paths=(f"{path}/uv.lock", f"{path}/pyproject.toml")),
    )
    return PythonToolchain(path=path, installed=synced)


class _PythonEntry:
    def __call__(
        self,
        *,
        path: str = ".",
        uv_version: str = "latest",
        image: str | None = None,
        base: Step | None = None,
    ) -> PythonToolchain:
        return _make_python(path=path, uv_version=uv_version, image=image, base=base)

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


python = _PythonEntry()
