import json

import harmont as hm


def _nodes(p):
    return json.loads(hm.pipeline_to_json(p))["graph"]["nodes"]


def test_secret_ref_is_repr_safe():
    ref = hm.secrets["DEPLOY_TOKEN"]
    assert isinstance(ref, hm.SecretRef)
    assert ref.name == "DEPLOY_TOKEN"
    assert "DEPLOY_TOKEN" in repr(ref)


def test_env_secret_ref_lowers_into_secrets_map():
    p = hm.pipeline(
        [hm.sh("deploy", env={"TOKEN": hm.secrets["DEPLOY_TOKEN"], "CI": "true"})],
    )
    node = _nodes(p)[0]
    # NOTE: the current DSL also seeds baseline env (DEBIAN_FRONTEND, TERM).
    # Assert our keys are present rather than equality on the whole dict.
    assert node["env"]["CI"] == "true"
    assert "TOKEN" not in node["env"]
    assert node["secrets"] == {"TOKEN": "DEPLOY_TOKEN"}
    assert node["step"]["secrets"] == {"TOKEN": "DEPLOY_TOKEN"}


def test_pipeline_level_secret_merges_under_step():
    p = hm.pipeline(
        [hm.sh("deploy", env={"TOKEN": hm.secrets["PIPELINE_TOKEN"]})],
        env={"TOKEN": hm.secrets["GLOBAL_TOKEN"], "SHARED": hm.secrets["SHARED_SECRET"]},
    )
    node = _nodes(p)[0]
    assert node["secrets"] == {"TOKEN": "PIPELINE_TOKEN", "SHARED": "SHARED_SECRET"}


def test_invalid_secret_name_rejected():
    import pytest

    with pytest.raises(ValueError, match="secret name"):
        hm.secrets["not valid!"]
