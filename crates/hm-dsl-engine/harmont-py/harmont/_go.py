"""Go toolchain abstraction.

Chain: scratch -> apt-base (curl, ca-certificates) -> go-install (download
official tarball to /usr/local/go) -> action leaves. The go-install step
is cached forever, keyed on the Go version baked into the command.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any, Self

from ._toolchain import advance_install, make_install_chain
from .cache import CacheForever

if TYPE_CHECKING:
    from ._step import Step
    from .cache import CachePolicy

APT_PACKAGES = ("curl", "ca-certificates", "git")

_ACTION_KWARGS = frozenset(("cache", "env", "label", "key"))

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
    """Go toolchain install chain — constructed via ``hm.go()``.

    Holds the go-install step. Action methods (``build``, ``test``, ``vet``,
    ``fmt``) attach leaves to ``installed``.
    """

    path: str
    installed: Step

    def setup(
        self,
        cmd: str,
        *,
        cwd: str | None = None,
        label: str | None = None,
        cache: CachePolicy | None = None,
        env: dict[str, str] | None = None,
    ) -> Self:
        """Append a post-install command and return an advanced toolchain; chainable.

        Use for prep steps the toolchain's actions must depend on but that the SDK
        does not model natively — code generation, fixtures, extra tooling. The
        returned object's action methods fork from this step.

        Examples:
            >>> import harmont as hm
            >>> tc = hm.go(path=".").setup("go generate ./...")
        """
        return advance_install(self, cmd, cwd=cwd, label=label, cache=cache, env=env)

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


class GoEntry:
    """Callable singleton for the Go toolchain — access as ``hm.go``.

    Call directly to construct a ``GoToolchain``, or use the bare-form
    action methods (``go.build()``, ``go.test()``, etc.) for a one-shot leaf.
    """

    def __call__(
        self,
        *,
        path: str = ".",
        version: str = "1.23.2",
        image: str | None = None,
        base: Step | None = None,
    ) -> GoToolchain:
        """Install Go and return a toolchain object.

        Args:
            path: Path to the Go module root.
            version: Go version to install (e.g. ``"1.23.2"``). Must be a
                full ``MAJOR.MINOR.PATCH`` version string.
            image: Local-mode Docker base image override.
            base: Existing ``Step`` to attach to instead of emitting a fresh
                apt-base step.

        Returns:
            A ``GoToolchain`` ready for action methods.

        Examples:
            >>> import harmont as hm
            >>> tc = hm.go(version="1.23.2")
            >>> hm.pipeline([tc.test(), tc.vet()])
        """
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


go: GoEntry = GoEntry()
