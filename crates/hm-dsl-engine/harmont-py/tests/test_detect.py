"""Tests for JS runtime/PM auto-detection."""

from __future__ import annotations

import json

from harmont._detect import (
    DetectedToolchain,
    detect,
    detect_from_lockfiles,
    detect_from_package_json,
)


class TestDetectFromPackageJson:
    def test_empty_object(self) -> None:
        assert detect_from_package_json({}) == DetectedToolchain()

    def test_engines_node(self) -> None:
        pkg = {"engines": {"node": ">=18"}}
        assert detect_from_package_json(pkg) == DetectedToolchain(runtime="node")

    def test_engines_bun(self) -> None:
        pkg = {"engines": {"bun": ">=1.0"}}
        assert detect_from_package_json(pkg) == DetectedToolchain(runtime="bun", pm="bun")

    def test_engines_deno(self) -> None:
        pkg = {"engines": {"deno": ">=2.0"}}
        assert detect_from_package_json(pkg) == DetectedToolchain(runtime="deno")

    def test_package_manager_pnpm(self) -> None:
        pkg = {"packageManager": "pnpm@8.15.4"}
        assert detect_from_package_json(pkg) == DetectedToolchain(pm="pnpm", pm_version="8.15.4")

    def test_package_manager_bun(self) -> None:
        pkg = {"packageManager": "bun@1.1.0"}
        assert detect_from_package_json(pkg) == DetectedToolchain(pm="bun", pm_version="1.1.0")

    def test_package_manager_npm(self) -> None:
        pkg = {"packageManager": "npm@10.2.4"}
        assert detect_from_package_json(pkg) == DetectedToolchain(pm="npm", pm_version="10.2.4")

    def test_package_manager_yarn_classic(self) -> None:
        pkg = {"packageManager": "yarn@1.22.22"}
        assert detect_from_package_json(pkg) == DetectedToolchain(
            pm="yarn-classic", pm_version="1.22.22"
        )

    def test_package_manager_yarn_berry(self) -> None:
        pkg = {"packageManager": "yarn@4.0.0"}
        assert detect_from_package_json(pkg) == DetectedToolchain(
            pm="yarn-berry", pm_version="4.0.0"
        )

    def test_package_manager_without_version_omits_pm_version(self) -> None:
        pkg = {"packageManager": "pnpm"}
        assert detect_from_package_json(pkg) == DetectedToolchain(pm="pnpm", pm_version=None)

    def test_ignores_unknown_package_manager(self) -> None:
        pkg = {"packageManager": "unknown@1.0"}
        assert detect_from_package_json(pkg) == DetectedToolchain()

    def test_engines_bun_overrides_package_manager(self) -> None:
        pkg = {"engines": {"bun": ">=1.0"}, "packageManager": "pnpm@8"}
        result = detect_from_package_json(pkg)
        assert result == DetectedToolchain(runtime="bun", pm="bun")

    def test_engines_node_plus_package_manager_pnpm(self) -> None:
        pkg = {"engines": {"node": ">=18"}, "packageManager": "pnpm@8"}
        result = detect_from_package_json(pkg)
        assert result == DetectedToolchain(runtime="node", pm="pnpm", pm_version="8")


class TestDetectFromLockfiles:
    def test_empty(self) -> None:
        assert detect_from_lockfiles([]) == DetectedToolchain()

    def test_bun_lock(self) -> None:
        result = detect_from_lockfiles(["bun.lock"])
        assert result == DetectedToolchain(pm="bun", runtime="bun")

    def test_bun_lockb(self) -> None:
        result = detect_from_lockfiles(["bun.lockb"])
        assert result == DetectedToolchain(pm="bun", runtime="bun")

    def test_pnpm_lock(self) -> None:
        result = detect_from_lockfiles(["pnpm-lock.yaml"])
        assert result == DetectedToolchain(pm="pnpm")

    def test_deno_lock(self) -> None:
        result = detect_from_lockfiles(["deno.lock"])
        assert result == DetectedToolchain(runtime="deno")

    def test_package_lock(self) -> None:
        result = detect_from_lockfiles(["package-lock.json"])
        assert result == DetectedToolchain(pm="npm")

    def test_yarn_lock(self) -> None:
        result = detect_from_lockfiles(["yarn.lock"])
        assert result == DetectedToolchain(pm="yarn-classic")

    def test_bun_beats_package_lock(self) -> None:
        result = detect_from_lockfiles(["package-lock.json", "bun.lock"])
        assert result == DetectedToolchain(pm="bun", runtime="bun")


class TestDetect:
    def test_empty_dir(self, tmp_path) -> None:
        assert detect(str(tmp_path)) == DetectedToolchain()

    def test_engines_bun(self, tmp_path) -> None:
        pkg = json.dumps({"engines": {"bun": ">=1.0"}})
        (tmp_path / "package.json").write_text(pkg)
        assert detect(str(tmp_path)) == DetectedToolchain(runtime="bun", pm="bun")

    def test_lockfile(self, tmp_path) -> None:
        (tmp_path / "pnpm-lock.yaml").touch()
        assert detect(str(tmp_path)) == DetectedToolchain(pm="pnpm")

    def test_package_json_pm_beats_lockfile_pm(self, tmp_path) -> None:
        pkg = json.dumps({"packageManager": "pnpm@8"})
        (tmp_path / "package.json").write_text(pkg)
        (tmp_path / "bun.lock").touch()
        result = detect(str(tmp_path))
        assert result.pm == "pnpm"
        assert result.runtime == "bun"

    def test_merges_runtime_and_pm(self, tmp_path) -> None:
        pkg = json.dumps({"engines": {"node": ">=18"}})
        (tmp_path / "package.json").write_text(pkg)
        (tmp_path / "pnpm-lock.yaml").touch()
        assert detect(str(tmp_path)) == DetectedToolchain(runtime="node", pm="pnpm")

    def test_nonexistent_path(self, tmp_path) -> None:
        assert detect(str(tmp_path / "does-not-exist")) == DetectedToolchain()

    def test_yarn_berry_from_package_manager(self, tmp_path) -> None:
        pkg = json.dumps({"packageManager": "yarn@4.5.0"})
        (tmp_path / "package.json").write_text(pkg)
        (tmp_path / "yarn.lock").touch()
        assert detect(str(tmp_path)) == DetectedToolchain(pm="yarn-berry", pm_version="4.5.0")

    def test_pm_version_propagates_through_detect(self, tmp_path) -> None:
        pkg = json.dumps({"packageManager": "pnpm@10.33.0"})
        (tmp_path / "package.json").write_text(pkg)
        (tmp_path / "pnpm-lock.yaml").touch()
        result = detect(str(tmp_path))
        assert result.pm == "pnpm"
        assert result.pm_version == "10.33.0"
