"""The egress audit's sign-in trace (AUTH §16): the primary's gate is admitted only on a pinned chain."""

import json
import ssl
import subprocess
from unittest.mock import patch

from dreamference.audit import EgressAudit
from dreamference.audit.auth_trace import AuthTrace
from dreamference.audit.egress_trace import EgressTrace
from dreamference.audit.egress_verdict import FAIL, PASS

NODE = "https://node.local:8000"
PRIMARY_INFO = {"issuer": "https://primary.local:8000"}
LAN = "192.0.2.7:8000"


def _trace(*targets):
    return EgressTrace(destinations=dict.fromkeys(targets, 1), processes={"ragon": 1})


def test_the_primary_gate_is_named_only_when_it_is_not_the_nodes_own():
    assert AuthTrace.primary_gate(PRIMARY_INFO, NODE) == ("primary.local", 8000)
    assert AuthTrace.primary_gate({"issuer": NODE}, NODE) is None
    assert AuthTrace.primary_gate({"issuer": None}, NODE) is None
    assert AuthTrace.primary_gate({}, NODE) is None
    # An issuer that names no host cannot be admitted either.
    assert AuthTrace.primary_gate({"issuer": "https://"}, NODE) is None


def test_the_primary_is_admitted_only_when_its_certificate_chains_to_the_pinned_ca():
    addrinfo = [(0, 0, 0, "", ("192.0.2.7", 8000))]
    with patch("socket.getaddrinfo", return_value=addrinfo):
        with patch.object(AuthTrace, "chains_to_ca", return_value=True):
            assert AuthTrace.admitted(PRIMARY_INFO, NODE, b"ca") == (
                frozenset({LAN}),
                ("primary.local",),
            )
        with patch.object(AuthTrace, "chains_to_ca", return_value=False):
            assert AuthTrace.admitted(PRIMARY_INFO, NODE, b"ca") == (frozenset(), ())
        assert AuthTrace.admitted(PRIMARY_INFO, NODE, None) == (frozenset(), ())


def test_a_handshake_that_fails_is_not_a_chain():
    with patch("socket.create_connection", side_effect=ssl.SSLError("untrusted")):
        assert AuthTrace.chains_to_ca("primary.local", 8000, b"ca") is False


def test_the_judge_passes_the_admitted_address_and_fails_any_other():
    allowed = {8000: "model server"}
    ok = EgressAudit.judge(
        _trace("127.0.0.1:8000", LAN), allowed, True, remote_allowed=frozenset({LAN})
    )
    assert ok.status == PASS
    other = EgressAudit.judge(
        _trace("127.0.0.1:8000", "192.0.2.9:8000"), allowed, True, remote_allowed=frozenset({LAN})
    )
    assert other.status == FAIL
    assert EgressAudit.judge(_trace(LAN), allowed, True).status == FAIL


def test_the_trace_runs_login_status_refresh_and_logout_with_the_revoke_last(tmp_path):
    ran = []

    def fake_run(command, **kwargs):
        ran.append(command[command.index("ragon") + 1 :] if "ragon" in command else command[12:])
        return subprocess.CompletedProcess(command, 0, "", "")

    home = tmp_path / "home"
    home.mkdir()
    (home / "auth.json").write_text(json.dumps({"tokens": {"access_token": "x"}}))
    with patch("subprocess.run", fake_run):
        _, codes = AuthTrace.trace(
            "ragon", {"CODEX_HOME": str(home), "RAGON_API_KEY": "k"}, str(tmp_path)
        )
    assert list(codes) == ["login", "status", "refresh", "logout"]
    assert ran[-1][-1] == "logout"
    assert ran[0][0] == "login" and "--create" in ran[0]
    # The refresh step made the stored access token stale.
    stale = json.loads((home / "auth.json").read_text())["tokens"]["access_token"]
    assert stale.endswith(".") and stale != "x"
