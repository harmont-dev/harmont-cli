"""CMake (C/C++) toolchain.

Public surface lives on the module-level singleton ``cmake``. Call it
to construct a ``CMakeProject``, or use the bare-form action methods
(``cmake.configure()``, ``cmake.build()``, etc.) for a one-shot leaf.

The chain is:

    scratch -> apt-base (build-essential, cmake, ninja-build, clang-format)
            -> cmake-verify (cmake --version && clang-format --version,
               cached forever)
            -> action leaves

The ``lang="cpp"`` switch swaps the label prefix from ``:c:`` to
``:cpp:`` only — cmake routes by ``CMakeLists.txt`` and the shell
commands are identical for both languages.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, Any

from ._toolchain import make_install_chain
from .cache import CacheForever

if TYPE_CHECKING:
    from ._step import Step

APT_PACKAGES = ("build-essential", "cmake", "ninja-build", "clang-format")

_ACTION_KWARGS = frozenset(("cache", "env", "timeout_seconds", "label", "key"))


@dataclass(frozen=True)
class CMakeProject:
    """CMake (C/C++) project install chain — constructed via ``hm.cmake()``.

    ``installed`` is the cmake-verify step. Action methods (``configure``,
    ``build``, ``test``, ``fmt``) attach leaves to ``installed``.
    """

    path: str
    installed: Step
    _tag: str

    def _emit(self, cmd: str, default_label: str, **kw: Any) -> Step:
        if kw.get("label") is None:
            kw["label"] = default_label
        return self.installed.sh(cmd, **kw)

    def configure(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && cmake -S . -B build",
            f":{self._tag}: configure",
            **kw,
        )

    def build(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && cmake -S . -B build && cmake --build build",
            f":{self._tag}: build",
            **kw,
        )

    def test(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && cmake -S . -B build && cmake --build build "
            "&& ctest --test-dir build --output-on-failure",
            f":{self._tag}: test",
            **kw,
        )

    def fmt(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && find src tests -name '*.[ch]' "
            f"-o -name '*.cpp' -o -name '*.hpp' | "
            f"xargs clang-format --dry-run --Werror",
            f":{self._tag}: fmt",
            **kw,
        )


def _make_cmake(
    *,
    path: str = ".",
    lang: str = "c",
    image: str | None = None,
    base: Step | None = None,
) -> CMakeProject:
    if lang not in ("c", "cpp"):
        msg = f'hm.cmake: invalid lang {lang!r}\n  → use "c" or "cpp"'
        raise ValueError(msg)
    installed = make_install_chain(
        apt_packages=APT_PACKAGES,
        install_cmd="cmake --version && clang-format --version",
        install_cache=CacheForever(env_keys=()),
        lang_tag=lang,
        install_tag="cmake-verify",
        image=image,
        base=base,
    )
    return CMakeProject(path=path, installed=installed, _tag=lang)


class CMakeEntry:
    """Callable singleton for the CMake toolchain — access as ``hm.cmake``.

    Call directly to construct a ``CMakeProject``, or use the bare-form
    action methods (``cmake.build()``, ``cmake.test()``, etc.) for a
    one-shot leaf.
    """

    def __call__(
        self,
        *,
        path: str = ".",
        lang: str = "c",
        image: str | None = None,
        base: Step | None = None,
    ) -> CMakeProject:
        """Verify cmake and clang-format are available, returning a project object.

        Args:
            path: Path to the project root (where ``CMakeLists.txt`` lives).
            lang: Language tag, either ``"c"`` or ``"cpp"``. Affects label
                prefixes only; the cmake commands are identical.
            image: Local-mode Docker base image override.
            base: Existing ``Step`` to attach to instead of emitting a fresh
                apt-base step.

        Returns:
            A ``CMakeProject`` ready for action methods.

        Examples:
            >>> import harmont as hm
            >>> proj = hm.cmake(lang="cpp")
            >>> hm.pipeline(proj.build(), proj.test())
        """
        return _make_cmake(path=path, lang=lang, image=image, base=base)

    def configure(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).configure(**action_kw)

    def build(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).build(**action_kw)

    def test(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).test(**action_kw)

    def fmt(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).fmt(**action_kw)


cmake: CMakeEntry = CMakeEntry()
