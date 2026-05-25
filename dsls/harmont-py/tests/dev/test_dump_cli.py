"""`python -m harmont.dev --dump-registry` integration."""

from __future__ import annotations

import json
import subprocess
import sys
import textwrap
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pathlib import Path


def test_dump_cli_walks_harmont_dir_and_prints_registry(tmp_path: Path):
    pkg = tmp_path / ".harmont"
    pkg.mkdir()
    (pkg / "deploys.py").write_text(
        textwrap.dedent("""
        import harmont as hm

        @hm.deploy("db")
        def db():
            return hm.dev.deploy(
                image="postgres:16",
                port_mapping={5432: hm.dev.port()},
                env={"POSTGRES_PASSWORD": "dev"},
            )
    """)
    )
    result = subprocess.run(
        [sys.executable, "-m", "harmont.dev", "--dump-registry"],
        cwd=tmp_path,
        capture_output=True,
        text=True,
        check=True,
    )
    out = json.loads(result.stdout)
    assert out["schema_version"] == "0"
    assert out["worktree"] == str(tmp_path)
    assert "db" in out["deployments"]
    assert out["deployments"]["db"]["image"] == "postgres:16"


def test_dump_cli_errors_when_no_harmont_dir(tmp_path: Path):
    result = subprocess.run(
        [sys.executable, "-m", "harmont.dev", "--dump-registry"],
        cwd=tmp_path,
        capture_output=True,
        text=True,
    )
    assert result.returncode != 0
    assert "no .harmont/ directory" in result.stderr


def test_dump_cli_errors_on_bad_argument(tmp_path: Path):
    result = subprocess.run(
        [sys.executable, "-m", "harmont.dev", "--no-such-flag"],
        cwd=tmp_path,
        capture_output=True,
        text=True,
    )
    assert result.returncode == 2  # argparse default
