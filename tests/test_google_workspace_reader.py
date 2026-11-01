"""The Google Workspace reader: Drive search and read, Calendar listing, with Google mocked."""

import io
import json
import urllib.error

from dreamference.chat.google_workspace_reader import (
    CALENDAR_SCOPE,
    DRIVE_SCOPE,
    MAX_DOWNLOAD_BYTES,
    MAX_EVENTS,
    MAX_RESULTS,
    MAX_TEXT_CHARACTERS,
    GoogleWorkspaceReader,
)

BOTH = [DRIVE_SCOPE, CALENDAR_SCOPE]


def account(email, scopes=None, token="tok"):
    return {
        "email": email,
        "access_token": token + email,
        "scopes": BOTH if scopes is None else scopes,
    }


class FakeResponse:
    def __init__(self, body, status=200):
        self.body, self.status = body, status

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False

    def read(self, size=-1):
        return self.body if size < 0 else self.body[:size]


def route(monkeypatch, handler):
    """Replaces `_get` with `handler(url, token, raw)`; records the calls."""
    calls = []

    def fake(cls, url, token, raw=False):
        calls.append(url)
        return handler(url, token, raw)

    monkeypatch.setattr(GoogleWorkspaceReader, "_get", classmethod(fake))
    return calls


def http_error(code, payload):
    return urllib.error.HTTPError("u", code, "x", {}, io.BytesIO(payload))


def test_get_parses_json_and_sends_the_bearer_token(monkeypatch):
    seen = {}

    def opener(request, timeout):
        seen["auth"] = request.get_header("Authorization")
        return FakeResponse(b'{"a": 1}')

    monkeypatch.setattr(GoogleWorkspaceReader, "opener", staticmethod(opener))
    assert GoogleWorkspaceReader._get("http://x", "T") == (200, {"a": 1})
    assert seen["auth"] == "Bearer T"


def test_get_empty_body_is_an_empty_object_and_raw_returns_bytes(monkeypatch):
    monkeypatch.setattr(
        GoogleWorkspaceReader, "opener", staticmethod(lambda r, timeout: FakeResponse(b""))
    )
    assert GoogleWorkspaceReader._get("http://x", "T") == (200, {})
    assert GoogleWorkspaceReader._get("http://x", "T", raw=True) == (200, b"")


def test_get_reads_at_most_the_download_cap_plus_one(monkeypatch):
    big = b"x" * (MAX_DOWNLOAD_BYTES + 50)
    monkeypatch.setattr(
        GoogleWorkspaceReader, "opener", staticmethod(lambda r, timeout: FakeResponse(big))
    )
    _, body = GoogleWorkspaceReader._get("http://x", "T", raw=True)
    assert len(body) == MAX_DOWNLOAD_BYTES + 1


def test_get_turns_http_errors_into_google_messages(monkeypatch):
    def opener(request, timeout):
        raise http_error(403, json.dumps({"error": {"message": "no scope"}}).encode())

    monkeypatch.setattr(GoogleWorkspaceReader, "opener", staticmethod(opener))
    assert GoogleWorkspaceReader._get("http://x", "T") == (403, {"error": "no scope"})


def test_get_http_error_with_unreadable_or_empty_body_falls_back_to_the_code(monkeypatch):
    bodies = iter([b"<html>", b""])

    def opener(request, timeout):
        raise http_error(502, next(bodies))

    monkeypatch.setattr(GoogleWorkspaceReader, "opener", staticmethod(opener))
    assert GoogleWorkspaceReader._get("http://x", "T") == (502, {"error": "HTTP 502"})
    assert GoogleWorkspaceReader._get("http://x", "T") == (502, {"error": "HTTP 502"})


def test_holding_keeps_order_and_tolerates_missing_scopes():
    accounts = [account("a"), account("b", scopes=[]), account("c", scopes=None)]
    accounts.append({"email": "d", "access_token": "t"})
    accounts[2]["scopes"] = None
    assert [a["email"] for a in GoogleWorkspaceReader.holding(accounts, DRIVE_SCOPE)] == ["a"]


def test_drive_query_escapes_quotes_and_backslashes():
    query = GoogleWorkspaceReader._drive_query("o'neil\\x")
    assert "name contains 'o\\'neil\\\\x'" in query
    assert query.endswith("and trashed = false")


def test_drive_search_without_a_drive_grant_is_refused():
    answer = GoogleWorkspaceReader.drive_search([account("a", scopes=[CALENDAR_SCOPE])], "x", 5)
    assert "No connected account has Google Drive access" in answer["error"]


