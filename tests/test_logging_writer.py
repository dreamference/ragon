"""The log level: one grammar (LOGGING §4.2) and one precedence (§4.3) in Python and in Rust."""

import fcntl
import gzip
import io
import json
import os
import re
import threading
from pathlib import Path

import pytest

from dreamference.config.dreamference_config import DEFAULT_LOG_LEVEL, DreamferenceConfig
from dreamference.config.log_field import LogField
from dreamference.config.log_file_sink import LogFileSink
from dreamference.config.log_level_spec import LogLevelSpec
from dreamference.config.log_line_formatter import LogLineFormatter
from dreamference.config.log_redactor import TOKEN_SHAPES, LogRedactor
from dreamference.config.log_stream_sink import LogStreamSink
from dreamference.config.log_writer import LogWriter
from dreamference.config.setup_logging import setup_logging

REPO = Path(__file__).resolve().parent.parent
VECTORS = REPO / "ragon-rs" / "vectors" / "log_level_cases.txt"
RUST_TEST = REPO / "ragon-rs" / "log" / "tests" / "vectors.rs"


def _vector_lines() -> list[list[str]]:
    rows = []
    for line in VECTORS.read_text().splitlines():
        if line.startswith("#") or not line.strip():
            continue
        rows.append([field.strip() for field in line.split("|")])
    return rows


@pytest.mark.parametrize("fields", _vector_lines(), ids=lambda fields: fields[0] or "blank")
def test_every_shared_vector_parses_to_the_expected_levels(fields):
    spec_text, want_default, want_overrides, want_warnings = fields
    spec = LogLevelSpec.parse(spec_text)
    assert spec.default == (None if want_default == "-" else want_default)
    got = [f"{component}={level}" for component, level in spec.overrides.items()]
    assert got == ([] if want_overrides == "-" else want_overrides.split(";"))
    assert len(spec.warnings) == int(want_warnings)


def test_the_rust_crate_reads_the_same_vectors():
    # The Rust test includes the same file; a rename of either side must fail here.
    assert "log_level_cases.txt" in RUST_TEST.read_text()
    assert len(_vector_lines()) >= 10


def test_the_level_resolves_through_the_tiers(tmp_path, monkeypatch):
    monkeypatch.delenv("RAGON_LOG", raising=False)
    cfg_file = tmp_path / "tiers.yaml"
    cfg_file.write_text("vllm_host: http://localhost:8000\n")
    assert DreamferenceConfig(config_file=str(cfg_file)).log_level == DEFAULT_LOG_LEVEL

    cfg_file.write_text('vllm_host: http://localhost:8000\nlog_level: "warn,engine=debug"\n')
    assert DreamferenceConfig(config_file=str(cfg_file)).log_level == "warn,engine=debug"

    monkeypatch.setenv("RAGON_LOG", "debug")
    assert DreamferenceConfig(config_file=str(cfg_file)).log_level == "debug"

    monkeypatch.setenv("RAGON_LOG", "   ")
    assert DreamferenceConfig(config_file=str(cfg_file)).log_level == "warn,engine=debug"

    assert DreamferenceConfig(config_file=str(cfg_file), log_level="trace").log_level == "trace"


def test_a_default_log_level_is_not_written_back(tmp_path, monkeypatch):
    monkeypatch.delenv("RAGON_LOG", raising=False)
    cfg_file = tmp_path / "save.yaml"
    cfg_file.write_text("vllm_host: http://localhost:8000\n")
    config = DreamferenceConfig(config_file=str(cfg_file))
    saved = config.save_config(tmp_path / "out.yaml").read_text()
    assert "log_level" not in saved
    config = DreamferenceConfig(config_file=str(cfg_file), log_level="info,engine=debug")
    saved = config.save_config(tmp_path / "out2.yaml").read_text()
    assert "info,engine=debug" in saved


# --- the writer: format, redaction vectors, sampler, rotation, never blocking (LOGGING §5-§7, §11) ---

LOGGING_VECTORS = REPO / "ragon-rs" / "vectors" / "logging"
LINE_RE = re.compile(LOGGING_VECTORS.joinpath("line.re").read_text().strip())
EPOCH = 1794408000.123  # fixed: the fixtures strip the timestamp


