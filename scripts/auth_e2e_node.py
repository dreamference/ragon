"""The node side of the end-to-end auth gate (`scripts/auth_e2e.sh`, specs/DREAMFERENCE_AUTH.md §17).

`auth_e2e.sh` copies this file into a private run folder on a node (mode 700) and runs one
subcommand per step with `python3 -I auth_e2e_node.py <command> <run folder>`. Secrets never
travel in argv: the test user's name and password sit in `user` and `pass` (mode 600) in the run
folder, an API key in `key`, the CA in `ca.pem`. Every subcommand prints what it checked, masks
anything that looks like a credential, and exits 0 only when every assertion ran and held; an
uncaught exception exits 1.

Subcommands: `ca`, `signin`, `device`, `key`, `revoke-key`, `sdk`, `forgejo`, `night`, `unauth`,
`refusals`, `pty`, `web`.
"""

from __future__ import annotations

import base64
import http.cookiejar
import json
import os
import re
import secrets
import signal
import ssl
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any

GATE = "https://localhost:8000"
WEB = "https://localhost:3100"
ACCOUNTS = "http://127.0.0.1:18100"
FORGEJO = "http://127.0.0.1:3200"
CLIENT_ID = "ragon-auth-e2e"
GATE_STATE = Path("~/.local/state/dreamference/model-gate").expanduser()
GATE_LABEL = "ragon.model-gate=1"
POLL_S = 5
DEVICE_WAIT_S = 120
HTTP_TIMEOUT_S = 30
BAD_MODEL = "ragon-auth-e2e-no-such-model"
REFUSED = "model gate: auth refused"
NO_CREDENTIAL = "(no credential, "
MASKS = (
    (re.compile(r"(?i)(bearer\s+)\S+"), r"\1***"),
    (re.compile(r"sk-ragon-[A-Za-z0-9_\-]+"), "sk-ragon-***"),
    (re.compile(r"eyJ[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+(\.[A-Za-z0-9_\-]*)?"), "eyJ***"),
    (
        re.compile(
            r'(?i)("?(?:access_token|refresh_token|id_token|code_verifier|password)"?\s*[:=]\s*"?)[^"\s,}]+'
        ),
        r"\1***",
    ),
)


class Failed(Exception):
    """An assertion of the gate did not hold."""


def mask(text: str) -> str:
    """Replaces anything that looks like a bearer token, an API key or a JWT."""
    for pattern, repl in MASKS:
        text = pattern.sub(repl, text)
    return text


def say(line: str) -> None:
    """Prints one masked line."""
    print(mask(line), flush=True)


def check(ok: bool, what: str) -> None:
    """Records one assertion; raises when it does not hold."""
    say(("ok   " if ok else "FAIL ") + what)
    if not ok:
        raise Failed(what)


def error_shape(body: Any) -> bool:
    """Whether a body has the vendor's error shape: `{"error": {"message", "type", "code"}}`."""
    err = body.get("error") if isinstance(body, dict) else None
    return (
        isinstance(err, dict)
        and all(isinstance(err.get(k), str) for k in ("message", "type"))
        and "code" in err
    )


def unauth_rows(text: str) -> int:
    """Sums the `N requests` column of `ragon-admin node user usage --unauthenticated` rows."""
    total = 0
    for line in text.splitlines():
        cols = line.split("\t")
        if len(cols) >= 5:  # noqa: PLR2004 - day, node, kind, source, requests
            found = re.match(r"\s*(\d+) requests", cols[4])
            total += int(found.group(1)) if found else 0
    return total


def tampered(token: str) -> str:
    """The token with one bit of its JWT signature flipped, same length and same shape.

    Spec §7 (specs/DREAMFERENCE_AUTH.md): a secondary verifies JWT signatures locally, and the
    signature alone never admits a request. Flipping the last character would not always change the
    signature: at a base64url padding boundary the last character carries unused bits. So the bit is
    flipped in the decoded bytes and encoded again. A token without a signature segment (not a JWT)
    gets its last character changed instead.
    """
    head, _, signature = token.rpartition(".")
    try:
        raw = bytearray(base64.urlsafe_b64decode(signature + "=" * (-len(signature) % 4)))
    except ValueError:
        raw = bytearray()
    if not head or not raw:
        return token[:-1] + ("A" if token[-1] != "A" else "B")
    raw[-1] ^= 0x01
    flipped = base64.urlsafe_b64encode(bytes(raw)).decode("ascii").rstrip("=")
    return f"{head}.{flipped}"


