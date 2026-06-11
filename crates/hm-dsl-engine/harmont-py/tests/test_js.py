"""Unified JS/TS toolchain tests — runtime x package-manager axes."""

from __future__ import annotations

import json

import pytest

import harmont as hm
from harmont._js import JsProject, js, ts

# ---------------------------------------------------------------------------
# Factory defaults
# ---------------------------------------------------------------------------


def test_defaults_node_npm() -> None:
    p = js.project()
    assert isinstance(p, JsProject)
    assert p.path == "."
    assert "npm ci" in p.install().cmd


def test_accepts_path() -> None:
    p = js.project(path="packages/app")
    assert p.path == "packages/app"
    assert "packages/app" in p.install().cmd


def test_accepts_node_version() -> None:
    p = js.project(version="22")
    assert "setup_22" in p.install().parent.cmd


def test_pm_defaults_to_bun_for_bun_runtime() -> None:
    p = js.project(runtime="bun")
    assert "bun install" in p.install().cmd


# ---------------------------------------------------------------------------
# Version validation
# ---------------------------------------------------------------------------


def test_rejects_invalid_node_version() -> None:
    with pytest.raises(ValueError, match="invalid version"):
        js.project(version="abc")


def test_accepts_node_version_x_suffix() -> None:
    js.project(version="22.x")


def test_rejects_invalid_bun_version() -> None:
    with pytest.raises(ValueError, match="invalid version"):
        js.project(runtime="bun", version="abc")


@pytest.mark.parametrize("version", ["1.2", "1.2.3"])
def test_accepts_bun_semver(version: str) -> None:
    js.project(runtime="bun", version=version)


def test_rejects_invalid_deno_version() -> None:
    with pytest.raises(ValueError, match="invalid version"):
        js.project(runtime="deno", version="abc")


def test_accepts_deno_semver() -> None:
    js.project(runtime="deno", version="2.0.0")


# ---------------------------------------------------------------------------
# PM / runtime validation
# ---------------------------------------------------------------------------


@pytest.mark.parametrize("pm", ["npm", "pnpm", "yarn-berry"])
def test_rejects_non_bun_pm_with_bun_runtime(pm: str) -> None:
    with pytest.raises(ValueError, match='runtime="bun" only supports pm="bun"'):
        js.project(pm=pm, runtime="bun")


def test_rejects_pm_with_deno_runtime() -> None:
    with pytest.raises(ValueError, match="do not set pm"):
        js.project(pm="npm", runtime="deno")


@pytest.mark.parametrize("pm", ["bun", "yarn-classic", "yarn-berry", "pnpm"])
def test_allows_pm_with_node_runtime(pm: str) -> None:
    js.project(pm=pm, runtime="node")


def test_allows_bun_pm_with_bun_runtime() -> None:
    js.project(pm="bun", runtime="bun")


@pytest.mark.parametrize("runtime", ["node", "bun"])
def test_rejects_deno_as_pm(runtime: str) -> None:
    with pytest.raises(ValueError, match='pm="deno" is not valid'):
        js.project(pm="deno", runtime=runtime)


# ---------------------------------------------------------------------------
# Install chain structure
# ---------------------------------------------------------------------------


def test_chain_node_npm() -> None:
    """scratch -> apt-base -> node-install -> npm-ci."""
    npm_ci = js.project().install()
    assert "npm ci" in npm_ci.cmd

    node_install = npm_ci.parent
    assert "nodejs" in node_install.cmd
    assert node_install.cache is not None

    apt_base = node_install.parent
    assert "apt-get" in apt_base.cmd

    assert apt_base.parent.cmd is None  # scratch


def test_chain_node_pnpm() -> None:
    """scratch -> apt-base -> node-install -> corepack-pnpm -> pnpm-deps."""
    pnpm_deps = js.project(pm="pnpm").install()
    assert "pnpm install --frozen-lockfile" in pnpm_deps.cmd

    corepack = pnpm_deps.parent
    assert "corepack enable pnpm" in corepack.cmd

    assert "nodejs" in corepack.parent.cmd


def test_chain_node_yarn_classic() -> None:
    deps = js.project(pm="yarn-classic").install()
    assert "yarn install --frozen-lockfile" in deps.cmd
    assert "corepack enable" in deps.parent.cmd
    assert "nodejs" in deps.parent.parent.cmd


def test_chain_node_yarn_berry() -> None:
    deps = js.project(pm="yarn-berry").install()
    assert "yarn install --immutable" in deps.cmd
    assert "corepack enable" in deps.parent.cmd


@pytest.mark.parametrize("pm", ["yarn-classic", "yarn-berry"])
def test_yarn_watches_yarn_lock(pm: str) -> None:
    deps = js.project(pm=pm).install()
    assert deps.cache.paths == ("./yarn.lock",)


