"""A web tool's session sends the login its caller set, and only that one."""

import base64
import os
from pathlib import Path

import requests

from dreamference.mcp_server.web_session import AUTHORIZATION, WebSession


def _prepared(url: str, auth: tuple[str, str] | None = None) -> requests.PreparedRequest:
    prepared = requests.PreparedRequest()
    prepared.prepare(method="GET", url=url)
    if auth is not None:
        prepared.prepare_auth(auth)
    return prepared


def _response(prepared: requests.PreparedRequest) -> requests.Response:
    response = requests.Response()
    response.request = prepared
    return response


def _basic(user: str, password: str) -> str:
    return "Basic " + base64.b64encode(f"{user}:{password}".encode()).decode()


def test_a_login_the_caller_sets_is_sent_and_not_replaced_by_the_url_or_netrc():
    netrc = Path(os.environ["HOME"]) / ".netrc"
    netrc.write_text("machine example.com login netrc-user password netrc-secret\n")
    netrc.chmod(0o600)
    session = WebSession()

    explicit = session.prepare_request(
        requests.Request("GET", "http://url-user:url-pass@example.com/", auth=("caller", "pw"))
    )
    assert explicit.headers["Authorization"] == _basic("caller", "pw")

    # Without one, only the URL's own login goes out, and with none there is no header at all.
    from_url = session.prepare_request(
        requests.Request("GET", "http://url-user:url-pass@example.com/")
    )
    assert from_url.headers["Authorization"] == _basic("url-user", "url-pass")
    bare = session.prepare_request(requests.Request("GET", "http://example.com/"))
    assert "Authorization" not in bare.headers


def test_web_session_rebuild_auth_strips_the_login_only_on_a_change_of_host():
    session = WebSession()
    original = _prepared("http://from.example/", auth=("u", "p"))

    redirected = _prepared("http://to.example/", auth=("u", "p"))
    session.rebuild_auth(redirected, _response(original))
    assert AUTHORIZATION not in redirected.headers

    same_host = _prepared("http://from.example/", auth=("u", "p"))
    session.rebuild_auth(same_host, _response(original))
    assert same_host.headers[AUTHORIZATION] == _basic("u", "p")