class Run:
    """One run folder on a node."""

    def __init__(self, folder: str) -> None:
        self.dir = Path(folder)
        if not self.dir.is_dir():
            raise Failed(f"no run folder {folder}")
        self.ctx = (
            ssl.create_default_context(cafile=str(self.dir / "ca.pem"))
            if (self.dir / "ca.pem").exists()
            else None
        )

    def secret(self, name: str) -> str:
        """A secret file's content (never printed)."""
        return (self.dir / name).read_text().strip()

    def write_secret(self, name: str, value: str) -> None:
        """Writes a mode-600 file."""
        path = self.dir / name
        path.touch(mode=0o600)
        path.write_text(value)

    @property
    def user(self) -> str:
        return self.secret("user")

    def http(
        self,
        method: str,
        url: str,
        body: Any = None,
        bearer: str | None = None,
        form: bool = False,
        headers: dict[str, str] | None = None,
        opener: urllib.request.OpenerDirector | None = None,
    ) -> tuple[int, Any, Any]:
        """One request (headers: a case-insensitive HTTPMessage); answers status, parsed JSON (or text) and headers. Never raises on 4xx."""
        data = None
        hdrs = dict(headers or {})
        if body is not None:
            data = urllib.parse.urlencode(body).encode() if form else json.dumps(body).encode()
            hdrs["Content-Type"] = (
                "application/x-www-form-urlencoded" if form else "application/json"
            )
        if bearer:
            hdrs["Authorization"] = f"Bearer {bearer}"
        if method != "GET" and url.startswith(WEB):
            # ragon web refuses a change without its own Origin (server.rs CSRF guard), as a
            # browser always sends one.
            hdrs.setdefault("Origin", WEB)
        req = urllib.request.Request(url, data=data, method=method, headers=hdrs)
        try:
            if opener is not None:
                resp = opener.open(req, timeout=HTTP_TIMEOUT_S)
            else:
                resp = urllib.request.urlopen(
                    req,
                    timeout=HTTP_TIMEOUT_S,
                    context=self.ctx if url.startswith("https") else None,
                )
            status, raw, rh = resp.status, resp.read(), resp.headers
        except urllib.error.HTTPError as err:
            status, raw, rh = err.code, err.read(), err.headers
        text = raw.decode("utf-8", "replace")
        try:
            return status, json.loads(text), rh
        except ValueError:
            return status, text, rh

    def password_grant(self, user: str | None = None, password: str | None = None) -> dict:
        """The token answer of the password grant."""
        status, body, _ = self.http(
            "POST",
            f"{GATE}/oauth/token",
            {
                "grant_type": "password",
                "username": user or self.user,
                "password": self.secret("pass") if password is None else password,
                "client_id": CLIENT_ID,
            },
            form=True,
        )
        check(
            status == 200 and isinstance(body, dict) and "access_token" in body,
            f"password grant for the test user answers a token ({status})",
        )
        return body

    def password_token(self, user: str | None = None, password: str | None = None) -> str:
        """An access token by the password grant."""
        return str(self.password_grant(user, password)["access_token"])

    def refresh(self, token: str) -> tuple[int, object]:
        """POST the refresh grant for one refresh token."""
        status, body, _ = self.http(
            "POST",
            f"{GATE}/oauth/token",
            {"grant_type": "refresh_token", "refresh_token": token, "client_id": CLIENT_ID},
            form=True,
        )
        return status, body

    def me(self, token: str, expect: str, what: str) -> None:
        """`/ragon/v1/users/me` names `expect`; a tampered token gets 401 on a gated route."""
        status, body, _ = self.http("GET", f"{GATE}/ragon/v1/users/me", bearer=token)
        name = body.get("username") if isinstance(body, dict) else None
        check(
            status == 200 and name == expect,
            f"{what}: /ragon/v1/users/me names the test user ({status})",
        )
        status, _, _ = self.http("GET", f"{GATE}/v1/models", bearer=token)
        check(status == 200, f"{what}: /v1/models with the token answers 200")
        status, _, _ = self.http(
            "POST",
            f"{GATE}/v1/chat/completions",
            {"model": BAD_MODEL, "messages": []},
            bearer=tampered(token),
        )
        check(status == 401, f"{what}: a tampered token is refused with 401 ({status})")

    def env(self, home: str) -> dict[str, str]:
        """The environment of a launcher that keeps its session in the run folder."""
        path = self.dir / home
        path.mkdir(mode=0o700, exist_ok=True)
        return {**os.environ, "CODEX_HOME": str(path), "RAGON_HOME": str(path)}


