"""The gate's usage line: a tagged generation request's token counts, read as the answer streams by."""

import asyncio
import json

from dreamference.vllm_server.model_gate_service import USAGE_LIMIT, ModelGateService

TAGGED = [(b"x-ragon-job", b"ct-route-to-ragon")]
COMPLETED = (
    b'data: {"type":"response.completed","response":{"id":"r1","usage":{"input_tokens":1200,'
    b'"input_tokens_details":{"cached_tokens":800},"output_tokens":345,"total_tokens":1545}}}\n\n'
)


def _fed(*pieces):
    tap = ModelGateService.usage_tap("POST", "/v1/responses", TAGGED)
    for piece in pieces:
        ModelGateService.feed_usage(tap, piece)
    return tap


def _line(tap):
    head, _, body = ModelGateService.usage_line(tap).partition(" ")
    assert head == "usage"
    return json.loads(body)


# -- usage_tap -----------------------------------------------------------------------------------------


def test_only_a_tagged_generation_request_gets_a_tap():
    assert ModelGateService.usage_tap("POST", "/v1/responses", TAGGED)["job"] == "ct-route-to-ragon"
    assert ModelGateService.usage_tap("POST", "/v1/chat/completions", TAGGED) is not None
    assert ModelGateService.usage_tap("GET", "/v1/responses", TAGGED) is None
    assert ModelGateService.usage_tap("POST", "/v1/models", TAGGED) is None
    assert ModelGateService.usage_tap("POST", "/v1/responses", []) is None


def test_a_tag_that_could_break_the_log_line_is_ignored():
    for tag in (b"a b", b"job\r\nforged", b'job"x', b"x" * 121, "jöb".encode()):
        assert ModelGateService.usage_tap("POST", "/v1/responses", [(b"x-ragon-job", tag)]) is None


# -- feed_usage ----------------------------------------------------------------------------------------


def test_the_completed_events_usage_is_read():
    tap = _fed(b'data: {"type":"response.created","response":{"usage":null}}\n\n', COMPLETED)

    assert _line(tap) == {
        "job": "ct-route-to-ragon",
        "path": "/v1/responses",
        "generated": 345,
        "prompt": 1200,
        "cached": 800,
    }


def test_a_usage_object_split_across_reads_is_read_whole():
    for cut in range(1, len(COMPLETED)):
        assert _line(_fed(COMPLETED[:cut], COMPLETED[cut:]))["generated"] == 345, cut


def test_a_chunk_size_line_inside_the_object_is_skipped():
    cut = COMPLETED.index(b'"output_tokens"')
    chunked = COMPLETED[:cut] + b"\r\n1f4\r\n" + COMPLETED[cut:]

    assert _line(_fed(chunked[: cut + 3], chunked[cut + 3 :]))["generated"] == 345


def test_the_last_usage_wins():
    later = COMPLETED.replace(b'"output_tokens":345', b'"output_tokens":400')

    assert _line(_fed(COMPLETED, later))["generated"] == 400


def test_chat_completions_counts_are_read():
    tap = ModelGateService.usage_tap("POST", "/v1/chat/completions", TAGGED)
    ModelGateService.feed_usage(
        tap,
        b'data: {"choices":[],"usage":{"prompt_tokens":50,"completion_tokens":7,'
        b'"prompt_tokens_details":null}}\n\n',
    )

    assert _line(tap) == {
        "job": "ct-route-to-ragon",
        "path": "/v1/chat/completions",
        "generated": 7,
        "prompt": 50,
        "cached": None,
    }


def test_no_usage_and_odd_values_give_nulls():
    assert _line(_fed(b"data: [DONE]\n\n"))["generated"] is None
    assert _line(_fed(b'{"usage":{"output_tokens":"12","input_tokens":true}}'))["generated"] is None
    assert _line(_fed(b'{"usage":{"output_tokens":3,'))["generated"] is None


def test_an_object_longer_than_the_limit_is_given_up_and_not_held():
    tap = _fed(b'{"usage":{"output_tokens":3,"pad":"' + b"x" * USAGE_LIMIT)
    ModelGateService.feed_usage(tap, b"x" * USAGE_LIMIT)

    assert tap["usage"] is None
    assert len(tap["carry"]) < len(b'"usage":')


# -- handle ----------------------------------------------------------------------------------------------


class Sink:
    """A stand-in for a connection's writer: it keeps what it is sent."""

    def __init__(self):
        self.data = b""

    def write(self, data):
        self.data += data

    async def drain(self):
        pass

    def get_extra_info(self, name):
        return ("127.0.0.1", 50000) if name == "peername" else None

    def close(self):
        pass


def _serve(monkeypatch, tmp_path, extra_header):
    body = b'{"model":"m","input":"hi"}'
    answer = b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\r\n" + COMPLETED
    logged = []
    monkeypatch.setattr(ModelGateService, "log", classmethod(lambda cls, line: logged.append(line)))

    async def engine(*args, **kwargs):
        reader = asyncio.StreamReader()
        reader.feed_data(answer)
        reader.feed_eof()
        return reader, Sink()

    monkeypatch.setattr(asyncio, "open_connection", engine)
    service = ModelGateService(("127.0.0.1", 0), ("127.0.0.1", 1), str(tmp_path))
    client = Sink()

    async def go():
        reader = asyncio.StreamReader()
        reader.feed_data(
            b"POST /v1/responses HTTP/1.1\r\nHost: x\r\n"
            + extra_header
            + b"Content-Length: %d\r\n\r\n" % len(body)
            + body
        )
        await service.handle(reader, client)

    asyncio.run(go())
    return client.data, [line for line in logged if line.startswith("usage ")]


def test_a_tagged_request_logs_one_usage_line_and_its_answer_is_untouched(monkeypatch, tmp_path):
    sent, lines = _serve(monkeypatch, tmp_path, b"X-Ragon-Job: ct-route-to-ragon\r\n")

    assert sent.endswith(COMPLETED)
    assert len(lines) == 1
    assert json.loads(lines[0][len("usage ") :])["generated"] == 345


def test_an_untagged_request_logs_no_usage(monkeypatch, tmp_path):
    sent, lines = _serve(monkeypatch, tmp_path, b"")

    assert sent.endswith(COMPLETED)
    assert lines == []