class ListSink:
    """A sink that keeps lines, or fails on demand."""

    def __init__(self):
        self.lines: list[str] = []
        self.fail = False
        self.gate: threading.Event | None = None

    def write(self, line):
        if self.gate is not None:
            self.gate.wait()
        if self.fail:
            raise OSError("disk full")
        self.lines.append(line)

    def close(self):
        pass


def _writer(spec="debug", sink=None, component="engine"):
    from dreamference.config.log_level_spec import LogLevelSpec

    clock = [EPOCH]
    writer = LogWriter(clock=lambda: clock[0])
    sink = sink or ListSink()
    writer.configure(
        component, LogLevelSpec.parse(spec), sink, LogRedactor("/home/jane", "box1", "jane", b"s")
    )
    writer.clock_value = clock
    return writer, sink


def _build_field(spec):
    kind, key, value = spec["type"], spec["key"], spec["value"]
    if kind == "error" and "hex" in value:  # a message that is not UTF-8
        return (LogField("error", key, {**value, "message": bytes.fromhex(value["hex"])}),)
    if kind == "error":
        return LogField.error(value, key)
    if kind == "enum":
        return LogField.enum(tuple(spec.get("allowed", ())), **{key: value})
    return getattr(LogField, {"id": "id_", "bytes": "bytes_"}.get(kind, kind))(**{key: value})


def test_the_shared_redaction_fixtures_pass_byte_for_byte():
    cases = [
        c
        for f in sorted((LOGGING_VECTORS / "redact").glob("*.json"))
        for c in json.loads(f.read_text())
    ]
    assert cases
    for case in cases:
        env = case["env"]
        redactor = LogRedactor(env["home"], env["host"], env["user"], env["salt"].encode())
        fields = tuple(x for spec in case["fields"] for x in _build_field(spec))
        level = ("error", "warn", "info", "debug", "trace").index(case["level"])
        line = LogLineFormatter.format(
            EPOCH, case["component"], level, case["message"], fields, redactor
        )
        assert LINE_RE.match(line), (case["name"], line)
        expected = case["expected"]
        if expected == "PSEUDO":
            expected = f"web INFO chat opened chat={redactor.pseudonym('chat', 'my chat')}"
        assert line.split(" ", 1)[1] == expected, case["name"]


def test_a_pseudonym_is_stable_for_one_salt_and_differs_for_another():
    one, other = LogRedactor(salt=b"a"), LogRedactor(salt=b"b")
    assert one.pseudonym("chat", "x") == one.pseudonym("chat", "x")
    assert one.pseudonym("chat", "x") != other.pseudonym("chat", "x")
    assert re.fullmatch(r"p-[0-9a-f]{12}", one.pseudonym("chat", "x"))


def test_the_token_shapes_file_equals_the_python_shapes():
    rows = [
        ln
        for ln in LOGGING_VECTORS.joinpath("secrets.txt").read_text().splitlines()
        if ln and ln[0] != "#"
    ]
    assert tuple(tuple(r.split("|", 2)) for r in rows) == TOKEN_SHAPES


def _sample(name):
    return {
        "github": "ghp_" + "a1B2" * 6,
        "github-pat": "github_pat_" + "a1B2c3" * 5,
        "slack": "xoxb-" + "1234567890" * 2,
        "openai": "sk-proj-" + "a1B2" * 6,
        "jwt": "eyJhbGciOi.eyJzdWIiOjEyMzQ.SflKxwRJSMeKKF2QT4",
        "aws": "AKIA" + "ABCDEFGH12345678",
        "google": "AIza" + "a" * 35,
        "gitlab": "glpat-" + "a1B2" * 6,
        "npm": "npm_" + "a1B2" * 9,
        "pypi": "pypi-" + "a1B2" * 6,
        "huggingface": "hf_" + "a1B2" * 9,
        "telegram": "123456789:" + "a" * 35,
    }[name]