# -- the subcommands ------------------------------------------------------------------------------


def cmd_ca(run: Run) -> None:
    """Fetches the node CA over loopback (only the loopback hop is trusted unverified)."""
    raw = urllib.request.urlopen(
        f"{GATE}/ragon/v1/ca.pem", timeout=HTTP_TIMEOUT_S, context=ssl._create_unverified_context()
    ).read()
    check(b"BEGIN CERTIFICATE" in raw, "the gate serves the node CA at /ragon/v1/ca.pem")
    (run.dir / "ca.pem").write_bytes(raw)
    run.ctx = ssl.create_default_context(cafile=str(run.dir / "ca.pem"))
    status, _, _ = run.http("GET", f"{GATE}/v1/models")
    check(status == 200, "the gate's certificate verifies against the node CA for localhost")


def cmd_signin(run: Run) -> None:
    """`ragon login --username U --password-stdin` into the run's home; the session names U."""
    proc = subprocess.run(
        ["ragon", "login", "--username", run.user, "--password-stdin"],
        input=run.secret("pass") + "\n",
        capture_output=True,
        text=True,
        env=run.env("home"),
        timeout=120,
        check=False,
    )
    say(proc.stdout + proc.stderr)
    check(proc.returncode == 0, f"ragon login --password-stdin exits 0 ({proc.returncode})")
    check(
        (run.dir / "home" / "auth.json").exists(), "the session is written to the run's auth.json"
    )
    out = subprocess.run(
        ["ragon", "login", "status"],
        capture_output=True,
        text=True,
        env=run.env("home"),
        timeout=60,
        check=False,
    )
    say(out.stdout + out.stderr)
    check(out.returncode == 0 and run.user in out.stdout, "ragon login status names the test user")


def cmd_logout(run: Run) -> None:
    """`ragon logout --all` revokes every session of the user, not only the run's own.

    A second session minted by the password grant is the control: its refresh
    answers 200 before the logout (so the request form is right) and
    invalid_grant after it, as does the run session's refresh token.
    """
    auth = json.loads((run.dir / "home" / "auth.json").read_text())
    found = re.search(r'"refresh_token"\s*:\s*"([^"]+)"', json.dumps(auth))
    check(found is not None, "the run's auth.json holds a refresh token")
    other = run.password_grant()
    check("refresh_token" in other, "the password grant answers a refresh token")
    status, body = run.refresh(str(other["refresh_token"]))
    check(
        status == 200 and isinstance(body, dict) and "access_token" in body,
        f"control: a live session's refresh answers 200 before logout ({status})",
    )
    second = str(body.get("refresh_token") or other["refresh_token"])
    out = subprocess.run(
        ["ragon", "logout", "--all"],
        capture_output=True,
        text=True,
        env=run.env("home"),
        timeout=60,
        check=False,
    )
    say(out.stdout + out.stderr)
    check(out.returncode == 0, "ragon logout --all exits 0")
    for name, token in (("the run session", found.group(1)), ("the second session", second)):
        status, body = run.refresh(token)
        refused = isinstance(body, dict) and body.get("error") == "invalid_grant"
        check(
            status in (400, 401) and refused,
            f"{name}'s refresh token is refused with invalid_grant after logout --all ({status})",
        )


def approve(run: Run, user_code: str) -> None:
    """The signed-in test user approves a code through ragon web's device page API."""
    status, body, _ = run.http(
        "POST", f"{WEB}/api/auth/device", {"user_code": user_code}, bearer=run.password_token()
    )
    check(status == 200, f"ragon web /api/auth/device approves the code ({status} {body})")


