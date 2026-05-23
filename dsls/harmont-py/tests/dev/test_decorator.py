"""@hm.deploy decorator: registration, slug derivation, fixture injection."""
from __future__ import annotations

import pytest

import harmont as hm
from harmont._deploy import DEPLOYMENTS
from harmont.dev import LocalDeployment


def test_deploy_registers_under_explicit_slug():
    @hm.deploy("db")
    def db():
        return hm.dev.deploy(image="postgres:16", port_mapping={5432: hm.dev.port()})

    assert "db" in DEPLOYMENTS
    resolved = DEPLOYMENTS["db"]()
    assert isinstance(resolved, LocalDeployment)
    assert resolved.name == "db"           # decorator stamped slug in
    assert resolved.image == "postgres:16"


def test_deploy_uses_function_name_when_slug_omitted():
    @hm.deploy()
    def redis():
        return hm.dev.deploy(image="redis:7", port_mapping={6379: hm.dev.port()})

    assert "redis" in DEPLOYMENTS


def test_deploy_rejects_invalid_slug():
    with pytest.raises(ValueError, match="invalid deployment slug"):
        @hm.deploy("Bad Slug")
        def x():
            return hm.dev.deploy(image="x", port_mapping={5432: hm.dev.port()})


def test_deploy_rejects_duplicate_slug():
    @hm.deploy("db")
    def db1():
        return hm.dev.deploy(image="postgres:16", port_mapping={5432: hm.dev.port()})

    with pytest.raises(ValueError, match="duplicate deployment slug"):
        @hm.deploy("db")
        def db2():
            return hm.dev.deploy(image="postgres:15", port_mapping={5432: hm.dev.port()})


def test_deploy_requires_marker_on_param():
    # validate_target_signature (the shared validator used by @hm.target,
    # @hm.pipeline, and @hm.deploy) raises TypeError for unmarkered params.
    with pytest.raises(TypeError, match=r"parameter 'db' has no marker"):
        @hm.deploy("api")
        def api(db):  # type: ignore[no-untyped-def]
            return hm.dev.deploy(image="x", port_mapping={8000: hm.dev.port()})


def test_deploy_injects_dep_value():
    @hm.deploy("db")
    def db():
        return hm.dev.deploy(image="postgres:16", port_mapping={5432: hm.dev.port()})

    @hm.deploy("api")
    def api(db: hm.Dep[hm.Deployment]):
        # db.name comes from the resolved upstream Deployment
        return hm.dev.deploy(
            image="x",
            port_mapping={8000: hm.dev.port()},
            env={"DB_HOST": db.name},
        )

    resolved = DEPLOYMENTS["api"]()
    assert resolved.env["DB_HOST"] == "db"


def test_deploy_with_explicit_name_arg():
    @hm.deploy("db", name="primary-db")
    def db():
        return hm.dev.deploy(image="postgres:16", port_mapping={5432: hm.dev.port()})

    # The display name is held alongside the slug; the registry is keyed by slug.
    assert "db" in DEPLOYMENTS
    # In v1 we don't expose `name` separately on the returned Deployment;
    # the slug IS the public identity. The kwarg is reserved for future use.


def test_deploy_function_can_return_remote_driver_value():
    # Simulate a future driver: a function that returns a Deployment with
    # driver != "local". The decorator must register it without complaint.
    from harmont._deploy import Deployment

    @hm.deploy("prod-api")
    def prod_api():
        return Deployment(name="", driver="aws")

    resolved = DEPLOYMENTS["prod-api"]()
    assert resolved.driver == "aws"
    assert resolved.name == "prod-api"
