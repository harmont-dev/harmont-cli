"""dep_graph extraction + topo_order on the deployment registry."""
from __future__ import annotations

import pytest

import harmont as hm
from harmont._deploy import dep_graph, topo_order


def test_dep_graph_empty_when_no_deps():
    @hm.deploy("db")
    def db():
        return hm.dev.deploy(image="postgres:16", port_mapping={5432: hm.dev.port()})

    g = dep_graph()
    assert g == {"db": ()}


def test_dep_graph_lists_param_names_in_order():
    @hm.deploy("db")
    def db():
        return hm.dev.deploy(image="postgres:16", port_mapping={5432: hm.dev.port()})

    @hm.deploy("api")
    def api(db: hm.Dep[hm.Deployment]):
        return hm.dev.deploy(image="x", port_mapping={8000: hm.dev.port()},
                             env={"DB": db.name})

    g = dep_graph()
    assert g == {"db": (), "api": ("db",)}


def test_topo_order_is_stable_and_deps_first():
    @hm.deploy("db")
    def db():
        return hm.dev.deploy(image="postgres:16", port_mapping={5432: hm.dev.port()})

    @hm.deploy("api")
    def api(db: hm.Dep[hm.Deployment]):
        return hm.dev.deploy(image="x", port_mapping={8000: hm.dev.port()})

    @hm.deploy("web")
    def web(api: hm.Dep[hm.Deployment]):
        return hm.dev.deploy(image="x", port_mapping={3000: hm.dev.port()})

    order = topo_order()
    # db before api before web
    assert order.index("db") < order.index("api") < order.index("web")


def test_topo_order_raises_on_cycle():
    from harmont._deploy import Deployment

    @hm.deploy("a")
    def a(b: hm.Dep[hm.Deployment]):
        return Deployment(name="", driver="local")

    @hm.deploy("b")
    def b(a: hm.Dep[hm.Deployment]):
        return Deployment(name="", driver="local")

    with pytest.raises(RuntimeError, match="dep cycle"):
        topo_order()
