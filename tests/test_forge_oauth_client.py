"""The node's Forgejo as an OAuth2 client of the primary (specs/DREAMFERENCE_AUTH.md §14.2)."""

import inspect
import json
import subprocess
import urllib.error
from pathlib import Path
from types import SimpleNamespace

import pytest

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


# -- registration on the primary and the source at start and tick


@pytest.fixture
def kept(monkeypatch):
    """The forge's secrets and state in memory."""
    secrets_ = {}
    state = {}
    monkeypatch.setattr(ForgeSettings, "read_secret", classmethod(lambda c, n: secrets_.get(n)))
    monkeypatch.setattr(
        ForgeSettings, "write_secret", classmethod(lambda c, n, v: secrets_.update({n: v}))
    )
    monkeypatch.setattr(ForgeSettings, "state", classmethod(lambda c: dict(state)))
    monkeypatch.setattr(
        ForgeSettings, "update_state", classmethod(lambda c, **k: state.update(k) or dict(state))
    )
    return SimpleNamespace(secrets=secrets_, state=state)


def test_a_kept_secret_gives_a_record_whose_hash_matches_it(kept):
    first, secret = ForgeOauthClient.register("n1", "http://box.local:3100/forge/")
    again, none = ForgeOauthClient.register("n1", "http://box.local:3100/forge/")
    assert none is None
    assert again.secret_hash == first.secret_hash and again.secret_matches(secret)
    assert again.allows("http://box.local:3100/forge/user/oauth2/ragon/callback")


@pytest.fixture
def primary(monkeypatch, kept):
    """A node whose accounts service answers discovery; records the clients it stores."""
    from dreamference.node.node_settings import NodeSettings

    seen = {"posts": [], "sources": [], "stored": [], "pinned": None, "key": None}
    monkeypatch.setattr(NodeSettings, "pinned_primary", classmethod(lambda c: seen["pinned"]))

    def local_json(cls, path, payload=None, key=None):
        if path == module.DISCOVERY_PATH:
            return {"issuer": ISSUER}
        seen["posts"].append((path, payload, key))
        return {"client_id": "forgejo-n1", "changed": True}

    monkeypatch.setattr(ForgeOauthClient, "local_json", classmethod(local_json))
    monkeypatch.setattr(
        ForgeOauthClient,
        "ensure_source",
        classmethod(lambda c, issuer: seen["sources"].append(issuer) or True),
    )

    class Store:
        def put_forgejo_client(self, node_id, record):
            seen["stored"].append((node_id, record))
            return True

    from dreamference.accounts import accounts_store

    monkeypatch.setattr(accounts_store, "AccountsStore", Store)
    from dreamference.node.service_key import ServiceKey

    monkeypatch.setattr(ServiceKey, "read", classmethod(lambda c: seen["key"]))
    return seen


def test_the_primary_stores_the_client_itself_and_adds_the_source_once(primary, kept):
    lines, changed = ForgeOauthClient.ensure("n1", "http://box.local:3100/forge/")
    assert changed and "box" not in lines[0] and "primary.local" in lines[0]
    ((node_id, record),) = primary["stored"]
    assert node_id == "n1" and record.client_id == "forgejo-n1"
    assert record.secret_matches(kept.secrets[module.SECRET_SECRET])
    assert primary["sources"] == [ISSUER] and primary["posts"] == []
    assert ForgeOauthClient.ensure("n1", "http://box.local:3100/forge/") == ([], False)
    assert primary["sources"] == [ISSUER] and len(primary["stored"]) == 2


def test_a_secondary_posts_the_hash_with_its_service_key(primary, kept):
    primary["pinned"] = "p-1"
    primary["key"] = "sk-ragon-node"
    _, changed = ForgeOauthClient.ensure("n1", "http://box.local:3100/forge/")
    ((path, payload, key),) = primary["posts"]
    assert changed and path == "/ragon/v1/nodes/forgejo" and key == "sk-ragon-node"
    assert payload["redirect_uris"] == ["http://box.local:3100/forge/user/oauth2/ragon/callback"]
    assert kept.secrets[module.SECRET_SECRET] not in json.dumps(payload)
    assert primary["stored"] == []