def cmd_device(run: Run) -> None:
    """Both device-auth shapes to completion; the issued tokens name the test user."""
    # Codex's shape: user code, polled with the device auth id, exchanged as an authorization code.
    status, start, _ = run.http(
        "POST", f"{ACCOUNTS}/api/accounts/deviceauth/usercode", {"client_id": CLIENT_ID}
    )
    check(
        status == 200 and isinstance(start, dict) and "device_auth_id" in start,
        f"user-code shape: a code is issued ({status})",
    )
    approve(run, str(start["user_code"]))
    grant = poll(
        run,
        f"{ACCOUNTS}/api/accounts/deviceauth/token",
        {"device_auth_id": start["device_auth_id"], "user_code": start["user_code"]},
        False,
        "authorization_code",
    )
    _, oidc, _ = run.http("GET", f"{GATE}/.well-known/openid-configuration")
    issuer = str(oidc.get("issuer", "")).rstrip("/") if isinstance(oidc, dict) else ""
    status, tokens, _ = run.http(
        "POST",
        f"{GATE}/oauth/token",
        {
            "grant_type": "authorization_code",
            "code": grant["authorization_code"],
            "code_verifier": grant["code_verifier"],
            "redirect_uri": f"{issuer}/deviceauth/callback",
            "client_id": CLIENT_ID,
        },
        form=True,
    )
    check(
        status == 200 and "access_token" in tokens,
        f"user-code shape: the code exchanges for tokens ({status})",
    )
    run.me(str(tokens["access_token"]), run.user, "user-code shape")
    # RFC 8628: the browser opens verification_uri_complete, which carries the code.
    status, start, _ = run.http(
        "POST", f"{ACCOUNTS}/oauth/device/code", {"client_id": CLIENT_ID}, form=True
    )
    check(
        status == 200 and isinstance(start, dict) and "verification_uri_complete" in start,
        f"complete-URI shape: a code is issued ({status})",
    )
    query = urllib.parse.parse_qs(
        urllib.parse.urlparse(str(start["verification_uri_complete"])).query
    )
    status, page, _ = run.http(
        "GET", f"{WEB}/device?" + urllib.parse.urlencode({"user_code": query["user_code"][0]})
    )
    check(
        status == 200 and isinstance(page, str) and 'id="do-approve"' in page,
        "complete-URI shape: the verification page answers 200 with the approve control",
    )
    approve(run, query["user_code"][0])
    tokens = poll(
        run,
        f"{GATE}/oauth/token",
        {
            "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
            "device_code": start["device_code"],
            "client_id": CLIENT_ID,
        },
        True,
        "access_token",
    )
    run.me(str(tokens["access_token"]), run.user, "complete-URI shape")


def poll(run: Run, url: str, body: dict[str, Any], form: bool, field: str) -> dict[str, Any]:
    """Polls a device token endpoint until it answers `field` or the wait runs out."""
    deadline = time.monotonic() + DEVICE_WAIT_S
    while time.monotonic() < deadline:
        status, answer, _ = run.http("POST", url, body, form=form)
        if status == 200 and isinstance(answer, dict) and field in answer:
            say(f"ok   the polled endpoint answers {field}")
            return answer
        time.sleep(POLL_S)
    raise Failed(f"the device poll at {url} never answered {field}")


def admin(*args: str) -> subprocess.CompletedProcess[str]:
    """One `ragon-admin` call, its output masked on print."""
    proc = subprocess.run(
        ["ragon-admin", *args], capture_output=True, text=True, timeout=120, check=False
    )
    say(proc.stdout + proc.stderr)
    return proc


def cmd_key(run: Run) -> None:
    """Creates the test user's API key; the key goes to `key` (600), never to the output."""
    proc = admin("node", "user", "key", run.user, "--name", "e2e-sdk")
    found = re.search(r"sk-ragon-[A-Za-z0-9_\-]+", proc.stdout)
    check(proc.returncode == 0 and found is not None, "ragon-admin node user key prints a key once")
    run.write_secret("key", found.group(0))


def cmd_revoke_key(run: Run) -> None:
    """Revokes the test user's API key."""
    check(
        admin("node", "user", "key", run.user, "--revoke", "e2e-sdk").returncode == 0,
        "the key is revoked",
    )


FRESH_KEY = "e2e-revoke"


def cmd_mint_fresh(run: Run) -> None:
    """E13 on the primary: mints a fresh key for the test user.

    The key is never printed; it goes to a mode-600 file the secondary reads (`key-accepted`).
    """
    proc = admin("node", "user", "key", run.user, "--name", FRESH_KEY)
    found = re.search(r"sk-ragon-[A-Za-z0-9_\-]+", proc.stdout)
    check(
        proc.returncode == 0 and found is not None, "ragon-admin node user key prints a fresh key"
    )
    run.write_secret("revoke-key", found.group(0))


