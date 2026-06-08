"""Elixir toolchain abstraction tests."""

from __future__ import annotations

import pytest

import harmont as hm


def _cmds(p: dict) -> list[str]:
    return [n["step"]["cmd"] for n in p["graph"]["nodes"]]


def _step_by_substring(p: dict, needle: str) -> dict:
    for n in p["graph"]["nodes"]:
        if needle in (n["step"].get("cmd") or ""):
            return n["step"]
    msg = f"no command step containing {needle!r}"
    raise AssertionError(msg)


def test_elixir_object_form_full_chain():
    ex = hm.elixir(path="apps/api")
    p = hm.pipeline([ex.compile()], default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert any("apt-get install" in c for c in cmds)
    assert any("erlang" in c.lower() for c in cmds)
    assert any("elixir" in c.lower() for c in cmds)
    assert any("mix compile --warnings-as-errors" in c for c in cmds)


def test_elixir_actions_share_install_step():
    ex = hm.elixir(path=".")
    p = hm.pipeline([ex.compile(), ex.test(), ex.format(), ex.credo()], default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert len([c for c in cmds if "mix deps.get" in c]) == 1
    assert any("mix compile --warnings-as-errors" in c for c in cmds)
    assert any("mix test" in c for c in cmds)
    assert any("mix format --check-formatted" in c for c in cmds)
    assert any("mix credo --strict" in c for c in cmds)


def test_elixir_install_cache_forever():
    ex = hm.elixir(path=".")
    p = hm.pipeline([ex.compile()])
    erlang = _step_by_substring(p, "erlang")
    assert erlang["cache"]["policy"] == "forever"
    elixir_step = _step_by_substring(p, "elixir --version")
    assert elixir_step["cache"]["policy"] == "forever"


def test_elixir_version_in_install_cmd():
    ex = hm.elixir(elixir_version="1.18.3", otp_version="27.3.3")
    p = hm.pipeline([ex.compile()])
    elixir_step = _step_by_substring(p, "elixir-otp")
    assert "1.18.3" in elixir_step["cmd"]
    assert "27" in elixir_step["cmd"]


def test_elixir_invalid_version_rejected():
    with pytest.raises(ValueError, match="elixir version"):
        hm.elixir(elixir_version="bogus")


def test_elixir_invalid_otp_version_rejected():
    with pytest.raises(ValueError, match="otp version"):
        hm.elixir(otp_version="xyz!")


def test_elixir_bare_form_actions():
    p = hm.pipeline([hm.elixir.compile(), hm.elixir.test(), hm.elixir.format()])
    cmds = _cmds(p)
    assert any("mix compile" in c for c in cmds)
    assert any("mix test" in c for c in cmds)
    assert any("mix format" in c for c in cmds)


def test_elixir_action_labels():
    ex = hm.elixir(path=".")
    assert ex.compile().label == ":ex: compile"
    assert ex.test().label == ":ex: test"
    assert ex.format().label == ":ex: format"
    assert ex.credo().label == ":ex: credo"
    assert ex.dialyzer().label == ":ex: dialyzer"
    assert ex.sobelow().label == ":ex: sobelow"
    assert ex.deps_audit().label == ":ex: deps-audit"
    assert ex.hex_audit().label == ":ex: hex-audit"
    assert ex.release().label == ":ex: release"


def test_elixir_plt_cached_on_lock():
    ex = hm.elixir()
    step = ex.plt()
    assert "mix dialyzer --plt" in (step.cmd or "")
    assert step.label == ":ex: plt"
    p = hm.pipeline([step])
    plt_ir = next(
        n["step"] for n in p["graph"]["nodes"] if "dialyzer --plt" in (n["step"].get("cmd") or "")
    )
    assert plt_ir["cache"]["policy"] == "on_change"
    assert "./mix.lock" in plt_ir["cache"]["paths"]


def test_elixir_dialyzer_chains_through_plt():
    ex = hm.elixir()
    step = ex.dialyzer()
    assert step.label == ":ex: dialyzer"
    assert step.parent is not None
    assert step.parent.label == ":ex: plt"


def test_elixir_with_base_skips_apt():
    base = hm.scratch().sh("custom base", label="base")
    ex = hm.elixir(path=".", base=base)
    p = hm.pipeline([ex.compile()], default_image="ubuntu:24.04")
    cmds = _cmds(p)
    assert not any("apt-get update && apt-get install -y" in c for c in cmds)
    assert any("custom base" in c for c in cmds)


def test_elixir_test_cover_flag():
    ex = hm.elixir()
    step = ex.test(cover=True)
    assert "--cover" in (step.cmd or "")


def test_elixir_test_partitions_flag():
    ex = hm.elixir()
    step = ex.test(partitions=4)
    assert "--partitions 4" in (step.cmd or "")


def test_elixir_credo_no_strict():
    ex = hm.elixir()
    step = ex.credo(strict=False)
    assert "--strict" not in (step.cmd or "")
    assert "mix credo" in (step.cmd or "")


def test_elixir_release_custom_env():
    ex = hm.elixir()
    step = ex.release(mix_env="staging")
    assert "MIX_ENV=staging" in (step.cmd or "")


def test_elixir_mix_escape_hatch():
    ex = hm.elixir()
    step = ex.mix("phx.digest")
    assert "mix phx.digest" in (step.cmd or "")
    assert step.label == ":ex: phx.digest"
