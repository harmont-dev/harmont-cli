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


def _graph_nodes(definition):
    return definition["graph"]["nodes"]


def _graph_edges(definition):
    return definition["graph"]["edges"]


def _step_cmds(definition):
    return [n["step"].get("cmd") for n in _graph_nodes(definition)]


def _builds_in_children(definition, parent_key):
    """Return nodes whose builds_in parent is parent_key."""
    nodes = _graph_nodes(definition)
    parent_idx = None
    for i, n in enumerate(nodes):
        if n["step"]["key"] == parent_key:
            parent_idx = i
            break
    if parent_idx is None:
        return []
    children = []
    for src, dst, kind in _graph_edges(definition):
        if kind == "builds_in" and src == parent_idx:
            children.append(nodes[dst])
    return children


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
    definition = p["definition"]
    assert definition["version"] == "0"
    nodes = _graph_nodes(definition)
    assert len(nodes) == 1
    assert nodes[0]["step"]["cmd"] == "echo hi"
    assert nodes[0]["step"]["label"] == "hi"


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
    cmds = sorted(n["step"]["cmd"] for n in _graph_nodes(p["definition"]))
    assert cmds == ["a", "b"]


def test_pipeline_forwards_env_to_assemble():
    @hm.pipeline("ci", env={"CI": "true"})
    def ci() -> hm.Step:
        return hm.scratch().sh("echo")

    out = json.loads(hm.dump_registry_json())
    definition = out["pipelines"][0]["definition"]
    # Pipeline-level env is merged into node env dicts.
    for node in _graph_nodes(definition):
        assert node["env"].get("CI") == "true"


def test_envelope_resolves_cache_keys(tmp_path):
    @hm.pipeline("ci")
    def ci() -> hm.Step:
        return hm.scratch().sh("echo", label="run", cache=hm.forever())

    out = json.loads(
        hm.dump_registry_json(
            pipeline_org="acme",
            now=1700000000,
            base_path=tmp_path,
            env={},
        )
    )
    step = _graph_nodes(out["pipelines"][0]["definition"])[0]["step"]
    assert step["cache"]["policy"] == "forever"
    assert "key" in step["cache"]
    assert len(step["cache"]["key"]) == 64


def test_envelope_auto_unwraps_go_toolchain():
    """A pipeline returning a GoToolchain emits the build leaf."""

    @hm.pipeline("ci")
    def ci():
        return hm.go(path="api").build()

    out = json.loads(hm.dump_registry_json())
    nodes = _graph_nodes(out["pipelines"][0]["definition"])
    cmds = [n["step"].get("cmd") for n in nodes]
    assert any("go build" in (c or "") for c in cmds)


def test_envelope_composes_targets_with_dedup(tmp_path, monkeypatch):
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
    definition = out["pipelines"][0]["definition"]
    nodes = _graph_nodes(definition)
    apt_nodes = [n for n in nodes if n["step"].get("cmd") == "apt-get update"]
    assert len(apt_nodes) == 1  # deduplicated via target memoization
    children = _builds_in_children(definition, apt_nodes[0]["step"]["key"])
    assert len(children) == 2
    child_cmds = sorted(n["step"]["cmd"] for n in children)
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
    # by re-running and confirming success (would TypeError otherwise if
    # the first render's cached Step somehow propagated through dataclass
    # frozen-equality into the second render's IR).
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

    env = json.loads(hm.dump_registry_json(now=0))
    defn = env["pipelines"][0]["definition"]
    assert defn["timeout_seconds"] == 1200
    REGISTRATIONS.clear()