def test_a_secondary_without_a_service_key_says_so_and_changes_nothing(primary, kept):
    primary["pinned"] = "p-1"
    lines, changed = ForgeOauthClient.ensure("n1", "http://box.local:3100/forge/")
    assert not changed and "no service key" in lines[0] and "local sign-in" in lines[0]
    assert primary["sources"] == [] and kept.state == {}


def test_a_refusing_or_unreachable_primary_leaves_local_sign_in(primary, kept, monkeypatch):
    primary["pinned"] = "p-1"
    primary["key"] = "sk-ragon-node"

    def refused(cls, path, payload=None, key=None):
        if path == module.DISCOVERY_PATH:
            return {"issuer": ISSUER}
        raise urllib.error.HTTPError(path, 403, "forbidden", {}, None)

    monkeypatch.setattr(ForgeOauthClient, "local_json", classmethod(refused))
    lines, changed = ForgeOauthClient.ensure("n1", "http://box.local:3100/forge/")
    assert not changed and "HTTP 403" in lines[0] and "an admin registers it" in lines[0]

    def down(cls, path, payload=None, key=None):
        raise OSError("connection refused")

    monkeypatch.setattr(ForgeOauthClient, "local_json", classmethod(down))
    lines, changed = ForgeOauthClient.ensure("n1", "http://box.local:3100/forge/")
    assert not changed and "primary could not be reached" in lines[0]
    assert primary["sources"] == []


def test_a_source_forgejo_refuses_is_tried_again_next_time(primary, kept, monkeypatch):
    monkeypatch.setattr(ForgeOauthClient, "ensure_source", classmethod(lambda c, issuer: False))
    lines, changed = ForgeOauthClient.ensure("n1", "http://box.local:3100/forge/")
    assert not changed and "did not take" in lines[0] and kept.state == {}


def test_the_container_reaches_the_issuer_by_its_address_and_trusts_its_ca(
    kept, monkeypatch, tmp_path
):
    assert ForgeOauthClient.container_arguments() == ([], [], {})
    kept.state[module.STATE_ISSUER] = ISSUER
    monkeypatch.setattr(
        module.socket,
        "getaddrinfo",
        lambda host, port, proto=0: [
            (0, 0, 0, "", ("127.0.1.1", 0)),
            (0, 0, 0, "", ("fd00::5", 0, 0, 0)),
            (0, 0, 0, "", ("192.168.1.5", 0)),
        ],
    )
    ca = tmp_path / "ca.pem"
    monkeypatch.setattr(ForgeOauthClient, "ca_file", classmethod(lambda c: None))
    assert ForgeOauthClient.container_arguments() == ([("primary.local", "192.168.1.5")], [], {})
    ca.write_text("pem")
    monkeypatch.setattr(ForgeOauthClient, "ca_file", classmethod(lambda c: ca))
    _hosts, mounts, env = ForgeOauthClient.container_arguments()
    assert mounts == [(str(ca), "/etc/ragon-ca/ca.pem")]
    assert env == {"SSL_CERT_DIR": "/etc/ssl/certs:/etc/ragon-ca"}


def test_a_name_that_does_not_resolve_adds_no_host(monkeypatch):
    def fail(host, port, proto=0):
        raise OSError("no such name")

    monkeypatch.setattr(module.socket, "getaddrinfo", fail)
    assert ForgeOauthClient.resolve("primary.local") is None


def test_the_ca_is_the_pinned_one_on_a_secondary(monkeypatch, tmp_path):
    from dreamference.accounts.accounts_secondary import AccountsSecondary
    from dreamference.node.node_settings import NodeSettings

    pinned = tmp_path / "pinned.pem"
    pinned.write_text("pem")
    monkeypatch.setattr(NodeSettings, "pinned_primary", classmethod(lambda c: "p-1"))
    monkeypatch.setattr(AccountsSecondary, "ca_path", classmethod(lambda c, node_id: pinned))
    assert ForgeOauthClient.ca_file() == pinned