def test_every_token_shape_is_redacted_and_a_near_miss_stays():
    redactor = LogRedactor()
    for name, _, _ in TOKEN_SHAPES:
        sample = _sample(name)
        assert redactor.redact(f"failed with {sample} twice") == "failed with [secret] twice", name
        assert redactor.redact(f"plain {sample[:6]} word") == f"plain {sample[:6]} word", name


def test_log_redactor_url_keeps_trailing_punctuation_away_from_the_match():
    redactor = LogRedactor()
    assert redactor.redact("see https://h.example/p now") == "see https://h.example/p now"
    assert redactor.redact("see https://h.example/p,.") == "see https://h.example/p,."


def test_log_redactor_url_keeps_the_port_and_brackets_an_ipv6_host():
    redactor = LogRedactor()
    assert redactor.redact("see https://h.example:8443/p") == "see https://h.example:8443/p"
    assert redactor.redact("see http://[2001:db8::1]:8443/p") == "see http://[[addr]]:8443/p"


def test_log_redactor_swap_keeps_a_match_that_is_not_an_address():
    redactor = LogRedactor()
    assert redactor._addresses("loop 127.0.0.1 and 2001:db8::1, not a 999.999.999.999 one") == (
        "loop 127.0.0.1 and [addr], not a 999.999.999.999 one"
    )


def test_log_redactor_url_unsplittable_input_keeps_its_host_down_to_the_user_info():
    redactor = LogRedactor()
    assert redactor.redact("see https://[::zz]/x?q=1#f") == "see https://[::zz]/x"


def test_a_ten_kilobyte_value_is_cut_at_the_last_whole_field():
    redactor = LogRedactor()
    fields = (*LogField.count(a=1), *LogField.error({"class": "E", "message": "x" * 10000}))
    line = LogLineFormatter.format(EPOCH, "web", 2, "big value", fields, redactor)
    assert (
        len(line.encode()) <= 4096
        and line.endswith(" a=1 error=E truncated=1")
        and LINE_RE.match(line)
    )


def test_arbitrary_values_and_keys_always_match_the_line_grammar():
    redactor = LogRedactor("/home/jane", "box1", "jane", b"s")
    nasty = [
        "",
        " ",
        'a"b',
        "a\\b",
        "a\nb\r\tc",
        "\x1b[31m",
        "\u0085",
        "‮",
        "⁦",
        "é" * 3,
        "a=b c",
        "\ud800",
    ]
    nasty += [bytes([0xFF, 0xFE, 0x41]), "x" * 5000]
    for index, value in enumerate(nasty):
        for field in (
            LogField.error({"class": "E", "message": value}, key=f"K y{index}"),
            LogField.path(p=value),
        ):
            line = LogLineFormatter.format(EPOCH, "web", 2, "Bad Message!\n", field, redactor)
            assert LINE_RE.match(line), repr(line)
            assert "\n" not in line and "\r" not in line
    assert "\\x{FF}" in LogLineFormatter.quote(LogLineFormatter.text_of(b"\xff"))


def test_mistyped_and_undeclared_values_are_never_written():
    line = LogLineFormatter.format(
        EPOCH,
        "web",
        2,
        "typed",
        (
            *LogField.count(n="secret text"),
            *LogField.id_(session="free text"),
            *LogField.enum(("stop",), finish="free text"),
        ),
        LogRedactor(),
    )
    assert "free text" not in line and "n=[number] session=[id] finish=other" in line


def test_log_line_formatter_format_wraps_a_failing_field_value_as_an_error_placeholder():
    redactor = LogRedactor()
    broken = LogField("error", "k", {"class": 1, "message": 2})  # value() raises inside the loop
    line = LogLineFormatter.format(
        EPOCH, "web", 2, "boom", (*LogField.count(n=3), broken), redactor
    )
    assert line.endswith(" n=3 k=[error]")
    assert LINE_RE.match(line)


def test_log_line_formatter_number_writes_numbers_and_masks_non_finite_floats():
    number = LogLineFormatter.number
    assert number(0) == "0"
    assert number(-12) == "-12"
    assert number(3.14159) == "3.142"
    assert number(2.0) == "2"
    assert number(0.5) == "0.5"
    assert number(1.0) == "1"
    assert number(float("inf")) == "[number]"
    assert number(float("-inf")) == "[number]"
    assert number(float("nan")) == "[number]"
    assert number(True) == "[number]"
    assert number("3") == "[number]"


