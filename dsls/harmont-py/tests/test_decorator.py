"""@hm.pipeline decorator surface."""

import pytest

import harmont as hm
from harmont._registry import REGISTRATIONS, clear_registry


@pytest.fixture(autouse=True)
def _reset_registry():
    clear_registry()
    yield
    clear_registry()


def test_explicit_slug():
    @hm.pipeline("ci")
    def whatever() -> hm.Step:
        return hm.scratch().sh("echo hi", label="hi")

    assert len(REGISTRATIONS) == 1
    reg = REGISTRATIONS[0]
    assert reg.slug == "ci"
    assert reg.name == "ci"
    assert reg.triggers == ()
    assert reg.allow_manual is True
    assert reg.env is None
    assert reg.default_image is None


def test_default_slug_from_function_name():
    @hm.pipeline()
    def nightly() -> hm.Step:
        return hm.scratch().sh("echo n")

    assert REGISTRATIONS[0].slug == "nightly"


def test_name_override():
    @hm.pipeline("ci", name="Continuous Integration")
    def ci() -> hm.Step:
        return hm.scratch().sh("echo")

    assert REGISTRATIONS[0].name == "Continuous Integration"


def test_forwards_env_and_default_image():
    @hm.pipeline("ci", env={"FOO": "bar"}, default_image="alpine:3.20")
    def ci() -> hm.Step:
        return hm.scratch().sh("echo")

    reg = REGISTRATIONS[0]
    assert reg.env == {"FOO": "bar"}
    assert reg.default_image == "alpine:3.20"


def test_allow_manual_false():
    @hm.pipeline("ci", allow_manual=False)
    def ci() -> hm.Step:
        return hm.scratch().sh("echo")

    assert REGISTRATIONS[0].allow_manual is False


def test_decorator_returns_function_unchanged():
    @hm.pipeline("ci")
    def ci() -> hm.Step:
        return hm.scratch().sh("echo hi")

    result = ci()
    assert isinstance(result, hm.Step)


def test_invalid_slug_uppercase():
    with pytest.raises(ValueError, match="invalid pipeline slug 'CI'"):

        @hm.pipeline("CI")
        def ci() -> hm.Step:
            return hm.scratch().sh("echo")


def test_invalid_slug_starts_with_digit():
    with pytest.raises(ValueError, match="invalid pipeline slug '1ci'"):

        @hm.pipeline("1ci")
        def x() -> hm.Step:
            return hm.scratch().sh("echo")


def test_invalid_slug_too_long():
    long = "a" * 65
    with pytest.raises(ValueError, match="invalid pipeline slug"):

        @hm.pipeline(long)
        def x() -> hm.Step:
            return hm.scratch().sh("echo")


def test_duplicate_slug_raises():
    @hm.pipeline("ci")
    def a() -> hm.Step:
        return hm.scratch().sh("echo")

    with pytest.raises(ValueError, match="duplicate pipeline slug"):

        @hm.pipeline("ci")
        def b() -> hm.Step:
            return hm.scratch().sh("echo")
