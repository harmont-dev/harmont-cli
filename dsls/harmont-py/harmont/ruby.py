"""Ruby toolchain abstraction.

Chain: scratch -> apt-base (ruby-full, build-essential, git) ->
bundler-install (gem install bundler, cached forever) ->
bundle-deps (cached on Gemfile.lock) -> action leaves.

The ``version`` parameter is validated as ``"default" | "X.Y" | "X.Y.Z"``;
``"default"`` installs whichever ruby-full ships in the apt repository.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any

from ._toolchain import make_install_chain
from .cache import CacheForever, CacheOnChange

if TYPE_CHECKING:
    from ._step import Step

APT_PACKAGES = ("ruby-full", "build-essential", "git")

_ACTION_KWARGS = frozenset(("cache", "env", "timeout_seconds", "label", "key"))

_VERSION_RE = re.compile(r"^(default|[0-9]+\.[0-9]+(\.[0-9]+)?)$")


@dataclass(frozen=True)
class RubyProject:
    path: str
    installed: Step

    def _emit(self, cmd: str, default_label: str, **kw: Any) -> Step:
        if kw.get("label") is None:
            kw["label"] = default_label
        return self.installed.sh(cmd, **kw)

    def test(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && bundle exec rspec",
            ":ruby: test",
            **kw,
        )

    def lint(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && bundle exec rubocop",
            ":ruby: lint",
            **kw,
        )


def _make_ruby(
    *,
    path: str = ".",
    version: str = "default",
    image: str | None = None,
    base: Step | None = None,
) -> RubyProject:
    if not _VERSION_RE.match(version):
        msg = (
            f"hm.ruby: invalid version {version!r}\n"
            '  → use "default" (apt) or a version like "3.2.2"'
        )
        raise ValueError(msg)
    if version != "default":
        msg = (
            f"hm.ruby: pinned ruby version {version!r} not yet wired in\n"
            '  → use version="default" (apt ruby-full); pinned versions need'
            " rbenv/asdf support, which is not implemented yet"
        )
        raise NotImplementedError(msg)
    bundler_installed = make_install_chain(
        apt_packages=APT_PACKAGES,
        install_cmd="gem install bundler && bundle --version",
        install_cache=CacheForever(env_keys=()),
        lang_tag="ruby",
        install_tag="bundler",
        image=image,
        base=base,
    )
    deps = bundler_installed.sh(
        f"cd {path} && bundle install",
        label=":ruby: deps",
        cache=CacheOnChange(paths=(f"{path}/Gemfile.lock",)),
    )
    return RubyProject(path=path, installed=deps)


class _RubyEntry:
    def __call__(
        self,
        *,
        path: str = ".",
        version: str = "default",
        image: str | None = None,
        base: Step | None = None,
    ) -> RubyProject:
        return _make_ruby(path=path, version=version, image=image, base=base)

    def test(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).test(**action_kw)

    def lint(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).lint(**action_kw)


ruby = _RubyEntry()
