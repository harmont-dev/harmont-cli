"""hm.dev.port() — the OS-assigned-host-port sentinel.

The sentinel is only meaningful as a value in
``hm.dev.deploy(..., port_mapping={CONTAINER_PORT: hm.dev.port()})``.
Any other position (env value, cmd arg, …) is rejected at the call
site that consumes it, with a fix-directed message per PRINCIPLES § 5.
"""

from __future__ import annotations


class _PortSentinel:
    __slots__ = ()

    def __repr__(self) -> str:
        return "<hm.dev.port>"

    def __eq__(self, other: object) -> bool:
        return isinstance(other, _PortSentinel)

    def __hash__(self) -> int:
        return hash(_PortSentinel)


_SINGLETON = _PortSentinel()


def port() -> _PortSentinel:
    """Return the sentinel for an OS-assigned host port.

    Use only as a ``port_mapping`` value:

        hm.dev.deploy(
            image="postgres:16",
            port_mapping={5432: hm.dev.port()},
        )
    """
    return _SINGLETON
