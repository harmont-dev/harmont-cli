"""Abstract Deployment + LocalDeployment construction tests."""

from __future__ import annotations

from collections.abc import Mapping

import pytest

from harmont._deploy import Deployment
from harmont._step import scratch
from harmont.dev import port
from harmont.dev._deployment import LocalDeployment
from harmont.dev._port import _PortSentinel


def test_deployment_is_abstract_dataclass():
    """Deployment carries name + driver, is frozen, and is constructible (sentinel-level)."""
    d = Deployment(name="db", driver="local")
    assert d.name == "db"
    assert d.driver == "local"
    with pytest.raises(AttributeError):
        d.name = "other"  # type: ignore[misc]  # frozen


# ---------------------------------------------------------------------------
# Task 3: LocalDeployment tests
# ---------------------------------------------------------------------------


def test_local_deployment_is_a_deployment_with_driver_local():
    d = LocalDeployment(
        name="db",
        driver="local",
        image="postgres:16",
        from_step=None,
        cmd=None,
        port_mapping={5432: port()},
        env={},
        volumes={},
        workdir=None,
    )
    assert isinstance(d, Deployment)
    assert d.driver == "local"
    assert d.image == "postgres:16"


def test_local_deployment_rejects_non_local_driver():
    with pytest.raises(ValueError, match="driver must be 'local'"):
        LocalDeployment(
            name="db",
            driver="aws",
            image="postgres:16",
            from_step=None,
            cmd=None,
            port_mapping={5432: port()},
            env={},
            volumes={},
            workdir=None,
        )


def test_local_deployment_holds_step_chain():
    s = scratch().sh("echo hi", image="alpine:3.20")
    d = LocalDeployment(
        name="api",
        driver="local",
        image=None,
        from_step=s,
        cmd=None,
        port_mapping={8000: port()},
        env={},
        volumes={},
        workdir=None,
    )
    assert d.from_step is s
    assert d.image is None


def test_port_mapping_is_a_mapping_of_int_to_port_sentinel():
    d = LocalDeployment(
        name="db",
        driver="local",
        image="postgres:16",
        from_step=None,
        cmd=None,
        port_mapping={5432: port()},
        env={},
        volumes={},
        workdir=None,
    )
    assert isinstance(d.port_mapping, Mapping)
    [(cport, sentinel)] = d.port_mapping.items()
    assert cport == 5432
    assert isinstance(sentinel, _PortSentinel)
