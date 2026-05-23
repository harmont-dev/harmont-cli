"""Perl toolchain abstraction.

Chain: scratch -> apt-base (perl + cpanminus) -> cpanm-deps -> action
leaves. The cpanm-deps step is cached on the project's ``cpanfile``.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, Any

from ._toolchain import make_install_chain
from .cache import CacheForever, CacheOnChange

if TYPE_CHECKING:
    from ._step import Step

APT_PACKAGES = ("perl", "cpanminus", "build-essential")

_ACTION_KWARGS = frozenset(("cache", "env", "timeout_seconds", "label", "key"))


@dataclass(frozen=True)
class PerlProject:
    path: str
    installed: Step

    def _emit(self, cmd: str, default_label: str, **kw: Any) -> Step:
        if kw.get("label") is None:
            kw["label"] = default_label
        return self.installed.sh(cmd, **kw)

    def test(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && prove -lv t/", ":perl: test", **kw,
        )

    def lint(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && perlcritic lib/", ":perl: lint", **kw,
        )


def _make_perl(
    *,
    path: str = ".",
    image: str | None = None,
    base: Step | None = None,
) -> PerlProject:
    cpanm_installed = make_install_chain(
        apt_packages=APT_PACKAGES,
        install_cmd="cpanm --notest --quiet Perl::Critic && perl --version",
        install_cache=CacheForever(env_keys=()),
        lang_tag="perl",
        install_tag="cpanm",
        image=image,
        base=base,
    )
    deps = cpanm_installed.sh(
        f"cd {path} && cpanm --installdeps --notest .",
        label=":perl: deps",
        cache=CacheOnChange(paths=(f"{path}/cpanfile",)),
    )
    return PerlProject(path=path, installed=deps)


class _PerlEntry:
    def __call__(
        self,
        *,
        path: str = ".",
        image: str | None = None,
        base: Step | None = None,
    ) -> PerlProject:
        return _make_perl(path=path, image=image, base=base)

    def test(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).test(**action_kw)

    def lint(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).lint(**action_kw)


perl = _PerlEntry()