def test_log_line_formatter_value_writes_a_type_placeholder_for_unknown_field_types():
    redactor = LogRedactor()
    assert LogLineFormatter.value(LogField("weird", "k", "v"), "info", redactor) == [
        ("k", "[type]")
    ]
    line = LogLineFormatter.format(
        EPOCH, "web", 2, "odd type", (LogField("weird", "k", "v"),), redactor
    )
    assert line.endswith(" k=[type]")


def test_a_thousand_lines_of_one_message_give_ten_and_a_suppressed_line():
    writer, sink = _writer("info")
    for _ in range(1000):
        writer.process((EPOCH, 2, "noisy call", ()))
    writer.process((EPOCH, 0, "another failure", ()))
    writer.process((EPOCH + 1, 2, "next second", ()))
    noisy = [ln for ln in sink.lines if " noisy call" in ln]
    assert len(noisy) == 10
    assert any('log lines suppressed site="noisy call" count=990' in ln for ln in sink.lines)
    assert any("ERROR another failure" in ln for ln in sink.lines)
    assert all(LINE_RE.match(ln) for ln in sink.lines)


def test_the_process_cap_drops_and_the_first_error_of_each_key_passes():
    writer, sink = _writer("info")
    for index in range(80):
        writer.process((EPOCH, 2, f"site {index}", ()))
    writer.process((EPOCH, 0, "late error", ()))
    writer.process((EPOCH + 1, 2, "after", ()))
    assert len([ln for ln in sink.lines if "INFO site" in ln]) == 50
    assert any("ERROR late error" in ln for ln in sink.lines)
    assert any("log lines dropped count=30" in ln for ln in sink.lines)


def test_request_closing_lines_have_their_own_budget_and_a_summary():
    writer, sink = _writer("info")
    for _ in range(300):
        writer.process((EPOCH, 2, "request done", LogField.ms(ms=40)))
    writer.process((EPOCH + 1, 2, "after", ()))
    writer.process((EPOCH + 11, 2, "later", ()))
    assert len([ln for ln in sink.lines if "request done" in ln]) == 200
    assert any("requests summary count=100 errors=0 p50_ms=40 p95_ms=40" in ln for ln in sink.lines)


def test_debug_lines_are_dropped_below_their_level():
    writer, sink = _writer("warn")
    writer.info("hidden")
    writer.warn("shown")
    writer.flush()
    assert [ln.split(" ", 3)[3] for ln in sink.lines] == ["shown"]


def test_a_failing_sink_drops_counts_and_never_raises():
    writer, sink = _writer("info")
    sink.fail = True
    writer.process((EPOCH, 2, "one", ()))
    writer.process((EPOCH + 0.5, 2, "two", ()))
    sink.fail = False
    writer.process((EPOCH + 3, 2, "three", ()))
    assert sink.lines[0].endswith("log lines dropped count=2 reason=io") and sink.lines[1].endswith(
        "three"
    )


def test_a_stalled_sink_never_blocks_the_caller_and_the_drop_count_is_reported():
    writer, sink = _writer("info")
    sink.gate = threading.Event()
    for _ in range(100_000):
        writer.info("call")
    assert writer.queue_drops > 0
    sink.gate.set()
    writer.flush()
    writer.info("after")
    writer.flush()
    assert any("log lines dropped count=" in ln for ln in sink.lines)


def test_log_writer_flush_drains_without_a_thread_and_waits_with_one_until_the_deadline():
    writer, sink = _writer("info")
    writer.queue.put((EPOCH, 2, "queued", ()))
    writer.queue.put(None)
    writer.flush()
    assert sink.lines[-1].endswith("queued") and writer.queue.unfinished_tasks == 0
    sink.gate = threading.Event()
    writer.info("held")
    writer.flush(timeout=0.05)
    assert writer.queue.unfinished_tasks
    sink.gate.set()
    writer.flush()
    assert sink.lines[-1].endswith("held")


