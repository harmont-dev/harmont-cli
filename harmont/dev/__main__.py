"""`python -m harmont.dev` — registry-dump entry point for the CLI.

Walks ``.harmont/*.py`` (importing each by file path), letting
``@hm.deploy``-decorated functions register themselves into
``harmont._deploy.DEPLOYMENTS`` as a side effect. Then emits the
deployment registry JSON to stdout.

Errors go to stderr with exit code 1 (DSL error) or 2 (argparse
usage error), matching ``harmont``'s convention.
"""
from __future__ import annotations

import argparse
import importlib.util
import sys
from pathlib import Path


def _import_path(path: Path) -> None:
    spec = importlib.util.spec_from_file_location(
        name=f"_harmont_dev_user_{path.stem}",
        location=str(path),
    )
    if spec is None or spec.loader is None:
        msg = f"cannot load module from {path}"
        raise RuntimeError(msg)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)


def _walk_harmont_dir(root: Path) -> None:
    harmont_dir = root / ".harmont"
    if not harmont_dir.is_dir():
        sys.stderr.write(
            f"hm: no .harmont/ directory in {root}\n"
            "  → create .harmont/ and add @hm.deploy-decorated functions\n"
        )
        sys.exit(1)
    for py in sorted(harmont_dir.glob("*.py")):
        _import_path(py)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="python -m harmont.dev")
    parser.add_argument(
        "--dump-registry",
        action="store_true",
        help="walk .harmont/*.py and emit the v0 deployment registry JSON",
    )
    parser.add_argument(
        "--worktree-root",
        type=Path,
        default=None,
        help="path to the worktree root; defaults to cwd",
    )
    args = parser.parse_args(argv)

    if not args.dump_registry:
        # parser.error() is NoReturn (calls sys.exit(2)); execution stops here.
        parser.error("nothing to do; pass --dump-registry")

    from harmont.dev import dump_registry_json

    root = args.worktree_root if args.worktree_root is not None else Path.cwd()
    _walk_harmont_dir(root)
    sys.stdout.write(dump_registry_json(worktree_root=root) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
