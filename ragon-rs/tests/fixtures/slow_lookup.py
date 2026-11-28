"""Run the Rust startup test in a private home with a TLS gate and slow DNS."""

import os
import ssl
import subprocess
import sys
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


def fixture(directory):
    shim = Path(__file__).with_suffix(".c")
    subprocess.run(["cc", "-shared", "-fPIC", shim, "-ldl", "-o", "resolver.so"], check=True)

    def openssl(*args):
        subprocess.run(["openssl", *args], check=True, stdout=subprocess.DEVNULL,
                       stderr=subprocess.PIPE)

    openssl("req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
            "-subj", "/CN=Fixture CA", "-keyout", "ca.key", "-out", "ca.pem",
            "-addext", "basicConstraints=critical,CA:TRUE")
    openssl("req", "-new", "-newkey", "rsa:2048", "-nodes", "-subj", "/CN=fixture.local",
            "-keyout", "gate.key", "-out", "gate.csr")
    Path("extensions").write_text("subjectAltName=DNS:fixture.local\nbasicConstraints=CA:FALSE\n")
    openssl("x509", "-req", "-in", "gate.csr", "-CA", "ca.pem", "-CAkey", "ca.key",
            "-CAcreateserial", "-days", "1", "-extfile", "extensions", "-out", "gate.pem")

    routes = {
        "/ragon/v1/info": b'{"node_id":"fixture","accounts":true}',
        "/ragon/v1/ca.pem": Path("ca.pem").read_bytes(),
        "/ragon/v1/setup": b'{"setup":false}',
        "/v1/models": b'{"data":[{"id":"stub"}]}',
    }
    requests = []

    class Gate(BaseHTTPRequestHandler):
        def do_GET(self):
            body = routes[self.path]
            requests.append((self.headers["Host"], self.path))
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *args):
            pass

    with ThreadingHTTPServer(("127.0.0.1", 0), Gate) as gate, ThreadingHTTPServer(("127.0.0.1", 0), Gate) as plain:
        tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        tls.load_cert_chain("gate.pem", "gate.key")
        gate.socket = tls.wrap_socket(gate.socket, server_side=True)
        for server in (gate, plain):
            threading.Thread(target=server.serve_forever, daemon=True).start()
        env = dict(HOME=directory, CODEX_HOME=directory, PATH=os.defpath,
                   LD_PRELOAD=str(Path("resolver.so").resolve()),
                   RAGON_TEST_GATE_PORT=str(gate.server_port),
                   RAGON_TEST_PLAIN_PORT=str(plain.server_port))
        subprocess.run(sys.argv[1:], env=env, check=True, timeout=120)
        name = f"fixture.local:{gate.server_port}"
        assert [path for host, path in requests if host == name] == [
            "/ragon/v1/info", "/v1/models", "/ragon/v1/info", "/ragon/v1/ca.pem", "/ragon/v1/setup",
        ], requests


if __name__ == "__main__":
    with tempfile.TemporaryDirectory() as directory:
        os.chdir(directory)
        fixture(directory)
