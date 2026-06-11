"""Cargo argument assembler tests."""

from __future__ import annotations

import pytest

from harmont._cargo import CargoOpts, cargo_flags


def test_empty_opts_emit_only_locked():
    assert cargo_flags(CargoOpts()) == " --locked"


def test_locked_can_be_disabled():
    assert cargo_flags(CargoOpts(locked=False)) == ""


def test_workspace_scope():
    assert cargo_flags(CargoOpts(workspace=True)) == " --workspace --locked"


def test_packages_take_precedence_over_workspace():
    out = cargo_flags(CargoOpts(workspace=True, packages=("a", "b")))
    assert out == " -p a -p b --locked"


def test_exclude_pairs_with_workspace():
    out = cargo_flags(CargoOpts(workspace=True, exclude=("b", "c")))
    assert out == " --workspace --exclude b --exclude c --locked"


def test_exclude_without_workspace_raises():
    with pytest.raises(ValueError, match="workspace"):
        cargo_flags(CargoOpts(exclude=("b",)))


def test_exclude_with_packages_raises():
    with pytest.raises(ValueError, match="exclude"):
        cargo_flags(CargoOpts(packages=("a",), exclude=("b",)))


def test_all_features():
    assert cargo_flags(CargoOpts(all_features=True)) == " --all-features --locked"


def test_features_joined_comma():
    out = cargo_flags(CargoOpts(features=("x", "y")))
    assert out == " --features x,y --locked"


def test_no_default_features_with_features():
    out = cargo_flags(CargoOpts(no_default_features=True, features=("x",)))
    assert out == " --no-default-features --features x --locked"


def test_target_and_release():
    out = cargo_flags(CargoOpts(target="wasm32-unknown-unknown", release=True))
    assert out == " --target wasm32-unknown-unknown --release --locked"


def test_profile_overrides_release_token_position():
    out = cargo_flags(CargoOpts(profile="ci"))
    assert out == " --profile ci --locked"


def test_all_targets():
    out = cargo_flags(CargoOpts(workspace=True, all_targets=True))
    assert out == " --workspace --all-targets --locked"


def test_flags_appended_verbatim_after_locked():
    out = cargo_flags(CargoOpts(workspace=True, flags=("--no-fail-fast",)))
    assert out == " --workspace --locked --no-fail-fast"


def test_full_token_order():
    out = cargo_flags(
        CargoOpts(
            packages=("core",),
            all_targets=True,
            no_default_features=True,
            features=("a", "b"),
            target="x86_64-unknown-linux-gnu",
            profile="ci",
            flags=("--keep-going",),
        )
    )
    assert out == (
        " -p core --all-targets --no-default-features --features a,b"
        " --target x86_64-unknown-linux-gnu --profile ci --locked --keep-going"
    )


def test_package_value_is_shell_quoted():
    out = cargo_flags(CargoOpts(packages=("evil; rm -rf /",)))
    assert out == " -p 'evil; rm -rf /' --locked"


def test_simple_identifier_not_quoted():
    assert cargo_flags(CargoOpts(packages=("harmont-core",))) == " -p harmont-core --locked"


def test_flags_are_not_quoted():
    out = cargo_flags(CargoOpts(locked=False, flags=("--features=a b",)))
    assert out == " --features=a b"


def test_all_features_conflict_raises():
    with pytest.raises(ValueError, match="all-features"):
        cargo_flags(CargoOpts(all_features=True, features=("x",)))


def test_release_profile_conflict_raises():
    with pytest.raises(ValueError, match="profile"):
        cargo_flags(CargoOpts(release=True, profile="ci"))