def cmd_revoke_fresh(run: Run) -> None:
    """E13 on the primary: revokes the fresh key minted by `mint-fresh`."""
    check(
        admin("node", "user", "key", run.user, "--revoke", FRESH_KEY).returncode == 0,
        "the fresh key is revoked on the primary",
    )


def keyed_request(run: Run) -> tuple[int, Any]:
    """One 1-token chat completion with the fresh key: a route that checks the key (/v1/models is open)."""
    _, models, _ = run.http("GET", f"{GATE}/v1/models")
    model = models["data"][0]["id"]
    body = {"model": model, "max_tokens": 1, "messages": [{"role": "user", "content": "hi"}]}
    status, answer, _ = run.http(
        "POST", f"{GATE}/v1/chat/completions", body, bearer=run.secret("revoke-key")
    )
    return status, answer


def cmd_key_accepted(run: Run) -> None:
    """E13 on the secondary: the fresh key is accepted before its revocation (200 on the gate)."""
    status, _ = keyed_request(run)
    check(
        status == 200,  # noqa: PLR2004
        f"the fresh key is accepted through this node before revocation ({status})",
    )


def cmd_key_refused(run: Run) -> None:
    """E13 on the secondary, after the revocation and one cache TTL: the key gets 401."""
    status, answer = keyed_request(run)
    code = (
        answer.get("error", {}).get("code")
        if isinstance(answer, dict) and isinstance(answer.get("error"), dict)
        else None
    )
    check(
        status == 401 and error_shape(answer) and code == "invalid_api_key",  # noqa: PLR2004  # noqa: PLR2004
        f"the revoked key is refused with 401 through this node ({status}, {code})",
    )


def cmd_sdk(run: Run) -> None:
    """The vendor's SDK against the node: list, one respond, one stream, one error."""
    import httpx
    import openai

    client = openai.OpenAI(
        base_url=f"{GATE}/v1",
        api_key=run.secret("key"),
        http_client=httpx.Client(verify=str(run.dir / "ca.pem")),
        max_retries=0,
    )
    models = [m.id for m in client.models.list()]
    check(bool(models), f"models.list answers {len(models)} model(s)")
    # 256, not 16: this model spends its output budget on reasoning first, so 16 tokens came back
    # empty (E6 try 2: "responses.create answers text" failed); 256 leaves room for the answer.
    reply = client.responses.create(
        model=models[0], input="Answer with the word ok.", max_output_tokens=256
    )
    check(bool(reply.output_text.strip()), "responses.create answers text")
    kinds = [
        event.type
        for event in client.responses.create(
            model=models[0], input="Answer with the word ok.", max_output_tokens=256, stream=True
        )
    ]
    check(
        "response.output_text.delta" in kinds and kinds[-1] == "response.completed",
        f"the stream sends deltas and ends completed ({len(kinds)} events)",
    )
    try:
        client.responses.create(model=BAD_MODEL, input="ok", max_output_tokens=4)
        check(False, "a wrong model is refused")
    except openai.APIStatusError as err:
        check(
            400 <= err.status_code < 500 and error_shape(err.response.json()),
            f"a wrong model gets {err.status_code} in the error shape",
        )


