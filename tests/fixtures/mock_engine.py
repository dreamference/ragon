"""A stand-in model server speaking the OpenAI wire format, for tests of the gate.

It serves `GET /v1/models`, `POST /v1/chat/completions` (one JSON response, or a server-sent
stream when the body says `"stream": true`) and answers a model it does not know with the
engine's own error body. Every request path it saw is kept in `seen`.
"""

import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

MODEL = "mock-model"
REPLY = "pong"
ERROR_MESSAGE = "The model `nope` does not exist."


class MockEngineHandler(BaseHTTPRequestHandler):
    """One request of the mock engine."""

    protocol_version = "HTTP/1.1"

    def log_message(self, message_format, *args):
        return

    def reply(self, status: int, body: dict) -> None:
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        self.server.seen.append(self.path)
        if self.path.split("?")[0].rstrip("/") == "/v1/models":
            self.reply(
                200,
                {"object": "list", "data": [{"id": MODEL, "object": "model", "created": 0}]},
            )
        else:
            self.reply(404, {"error": {"message": "not found"}})

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))) or b"{}")
        self.server.seen.append(self.path)
        if request.get("model") != MODEL:
            self.reply(
                404,
                {
                    "error": {
                        "message": ERROR_MESSAGE,
                        "type": "invalid_request_error",
                        "param": "model",
                        "code": "model_not_found",
                    }
                },
            )
        elif request.get("stream"):
            self.stream()
        else:
            self.reply(
                200,
                {
                    "id": "chatcmpl-1",
                    "object": "chat.completion",
                    "created": 0,
                    "model": MODEL,
                    "choices": [
                        {
                            "index": 0,
                            "message": {"role": "assistant", "content": REPLY},
                            "finish_reason": "stop",
                        }
                    ],
                    "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
                },
            )

    def stream(self) -> None:
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Connection", "close")
        self.end_headers()
        pieces = [REPLY[:2], REPLY[2:], None]
        for piece in pieces:
            delta = {"role": "assistant", "content": piece} if piece else {}
            chunk = {
                "id": "chatcmpl-1",
                "object": "chat.completion.chunk",
                "created": 0,
                "model": MODEL,
                "choices": [
                    {"index": 0, "delta": delta, "finish_reason": None if piece else "stop"}
                ],
            }
            self.wfile.write(b"data: " + json.dumps(chunk).encode() + b"\n\n")
            self.wfile.flush()
        self.wfile.write(b"data: [DONE]\n\n")


class MockEngine:
    """The mock engine on a loopback port, run in a thread."""

    def __init__(self) -> None:
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), MockEngineHandler)
        self.server.seen = []
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    @property
    def port(self) -> int:
        return self.server.server_address[1]

    @property
    def seen(self) -> list[str]:
        return self.server.seen

    def start(self) -> "MockEngine":
        self.thread.start()
        return self

    def stop(self) -> None:
        self.server.shutdown()
        self.server.server_close()
