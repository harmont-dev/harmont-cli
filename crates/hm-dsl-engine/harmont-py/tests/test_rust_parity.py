"""Golden cargo strings — must stay byte-identical to the TS parity test."""

from __future__ import annotations

import harmont as hm


def _tail(cmd: str) -> str:
    # strip the ". $HOME/.cargo/env && cd . && " prefix for comparison
    marker = "cd . && "
    return cmd[cmd.index(marker) + len(marker) :]


def test_golden_commands():
    p = hm.rust.project(path=".")
    assert _tail(p.test(features=("a", "b"), nextest=True).cmd) == (
        "cargo nextest run --workspace --features a,b --locked"
    )
    assert _tail(p.clippy(all_features=True).cmd) == (
        "cargo clippy --workspace --all-targets --all-features --locked -- -D warnings"
    )
    assert _tail(p.fmt().cmd) == "cargo fmt --all --check"
    assert _tail(p.doc(document_private_items=True).cmd) == (
        "cargo doc --no-deps --document-private-items --workspace --locked"
    )
    assert _tail(p.build(packages=("core",), target="wasm32-unknown-unknown").cmd) == (
        "cargo build -p core --target wasm32-unknown-unknown --locked"
    )
    assert (
        _tail(p.feature_powerset(subcommand="check", skip=("a b", "c")).cmd)
        == "cargo hack check --feature-powerset --depth 2 --no-dev-deps --skip 'a b',c"
    )
