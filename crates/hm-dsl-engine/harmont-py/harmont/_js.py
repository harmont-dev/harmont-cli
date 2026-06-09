"""Unified JavaScript/TypeScript toolchain — runtime x package-manager axes.

Replaces the separate npm/bun toolchains with one ``js`` entry (``ts`` is an
alias). Runtimes execute JS/TS; package managers install dependencies. ``deno``
is a runtime whose dependency management is intrinsic, so it is also its own
``pm`` value — setting ``pm`` explicitly with ``runtime="deno"`` is rejected.
yarn's classic/berry split is two ``pm`` values rather than a version axis
because the lockfile install flag differs between them.

The chain is:

    scratch -> apt-base -> runtime-install -> [pm-bootstrap] -> deps -> leaves

The package manager is layered onto the runtime image only when it isn't
bundled (npm ships with node, bun with the bun runtime). pnpm/yarn are brought
in via ``corepack``, which pins their version from the project's
``packageManager`` field.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any, Literal

from ._detect import detect
from ._toolchain import (
    bun_install_cmd,
    deno_install_cmd,
    make_install_chain,
    node_install_cmd,
)
from .cache import CacheForever, CacheOnChange

if TYPE_CHECKING:
    from ._step import Step

Runtime = Literal["node", "bun", "deno"]
PackageManager = Literal["npm", "pnpm", "yarn-classic", "yarn-berry", "bun", "deno"]

_NODE_VERSION_RE = re.compile(r"^[0-9]+(\.x)?$")
_SEMVER_RE = re.compile(r"^[0-9]+\.[0-9]+(\.[0-9]+)?$")

_LOCKFILES: dict[PackageManager, str] = {
    "npm": "package-lock.json",
    "pnpm": "pnpm-lock.yaml",
    "yarn-classic": "yarn.lock",
    "yarn-berry": "yarn.lock",
    "bun": "bun.lock",
    "deno": "deno.lock",
}

_DEPS_CMD: dict[PackageManager, str] = {
    "npm": "npm ci",
    "pnpm": "pnpm install --frozen-lockfile",
    "yarn-classic": "yarn install --frozen-lockfile",
    "yarn-berry": "yarn install --immutable",
    "bun": "bun install --frozen-lockfile",
    "deno": "deno install",
}

_RUN_PREFIX: dict[PackageManager, str] = {
    "npm": "npm run",
    "pnpm": "pnpm run",
    "yarn-classic": "yarn run",
    "yarn-berry": "yarn run",
    "bun": "bun run",
    "deno": "deno task",
}


def _pm_bootstrap(pm: PackageManager, runtime: Runtime) -> str | None:
    """Command to bring ``pm`` onto the runtime image, or ``None`` when the PM
    already ships with the runtime (npm with node, bun/deno with their own
    runtime)."""
    if pm == "npm":
        return None  # bundled with node
    if pm == "bun":
        return None if runtime == "bun" else bun_install_cmd()
    if pm == "deno":
        return None  # bundled with deno
    if pm == "pnpm":
        return "corepack enable pnpm"
    # corepack resolves the exact yarn from the `packageManager` field; its
    # bundled default is classic 1.x, which suits yarn-classic.
    return "corepack enable"


def _validate_version(runtime: Runtime, version: str) -> None:
    if runtime == "node":
        if not _NODE_VERSION_RE.match(version):
            msg = (
                f"hm.js: invalid version {version!r}\n"
                '  → use a Node major version like "22" or "22.x"'
            )
            raise ValueError(msg)
    elif not _SEMVER_RE.match(version):
        msg = (
            f"hm.js: invalid version {version!r}\n"
            '  → use a semver version like "1.2" or "1.2.0"'
        )
        raise ValueError(msg)


@dataclass(frozen=True)
class JsProject:
    """JS/TS project install chain — constructed via ``hm.js.project()``.

    ``installed`` is the dependency-install step (``npm ci``, ``bun install``,
    ``deno install``, …). ``run`` attaches leaves to it so installation is
    shared across CI actions.
    """

    path: str
    installed: Step
    run_prefix: str
    tag: str

    def install(self) -> Step:
        """Return the dependency-install step (the unambiguous default leaf)."""
        return self.installed

    def run(self, script: str, **kw: Any) -> Step:
        """Run a package.json script / deno.json task by name.

        This is the uniform action across all package managers — for native
        tooling (``deno test``, ``bun test``) define a script or drop to
        ``.sh()``.
        """
        if kw.get("label") is None:
            kw["label"] = f":{self.tag}: {script}"
        return self.installed.sh(f"cd {self.path} && {self.run_prefix} {script}", **kw)


def _make_js(
    *,
    path: str = ".",
    pm: PackageManager | None = None,
    runtime: Runtime | None = None,
    version: str | None = None,
    image: str | None = None,
    base: Step | None = None,
) -> JsProject:
    detected = detect(path) if runtime is None and pm is None else None
    runtime = (
        runtime
        if runtime is not None
        else (detected.runtime if detected and detected.runtime else "node")
    )

    if version is not None:
        _validate_version(runtime, version)

    # --- Deno: built-in PM, no pm option ---
    if runtime == "deno":
        if pm is not None:
            msg = 'hm.js: runtime="deno" manages its own dependencies — do not set pm'
            raise ValueError(msg)
        runtime_installed = make_install_chain(
            apt_packages=("curl", "ca-certificates", "unzip"),
            install_cmd=deno_install_cmd(version),
            install_cache=CacheForever(env_keys=()),
            lang_tag="deno",
            install_tag="install",
            image=image,
            base=base,
        )
        deps = runtime_installed.sh(
            f"cd {path} && {_DEPS_CMD['deno']}",
            label=":deno: deps",
            cache=CacheOnChange(paths=(f"{path}/{_LOCKFILES['deno']}",)),
        )
        return JsProject(path=path, installed=deps, run_prefix=_RUN_PREFIX["deno"], tag="deno")

    # --- Node / Bun runtime ---
    detected_pm = detected.pm if detected else None
    resolved_pm: PackageManager = (
        pm
        if pm is not None
        else (detected_pm if detected_pm is not None else ("bun" if runtime == "bun" else "npm"))
    )

    if resolved_pm == "deno":
        msg = 'hm.js: pm="deno" is not valid — use runtime="deno" instead'
        raise ValueError(msg)

    if runtime == "bun" and resolved_pm != "bun":
        msg = 'hm.js: runtime="bun" only supports pm="bun"'
        raise ValueError(msg)

    apt = ["curl", "ca-certificates"]
    if runtime == "bun" or resolved_pm == "bun":
        apt.append("unzip")  # bun's installer needs unzip

    lang_tag = "bun" if runtime == "bun" else "node"
    runtime_cmd = (
        bun_install_cmd(version) if runtime == "bun" else node_install_cmd(version or "22")
    )

    runtime_installed = make_install_chain(
        apt_packages=tuple(apt),
        install_cmd=runtime_cmd,
        install_cache=CacheForever(env_keys=()),
        lang_tag=lang_tag,
        install_tag="install",
        image=image,
        base=base,
    )

    # Layer the package manager onto the runtime image when it isn't bundled.
    bootstrap = _pm_bootstrap(resolved_pm, runtime)
    pm_ready = (
        runtime_installed
        if bootstrap is None
        else runtime_installed.sh(
            bootstrap,
            label=f":{lang_tag}: {resolved_pm}",
            cache=CacheForever(env_keys=()),
        )
    )

    deps = pm_ready.sh(
        f"cd {path} && {_DEPS_CMD[resolved_pm]}",
        label=f":{lang_tag}: deps",
        cache=CacheOnChange(paths=(f"{path}/{_LOCKFILES[resolved_pm]}",)),
    )
    return JsProject(path=path, installed=deps, run_prefix=_RUN_PREFIX[resolved_pm], tag=lang_tag)


class _JsEntry:
    """Entry for the unified JS/TS toolchain — access as ``hm.js`` (or ``hm.ts``).

    Call ``hm.js.project(...)`` to construct a :class:`JsProject`.
    """

    def project(
        self,
        *,
        path: str = ".",
        pm: PackageManager | None = None,
        runtime: Runtime | None = None,
        version: str | None = None,
        image: str | None = None,
        base: Step | None = None,
    ) -> JsProject:
        """Install a JS/TS runtime + package manager and return a project.

        Args:
            path: Path to the project root (containing the lockfile).
            pm: Package manager. Defaults to ``bun`` when ``runtime="bun"``,
                else ``npm``. Must not be set when ``runtime="deno"``.
            runtime: ``"node"`` (default), ``"bun"``, or ``"deno"``.
            version: Runtime version — Node major (``"22"``/``"22.x"``) or
                Bun/Deno semver (``"1.2.3"``). PM versions are pinned by the
                project's ``packageManager`` field.
            image: Local-mode Docker base image override.
            base: Existing ``Step`` to attach to instead of a fresh apt-base.

        Returns:
            A :class:`JsProject` whose ``installed`` step installs dependencies.

        Examples:
            >>> import harmont as hm
            >>> proj = hm.js.project(path="web", runtime="bun")
            >>> hm.pipeline([proj.run("test"), proj.run("lint")])
        """
        return _make_js(
            path=path, pm=pm, runtime=runtime, version=version, image=image, base=base
        )


js: _JsEntry = _JsEntry()
ts: _JsEntry = js
