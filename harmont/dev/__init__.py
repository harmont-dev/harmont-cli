"""harmont.dev — local Docker deployment driver.

Public surface:

    deploy(*, image=None, from_=None, cmd=None,
           port_mapping=None, env=None,
           volumes=None, workdir=None) -> LocalDeployment
    port()                                  -> _PortSentinel
    LocalDeployment                          (concrete subclass)
    dump_registry_json(*, worktree_root)    -> str
"""
from __future__ import annotations

from ._deployment import LocalDeployment
from ._factory import deploy
from ._port import port
from ._registry_dump import dump_registry_json

__all__ = ["LocalDeployment", "deploy", "dump_registry_json", "port"]
