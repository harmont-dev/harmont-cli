"""End-to-end render checks against harmont-cli example pipelines.

Gated: skipped when HM_CLI_PATH is unset. CI sets it after
cloning harmont-cli.
"""

from __future__ import annotations

import json
from typing import TYPE_CHECKING

import pytest

if TYPE_CHECKING:
    import pathlib

from .examples_render_conftest import (
    harmont_cli_examples_root,
    isolated_registry,
    load_pipeline_module,
)

EXAMPLES_ROOT = harmont_cli_examples_root()

pytestmark = pytest.mark.skipif(
    EXAMPLES_ROOT is None,
    reason="HM_CLI_PATH not set or examples/ missing",
)


def _example_dirs() -> list[pathlib.Path]:
    if EXAMPLES_ROOT is None:
        return []
    return sorted(
        p for p in EXAMPLES_ROOT.iterdir() if p.is_dir() and (p / ".hm" / "pipeline.py").is_file()
    )


EXAMPLE_IDS = [p.name for p in _example_dirs()]


@pytest.mark.parametrize("example_dir", _example_dirs(), ids=EXAMPLE_IDS)
def test_example_renders_to_step_chain(
    example_dir: pathlib.Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    import harmont as hm

    monkeypatch.chdir(example_dir)
    with isolated_registry():
        load_pipeline_module(example_dir)
        envelope_json = hm.dump_registry_json()

    envelope = json.loads(envelope_json)
    assert envelope["schema_version"] == "1"
    assert envelope["pipelines"], f"{example_dir.name}: no pipelines registered"

    ci_pipeline = next((p for p in envelope["pipelines"] if p["slug"] == "ci"), None)
    assert ci_pipeline is not None, (
        f"{example_dir.name}: no 'ci' pipeline registered; "
        f"got slugs {[p['slug'] for p in envelope['pipelines']]}"
    )
    # The envelope now carries the raw step chain; lowering to the v0 IR
    # (env layering, key resolution, default-image stamp) happens Rust-side.
    step_chain = ci_pipeline["step_chain"]
    steps = step_chain["steps"]
    assert steps, f"{example_dir.name}: ci pipeline has no steps"
    assert step_chain["leaf_indices"], f"{example_dir.name}: ci pipeline has no leaves"
    assert any(s.get("cmd") for s in steps), (
        f"{example_dir.name}: ci pipeline has no command steps"
    )
