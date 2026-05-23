"""dotnet (C#) toolchain.

Chain: scratch -> apt-base (curl, ca-certificates, libicu-dev) ->
dotnet-install (via Microsoft's dotnet-install.sh) -> action leaves.
The dotnet-install step is cached forever, keyed on the channel baked
into the command.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any

from ._toolchain import make_install_chain
from .cache import CacheForever

if TYPE_CHECKING:
    from ._step import Step

APT_PACKAGES = ("curl", "ca-certificates", "libicu-dev")

_ACTION_KWARGS = frozenset(("cache", "env", "timeout_seconds", "label", "key"))

_CHANNEL_RE = re.compile(r"^([0-9]+\.[0-9]+|LTS|STS)$")

_INSTALL_SCRIPT = "/tmp/dotnet-install.sh"  # noqa: S108


def _dotnet_install_cmd(channel: str) -> str:
    return (
        f"curl -fsSL https://dot.net/v1/dotnet-install.sh -o {_INSTALL_SCRIPT} && "
        f"chmod +x {_INSTALL_SCRIPT} && "
        f"{_INSTALL_SCRIPT} --channel {channel} --install-dir /usr/local/dotnet && "
        "ln -sf /usr/local/dotnet/dotnet /usr/local/bin/dotnet && "
        "dotnet --info"
    )


@dataclass(frozen=True)
class DotnetProject:
    path: str
    installed: Step

    def _emit(self, cmd: str, default_label: str, **kw: Any) -> Step:
        if kw.get("label") is None:
            kw["label"] = default_label
        return self.installed.sh(cmd, **kw)

    def build(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && dotnet build", ":dotnet: build", **kw,
        )

    def test(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && dotnet test", ":dotnet: test", **kw,
        )

    def fmt(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && dotnet format --verify-no-changes",
            ":dotnet: fmt", **kw,
        )


def _make_dotnet(
    *,
    path: str = ".",
    channel: str = "8.0",
    image: str | None = None,
    base: Step | None = None,
) -> DotnetProject:
    if not _CHANNEL_RE.match(channel):
        msg = (
            f"hm.dotnet: invalid channel {channel!r}\n"
            '  → use "8.0", "LTS", or "STS"'
        )
        raise ValueError(msg)
    installed = make_install_chain(
        apt_packages=APT_PACKAGES,
        install_cmd=_dotnet_install_cmd(channel),
        install_cache=CacheForever(env_keys=()),
        lang_tag="dotnet",
        install_tag="install",
        image=image,
        base=base,
    )
    return DotnetProject(path=path, installed=installed)


class _DotnetEntry:
    def __call__(
        self,
        *,
        path: str = ".",
        channel: str = "8.0",
        image: str | None = None,
        base: Step | None = None,
    ) -> DotnetProject:
        return _make_dotnet(path=path, channel=channel, image=image, base=base)

    def build(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).build(**action_kw)

    def test(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).test(**action_kw)

    def fmt(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).fmt(**action_kw)


dotnet = _DotnetEntry()