def test_rotation_keeps_every_line_and_compresses_only_past_the_first_rotation(tmp_path):
    now = [EPOCH]
    first = LogFileSink(tmp_path, "gate", max_bytes=300, clock=lambda: now[0])
    second = LogFileSink(tmp_path, "gate", max_bytes=300, clock=lambda: now[0])
    written = []
    for index in range(40):
        sink = first if index % 2 == 0 else second
        now[0] += 0.6
        line = f"2026-11-11T00:00:00.000Z gate INFO line n={index:03d} " + "x" * 20
        sink.write(line)
        written.append(line)
    names = sorted(p.name for p in tmp_path.iterdir() if p.name.endswith(("log", "gz")))
    assert "gate.1.log" in names and any(n.endswith(".gz") for n in names) and len(names) <= 5
    text = ""
    for path in sorted(tmp_path.glob("gate*.log*")):
        opener = gzip.open if path.suffix == ".gz" else open
        with opener(path, "rt") as handle:
            text += handle.read()
    seen = re.findall(r"n=(\d{3})", text)
    assert len(seen) == len(set(seen))  # no duplicates; older files beyond the five kept are gone
    assert seen and seen[-1] != "" and "n=039" in text


def test_the_directory_and_file_modes_are_private(tmp_path):
    logs = tmp_path / "logs"
    sink = LogFileSink(logs, "web")
    sink.write("x")
    assert (logs.stat().st_mode & 0o777) == 0o700 and (
        (logs / "web.log").stat().st_mode & 0o777
    ) == 0o600


def test_setup_logging_writes_a_start_line_under_the_temporary_home(tmp_path, monkeypatch):
    monkeypatch.setenv("XDG_STATE_HOME", str(tmp_path / "state"))
    monkeypatch.delenv("JOURNAL_STREAM", raising=False)
    monkeypatch.delenv("RAGON_LOG_SINK", raising=False)
    writer = setup_logging("admin", "info", environ=dict(__import__("os").environ))
    writer.finish()
    text = (tmp_path / "state" / "dreamference" / "logs" / "admin.log").read_text()
    assert (
        LINE_RE.match(text.splitlines()[0])
        and " admin INFO start version=" in text
        and "log_format=1" in text
    )
    assert "jane" not in text and str(tmp_path) not in text


def test_no_product_code_builds_its_own_log_handler():
    banned = ("logging.basicConfig", "logging.StreamHandler(", "FileHandler(")
    offenders = []
    for path in (REPO / "dreamference").rglob("*.py"):
        if path.name.startswith("log_") or path.name == "setup_logging.py":
            continue
        text = path.read_text()
        offenders += [f"{path.name}: {b}" for b in banned if b in text]
    assert offenders == [], "move these to the writer (LOGGING §11.2): " + ", ".join(offenders)


def test_log_file_rotate_shifts_compressed_files_and_deletes_the_oldest(tmp_path, monkeypatch):
    import dreamference.config.log_file_sink as sink_module

    now = [EPOCH]
    sink = sink_module.LogFileSink(tmp_path, "rotate", max_bytes=100, clock=lambda: now[0])
    sink.write("a" * 100)  # rotates at 100 bytes
    assert (tmp_path / "rotate.1.log").exists() and (tmp_path / "rotate.lock").exists()
    assert sink.path.exists() and sink.path.stat().st_size == 0  # _open() opened a new file

    now[0] += 2
    sink.write("b" * 100)  # second rotation: .2.log.gz appears, .1.log is re-created
    assert (tmp_path / "rotate.2.log.gz").exists() and (tmp_path / "rotate.1.log").exists()
    assert sink.path.exists() and sink.path.stat().st_size == 0

    now[0] += 2
    sink.write("c" * 100)  # third rotation: .2.log.gz shifts to .3.log.gz
    assert (tmp_path / "rotate.3.log.gz").exists() and (tmp_path / "rotate.2.log.gz").exists()
    assert sink.path.exists() and sink.path.stat().st_size == 0