def cmd_forgejo(run: Run) -> None:
    """Forgejo sign-in through the accounts service, then Forgejo's API as the test user."""

    class AnyPath(http.cookiejar.DefaultCookiePolicy):
        # Forgejo serves at its root, but its cookies carry ROOT_URL's /forge path.
        def path_return_ok(self, path: str, request: Any) -> bool:
            return True

        def return_ok_path(self, cookie: Any, request: Any) -> bool:
            return True

    jar = http.cookiejar.CookieJar(AnyPath())
    opener = urllib.request.build_opener(
        urllib.request.HTTPCookieProcessor(jar),
        NoRedirect(),
        urllib.request.HTTPSHandler(context=run.ctx),
    )
    status, _, headers = run.http("GET", f"{FORGEJO}/user/oauth2/ragon", opener=opener)
    location = headers.get("Location", "")
    check(
        status in (302, 303, 307) and "/oauth/authorize" in location,
        f"Forgejo sends the browser to the accounts authorize page ({status})",
    )
    params = dict(urllib.parse.parse_qsl(urllib.parse.urlparse(location).query))
    status, body, _ = run.http(
        "POST", f"{WEB}/api/auth/authorize", params, bearer=run.password_token()
    )
    target = body.get("redirect_to", "") if isinstance(body, dict) else ""
    check(
        status == 200 and "/user/oauth2/ragon/callback" in target,
        f"the signed-in user allows Forgejo; the answer redirects to the callback ({status})",
    )
    callback = urllib.parse.urlparse(target)
    status, _, headers = run.http(
        "GET", f"{FORGEJO}{callback.path.removeprefix('/forge')}?{callback.query}", opener=opener
    )
    check(
        status in (302, 303) and "/user/login" not in headers.get("Location", ""),
        f"Forgejo accepts the callback and opens a session ({status})",
    )
    # Forgejo's API refuses a web session cookie ("Only signed in user is allowed to call APIs"),
    # so the signed-in dashboard, whose title names the user, is the check.
    status, page, _ = run.http("GET", f"{FORGEJO}/", opener=opener)
    title = (
        str(page).split("<title>", 1)[-1].split("</title>", 1)[0] if "<title>" in str(page) else ""
    )
    check(
        status == 200 and title.lower().startswith(f"{run.user.lower()} - dashboard"),
        f"Forgejo's dashboard, with the session, names the test user ({status})",
    )


def cmd_night(run: Run) -> None:
    """Queues a Night Shift task as the test user in the run's own queue; the record names U."""
    repo = run.dir / "repo"
    repo.mkdir(exist_ok=True)
    env = run.env("home")
    for args in (
        ["init", "-q"],
        [
            "-c",
            "user.name=e2e",
            "-c",
            "user.email=e2e@invalid",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "e2e",
        ],
    ):
        subprocess.run(["git", *args], cwd=repo, check=True, env=env, timeout=60)
    out = subprocess.run(
        ["ragon", "night", "add", "Print ok (auth e2e)"],
        cwd=repo,
        capture_output=True,
        text=True,
        env=env,
        timeout=120,
        check=False,
    )
    say(out.stdout + out.stderr)
    check(out.returncode == 0, "ragon night add exits 0")
    records = sorted((run.dir / "home" / "night").rglob("*.json"))
    owners = [json.loads(p.read_text()).get("user") for p in records]
    check(
        run.user in owners, f"the queued record's user is the test user ({len(records)} record(s))"
    )
    task_id = next(p.stem for p, o in zip(records, owners) if o == run.user)
    show = subprocess.run(
        ["ragon", "night", "show", task_id],
        cwd=repo,
        capture_output=True,
        text=True,
        env=env,
        timeout=60,
        check=False,
    )
    say(show.stdout)
    check(f"Queued by: {run.user}" in show.stdout, "ragon night show names the test user")
    subprocess.run(
        ["ragon", "night", "drop", task_id],
        cwd=repo,
        capture_output=True,
        env=env,
        timeout=60,
        check=False,
    )


def gate_refusals(needle: str = REFUSED) -> int:
    """The number of `auth refused` lines (or another needle) in the gate container's log."""
    ids = subprocess.run(
        ["docker", "ps", "-q", "--filter", f"label={GATE_LABEL}"],
        capture_output=True,
        text=True,
        timeout=30,
        check=True,
    ).stdout.split()
    if len(ids) != 1:
        raise Failed(f"expected one gate container, found {len(ids)}")
    logs = subprocess.run(
        ["docker", "logs", ids[0]], capture_output=True, text=True, timeout=60, check=True
    )
    return (logs.stdout + logs.stderr).count(needle)


def cmd_refusals(run: Run) -> None:
    """Prints `refusals=N` from the gate log (the counter E14 reads before and after)."""
    say(f"refusals={gate_refusals()}")