def test_drive_search_merges_accounts_newest_first_and_clamps_the_limit(monkeypatch):
    def handler(url, token, raw):
        if token.endswith("a"):
            return 200, {
                "files": [
                    {
                        "id": "1",
                        "name": "old",
                        "mimeType": "t",
                        "modifiedTime": "2020-01-01",
                        "owners": [{"emailAddress": "o1"}, {"emailAddress": "o2"}],
                    },
                ]
            }
        return 200, {
            "files": [{"id": "2", "name": "new", "modifiedTime": "2020-02-01", "driveId": "D"}]
        }

    calls = route(monkeypatch, handler)
    answer = GoogleWorkspaceReader.drive_search([account("a"), account("b")], "q", 10_000)
    assert [f["name"] for f in answer["files"]] == ["new", "old"]
    assert answer["files"][0]["id"] == "b|2"
    assert answer["files"][0]["shared_drive"] == "D"
    assert answer["files"][1]["owner"] == "o1, o2"
    assert "errors" not in answer
    assert f"pageSize={MAX_RESULTS}" in calls[0]


def test_drive_search_truncates_to_the_limit_and_reports_failing_accounts(monkeypatch):
    def handler(url, token, raw):
        if token.endswith("bad"):
            return 500, {"error": "boom"}
        return 200, {
            "files": [{"id": str(i), "modifiedTime": f"2020-01-{i:02d}"} for i in range(1, 5)]
        }

    calls = route(monkeypatch, handler)
    answer = GoogleWorkspaceReader.drive_search([account("bad"), account("ok")], "q", 0)
    assert len(answer["files"]) == 1
    assert answer["files"][0]["owner"] == ""
    assert answer["errors"] == [{"account": "bad", "error": "boom"}]
    assert "pageSize=1" in calls[0]


def test_split_picks_the_named_account_or_the_first_holder():
    accounts = [account("a"), account("b"), account("c", scopes=[])]
    split = GoogleWorkspaceReader._split
    assert split(accounts, DRIVE_SCOPE, "b|f1")[0]["email"] == "b"
    assert split(accounts, DRIVE_SCOPE, "bare")[0]["email"] == "a"
    assert split(accounts, DRIVE_SCOPE, "c|f1") == (None, "f1")
    assert split([], DRIVE_SCOPE, "bare") == (None, "bare")


def test_drive_read_refuses_unknown_account_or_empty_id():
    expected = {"error": "No connected account with Drive access holds that file."}
    assert GoogleWorkspaceReader.drive_read([account("a")], "zz|f") == expected
    assert GoogleWorkspaceReader.drive_read([account("a")], "a|") == expected


def test_drive_read_reports_a_metadata_failure(monkeypatch):
    route(monkeypatch, lambda url, token, raw: (404, {"error": "gone"}))
    assert GoogleWorkspaceReader.drive_read([account("a")], "a|f") == {"error": "gone"}
    route(monkeypatch, lambda url, token, raw: (500, {}))
    assert GoogleWorkspaceReader.drive_read([account("a")], "a|f") == {"error": "HTTP 500"}


def test_drive_read_exports_a_google_doc_and_caps_the_text(monkeypatch):
    def handler(url, token, raw):
        if "/export?" in url:
            assert "mimeType=text%2Fplain" in url and raw
            return 200, ("é" * (MAX_TEXT_CHARACTERS + 5)).encode()
        return 200, {
            "name": "N",
            "mimeType": "application/vnd.google-apps.document",
            "modifiedTime": "M",
        }

    route(monkeypatch, handler)
    answer = GoogleWorkspaceReader.drive_read([account("a")], "a|f/1")
    assert len(answer["text"]) == MAX_TEXT_CHARACTERS
    assert (answer["id"], answer["account"], answer["name"], answer["modified"]) == (
        "a|f/1",
        "a",
        "N",
        "M",
    )


def test_drive_read_downloads_small_text_files_and_replaces_bad_bytes(monkeypatch):
    def handler(url, token, raw):
        if "alt=media" in url:
            return 200, b"hi \xff"
        return 200, {"name": "n", "mimeType": "text/markdown", "size": "10"}

    route(monkeypatch, handler)
    assert GoogleWorkspaceReader.drive_read([account("a")], "a|f")["text"] == "hi �"


def test_drive_read_refuses_binary_and_oversized_text(monkeypatch):
    route(monkeypatch, lambda u, t, r: (200, {"name": "pic", "mimeType": "image/png", "size": "5"}))
    answer = GoogleWorkspaceReader.drive_read([account("a")], "a|f")
    assert "pic is image/png (5 bytes)" in answer["error"]
    big = {"name": "log", "mimeType": "text/plain", "size": str(MAX_DOWNLOAD_BYTES + 1)}
    route(monkeypatch, lambda u, t, r: (200, big))
    assert "log is text/plain" in GoogleWorkspaceReader.drive_read([account("a")], "a|f")["error"]
    route(monkeypatch, lambda u, t, r: (200, {}))
    assert (
        "a binary file (? bytes)"
        in GoogleWorkspaceReader.drive_read([account("a")], "a|f")["error"]
    )


