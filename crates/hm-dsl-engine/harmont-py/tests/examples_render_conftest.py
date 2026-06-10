"""Shared helpers for rendering external example pipelines.

These tests render the pipeline definitions in harmont-cli/examples/
to v0 IR JSON. They are gated behind HARMONT_CLI_PATH so they only
run when a sibling harmont-cli checkout is available.
"""

from __future__ import annotations

import importlib.util
import os
import pathlib
import sys
from contextlib import contextmanager
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Iterator


def harmont_cli_examples_root() -> pathlib.Path | None:
    raw = os.environ.get("HARMONT_CLI_PATH")
    if not raw:
        return None
    p = pathlib.Path(raw) / "examples"
    return p if p.is_dir() else None


@contextmanager
def isolated_registry() -> Iterator[None]:
    """Snapshot and restore the global @hm.pipeline and @hm.target
    registries so that each parametrized case renders against an
    empty slate. Without this, every case would accumulate pipelines
    from prior cases and duplicate slugs would raise.
    """
    from harmont import _deps, _registry, _target

    saved_regs = list(_registry.REGISTRATIONS)
    saved_targets_by_name = dict(_deps._TARGETS_BY_NAME)  # noqa: SLF001
    saved_target_cache = dict(_target._TARGET_CACHE)  # noqa: SLF001

    _registry.clear_registry()
    _deps.clear_target_names()
    _target.clear_target_cache()
    try:
        yield
    finally:
        _registry.clear_registry()
        _deps.clear_target_names()
        _target.clear_target_cache()
        _registry.REGISTRATIONS.extend(saved_regs)
        _deps._TARGETS_BY_NAME.update(saved_targets_by_name)  # noqa: SLF001
        _target._TARGET_CACHE.update(saved_target_cache)  # noqa: SLF001


def load_pipeline_module(example_dir: pathlib.Path) -> None:
    """Load .hm/pipeline.py from `example_dir`, executing decorator
    side-effects. Run with cwd = example_dir so on_change cache paths
    resolve correctly.
    """
    pipeline_py = example_dir / ".hm" / "pipeline.py"
    spec = importlib.util.spec_from_file_location(
        f"_harmont_example_{example_dir.name}", pipeline_py
    )
    assert spec is not None
    assert spec.loader is not None
    mod = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = mod
    try:
        spec.loader.exec_module(mod)
    finally:
        sys.modules.pop(spec.name, None)
