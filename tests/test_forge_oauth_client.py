"""The node's Forgejo as an OAuth2 client of the primary (specs/DREAMFERENCE_AUTH.md §14.2)."""

import inspect
import subprocess
from pathlib import Path

from dreamference.forge import forge_oauth_client as module
from dreamference.forge.forge_app_ini import ForgeAppIni
from dreamference.forge.forge_oauth_client import ForgeOauthClient
from dreamference.forge.forge_settings import ForgeSettings

ISSUER = "https://primary.local:8443"


def fake_forgejo(calls, listing=""):
    def run(arguments, timeout=300):
        calls.append(list(arguments))
        out = listing if arguments[:3] == ["admin", "auth", "list"] else ""
        return subprocess.CompletedProcess(arguments, 0, stdout=out, stderr="")

    return run


def test_register_keeps_the_secret_once_and_returns_the_record(monkeypatch):
    store = {}
    monkeypatch.setattr(ForgeSettings, "read_secret", classmethod(lambda c, n: store.get(n)))
    monkeypatch.setattr(
        ForgeSettings, "write_secret", classmethod(lambda c, n, v: store.update({n: v}))
    )
    record, secret = ForgeOauthClient.register("n1", "http://box.local:3000/")
    assert record.client_id == "forgejo-n1"
    assert record.allows("http://box.local:3000/user/oauth2/ragon/callback")
    assert secret and store[module.SECRET_SECRET] == secret
    again, none = ForgeOauthClient.register("n1", "http://box.local:3000/")
    assert none is None and again.client_id == record.client_id


def test_ensure_source_adds_then_updates_with_the_discovery_url(monkeypatch):
    store = {module.ID_SECRET: "forgejo-n1", module.SECRET_SECRET: "s3"}
    monkeypatch.setattr(ForgeSettings, "read_secret", classmethod(lambda c, n: store.get(n)))
    calls = []
    monkeypatch.setattr(module.ForgeContainer, "forgejo", staticmethod(fake_forgejo(calls)))
    assert ForgeOauthClient.ensure_source(ISSUER)
    add = calls[-1]
    assert add[:3] == ["admin", "auth", "add-oauth"]
    assert add[add.index("--provider") + 1] == "openidConnect"
    assert add[add.index("--auto-discover-url") + 1] == ISSUER + "/.well-known/openid-configuration"
    calls.clear()
    listing = "ID Name Type Enabled\n7   ragon OAuth2 true\n"
    monkeypatch.setattr(
        module.ForgeContainer, "forgejo", staticmethod(fake_forgejo(calls, listing))
    )
    assert ForgeOauthClient.ensure_source(ISSUER)
    assert calls[-1][:5] == ["admin", "auth", "update-oauth", "--id", "7"]


def test_ensure_source_without_a_registered_client_does_nothing(monkeypatch):
    monkeypatch.setattr(ForgeSettings, "read_secret", classmethod(lambda c, n: None))
    assert ForgeOauthClient.ensure_source(ISSUER) is False


def test_the_email_follows_the_username_and_the_primary_name():
    assert ForgeOauthClient.linked_email("ada", "primary") == "ada@primary.local"


def test_app_ini_links_oauth_accounts_automatically_and_registers_new_ones():
    source = Path(inspect.getsourcefile(ForgeAppIni)).read_text(encoding="utf-8")
    assert '"oauth2_client"' in source
    assert '"ACCOUNT_LINKING": "auto"' in source


def test_forge_oauth_client_source_id_is_none_when_the_listing_fails(monkeypatch):
    monkeypatch.setattr(
        module.ForgeContainer,
        "forgejo",
        staticmethod(
            lambda arguments, timeout=300: subprocess.CompletedProcess(
                list(arguments), 1, stdout="", stderr=""
            )
        ),
    )
    assert ForgeOauthClient.source_id() is None
