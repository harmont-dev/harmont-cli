"""Auto-detect JS runtime and package manager from project state."""

from __future__ import annotations

import contextlib
import json
import os
from dataclasses import dataclass
from typing import Literal

Runtime = Literal["node", "bun", "deno"]
Pm = Literal["npm", "pnpm", "yarn-classic", "yarn-berry", "bun"]


@dataclass(frozen=True)
class DetectedToolchain:
    runtime: Runtime | None = None
    pm: Pm | None = None
    pm_version: str | None = None


def detect_from_package_json(package_json: dict) -> DetectedToolchain:
    runtime: Runtime | None = None
    pm: Pm | None = None
    pm_version: str | None = None

    engines = package_json.get("engines")
    if isinstance(engines, dict):
        if "bun" in engines:
            runtime = "bun"
            pm = "bun"
        elif "deno" in engines:
            runtime = "deno"
        elif "node" in engines:
            runtime = "node"

    if pm is None:
        pm_field = package_json.get("packageManager")
        if isinstance(pm_field, str):
            parts = pm_field.split("@")
            name = parts[0]
            ver = parts[1] if len(parts) > 1 else ""
            if name == "pnpm":
                pm = "pnpm"
            elif name == "bun":
                pm = "bun"
            elif name == "npm":
                pm = "npm"
            elif name == "yarn":
                try:
                    major = int(ver.split(".")[0])
                except (ValueError, IndexError):
                    major = 1
                pm = "yarn-berry" if major >= 2 else "yarn-classic"
            if pm is not None and ver:
                pm_version = ver

    return DetectedToolchain(runtime=runtime, pm=pm, pm_version=pm_version)


def detect_from_lockfiles(files: list[str]) -> DetectedToolchain:
    file_set = set(files)

    if "bun.lock" in file_set or "bun.lockb" in file_set:
        return DetectedToolchain(pm="bun", runtime="bun")
    if "pnpm-lock.yaml" in file_set:
        return DetectedToolchain(pm="pnpm")
    if "deno.lock" in file_set:
        return DetectedToolchain(runtime="deno")
    if "package-lock.json" in file_set:
        return DetectedToolchain(pm="npm")
    if "yarn.lock" in file_set:
        return DetectedToolchain(pm="yarn-classic")

    return DetectedToolchain()


def detect(path: str) -> DetectedToolchain:
    from_pkg = DetectedToolchain()
    try:
        with open(os.path.join(path, "package.json")) as f:
            from_pkg = detect_from_package_json(json.load(f))
    except (FileNotFoundError, json.JSONDecodeError, OSError):
        pass

    from_lock = DetectedToolchain()
    with contextlib.suppress(OSError):
        from_lock = detect_from_lockfiles(os.listdir(path))

    return DetectedToolchain(
        runtime=from_pkg.runtime if from_pkg.runtime is not None else from_lock.runtime,
        pm=from_pkg.pm if from_pkg.pm is not None else from_lock.pm,
        pm_version=from_pkg.pm_version,
    )
