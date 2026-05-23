"""Gradle (Java/Kotlin) toolchain.

Chain: scratch -> apt-base (curl, openjdk-<N>-jdk-headless) -> jdk-verify
(``java -version && gradle --version`` smoke test, cached forever) ->
action leaves running ``gradle`` directly. The verify step lets
``make_install_chain`` enforce its standard shape even though the
JDK install happens via apt; it also gives the pipeline UI a single
named step that confirms the JDK is operational.

Gradle itself is installed from the official distribution zip into
``/opt/gradle`` and symlinked onto PATH. We deliberately do NOT rely
on a project-shipped ``./gradlew`` wrapper: for the examples and
small projects we want a working pipeline out of the box, not a
chicken-and-egg requirement that the user pre-populate ``gradlew``.
Pipelines that do ship a wrapper can still invoke it from their
own step layered on ``gradle.installed``.

The ``kotlin=True`` flag swaps the label prefix only — Gradle drives
both languages identically.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any

from ._toolchain import make_install_chain
from .cache import CacheForever

if TYPE_CHECKING:
    from ._step import Step

_ACTION_KWARGS = frozenset(("cache", "env", "timeout_seconds", "label", "key"))

_JDK_RE = re.compile(r"^(11|17|21)$")

# Pinned Gradle version — bumping requires re-running the example
# pipelines locally to confirm tasks still work; older Gradle releases
# may not support newer Kotlin/Java toolchain features.
_GRADLE_VERSION = "8.10"


def _apt_packages(jdk: str) -> tuple[str, ...]:
    return ("curl", "ca-certificates", "unzip", f"openjdk-{jdk}-jdk-headless")


def _install_cmd() -> str:
    return (
        f"curl -fsSL https://services.gradle.org/distributions/"
        f"gradle-{_GRADLE_VERSION}-bin.zip -o /tmp/gradle.zip && "
        "unzip -q /tmp/gradle.zip -d /opt && "
        f"ln -sf /opt/gradle-{_GRADLE_VERSION}/bin/gradle /usr/local/bin/gradle && "
        "rm /tmp/gradle.zip && java -version && gradle --version"
    )


@dataclass(frozen=True)
class GradleProject:
    path: str
    installed: Step
    _tag: str

    def _emit(self, cmd: str, default_label: str, **kw: Any) -> Step:
        if kw.get("label") is None:
            kw["label"] = default_label
        return self.installed.sh(cmd, **kw)

    def build(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && gradle build", f":{self._tag}: build", **kw,
        )

    def test(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && gradle test", f":{self._tag}: test", **kw,
        )

    def lint(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && gradle check", f":{self._tag}: lint", **kw,
        )


def _make_gradle(
    *,
    path: str = ".",
    jdk: str = "21",
    kotlin: bool = False,
    image: str | None = None,
    base: Step | None = None,
) -> GradleProject:
    if not _JDK_RE.match(jdk):
        msg = (
            f"hm.gradle: invalid jdk {jdk!r}\n"
            '  → use "11", "17", or "21"'
        )
        raise ValueError(msg)
    tag = "kotlin" if kotlin else "java"
    installed = make_install_chain(
        apt_packages=_apt_packages(jdk),
        install_cmd=_install_cmd(),
        install_cache=CacheForever(env_keys=()),
        lang_tag=tag,
        install_tag="jdk",
        image=image,
        base=base,
    )
    return GradleProject(path=path, installed=installed, _tag=tag)


class _GradleEntry:
    def __call__(
        self,
        *,
        path: str = ".",
        jdk: str = "21",
        kotlin: bool = False,
        image: str | None = None,
        base: Step | None = None,
    ) -> GradleProject:
        return _make_gradle(path=path, jdk=jdk, kotlin=kotlin, image=image, base=base)

    def build(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).build(**action_kw)

    def test(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).test(**action_kw)

    def lint(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).lint(**action_kw)


gradle = _GradleEntry()