def test_drive_read_download_failure_uses_the_message_or_the_status(monkeypatch):
    def make(body):
        def handler(url, token, raw):
            if raw:
                return 403, body
            return 200, {"mimeType": "application/vnd.google-apps.spreadsheet"}

        return handler

    route(monkeypatch, make({"error": "denied"}))
    assert GoogleWorkspaceReader.drive_read([account("a")], "a|f") == {"error": "denied"}
    route(monkeypatch, make(b"raw"))
    assert GoogleWorkspaceReader.drive_read([account("a")], "a|f") == {"error": "HTTP 403"}


def test_calendars_lists_ids_and_reports_failure(monkeypatch):
    route(monkeypatch, lambda u, t, r: (200, {"items": [{"id": "p"}, {}, {"id": "q"}]}))
    assert GoogleWorkspaceReader._calendars(account("a")) == (["p", "q"], None)
    route(monkeypatch, lambda u, t, r: (401, {}))
    assert GoogleWorkspaceReader._calendars(account("a")) == ([], "HTTP 401")


def test_event_flattens_dates_and_truncates_the_description():
    item = {
        "id": "e",
        "summary": "S",
        "start": {"dateTime": "2020-01-01T10:00:00Z"},
        "end": {"date": "2020-01-02"},
        "organizer": {"email": "o"},
        "attendees": [{}, {}],
        "description": "d" * (MAX_TEXT_CHARACTERS + 1),
    }
    event = GoogleWorkspaceReader._event(account("a"), "cal", item)
    assert event["start"] == "2020-01-01T10:00:00Z" and event["end"] == "2020-01-02"
    assert (event["calendar"], event["organiser"], event["attendees"]) == ("a|cal", "o", 2)
    assert len(event["description"]) == MAX_TEXT_CHARACTERS
    bare = GoogleWorkspaceReader._event(account("a"), "cal", {})
    assert bare["start"] == "" and bare["attendees"] == 0 and "description" not in bare


def test_moment_orders_zones_naive_dates_and_garbage():
    moment = GoogleWorkspaceReader._moment
    assert moment("2020-01-01T10:00:00+02:00") == moment("2020-01-01T08:00:00Z")
    assert moment("2020-01-01") == moment("2020-01-01T00:00:00Z")
    assert moment("nonsense") < moment("1990-01-01")


def test_calendar_events_merge_sort_and_clamp(monkeypatch):
    def handler(url, token, raw):
        if "calendarList" in url:
            return 200, {"items": [{"id": "c1"}]}
        if token.endswith("a"):
            return 200, {"items": [{"id": "late", "start": {"dateTime": "2020-01-01T12:00:00Z"}}]}
        return 200, {"items": [{"id": "early", "start": {"dateTime": "2020-01-01T12:00:00+05:00"}}]}

    calls = route(monkeypatch, handler)
    answer = GoogleWorkspaceReader.calendar_events(
        [account("a"), account("b")], "s", "e", terms="lunch", limit=9999
    )
    assert [e["id"] for e in answer["events"]] == ["early", "late"]
    assert any(f"maxResults={MAX_EVENTS}" in c and "q=lunch" in c for c in calls)


def test_calendar_events_named_calendar_skips_the_list_and_other_accounts(monkeypatch):
    calls = route(monkeypatch, lambda u, t, r: (200, {"items": []}))
    answer = GoogleWorkspaceReader.calendar_events(
        [account("a"), account("b")], "s", "e", calendar="b|team@x"
    )
    assert answer == {"events": []}
    assert len(calls) == 1 and "team%40x/events" in calls[0]


def test_calendar_events_collects_list_and_event_errors(monkeypatch):
    def handler(url, token, raw):
        if "calendarList" in url:
            if token.endswith("a"):
                return 500, {"error": "list failed"}
            return 200, {"items": [{"id": "c"}]}
        return 404, {"error": "nope"}

    route(monkeypatch, handler)
    answer = GoogleWorkspaceReader.calendar_events([account("a"), account("b")], "s", "e")
    assert answer["events"] == []
    assert answer["errors"] == [
        {"account": "a", "error": "list failed"},
        {"account": "b", "error": "c: nope"},
    ]


def test_calendar_event_reads_one_event_or_reports(monkeypatch):
    calls = route(monkeypatch, lambda u, t, r: (200, {"id": "e", "summary": "S"}))
    event = GoogleWorkspaceReader.calendar_event([account("a")], "a|cal/x", "e 1")
    assert event["title"] == "S"
    assert "cal%2Fx/events/e%201" in calls[0]
    route(monkeypatch, lambda u, t, r: (404, {"error": "no event"}))
    assert GoogleWorkspaceReader.calendar_event([account("a")], "a|c", "e") == {"error": "no event"}
    assert "Calendar access holds" in GoogleWorkspaceReader.calendar_event([], "c", "e")["error"]
