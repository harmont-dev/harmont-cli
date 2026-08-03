"""Mount action tests — Step.mount() and hm.mount()."""

from __future__ import annotations

import pytest

import harmont as hm
from harmont._step import Mount, scratch
from harmont.cache import CacheNone


def test_mount_creates_mount_action():
    s = scratch().mount(from_=".cache/data", to="./_hm_mount_1", strict=False)
    assert isinstance(s.action, Mount)
    assert s.action.from_ == ".cache/data"
    assert s.action.to == "./_hm_mount_1"
    assert not s.is_wait


def test_mount_after_sh_chains_properly():
    root = scratch().sh("npm ci")
    s = root.mount(from_=".cache/npm", to="./_hm_mount_2", strict=False)
    assert s.parent is root
    assert isinstance(s.action, Mount)


def test_mount_root_has_scratch_parent():
    root = scratch()
    s = root.mount(from_=".cache/go", to="./_hm_mount_3", strict=False)
    assert s.parent is root
    assert s.parent.action is None


def test_mount_carries_kwargs():
    s = scratch().mount(
        from_=".cache/gems",
        to="./_hm_mount_4",
        label="ruby gems",
        cache=CacheNone(),
        runner="krun",
        runner_args={"opt": 1},
        key="gem-mount",
        strict=False,
    )
    assert s.label == "ruby gems"
    assert s.cache == CacheNone()
    assert s.runner == "krun"
    assert s.runner_args == {"opt": 1}
    assert s.key_override == "gem-mount"


def test_mount_inherits_image_from_scratch():
    from harmont._step import Step

    root = Step(image="ubuntu-24.04")
    s = root.mount(from_=".cache/data", to="./_hm_mount_5", strict=False)
    assert s.image == "ubuntu-24.04"


def test_mount_explicit_image_wins():
    s = scratch().mount(from_=".cache/data", to="./_hm_mount_6", image="alpine:3.20", strict=False)
    assert s.image == "alpine:3.20"


def test_mount_does_not_inherit_image_from_sh_child():
    from harmont._step import Step

    root = Step(image="ubuntu-24.04")
    s = root.sh("echo a").mount(from_=".cache/data", to="./_hm_mount_7", strict=False)
    assert s.image is None


def test_mount_absolute_path_raises():
    with pytest.raises(ValueError, match="relative to the workspace"):
        scratch().mount(from_="/cache", to="./_hm_outside", strict=False)


def test_mount_source_path_raises_when_strict_and_doesnt_exists():
    with pytest.raises(ValueError, match="Source path does not exists"):
        scratch().mount(from_="./cache", to="./_hm_outside")


def test_mount_to_existing_file_raises(tmp_path):
    import os

    sf = tmp_path / "mount_target.txt"
    sf.write_text("data")
    to = str(sf.name)

    df = tmp_path / "mount_source.txt"
    df.write_text("data")
    from_ = str(df.name)

    old = os.getcwd()
    try:
        os.chdir(tmp_path)
        with pytest.raises(ValueError, match="should be a directory"):
            scratch().mount(from_=from_, to=to)
    finally:
        os.chdir(old)


def test_mount_outside_workspace_raises():
    with pytest.raises(ValueError, match="should be inside the workspace"):
        scratch().mount(from_=".cache/data", to="../_hm_outside", strict=False)


def test_hm_mount_convenience():
    s = hm.mount(from_=".cache/pip", to="./_hm_mount_9", strict=False)
    assert isinstance(s.action, Mount)
    assert s.action.from_ == ".cache/pip"
    assert s.action.to == "./_hm_mount_9"
    assert isinstance(s.cache, CacheNone)
    assert s.image is None
    assert s.parent is not None
    assert s.parent.action is None


def test_mount_lowers_to_json_with_correct_action_shape():
    """Mount step renders to IR JSON with the action dict containing from/to."""
    import json
    from pathlib import Path

    from harmont.json_emit import pipeline_to_json

    mount_dir = ".cache/data"
    dest_dir = "./_hm_mount_json"
    p = hm.pipeline([scratch().mount(from_=mount_dir, to=dest_dir, strict=False)])
    out = json.loads(pipeline_to_json(p, now=0, base_path=Path(), env={}))
    nodes = out["graph"]["nodes"]
    assert len(nodes) == 1
    step = nodes[0]["step"]
    assert step["action"]["from"] == mount_dir
    assert step["action"]["to"] == dest_dir
    # Step should have an image stamped (root mount step gets the default).
    assert "image" in step


def test_mount_and_sh_chain_in_pipeline():
    """Mount followed by a command step in the same chain."""
    import json
    from pathlib import Path

    from harmont.json_emit import pipeline_to_json

    s = (
        scratch()
        .mount(from_=".cache/npm", to="./_hm_mount_chain", strict=False)
        .sh("npm ci", label="install")
    )
    p = hm.pipeline([s])
    out = json.loads(pipeline_to_json(p, now=0, base_path=Path(), env={}))
    nodes = out["graph"]["nodes"]
    # Both the mount and the command step should be present.
    {n["step"]["key"] for n in nodes}
    # Find which node is the mount and which is the sh
    mount_nodes = [n for n in nodes if "from" in n["step"].get("action", {})]
    sh_nodes = [n for n in nodes if "cmd" in n["step"].get("action", {})]
    assert len(mount_nodes) == 1
    assert len(sh_nodes) == 1
    assert mount_nodes[0]["step"]["action"]["from"] == ".cache/npm"
    assert mount_nodes[0]["step"]["action"]["to"] == "./_hm_mount_chain"
    # Builds_in edge should connect mount -> sh
    edges = out["graph"]["edges"]
    idx_mount = nodes.index(mount_nodes[0])
    idx_sh = nodes.index(sh_nodes[0])
    assert [idx_mount, idx_sh, "builds_in"] in edges


def test_mount_pipeline_parallel_branches():
    """Two independent mount chains in the same pipeline."""
    import json
    from pathlib import Path

    from harmont.json_emit import pipeline_to_json

    a = scratch().mount(from_=".cache/go", to="./_hm_go", strict=False)
    b = scratch().mount(from_=".cache/npm", to="./_hm_npm", strict=False)
    p = hm.pipeline([a, b])
    out = json.loads(pipeline_to_json(p, now=0, base_path=Path(), env={}))
    nodes = out["graph"]["nodes"]
    assert len(nodes) == 2
    tos = {n["step"]["action"]["to"] for n in nodes}
    assert tos == {"./_hm_go", "./_hm_npm"}


def test_mount_absolute_path_both_sides_rejected():
    """Absolute source or destination path raises ValueError."""
    with pytest.raises(ValueError, match="relative to the workspace"):
        scratch().mount(from_=".cache/data", to="/etc/passwd", strict=False)
    with pytest.raises(ValueError, match="relative to the workspace"):
        scratch().mount(from_="/var/cache", to="./_hm_out", strict=False)


def test_mount_inside_workspace_valid_path(tmp_path):
    """Mount to a path that exists inside the workspace succeeds."""
    import os

    old = os.getcwd()
    try:
        os.chdir(tmp_path)
        os.makedirs("./subdir/mount_target")
        os.makedirs(".cache/data")
        s = scratch().mount(from_=".cache/data", to="subdir/mount_target")
        assert isinstance(s.action, Mount)
        assert s.action.to == "subdir/mount_target"
    finally:
        os.chdir(old)
