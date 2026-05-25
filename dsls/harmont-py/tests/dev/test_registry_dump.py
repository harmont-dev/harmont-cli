"""dump_registry_json — golden JSON shape for canonical examples."""

from __future__ import annotations

import json
from pathlib import Path

import harmont as hm
from harmont._deploy import Deployment
from harmont.dev import dump_registry_json


def test_dump_minimal_local_deployment():
    @hm.deploy("db")
    def db():
        return hm.dev.deploy(
            image="postgres:16",
            port_mapping={5432: hm.dev.port()},
            env={"POSTGRES_PASSWORD": "dev"},
        )

    out = json.loads(dump_registry_json(worktree_root=Path("/tmp/wt")))  # noqa: S108
    assert out["schema_version"] == "0"
    assert out["worktree"] == "/tmp/wt"  # noqa: S108
    assert out["deployments"]["db"] == {
        "driver": "local",
        "image": "postgres:16",
        "from": None,
        "cmd": None,
        "port_mapping": {"5432": "__hm_dev_port__"},
        "env": {"POSTGRES_PASSWORD": "dev"},
        "volumes": {},
        "workdir": None,
        "deps": [],
    }


def test_dump_with_cmd_workdir_volumes():
    @hm.deploy("db")
    def db():
        return hm.dev.deploy(
            image="postgres:16",
            cmd=["postgres", "-c", "shared_buffers=128MB"],
            port_mapping={5432: hm.dev.port()},
            volumes={".": "/workspace"},
            workdir="/workspace",
        )

    out = json.loads(dump_registry_json(worktree_root=Path("/tmp/wt")))  # noqa: S108
    e = out["deployments"]["db"]
    assert e["cmd"] == ["postgres", "-c", "shared_buffers=128MB"]
    assert e["workdir"] == "/workspace"
    assert e["volumes"] == {".": "/workspace"}


def test_dump_with_deps_emits_deps_array_in_param_order():
    @hm.deploy("db")
    def db():
        return hm.dev.deploy(image="postgres:16", port_mapping={5432: hm.dev.port()})

    @hm.deploy("api")
    def api(db: hm.Dep[hm.Deployment]):
        return hm.dev.deploy(
            image="x",
            port_mapping={8000: hm.dev.port()},
            env={"DB_HOST": db.name},
        )

    out = json.loads(dump_registry_json(worktree_root=Path("/tmp/wt")))  # noqa: S108
    assert out["deployments"]["api"]["deps"] == ["db"]
    assert out["deployments"]["api"]["env"] == {"DB_HOST": "db"}


def test_dump_step_chain_emits_pipeline_v0_ir():
    @hm.deploy("api")
    def api():
        return hm.dev.deploy(
            from_=hm.sh("echo build", image="alpine:3.20"),
            port_mapping={8000: hm.dev.port()},
        )

    out = json.loads(dump_registry_json(worktree_root=Path("/tmp/wt")))  # noqa: S108
    f = out["deployments"]["api"]["from"]
    assert f["type"] == "step_chain"
    assert f["pipeline_v0"]["version"] == "0"
    assert f["pipeline_v0"]["graph"]["nodes"][0]["step"]["cmd"] == "echo build"


def test_dump_non_local_driver_is_marked_unhandled():
    @hm.deploy("prod-api")
    def prod_api():
        return Deployment(name="", driver="aws")

    out = json.loads(dump_registry_json(worktree_root=Path("/tmp/wt")))  # noqa: S108
    assert out["deployments"]["prod-api"] == {"driver": "aws", "_unhandled": True}
