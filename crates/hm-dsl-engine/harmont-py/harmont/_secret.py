"""Secret references for the Harmont pipeline SDK.

`hm.secrets["NAME"]` returns a SecretRef — a *reference* to a secret, never its
value. The value is resolved at run time (locally from .env + the process
environment; in the cloud from the org/pipeline secret store). Use it anywhere
an env value is accepted: `hm.sh("deploy", env={"TOKEN": hm.secrets["DEPLOY_TOKEN"]})`.
"""

from __future__ import annotations

import re
from dataclasses import dataclass

_SECRET_NAME = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")


@dataclass(frozen=True)
class SecretRef:
    """A reference to a stored secret, identified by name."""

    name: str

    def __repr__(self) -> str:  # never leaks a value — there is none to leak
        return f"SecretRef({self.name!r})"


class _Secrets:
    """Subscript accessor exposed as `hm.secrets`."""

    def __getitem__(self, name: str) -> SecretRef:
        if not isinstance(name, str) or not _SECRET_NAME.match(name):
            msg = (
                f"secret name {name!r} is invalid: names must match "
                r"[A-Za-z_][A-Za-z0-9_]* (letters, digits, underscores; no leading digit)."
            )
            raise ValueError(msg)
        return SecretRef(name)


secrets = _Secrets()
