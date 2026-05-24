"""hm.dev.deploy(...) field validation + LocalDeployment construction."""

from __future__ import annotations

import pytest

from harmont._step import scratch
from harmont.dev import LocalDeployment, deploy, port


def test_deploy_with_raw_image_returns_local_deployment():
    d = deploy(
        image="postgres:16",
        port_mapping={5432: port()},
        env={"POSTGRES_PASSWORD": "dev"},
    )
    assert isinstance(d, LocalDeployment)
    assert d.image == "postgres:16"
    assert d.from_step is None
    # name is set later by the @hm.deploy decorator; factory leaves it ""
    assert d.name == ""


def test_deploy_with_from_step():
    s = scratch().sh("echo build", image="alpine:3.20")
    d = deploy(from_=s, port_mapping={8000: port()})
    assert d.image is None
    assert d.from_step is s


def test_deploy_requires_exactly_one_of_image_or_from():
    with pytest.raises(ValueError, match="exactly one of `image=` or `from_=`"):
        deploy(port_mapping={5432: port()})
    with pytest.raises(ValueError, match="exactly one of `image=` or `from_=`"):
        deploy(image="x", from_=scratch().sh("echo"), port_mapping={5432: port()})


def test_port_mapping_keys_must_be_valid_container_ports():
    with pytest.raises(ValueError, match="port_mapping key must be int in"):
        deploy(image="x", port_mapping={0: port()})
    with pytest.raises(ValueError, match="port_mapping key must be int in"):
        deploy(image="x", port_mapping={70000: port()})
    with pytest.raises(ValueError, match="port_mapping key must be int in"):
        deploy(image="x", port_mapping={"5432": port()})  # type: ignore[dict-item]


def test_port_mapping_values_must_be_hm_dev_port():
    with pytest.raises(TypeError, match=r"port_mapping value must be hm\.dev\.port"):
        deploy(image="x", port_mapping={5432: 31337})  # type: ignore[dict-item]


def test_env_values_must_be_strings():
    with pytest.raises(TypeError, match="env value for 'PORT' must be str"):
        deploy(image="x", port_mapping={5432: port()}, env={"PORT": 31337})  # type: ignore[dict-item]


def test_cmd_coerces_to_tuple_of_strings():
    d = deploy(
        image="x", port_mapping={5432: port()}, cmd=["postgres", "-c", "shared_buffers=128MB"]
    )
    assert d.cmd == ("postgres", "-c", "shared_buffers=128MB")


def test_cmd_rejects_non_string_elements():
    with pytest.raises(TypeError, match="cmd elements must be str"):
        deploy(image="x", port_mapping={5432: port()}, cmd=["postgres", 5432])  # type: ignore[list-item]


def test_volumes_preserves_host_path_verbatim():
    # The factory keeps host paths verbatim; resolution to absolute
    # worktree paths happens in _registry_dump.py.
    d = deploy(image="x", port_mapping={5432: port()}, volumes={".": "/workspace"})
    assert dict(d.volumes) == {".": "/workspace"}


def test_workdir_must_be_absolute():
    with pytest.raises(ValueError, match="workdir must be an absolute path"):
        deploy(image="x", port_mapping={5432: port()}, workdir="workspace")
