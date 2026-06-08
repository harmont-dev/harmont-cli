"""Elixir toolchain abstraction.

Chain: scratch -> apt-base (curl, ca-certificates, git, build-essential,
autoconf, libncurses-dev, libssl-dev) -> erlang-install (esl-erlang .deb,
cached forever) -> elixir-install (prebuilt zip, cached forever) ->
mix-deps (cached on mix.lock) -> action leaves.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from dataclasses import field as dataclass_field
from typing import TYPE_CHECKING, Any

from ._toolchain import make_install_chain
from .cache import CacheForever, CacheOnChange

if TYPE_CHECKING:
    from ._step import Step

APT_PACKAGES = (
    "curl",
    "ca-certificates",
    "git",
    "unzip",
    "build-essential",
    "autoconf",
    "libncurses-dev",
    "libssl-dev",
)

_ACTION_KWARGS = frozenset(("cache", "env", "timeout_seconds", "label", "key"))

_ELIXIR_ACTION_KWARGS = frozenset(("cover", "partitions", "strict", "mix_env"))

_ELIXIR_ENV = {"ELIXIR_ERL_OPTIONS": "+fnu"}

ELIXIR_VERSION_RE = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+$")
OTP_VERSION_RE = re.compile(r"^[0-9]+(\.[0-9]+(\.[0-9]+)?)?$")


def _erlang_install_cmd(otp_version: str) -> str:
    return (
        f"curl -fsSL https://binaries2.erlang-solutions.com/debian/pool/contrib/e/"
        f"esl-erlang/esl-erlang_{otp_version}-1~debian~bookworm_amd64.deb "
        f"-o /tmp/erlang.deb && (dpkg -i /tmp/erlang.deb || apt-get install -fy) && "
        f"erl -eval 'erlang:display(erlang:system_info(otp_release)), halt().' -noshell"
    )


def _elixir_install_cmd(elixir_version: str, otp_major: str) -> str:
    return (
        f"curl -fsSL https://github.com/elixir-lang/elixir/releases/download/"
        f"v{elixir_version}/elixir-otp-{otp_major}.zip -o /tmp/elixir.zip && "
        f"unzip -q /tmp/elixir.zip -d /usr/local/elixir && "
        f"ln -sf /usr/local/elixir/bin/elixir /usr/local/bin/elixir && "
        f"ln -sf /usr/local/elixir/bin/mix /usr/local/bin/mix && "
        f"ln -sf /usr/local/elixir/bin/iex /usr/local/bin/iex && "
        f"mix local.hex --force && mix local.rebar --force && elixir --version"
    )


@dataclass(frozen=True)
class ElixirProject:
    """Elixir project install chain — constructed via ``hm.elixir()``.

    ``installed`` is the ``mix deps.get && mix deps.compile`` step. Action
    methods attach leaves to ``installed``.
    """

    path: str
    installed: Step
    _plt_step: Step | None = dataclass_field(default=None, init=False, repr=False)

    def _emit(self, cmd: str, default_label: str, **kw: Any) -> Step:
        if kw.get("label") is None:
            kw["label"] = default_label
        kw["env"] = {**_ELIXIR_ENV, **(kw.get("env") or {})}
        return self.installed.sh(cmd, **kw)

    def compile(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && mix compile --warnings-as-errors",
            ":ex: compile",
            **kw,
        )

    def test(self, **kw: Any) -> Step:
        cover = kw.pop("cover", False)
        partitions = kw.pop("partitions", None)

        cmd = "mix test"
        if cover:
            cmd += " --cover"
        if partitions is not None:
            cmd += f" --partitions {partitions}"

        cmd = f"cd {self.path} && {cmd}"
        return self._emit(cmd, ":ex: test", **kw)

    def format(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && mix format --check-formatted",
            ":ex: format",
            **kw,
        )

    def credo(self, **kw: Any) -> Step:
        strict = kw.pop("strict", True)
        cmd = "mix credo"
        if strict:
            cmd += " --strict"
        return self._emit(f"cd {self.path} && {cmd}", ":ex: credo", **kw)

    def plt(self) -> Step:
        if self._plt_step is not None:
            return self._plt_step
        step = self.installed.sh(
            f"cd {self.path} && mix dialyzer --plt",
            label=":ex: plt",
            cache=CacheOnChange(paths=(f"{self.path}/mix.lock",)),
            env=_ELIXIR_ENV,
        )
        object.__setattr__(self, "_plt_step", step)
        return step

    def dialyzer(self, **kw: Any) -> Step:
        if kw.get("label") is None:
            kw["label"] = ":ex: dialyzer"
        kw["env"] = {**_ELIXIR_ENV, **(kw.get("env") or {})}
        return self.plt().sh(f"cd {self.path} && mix dialyzer", **kw)

    def sobelow(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && mix sobelow --exit",
            ":ex: sobelow",
            **kw,
        )

    def deps_audit(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && mix deps.audit",
            ":ex: deps-audit",
            **kw,
        )

    def hex_audit(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && mix hex.audit",
            ":ex: hex-audit",
            **kw,
        )

    def release(self, **kw: Any) -> Step:
        mix_env = kw.pop("mix_env", "prod")
        return self._emit(
            f"cd {self.path} && MIX_ENV={mix_env} mix release",
            ":ex: release",
            **kw,
        )

    def mix(self, task: str, **kw: Any) -> Step:
        return self._emit(f"cd {self.path} && mix {task}", f":ex: {task}", **kw)


def _make_elixir(
    *,
    path: str = ".",
    elixir_version: str = "1.18.3",
    otp_version: str = "27.3.3",
    image: str | None = None,
    base: Step | None = None,
) -> ElixirProject:
    if not ELIXIR_VERSION_RE.match(elixir_version):
        msg = (
            f"hm.elixir: invalid elixir version {elixir_version!r}\n"
            '  → use a version like "1.18.3"'
        )
        raise ValueError(msg)
    if not OTP_VERSION_RE.match(otp_version):
        msg = f'hm.elixir: invalid otp version {otp_version!r}\n  → use a version like "27.3.3"'
        raise ValueError(msg)

    otp_major = otp_version.split(".")[0]

    erlang_installed = make_install_chain(
        apt_packages=APT_PACKAGES,
        install_cmd=_erlang_install_cmd(otp_version),
        install_cache=CacheForever(env_keys=()),
        lang_tag="ex",
        install_tag="erlang-install",
        image=image,
        base=base,
    )
    elixir_installed = erlang_installed.sh(
        _elixir_install_cmd(elixir_version, otp_major),
        label=":ex: elixir-install",
        cache=CacheForever(env_keys=()),
        env=_ELIXIR_ENV,
    )
    deps = elixir_installed.sh(
        f"cd {path} && mix deps.get && mix deps.compile",
        label=":ex: mix-deps",
        cache=CacheOnChange(paths=(f"{path}/mix.lock",)),
        env=_ELIXIR_ENV,
    )
    return ElixirProject(path=path, installed=deps)


class ElixirEntry:
    """Callable singleton for the Elixir toolchain — access as ``hm.elixir``.

    Call directly to construct an ``ElixirProject``, or use the bare-form
    action methods (``elixir.compile()``, ``elixir.test()``, etc.) for a
    one-shot leaf.
    """

    def __call__(
        self,
        *,
        path: str = ".",
        elixir_version: str = "1.18.3",
        otp_version: str = "27.3.3",
        image: str | None = None,
        base: Step | None = None,
    ) -> ElixirProject:
        """Install Elixir (and Erlang/OTP) and return a project object.

        Args:
            path: Path to the Elixir project root (must contain a
                ``mix.lock``).
            elixir_version: Elixir version to install (e.g. ``"1.18.3"``).
            otp_version: Erlang/OTP version to install (e.g. ``"27.3.3"``).
            image: Local-mode Docker base image override.
            base: Existing ``Step`` to attach to instead of emitting a fresh
                apt-base step.

        Returns:
            An ``ElixirProject`` whose ``installed`` step is
            ``mix deps.get && mix deps.compile``.

        Examples:
            >>> import harmont as hm
            >>> proj = hm.elixir(elixir_version="1.18.3")
            >>> hm.pipeline(proj.compile(), proj.test())
        """
        return _make_elixir(
            path=path,
            elixir_version=elixir_version,
            otp_version=otp_version,
            image=image,
            base=base,
        )

    def compile(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS | _ELIXIR_ACTION_KWARGS}
        return self(**kw).compile(**action_kw)

    def test(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS | _ELIXIR_ACTION_KWARGS}
        return self(**kw).test(**action_kw)

    def format(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS | _ELIXIR_ACTION_KWARGS}
        return self(**kw).format(**action_kw)

    def credo(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS | _ELIXIR_ACTION_KWARGS}
        return self(**kw).credo(**action_kw)

    def plt(self, **kw: Any) -> Step:
        return self(**kw).plt()

    def dialyzer(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS | _ELIXIR_ACTION_KWARGS}
        return self(**kw).dialyzer(**action_kw)

    def sobelow(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS | _ELIXIR_ACTION_KWARGS}
        return self(**kw).sobelow(**action_kw)

    def deps_audit(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS | _ELIXIR_ACTION_KWARGS}
        return self(**kw).deps_audit(**action_kw)

    def hex_audit(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS | _ELIXIR_ACTION_KWARGS}
        return self(**kw).hex_audit(**action_kw)

    def release(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS | _ELIXIR_ACTION_KWARGS}
        return self(**kw).release(**action_kw)

    def mix(self, task: str, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).mix(task, **action_kw)


elixir: ElixirEntry = ElixirEntry()
