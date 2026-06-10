import harmont as hm


def test_env_returns_value(monkeypatch):
    monkeypatch.setenv("HARMONT_TEST_ENV", "configured")
    assert hm.env("HARMONT_TEST_ENV") == "configured"


def test_env_returns_default_when_unset(monkeypatch):
    monkeypatch.delenv("HARMONT_TEST_ENV", raising=False)
    assert hm.env("HARMONT_TEST_ENV", "fallback") == "fallback"


def test_env_returns_none_without_default(monkeypatch):
    monkeypatch.delenv("HARMONT_TEST_ENV", raising=False)
    assert hm.env("HARMONT_TEST_ENV") is None
