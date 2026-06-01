"""Composer (PHP / Laravel) toolchain abstraction.

Chain: scratch -> apt-base (php-cli + extensions + composer + git + unzip) ->
composer-verify (``composer --version && php --version``, cached forever) ->
composer-deps (``composer install``, cached on ``composer.lock``) ->
action leaves. The ``laravel=True`` switch swaps ``.test()`` to
``php artisan test`` and changes the label prefix from ``:php:`` to
``:laravel:``.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, Any

from ._toolchain import make_install_chain
from .cache import CacheForever, CacheOnChange

if TYPE_CHECKING:
    from ._step import Step

APT_PACKAGES = (
    "php-cli",
    "php-mbstring",
    "php-xml",
    "php-curl",
    "php-sqlite3",
    "composer",
    "git",
    "unzip",
)

_ACTION_KWARGS = frozenset(("cache", "env", "timeout_seconds", "label", "key"))


@dataclass(frozen=True)
class ComposerProject:
    """Composer (PHP/Laravel) project install chain — constructed via ``hm.composer()``.

    ``installed`` is the ``composer install`` step. Action methods (``test``,
    ``lint``) attach leaves to ``installed``.
    """

    path: str
    installed: Step
    _tag: str
    _laravel: bool

    def _emit(self, cmd: str, default_label: str, **kw: Any) -> Step:
        if kw.get("label") is None:
            kw["label"] = default_label
        return self.installed.sh(cmd, **kw)

    def test(self, **kw: Any) -> Step:
        cmd = (
            f"cd {self.path} && php artisan test"
            if self._laravel
            else f"cd {self.path} && vendor/bin/phpunit"
        )
        return self._emit(cmd, f":{self._tag}: test", **kw)

    def lint(self, **kw: Any) -> Step:
        return self._emit(
            f"cd {self.path} && vendor/bin/phpstan analyse",
            f":{self._tag}: lint",
            **kw,
        )


def _make_composer(
    *,
    path: str = ".",
    laravel: bool = False,
    image: str | None = None,
    base: Step | None = None,
) -> ComposerProject:
    tag = "laravel" if laravel else "php"
    composer_verified = make_install_chain(
        apt_packages=APT_PACKAGES,
        install_cmd="composer --version && php --version",
        install_cache=CacheForever(env_keys=()),
        lang_tag=tag,
        install_tag="composer",
        image=image,
        base=base,
    )
    deps = composer_verified.sh(
        f"cd {path} && composer install --no-interaction --prefer-dist",
        label=f":{tag}: deps",
        cache=CacheOnChange(paths=(f"{path}/composer.lock",)),
    )
    return ComposerProject(path=path, installed=deps, _tag=tag, _laravel=laravel)


class ComposerEntry:
    """Callable singleton for the Composer toolchain — access as ``hm.composer``.

    Call directly to construct a ``ComposerProject``, or use the bare-form
    action methods (``composer.test()``, ``composer.lint()``) for a one-shot leaf.
    """

    def __call__(
        self,
        *,
        path: str = ".",
        laravel: bool = False,
        image: str | None = None,
        base: Step | None = None,
    ) -> ComposerProject:
        """Install Composer dependencies and return a project object.

        Args:
            path: Path to the PHP project root (must contain a
                ``composer.lock``).
            laravel: When ``True``, uses ``php artisan test`` for ``.test()``
                and sets the label prefix to ``:laravel:`` instead of
                ``:php:``.
            image: Local-mode Docker base image override.
            base: Existing ``Step`` to attach to instead of emitting a fresh
                apt-base step.

        Returns:
            A ``ComposerProject`` whose ``installed`` step is ``composer install``.

        Examples:
            >>> import harmont as hm
            >>> proj = hm.composer(laravel=True)
            >>> hm.pipeline(proj.test())
        """
        return _make_composer(path=path, laravel=laravel, image=image, base=base)

    def test(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).test(**action_kw)

    def lint(self, **kw: Any) -> Step:
        action_kw = {k: kw.pop(k) for k in list(kw) if k in _ACTION_KWARGS}
        return self(**kw).lint(**action_kw)


composer: ComposerEntry = ComposerEntry()
