"""Tests for `scripts/search_eval.py` and its query set; fixtures only, no network."""

import importlib.util
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("search_eval", ROOT / "scripts" / "search_eval.py")
assert spec and spec.loader
se = importlib.util.module_from_spec(spec)
spec.loader.exec_module(se)

CLASSES = {"documentation", "error message", "release facts", "navigational"}


def test_the_set_is_about_two_hundred_queries_in_four_classes():
    queries = se.load_set(se.DEFAULT_SET)
    assert 180 <= len(queries) <= 220
    assert {q["class"] for q in queries} == CLASSES
    for cls in CLASSES:
        assert sum(q["class"] == cls for q in queries) >= 25
    assert len({q["query"] for q in queries}) == len(queries)


def test_the_set_holds_no_scheme_or_wildcard_in_accept_entries():
    for q in se.load_set(se.DEFAULT_SET):
        for entry in q["accept"]:
            assert not re.search(r"[*\s]|://", entry), entry


def test_accepts_matches_host_subdomain_and_prefix():
    assert se.accepts(["docs.python.org"], "https://docs.python.org/3/library/x.html")
    assert se.accepts(["python.org"], "https://www.python.org/downloads/")
    assert se.accepts(["python.org"], "https://docs.python.org/3/")
    assert not se.accepts(["python.org"], "https://notpython.org/")
    assert not se.accepts(["python.org"], "https://python.org.evil.example/")
    assert se.accepts(["pypi.org/project/requests"], "https://pypi.org/project/requests/")
    assert se.accepts(["pypi.org/project/requests"], "https://pypi.org/project/requests/2.0/")
    assert not se.accepts(
        ["pypi.org/project/requests"], "https://pypi.org/project/requests-oauthlib/"
    )


QUERIES = [
    {"id": "a", "class": "documentation", "query": "qa", "accept": ["docs.python.org"]},
    {"id": "b", "class": "documentation", "query": "qb", "accept": ["docs.rs"]},
    {"id": "c", "class": "navigational", "query": "qc", "accept": ["github.com"]},
    {"id": "d", "class": "navigational", "query": "qd", "accept": ["go.dev"]},
]


def _write(path, lines):
    path.write_text("\n".join(json.dumps(x) for x in lines) + "\n")


def test_score_counts_hits_at_one_and_five_by_class(tmp_path):
    f = tmp_path / "r.jsonl"
    _write(
        f,
        [
            {"id": "a", "results": [{"url": "https://docs.python.org/3/"}]},
            {
                "query": "qb",
                "results": [{"url": "https://x.example/"}, {"url": "https://docs.rs/serde"}],
            },
            {
                "id": "c",
                "results": [{"url": "https://example.com/"}] * 6
                + [{"url": "https://github.com/x"}],
            },
            {"id": "d", "error": "exit 1", "results": []},
        ],
    )
    result = se.score(QUERIES, se.load_results(f, QUERIES))
    doc = result["by_class"]["documentation"]
    assert (doc["hit_at_1"], doc["hit_at_5"], doc["rate_at_5"]) == (1, 2, 1.0)
    nav = result["by_class"]["navigational"]
    assert (nav["hit_at_1"], nav["hit_at_5"]) == (0, 0)
    assert result["overall"]["hit_at_5"] == 2 and result["overall"]["rate_at_1"] == 0.25
    assert result["failed"] == ["d"] and result["misses"] == ["c"]


def test_a_query_with_no_saved_result_is_a_miss_and_listed(tmp_path):
    f = tmp_path / "r.jsonl"
    _write(f, [{"id": "a", "results": [{"url": "https://docs.python.org/"}]}])
    result = se.score(QUERIES, se.load_results(f, QUERIES))
    assert result["missing"] == ["b", "c", "d"]
    assert result["overall"]["rate_at_5"] == 0.25
    assert "missing 3" in se.render(result)


def test_main_scores_a_saved_file_offline(tmp_path, capsys):
    q = tmp_path / "q.json"
    q.write_text(json.dumps({"queries": QUERIES}))
    f = tmp_path / "r.jsonl"
    _write(f, [{"id": "a", "results": [{"url": "https://docs.python.org/"}]}])
    assert se.main(["score", "--set", str(q), "--results", str(f), "--json"]) == 0
    assert json.loads(capsys.readouterr().out)["overall"]["hit_at_1"] == 1


def test_live_mode_runs_the_binary_and_saves_results(tmp_path, monkeypatch, capsys):
    q = tmp_path / "q.json"
    q.write_text(json.dumps({"queries": QUERIES[:2]}))
    calls = []

    def fake(binary, query):
        calls.append((binary, query))
        return {"results": [{"url": "https://docs.python.org/3/"}]}

    monkeypatch.setattr(se, "run_search", fake)
    out = tmp_path / "out.jsonl"
    assert se.main(["live", "--set", str(q), "--out", str(out), "--delay", "0"]) == 0
    assert [c[1] for c in calls] == ["qa", "qb"]
    assert len(out.read_text().splitlines()) == 2
    assert "documentation" in capsys.readouterr().out
