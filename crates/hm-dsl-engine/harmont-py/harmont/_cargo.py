"""Shared cargo-argument assembly for the Rust toolchain helper.

Turns structured options (features, packages, target, …) into a
canonically-ordered, shell-safe cargo argument string. Every user-supplied
*value* (package/exclude names, the joined feature list, target triple,
profile name) is shell-quoted; the raw escape-hatch ``flags`` pass through
verbatim because they are meant to be literal cargo arguments.
"""

from __future__ import annotations

import shlex
from dataclasses import dataclass


@dataclass(frozen=True)
class CargoOpts:
    """Structured cargo invocation options. See the SDK design reference for
    the canonical token order each field lowers to."""

    workspace: bool = False
    packages: tuple[str, ...] = ()
    exclude: tuple[str, ...] = ()
    all_features: bool = False
    no_default_features: bool = False
    features: tuple[str, ...] = ()
    target: str | None = None
    all_targets: bool = False
    release: bool = False
    profile: str | None = None
    locked: bool = True
    flags: tuple[str, ...] = ()


def _validate(opts: CargoOpts) -> None:
    if opts.all_features and (opts.features or opts.no_default_features):
        msg = (
            "hm.rust: --all-features conflicts with features=/no_default_features=\n"
            f"  observed: all_features=True, features={list(opts.features)!r}, "
            f"no_default_features={opts.no_default_features!r}\n"
            "  → pass all_features=True alone, or list explicit features= "
            "without all_features"
        )
        raise ValueError(msg)
    if opts.release and opts.profile is not None:
        msg = (
            "hm.rust: release=True conflicts with profile=\n"
            f"  observed: release=True, profile={opts.profile!r}\n"
            '  → use profile="release" (identical effect) or drop one'
        )
        raise ValueError(msg)
    if opts.exclude:
        if opts.packages:
            msg = (
                "hm.rust: exclude= cannot combine with packages=\n"
                f"  observed: packages={list(opts.packages)!r}, "
                f"exclude={list(opts.exclude)!r}\n"
                "  → --exclude pairs with --workspace; packages= already selects "
                "explicitly, so drop one"
            )
            raise ValueError(msg)
        if not opts.workspace:
            msg = (
                "hm.rust: exclude= requires workspace=True\n"
                f"  observed: exclude={list(opts.exclude)!r} without workspace=True\n"
                "  → cargo --exclude only applies to --workspace; pass workspace=True"
            )
            raise ValueError(msg)


def cargo_flags(opts: CargoOpts) -> str:
    """Assemble the cargo argument middle (after the subcommand, before any
    ``--`` passthrough). Returns a leading-space string, or ``""`` when empty.
    """
    _validate(opts)
    toks: list[str] = []

    # scope
    if opts.packages:
        toks += [f"-p {shlex.quote(p)}" for p in opts.packages]
    elif opts.workspace:
        toks.append("--workspace")
        toks += [f"--exclude {shlex.quote(e)}" for e in opts.exclude]

    # target selection
    if opts.all_targets:
        toks.append("--all-targets")

    # features
    if opts.all_features:
        toks.append("--all-features")
    else:
        if opts.no_default_features:
            toks.append("--no-default-features")
        if opts.features:
            toks.append("--features " + shlex.quote(",".join(opts.features)))

    # target triple
    if opts.target is not None:
        toks.append(f"--target {shlex.quote(opts.target)}")

    # profile / release
    if opts.profile is not None:
        toks.append(f"--profile {shlex.quote(opts.profile)}")
    elif opts.release:
        toks.append("--release")

    # lockfile
    if opts.locked:
        toks.append("--locked")

    # escape hatch — verbatim
    toks += list(opts.flags)

    return (" " + " ".join(toks)) if toks else ""