def cmd_unauth(run: Run) -> None:
    """A bad credential and no credential on a gated route; prints the refusal count's rise."""
    try:
        enforce = json.loads((GATE_STATE / "auth.json").read_text()).get("enforce") is True
    except (OSError, ValueError):
        enforce = False
    before = gate_refusals()
    served_before = gate_refusals(NO_CREDENTIAL)
    body = {"model": BAD_MODEL, "messages": [{"role": "user", "content": "ok"}], "max_tokens": 1}
    status, answer, headers = run.http(
        "POST", f"{GATE}/v1/chat/completions", body, bearer="sk-ragon-e2e-" + secrets.token_hex(8)
    )
    code = (
        answer.get("error", {}).get("code")
        if isinstance(answer, dict) and isinstance(answer.get("error"), dict)
        else None
    )
    check(
        status == 401 and code == "invalid_api_key" and error_shape(answer),
        f"a bad credential gets 401 invalid_api_key in the error shape ({status})",
    )
    check(
        headers.get("WWW-Authenticate", "").startswith("Bearer"),
        "the refusal carries WWW-Authenticate: Bearer",
    )
    status, answer, _ = run.http("POST", f"{GATE}/v1/chat/completions", body)
    if enforce:
        check(
            status == 401 and error_shape(answer),
            f"auth_enforce on: no credential gets 401 ({status})",
        )
    else:
        check(
            status != 401,
            f"auth_enforce off (0.9.9 default): no credential is served, not refused ({status})",
        )
        served = gate_refusals(NO_CREDENTIAL) - served_before
        check(served >= 1, f"the gate counted the request with no credential ({served} new lines)")
    rise = gate_refusals() - before
    expected = 2 if enforce else 1
    check(rise >= expected, f"the gate's refusal count rose by {rise} (at least {expected})")
    say(f"refusals_rise={rise}")


def cmd_pty(run: Run) -> None:
    """E1 through a pty: the sign-in screen, then a new account with an empty password."""
    import pty
    import select

    def screen(answers: list[tuple[str, str]], expect: str, home: str) -> str:
        """Runs `ragon login` in a pty and answers each prompt once it shows (line prompts)."""
        pid, fd = pty.fork()
        if pid == 0:
            os.execvpe(
                "ragon",
                ["ragon", "login"],
                {**run.env(home), "TERM": "xterm-256color", "COLUMNS": "100", "LINES": "30"},
            )
        seen = b""
        mark = 0
        deadline = time.monotonic() + 90
        pending = list(answers)
        ended = False
        while time.monotonic() < deadline:
            ready, _, _ = select.select([fd], [], [], 1.0)
            if ready:
                try:
                    chunk = os.read(fd, 65536)
                except OSError:
                    ended = True
                    break
                if not chunk:
                    ended = True
                    break
                seen += chunk
            if pending:
                label = pending[0][0].encode()
                found = seen.find(label, mark)
                if found >= 0:
                    mark = found + len(label)
                    os.write(fd, (pending.pop(0)[1] + "\r").encode())
        if not ended:
            os.kill(pid, signal.SIGKILL)
            os.waitpid(pid, 0)
        text = re.sub(r"\x1b\[[0-9;?]*[A-Za-z]", "", seen.decode("utf-8", "replace"))
        for line in text.splitlines()[-20:]:
            say(f"  {home}| {line}")
        check(ended, f"the screen ends within 90 s ({home})")
        _, status = os.waitpid(pid, 0)
        check(not pending, f"every prompt was asked ({home}; left {[p for p, _ in pending]})")
        check(os.waitstatus_to_exitcode(status) == 0, f"the screen exits 0 ({home})")
        check(expect in text, f"the screen shows {expect!r}")
        return text

    screen(
        [("Choose 1-4: ", "1"), ("Username: ", run.user), ("Password: ", run.secret("pass"))],
        "Sign in to",
        "pty-signin",
    )
    check((run.dir / "pty-signin" / "auth.json").exists(), "the screen's sign-in wrote a session")
    created = run.user + "-np"
    run.write_secret("created", created)
    screen(
        [
            ("Choose 1-4: ", "2"),
            ("Username: ", created),
            ("Password: ", ""),
            ("Confirm password: ", ""),
            ("Continue without a password? [y/N] ", "y"),
        ],
        "Continue without a password?",
        "pty-create",
    )
    check(
        (run.dir / "pty-create" / "auth.json").exists(),
        "the empty-password account wrote a session",
    )
    run.me(run.password_token(created, ""), created, "empty-password account")


class NoRedirect(urllib.request.HTTPRedirectHandler):
    """Keeps a redirect as the answer, so its Location and cookies can be read."""

    def redirect_request(self, *args: Any, **kwargs: Any) -> None:
        return None


