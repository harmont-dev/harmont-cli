"""Tests for JS runtime/PM auto-detection."""

from __future__ import annotations

import json
import os
import tempfile

import pytest

from harmont._detect import DetectedToolchain, detect, detect_from_lockfiles, detect_from_package_json


class TestDetectFromPackageJson:
    def test_empty_object(self) -> None:
        assert detect_from_package_json({}) == DetectedToolchain()

    def test_engines_node(self) -> None:
        assert detect_from_package_json({"engines": {"node": ">=18"}}) == DetectedToolchain(runtime="node")

    def test_engines_bun(self) -> None:
        assert detect_from_package_json({"engines": {"bun": ">=1.0"}}) == DetectedToolchain(runtime="bun", pm="bun")

    def test_engines_deno(self) -> None:
        assert detect_from_package_json({"engines": {"deno": ">=2.0"}}) == DetectedToolchain(runtime="deno")

    def test_package_manager_pnpm(self) -> None:
        assert detect_from_package_json({"packageManager": "pnpm@8.15.4"}) == DetectedToolchain(pm="pnpm")

    def test_package_manager_bun(self) -> None:
        assert detect_from_package_json({"packageManager": "bun@1.1.0"}) == DetectedToolchain(pm="bun")

    def test_package_manager_npm(self) -> None:
        assert detect_from_package_json({"packageManager": "npm@10.2.4"}) == DetectedToolchain(pm="npm")

    def test_package_manager_yarn_classic(self) -> None:
        assert detect_from_package_json({"packageManager": "yarn@1.22.22"}) == DetectedToolchain(pm="yarn-classic")

    def test_package_manager_yarn_berry(self) -> None:
        assert detect_from_package_json({"packageManager": "yarn@4.0.0"}) == DetectedToolchain(pm="yarn-berry")

    def test_ignores_unknown_package_manager(self) -> None:
        assert detect_from_package_json({"packageManager": "unknown@1.0"}) == DetectedToolchain()

    def test_engines_bun_overrides_package_manager(self) -> None:
        result = detect_from_package_json({"engines": {"bun": ">=1.0"}, "packageManager": "pnpm@8"})
        assert result == DetectedToolchain(runtime="bun", pm="bun")

    def test_engines_node_plus_package_manager_pnpm(self) -> None:
        result = detect_from_package_json({"engines": {"node": ">=18"}, "packageManager": "pnpm@8"})
        assert result == DetectedToolchain(runtime="node", pm="pnpm")


class TestDetectFromLockfiles:
    def test_empty(self) -> None:
        assert detect_from_lockfiles([]) == DetectedToolchain()

    def test_bun_lock(self) -> None:
        assert detect_from_lockfiles(["bun.lock"]) == DetectedToolchain(pm="bun", runtime="bun")

    def test_bun_lockb(self) -> None:
        assert detect_from_lockfiles(["bun.lockb"]) == DetectedToolchain(pm="bun", runtime="bun")

    def test_pnpm_lock(self) -> None:
        assert detect_from_lockfiles(["pnpm-lock.yaml"]) == DetectedToolchain(pm="pnpm")

    def test_deno_lock(self) -> None:
        assert detect_from_lockfiles(["deno.lock"]) == DetectedToolchain(runtime="deno")

    def test_package_lock(self) -> None:
        assert detect_from_lockfiles(["package-lock.json"]) == DetectedToolchain(pm="npm")

    def test_yarn_lock(self) -> None:
        assert detect_from_lockfiles(["yarn.lock"]) == DetectedToolchain(pm="yarn-classic")

    def test_bun_beats_package_lock(self) -> None:
        assert detect_from_lockfiles(["package-lock.json", "bun.lock"]) == DetectedToolchain(pm="bun", runtime="bun")


class TestDetect:
    def test_empty_dir(self, tmp_path) -> None:
        assert detect(str(tmp_path)) == DetectedToolchain()

    def test_engines_bun(self, tmp_path) -> None:
        (tmp_path / "package.json").write_text(json.dumps({"engines": {"bun": ">=1.0"}}))
        assert detect(str(tmp_path)) == DetectedToolchain(runtime="bun", pm="bun")

    def test_lockfile(self, tmp_path) -> None:
        (tmp_path / "pnpm-lock.yaml").touch()
        assert detect(str(tmp_path)) == DetectedToolchain(pm="pnpm")

    def test_package_json_pm_beats_lockfile_pm(self, tmp_path) -> None:
        (tmp_path / "package.json").write_text(json.dumps({"packageManager": "pnpm@8"}))
        (tmp_path / "bun.lock").touch()
        result = detect(str(tmp_path))
        assert result.pm == "pnpm"
        assert result.runtime == "bun"

    def test_merges_runtime_and_pm(self, tmp_path) -> None:
        (tmp_path / "package.json").write_text(json.dumps({"engines": {"node": ">=18"}}))
        (tmp_path / "pnpm-lock.yaml").touch()
        assert detect(str(tmp_path)) == DetectedToolchain(runtime="node", pm="pnpm")

    def test_nonexistent_path(self, tmp_path) -> None:
        assert detect(str(tmp_path / "does-not-exist")) == DetectedToolchain()

    def test_yarn_berry_from_package_manager(self, tmp_path) -> None:
        (tmp_path / "package.json").write_text(json.dumps({"packageManager": "yarn@4.5.0"}))
        (tmp_path / "yarn.lock").touch()
        assert detect(str(tmp_path)) == DetectedToolchain(pm="yarn-berry")