def test_chain_bun_runtime() -> None:
    """scratch -> apt-base(+unzip) -> bun-install -> bun-deps."""
    bun_deps = js.project(runtime="bun").install()
    assert "bun install --frozen-lockfile" in bun_deps.cmd

    bun_setup = bun_deps.parent
    assert "bun.sh/install" in bun_setup.cmd
    assert bun_setup.cache is not None

    apt_base = bun_setup.parent
    assert "apt-get" in apt_base.cmd
    assert "unzip" in apt_base.cmd
    assert apt_base.parent.cmd is None


def test_chain_node_bun_as_pm() -> None:
    """scratch -> apt-base(+unzip) -> node-install -> bun-pm -> bun-deps."""
    bun_deps = js.project(runtime="node", pm="bun").install()
    assert "bun install --frozen-lockfile" in bun_deps.cmd

    bun_pm = bun_deps.parent
    assert "bun.sh/install" in bun_pm.cmd

    node_install = bun_pm.parent
    assert "nodejs" in node_install.cmd

    apt_base = node_install.parent
    assert "unzip" in apt_base.cmd


def test_chain_deno() -> None:
    """scratch -> apt-base(+unzip) -> deno-install -> deno-deps."""
    deno_deps = js.project(runtime="deno").install()
    assert "deno install" in deno_deps.cmd

    deno_setup = deno_deps.parent
    assert "deno.land/install.sh" in deno_setup.cmd
    assert deno_setup.cache is not None

    apt_base = deno_setup.parent
    assert "unzip" in apt_base.cmd
    assert apt_base.parent.cmd is None


# ---------------------------------------------------------------------------
# Base step and custom image
# ---------------------------------------------------------------------------


def test_accepts_base_step() -> None:
    custom_base = hm.sh("custom base")
    node_install = js.project(base=custom_base).install().parent
    assert node_install.parent is custom_base


def test_accepts_custom_image() -> None:
    npm_ci = js.project(image="debian:12").install()
    apt_base = npm_ci.parent.parent
    assert apt_base.image == "debian:12"


# ---------------------------------------------------------------------------
# Actions — uniform run() across all PMs/runtimes
# ---------------------------------------------------------------------------


def test_run_uses_npm_run() -> None:
    assert "npm run typecheck" in js.project().run("typecheck").cmd


def test_run_uses_pnpm_run() -> None:
    assert "pnpm run typecheck" in js.project(pm="pnpm").run("typecheck").cmd


@pytest.mark.parametrize("pm", ["yarn-classic", "yarn-berry"])
def test_run_uses_yarn_run(pm: str) -> None:
    assert "yarn run test" in js.project(pm=pm).run("test").cmd


def test_run_uses_bun_run() -> None:
    assert "bun run typecheck" in js.project(runtime="bun").run("typecheck").cmd


def test_run_uses_deno_task() -> None:
    assert "deno task typecheck" in js.project(runtime="deno").run("typecheck").cmd


def test_actions_attach_to_install() -> None:
    p = js.project()
    assert p.run("test").parent is p.install()
    assert p.run("lint").parent is p.install()


def test_actions_respect_path() -> None:
    assert "cd packages/ui" in js.project(path="packages/ui").run("test").cmd


def test_actions_accept_step_options() -> None:
    t = hm.timeout(300, js.project().run("test", label="my test"))
    assert t.label == "my test"
    assert t.timeout_seconds == 300


@pytest.mark.parametrize(
    ("runtime", "expected"),
    [("node", ":node: test"), ("bun", ":bun: test"), ("deno", ":deno: test")],
)
def test_default_label(runtime: str, expected: str) -> None:
    assert js.project(runtime=runtime).run("test").label == expected


# ---------------------------------------------------------------------------
# Pipeline IR
# ---------------------------------------------------------------------------


@pytest.mark.parametrize(
    "opts",
    [{}, {"pm": "pnpm"}, {"pm": "yarn-berry"}, {"runtime": "bun"}, {"runtime": "deno"}],
)
def test_pipeline_ir(opts: dict) -> None:
    p = js.project(**opts)
    ir = hm.pipeline([p.run("test"), p.run("lint")])
    assert ir["version"] == "0"
    assert len(ir["graph"]["nodes"]) >= 4


# ---------------------------------------------------------------------------
# Auto-detection
# ---------------------------------------------------------------------------


