"""Trigger constructors — push/pull_request."""

import pytest

import harmont as hm


def test_push_branch_string():
    t = hm.push(branch="main")
    assert t.to_dict() == {"event": "push", "branches": ["main"]}


def test_push_branch_list():
    t = hm.push(branch=["main", "release/*"])
    assert t.to_dict() == {"event": "push", "branches": ["main", "release/*"]}


def test_push_tag_string():
    t = hm.push(tag="v*")
    assert t.to_dict() == {"event": "push", "tags": ["v*"]}


def test_push_both_branch_and_tag_raises():
    with pytest.raises(ValueError, match=r"hm\.push: pass exactly one of branch or tag"):
        hm.push(branch="main", tag="v*")


def test_push_neither_raises():
    with pytest.raises(ValueError, match=r"hm\.push: pass exactly one of branch or tag"):
        hm.push()


def test_pull_request_branches_string():
    t = hm.pull_request(branches="main")
    assert t.to_dict() == {
        "event": "pull_request",
        "branches": ["main"],
        "types": ["opened", "synchronize", "reopened"],
    }


def test_pull_request_no_filter():
    t = hm.pull_request()
    assert t.to_dict() == {
        "event": "pull_request",
        "types": ["opened", "synchronize", "reopened"],
    }


def test_pull_request_types_override():
    t = hm.pull_request(types=["opened", "ready_for_review"])
    assert t.to_dict()["types"] == ["opened", "ready_for_review"]


def test_pull_request_invalid_type():
    with pytest.raises(ValueError, match=r"unknown pull_request type 'merged'"):
        hm.pull_request(types=["merged"])


def test_pull_request_empty_types():
    with pytest.raises(ValueError, match=r"hm\.pull_request: types must be non-empty"):
        hm.pull_request(types=[])