def session_cookie(run: Run) -> str:
    """This machine's sign-in to ragon web: the one-time link `ragon web open` prints, then the
    `ragon_session` cookie its redirect sets (never printed). Spec: DREAMFERENCE_RAGON_ASK.md
    ("the URL is exchanged for a session cookie"); `ragon_device` is for a paired device."""
    proc = subprocess.run(
        ["ragon", "web", "open", "--print-url"], capture_output=True, text=True, timeout=60
    )
    check(proc.returncode == 0, f"ragon web open --print-url exits 0 ({proc.returncode})")
    link = urllib.parse.urlparse(proc.stdout.strip())
    opener = urllib.request.build_opener(NoRedirect(), urllib.request.HTTPSHandler(context=run.ctx))
    status, _, headers = run.http("GET", f"{WEB}{link.path}?{link.query}", opener=opener)
    pair = headers.get("Set-Cookie", "").split(";")[0]
    check(
        status in (302, 303) and pair.startswith("ragon_session="),
        f"the one-time link sets the sign-in cookie ({status})",
    )
    return pair


def cmd_web(run: Run) -> None:
    """E8 over HTTP: the sign-in page, its login API, the Account page and the API it calls."""
    for path in ("/signin", "/signin.js"):
        status, _, _ = run.http("GET", WEB + path)
        check(status == 200, f"ragon web {path} answers 200")
    status, _, _ = run.http("GET", f"{WEB}/account")
    check(status == 401, f"ragon web /account answers 401 without the sign-in cookie ({status})")
    cookie = session_cookie(run)
    for path in ("/account", "/account.js"):
        status, _, _ = run.http("GET", WEB + path, headers={"Cookie": cookie})
        check(status == 200, f"ragon web {path} answers 200 with the sign-in cookie ({status})")
    status, body, _ = run.http(
        "POST",
        f"{WEB}/api/auth/login",
        {"username": run.user, "password": "wrong-" + secrets.token_hex(4), "create": False},
    )
    check(status in (400, 401, 403), f"the login form refuses a wrong password ({status})")
    status, body, _ = run.http(
        "POST",
        f"{WEB}/api/auth/login",
        {"username": run.user, "password": run.secret("pass"), "create": False},
    )
    token = body.get("access_token") if isinstance(body, dict) else None
    check(status == 200 and bool(token), f"the login form signs the test user in ({status})")
    status, me, _ = run.http(
        "GET", f"{WEB}/ragon/v1/users/me", bearer=str(token), headers={"Cookie": cookie}
    )
    check(
        status == 200 and isinstance(me, dict) and me.get("username") == run.user,
        "the Account page's /ragon/v1/users/me names the test user",
    )
    status, _, _ = run.http(
        "GET", f"{WEB}/ragon/v1/users/me/keys", bearer=str(token), headers={"Cookie": cookie}
    )
    check(status == 200, "the Account page's key list answers 200")
    status, _, _ = run.http("GET", f"{WEB}/ragon/v1/users/me", bearer=tampered(str(token)))
    check(status == 401, f"the Account API refuses a tampered token ({status})")


COMMANDS = {
    "ca": cmd_ca,
    "signin": cmd_signin,
    "logout": cmd_logout,
    "device": cmd_device,
    "key": cmd_key,
    "revoke-key": cmd_revoke_key,
    "mint-fresh": cmd_mint_fresh,
    "revoke-fresh": cmd_revoke_fresh,
    "key-accepted": cmd_key_accepted,
    "key-refused": cmd_key_refused,
    "sdk": cmd_sdk,
    "forgejo": cmd_forgejo,
    "night": cmd_night,
    "unauth": cmd_unauth,
    "refusals": cmd_refusals,
    "pty": cmd_pty,
    "web": cmd_web,
}


def main(argv: list[str]) -> int:
    """Runs one subcommand; 0 only when every assertion held."""
    if len(argv) != 3 or argv[1] not in COMMANDS:  # noqa: PLR2004 - program, command, folder
        print(f"usage: auth_e2e_node.py {{{'|'.join(COMMANDS)}}} RUN_DIR", file=sys.stderr)
        return 2
    try:
        COMMANDS[argv[1]](Run(argv[2]))
    except Failed as err:
        say(f"FAILED: {err}")
        return 1
    except Exception as err:  # noqa: BLE001 - any surprise is a red step
        say(f"FAILED: {type(err).__name__}: {err}")
        return 1
    say("PASSED")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
