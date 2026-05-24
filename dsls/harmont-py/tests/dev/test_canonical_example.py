"""End-to-end test mirroring the spec's canonical hello+greeter example.

The deployments both use Python's stdlib `http.server` (no third-party
image dependency), which is the smallest practical "native language
facility" demonstration of an HTTP server in a harmont deployment.
"""

from __future__ import annotations

import json
from typing import TYPE_CHECKING

import harmont as hm

if TYPE_CHECKING:
    from pathlib import Path


def test_canonical_hello_greeter_dumps_expected_shape(tmp_path: Path) -> None:
    @hm.deploy("hello")
    def hello() -> hm.Deployment:
        return hm.dev.deploy(
            image="python:3.12-alpine",
            cmd=["python", "-m", "http.server", "5678"],
            port_mapping={5678: hm.dev.port()},
        )

    @hm.deploy("greeter")
    def greeter(hello: hm.Dep[hm.Deployment]) -> hm.Deployment:
        return hm.dev.deploy(
            image="python:3.12-alpine",
            cmd=["python", "-m", "http.server", "5678"],
            port_mapping={5678: hm.dev.port()},
            env={"HELLO_HOST": hello.name},
        )

    raw = hm.dev.dump_registry_json(worktree_root=tmp_path)
    out = json.loads(raw)
    assert out["schema_version"] == "0"
    assert list(out["deployments"].keys()) == ["hello", "greeter"]
    assert out["deployments"]["greeter"]["deps"] == ["hello"]
    assert out["deployments"]["hello"]["image"] == "python:3.12-alpine"
    assert out["deployments"]["hello"]["cmd"] == [
        "python",
        "-m",
        "http.server",
        "5678",
    ]
    assert out["deployments"]["greeter"]["env"] == {"HELLO_HOST": "hello"}
    assert out["deployments"]["hello"]["from"] is None
    assert out["deployments"]["greeter"]["from"] is None
