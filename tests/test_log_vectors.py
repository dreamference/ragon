"""The logging vectors (LOGGING §3.3, §5.1): their shape, and every case against the line grammar.

The Rust side reads the same files (`ragon-rs/src/log_vectors.rs`); the Python writer's own test
(`test_logging_writer.py`) runs the cases byte for byte.
"""

import json
import re
from pathlib import Path
from typing import Final

REPO: Final[Path] = Path(__file__).resolve().parent.parent
VECTORS: Final[Path] = REPO / "ragon-rs" / "vectors" / "logging"
RUST: Final[Path] = REPO / "ragon-rs" / "src" / "log_vectors.rs"
TS: Final[str] = "2026-01-01T00:00:00.000Z"


def _cases() -> list[tuple[str, dict]]:
    return [
        (path.name, case)
        for path in sorted((VECTORS / "redact").glob("*.json"))
        for case in json.loads(path.read_text(encoding="utf-8"))
    ]


def test_components_are_valid_and_unique() -> None:
    names = [
        line.strip()
        for line in (VECTORS / "components.txt").read_text(encoding="utf-8").splitlines()
        if line.strip() and not line.startswith("#")
    ]
    assert len(names) == len(set(names)) >= 30
    assert all(re.fullmatch(r"[a-z][a-z0-9-]*", name) for name in names)


def test_line_re_accepts_the_grammar_and_refuses_its_breaks() -> None:
    grammar = re.compile((VECTORS / "line.re").read_text(encoding="utf-8").strip())
    assert grammar.match(f'{TS} admin INFO start version=1.2.3 note="a \\"b\\""')
    assert not grammar.match(f"{TS} Admin INFO start")
    assert not grammar.match(f"{TS} admin NOTICE start")
    assert not grammar.match(f'{TS} admin INFO start k="a\nb"')


def test_every_case_is_well_formed_and_its_line_matches_the_grammar() -> None:
    grammar = re.compile((VECTORS / "line.re").read_text(encoding="utf-8").strip())
    cases = _cases()
    assert len(cases) >= 90
    names = [case["name"] for _, case in cases]
    assert len(names) == len(set(names)), "a case appears twice"
    for _, case in cases:
        assert {"level", "component", "message", "fields", "env", "expected"} <= set(case)
        if case["expected"] != "PSEUDO":
            assert grammar.match(f"{TS} {case['expected']}"), case["name"]


def test_the_rust_reader_lists_every_case_file() -> None:
    source = RUST.read_text(encoding="utf-8")
    for path in (VECTORS / "redact").glob("*.json"):
        assert f"redact/{path.name}" in source, path.name
