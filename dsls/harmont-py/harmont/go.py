"""Go toolchain abstraction.

Chain: scratch -> apt-base (curl, ca-certificates) -> go-install (download
official tarball to /usr/local/go) -> action leaves. The go-install step
is cached forever, keyed on the Go version in the command.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any

from ._toolchain import make_install_chain
from .cache import CacheForever

if TYPE_CHECKING:
    from ._step import Step

APT_PACKAGES = ("curl", "ca-certificates", "git")

_ACTION_KWARGS = frozenset(("cache", "env", "timeout_seconds", "label", "key"))

_VERSION_RE = re.compile(r"^[0-9]+\.[0-9]+(\.[0-9]+)?$")


def _go_install_cmd(version: str) -> str:
    return (
        f"curl -fsSL https://go.dev/dl/go{version}.linux-amd64.tar.gz "
        "-o /tmp/go.tgz && rm -rf /usr/local/go && "
        "tar -C /usr/local -xzf /tmp/go.tgz && "
        "ln -sf /usr/local/go/bin/go /usr/local/bin/go && "
        "ln -sf /usr/local/go/bin/gofmt /usr/local/bin/gofmt && "
        "go version"
    )


@dataclass(frozen=True)
class GoToolchain:
    path: str
    installed: Step

    def _emit(self, cmd: str, default_label: str, **kw: Any) -> Step:
        if kw.get("label") is None:
            kw["label"] = default_label
        return self.installed.sh(cmd, **kw)

    def build(self, **kw: Any) -> Step:
        return self._emit(f"cd {self.path} && go build ./...", ":go: build", **kw)

    def test(self, **kw: Any) -> Step:
        return self._emit(f"cd {self.path} && go test ./...", ":go: test", **kw)

    def vet(self, **kw: Any) -> Step:
        return self._emit(f"cd {self.path} && go vet ./...", ":go: vet", **kw)

    def fmt(self, **kw: Any) -> Step:
        return self._emit(
            f'cd {self.path} && test -z "$(gofmt -l .)"',
            ":go: fmt",
            **kw,
        )


def _make_go(
    *,
    path: str = ".",
    version: str = "1.23.2",
    image: str | None = None,
    base: Step | None = None,
) -> GoToolchain:
    if not _VERSION_RE.match(version):
        msg = f'hm.go: invalid version {version!r}\n  → use a Go version like "1.23.2"'
        raise ValueError(msg)
    installed = make_install_chain(
        apt_packages=APT_PACKAGES,
        install_cmd=_go_install_cmd(version),
        install_cache=CacheForever(env_keys=()),
        lang_tag="go",
        install_tag="install",
        image=image,
        base=base,
    )
    return GoToolchain(path=path, installed=installed)


class _GoEntry:
    def __call__(
        self,
        *,
        path: str = ".",
        version: str = "1.23.2",
        image: str | None = None,
        base: Step | None = None,
    ) -> GoToolchain:
        return _make_go(path=path, version=version, image=image, base=base)

    def build(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).build(**action_kw)

    def test(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).test(**action_kw)

    def vet(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).vet(**action_kw)

    def fmt(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).fmt(**action_kw)


go = _GoEntry()
