"""Envelope JSON shape -- what api/cli consume."""

import json

import pytest

import harmont as hm
from harmont._deps import clear_target_names
from harmont._registry import REGISTRATIONS, clear_registry
from harmont._target import clear_target_cache


@pytest.fixture(autouse=True)
def _reset_registry():
    clear_registry()
    clear_target_cache()
    clear_target_names()
    yield
    clear_registry()
    clear_target_cache()
    clear_target_names()


def _steps(step_chain):
    return step_chain["steps"]


def _cmds(step_chain):
    return [s.get("cmd") for s in step_chain["steps"]]


def _children_of(step_chain, parent_idx):
    """Return steps whose parent_idx is parent_idx."""
    return [s for s in step_chain["steps"] if s.get("parent_idx") == parent_idx]


def test_empty_registry_emits_empty_pipelines_list():
    out = json.loads(hm.dump_registry_json())
    assert out == {"schema_version": "1", "pipelines": []}


def test_single_pipeline_no_triggers():
    @hm.pipeline("ci")
    def ci() -> hm.Step:
        return hm.scratch().sh("echo hi", label="hi")

    out = json.loads(hm.dump_registry_json())
    assert out["schema_version"] == "1"
    assert len(out["pipelines"]) == 1
    p = out["pipelines"][0]
    assert p["slug"] == "ci"
    assert p["name"] == "ci"
    assert p["allow_manual"] is True
    assert p["triggers"] == []
    step_chain = p["step_chain"]
    steps = _steps(step_chain)
    # The scratch root rides along as a passthrough (cmd=None); Rust drops it.
    cmd_steps = [s for s in steps if s.get("cmd") == "echo hi"]
    assert len(cmd_steps) == 1
    assert cmd_steps[0]["label"] == "hi"
    (leaf,) = step_chain["leaf_indices"]
    assert steps[leaf]["cmd"] == "echo hi"


def test_pipeline_with_triggers():
    @hm.pipeline(
        "ci",
        triggers=[
            hm.push(branch="main"),
            hm.pull_request(branches="main"),
        ],
    )
    def ci() -> hm.Step:
        return hm.scratch().sh("echo")

    out = json.loads(hm.dump_registry_json())
    p = out["pipelines"][0]
    assert p["triggers"] == [
        {"event": "push", "branches": ["main"]},
        {
            "event": "pull_request",
            "branches": ["main"],
            "types": ["opened", "synchronize", "reopened"],
        },
    ]


def test_pipeline_with_tuple_leaves():
    @hm.pipeline("ci")
    def ci() -> hm.Pipeline:
        fork = hm.scratch().fork()
        return (fork.sh("a"), fork.sh("b"))

    out = json.loads(hm.dump_registry_json())
    p = out["pipelines"][0]
    cmds = sorted(c for c in _cmds(p["step_chain"]) if c in ("a", "b"))
    assert cmds == ["a", "b"]
    assert len(p["step_chain"]["leaf_indices"]) == 2


def test_pipeline_forwards_env_to_step_chain():
    @hm.pipeline("ci", env={"CI": "true"})
    def ci() -> hm.Step:
        return hm.scratch().sh("echo")

    out = json.loads(hm.dump_registry_json())
    step_chain = out["pipelines"][0]["step_chain"]
    # Pipeline-level env rides on the step chain; Rust layers it per node.
    assert step_chain["pipeline_env"] == {"CI": "true"}


def test_envelope_auto_unwraps_go_toolchain():
    """A pipeline returning a GoToolchain emits the build leaf."""

    @hm.pipeline("ci")
    def ci():
        return hm.go(path="api").build()

    out = json.loads(hm.dump_registry_json())
    cmds = _cmds(out["pipelines"][0]["step_chain"])
    assert any("go build" in (c or "") for c in cmds)


def test_envelope_composes_targets_with_dedup():
    """Two pipelines depending on the same target share the target step."""
    from harmont._target import clear_target_cache

    clear_target_cache()

    @hm.target()
    def apt_base() -> hm.Step:
        return hm.sh("apt-get update")

    @hm.pipeline("ci")
    def ci() -> tuple[hm.Step, ...]:
        return (
            apt_base().sh("cabal build"),
            apt_base().sh("pytest"),
        )

    out = json.loads(hm.dump_registry_json())
    step_chain = out["pipelines"][0]["step_chain"]
    steps = _steps(step_chain)
    apt_indices = [i for i, s in enumerate(steps) if s.get("cmd") == "apt-get update"]
    assert len(apt_indices) == 1  # deduplicated via target memoization
    children = _children_of(step_chain, apt_indices[0])
    child_cmds = sorted(c["cmd"] for c in children)
    assert child_cmds == ["cabal build", "pytest"]


def test_envelope_clears_target_cache_between_renders():
    """Two consecutive dump_registry_json calls must not share target state."""

    @hm.target()
    def apt_base() -> hm.Step:
        return hm.sh("apt-get update")

    @hm.pipeline("ci")
    def ci() -> hm.Step:
        return apt_base()

    hm.dump_registry_json()
    # After render, cache has one entry from the in-flight render. Trigger
    # a second render and verify the cache is cleared at render start
    # by re-running and confirming success.
    hm.dump_registry_json()


def test_envelope_wraps_typeerror_with_pipeline_slug():
    """Bad return from pipeline fn surfaces as TypeError naming the slug."""

    @hm.pipeline("broken")
    def broken():
        return 42  # not a Step / tuple / toolchain wrapper

    with pytest.raises(TypeError, match=r"pipeline 'broken': invalid return value"):
        hm.dump_registry_json()


def test_decorator_pipeline_timeout_in_envelope():
    REGISTRATIONS.clear()

    @hm.pipeline("timed", timeout="20m")
    def _timed() -> hm.Step:
        return hm.sh("make test")

    env = json.loads(hm.dump_registry_json())
    step_chain = env["pipelines"][0]["step_chain"]
    assert step_chain["pipeline_timeout_seconds"] == 1200
    REGISTRATIONS.clear()