def test_log_file_rotate_reopens_when_the_lock_is_held_elsewhere(tmp_path, monkeypatch):
    import dreamference.config.log_file_sink as sink_module

    now = [EPOCH]
    sink = sink_module.LogFileSink(tmp_path, "blocked", max_bytes=100, clock=lambda: now[0])
    sink.write("a" * 100)  # rotation already happened; the file is empty and _fd is None
    lock_fd = os.open(tmp_path / "blocked.lock", os.O_RDWR | os.O_CREAT, 0o600)
    fcntl.flock(lock_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    try:
        assert sink.rotate() is False  # the lock is held elsewhere
    finally:
        fcntl.flock(lock_fd, fcntl.LOCK_UN)
        os.close(lock_fd)


def test_log_file_sink_rotate_aborts_when_the_file_is_smaller_or_gone(tmp_path, monkeypatch):
    import dreamference.config.log_file_sink as sink_module

    now = [EPOCH]
    sink = sink_module.LogFileSink(tmp_path, "shrunken", max_bytes=100, clock=lambda: now[0])
    line = "a" * 20
    sink.write(line)  # below max_bytes: no rotation
    assert sink.path.stat().st_size == len(line) + 1 and sink._fd is not None
    assert sink.rotate() is False  # another process rotated first (or the file is smaller)
    assert sink._fd is not None and sink.path.stat().st_size == len(line) + 1

    now[0] += 2
    sink.path.unlink()  # the file is gone: stat() fails under the lock
    assert sink.rotate() is False
    assert sink.path.exists() and sink.path.stat().st_size == 0  # _open() recreated it


def test_log_file_sink_shift_deletes_only_files_older_than_the_cutoff(tmp_path, monkeypatch):
    import dreamference.config.log_file_sink as sink_module

    now = [EPOCH]
    sink = sink_module.LogFileSink(tmp_path, "aged", max_bytes=100, clock=lambda: now[0])
    old = tmp_path / "aged.9.log.gz"
    old.write_bytes(b"\x1f\x8b")  # older than MAX_AGE_S: deleted by _shift
    recent = tmp_path / "aged.8.log.gz"
    recent.write_bytes(b"\x1f\x8b")  # newer than the cutoff: kept
    for path in (old, recent):
        os.utime(path, (now[0] - sink_module.MAX_AGE_S - 60, now[0] - sink_module.MAX_AGE_S - 60))
    os.utime(recent, (now[0] - 60, now[0] - 60))

    sink.path.write_text("x")
    real_unlink = Path.unlink

    def fake_unlink(self, *args, **kwargs):
        if self == old:
            raise OSError("gone")  # the file vanishes between stat() and unlink()
        return real_unlink(self, *args, **kwargs)

    monkeypatch.setattr(Path, "unlink", fake_unlink)
    sink._shift()  # the vanished file must not stop the shift

    assert old.exists()  # the failed unlink was swallowed; the file remains
    assert recent.exists()


def test_log_stream_sink_prefixes_the_priority_under_journald_and_defaults_to_stderr(capsys):
    stream = io.StringIO()
    sink = LogStreamSink(stream, journald=True)
    sink.write("2026-01-01T00:00:00Z 1 ERROR boom")
    sink.write("2026-01-01T00:00:00Z 1 TRACE deep")
    sink.write("short")
    sink.write("2026-01-01T00:00:00Z 1 BOGUS odd")
    assert stream.getvalue().splitlines() == [
        "<3>2026-01-01T00:00:00Z 1 ERROR boom",
        "<7>2026-01-01T00:00:00Z 1 TRACE deep",
        "<6>short",
        "<6>2026-01-01T00:00:00Z 1 BOGUS odd",
    ]
    LogStreamSink().write("plain line")
    assert capsys.readouterr().err == "plain line\n"


def test_log_field_error_takes_an_exception_class_and_message():
    (field,) = LogField.error(ValueError("boom"))
    assert field.type == "error"
    assert field.key == "error"
    assert field.value == {"class": "ValueError", "message": "boom"}


def test_log_line_formatter_url_invalid_port_is_masked():
    assert LogLineFormatter.url("http://host:99999999/p", "info", LogRedactor()) == "[url]"


def test_log_line_formatter_url_brackets_an_ipv6_host():
    assert (
        LogLineFormatter.url("http://[::1]:8000/p?q=1", "info", LogRedactor())
        == "http://[::1]:8000/p"
    )