class TestAutoDetection:
    def test_detects_pnpm(self, tmp_path) -> None:
        (tmp_path / "package.json").write_text("{}")
        (tmp_path / "pnpm-lock.yaml").touch()
        p = js.project(path=str(tmp_path))
        assert "pnpm install --frozen-lockfile" in p.install().cmd

    def test_detects_bun(self, tmp_path) -> None:
        (tmp_path / "package.json").write_text("{}")
        (tmp_path / "bun.lock").touch()
        p = js.project(path=str(tmp_path))
        assert "bun install --frozen-lockfile" in p.install().cmd
        assert "bun.sh/install" in p.install().parent.cmd

    def test_detects_bun_from_engines(self, tmp_path) -> None:
        (tmp_path / "package.json").write_text(json.dumps({"engines": {"bun": ">=1.0"}}))
        p = js.project(path=str(tmp_path))
        assert "bun install --frozen-lockfile" in p.install().cmd

    def test_detects_deno(self, tmp_path) -> None:
        (tmp_path / "package.json").write_text("{}")
        (tmp_path / "deno.lock").touch()
        p = js.project(path=str(tmp_path))
        assert "deno install" in p.install().cmd

    def test_detects_yarn_berry(self, tmp_path) -> None:
        (tmp_path / "package.json").write_text(json.dumps({"packageManager": "yarn@4.5.0"}))
        (tmp_path / "yarn.lock").touch()
        p = js.project(path=str(tmp_path))
        assert "yarn install --immutable" in p.install().cmd

    def test_detects_yarn_classic(self, tmp_path) -> None:
        (tmp_path / "package.json").write_text("{}")
        (tmp_path / "yarn.lock").touch()
        p = js.project(path=str(tmp_path))
        assert "yarn install --frozen-lockfile" in p.install().cmd

    def test_explicit_opts_skip_detection(self, tmp_path) -> None:
        (tmp_path / "package.json").write_text("{}")
        (tmp_path / "bun.lock").touch()
        p = js.project(path=str(tmp_path), pm="npm", runtime="node")
        assert "npm ci" in p.install().cmd

    def test_defaults_when_no_signals(self, tmp_path) -> None:
        (tmp_path / "package.json").write_text("{}")
        p = js.project(path=str(tmp_path))
        assert "npm ci" in p.install().cmd

    def test_skips_detection_when_runtime_set(self, tmp_path) -> None:
        (tmp_path / "package.json").write_text("{}")
        (tmp_path / "pnpm-lock.yaml").touch()
        p = js.project(path=str(tmp_path), runtime="node")
        assert "npm ci" in p.install().cmd

    def test_skips_detection_when_pm_set(self, tmp_path) -> None:
        (tmp_path / "package.json").write_text("{}")
        (tmp_path / "bun.lock").touch()
        p = js.project(path=str(tmp_path), pm="pnpm")
        assert "pnpm install --frozen-lockfile" in p.install().cmd


# ---------------------------------------------------------------------------
# Corepack version pin (parity with the TypeScript DSL)
# ---------------------------------------------------------------------------


class TestCorepackPin:
    def test_pnpm_command_includes_version(self, tmp_path) -> None:
        (tmp_path / "package.json").write_text(json.dumps({"packageManager": "pnpm@10.33.0"}))
        (tmp_path / "pnpm-lock.yaml").touch()
        deps = js.project(path=str(tmp_path)).install()
        corepack = deps.parent
        assert corepack.cmd == "corepack enable pnpm && corepack install -g pnpm@10.33.0"

    def test_yarn_berry_command_includes_version(self, tmp_path) -> None:
        (tmp_path / "package.json").write_text(json.dumps({"packageManager": "yarn@4.5.0"}))
        (tmp_path / "yarn.lock").touch()
        deps = js.project(path=str(tmp_path)).install()
        corepack = deps.parent
        assert corepack.cmd == "corepack enable yarn && corepack install -g yarn@4.5.0"

    def test_command_has_no_version_when_field_absent(self, tmp_path) -> None:
        (tmp_path / "package.json").write_text("{}")
        (tmp_path / "pnpm-lock.yaml").touch()
        deps = js.project(path=str(tmp_path)).install()
        corepack = deps.parent
        assert corepack.cmd == "corepack enable pnpm"

    def test_explicit_pm_omits_version(self) -> None:
        # An explicit pm option skips detection entirely, so no pin is applied.
        deps = js.project(pm="pnpm").install()
        corepack = deps.parent
        assert corepack.cmd == "corepack enable pnpm"

    def test_cache_watches_package_json_when_pinned(self, tmp_path) -> None:
        (tmp_path / "package.json").write_text(json.dumps({"packageManager": "pnpm@10.33.0"}))
        (tmp_path / "pnpm-lock.yaml").touch()
        deps = js.project(path=str(tmp_path)).install()
        corepack = deps.parent
        assert corepack.cache.paths == (f"{tmp_path}/package.json",)

    def test_cache_is_forever_when_no_field(self, tmp_path) -> None:
        (tmp_path / "package.json").write_text("{}")
        (tmp_path / "pnpm-lock.yaml").touch()
        deps = js.project(path=str(tmp_path)).install()
        corepack = deps.parent
        assert corepack.cache == hm.CacheForever(env_keys=())


# ---------------------------------------------------------------------------
# ts alias
# ---------------------------------------------------------------------------


def test_ts_is_js() -> None:
    assert ts is js
    assert isinstance(ts.project(), JsProject)
