"""RequestDeadline against a loopback server on an ephemeral port."""

import socket
import threading
import time
from collections.abc import Iterator

import pytest
import urllib3

from dreamference.mcp_server import request_deadline
from dreamference.mcp_server.request_deadline import RequestDeadline, deadline_pools


@pytest.fixture()
def server() -> Iterator[tuple[int, list[socket.socket]]]:
    """A loopback server that accepts connections and never answers."""
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen(5)
    accepted: list[socket.socket] = []

    def accept() -> None:
        while True:
            try:
                conn, _ = listener.accept()
            except OSError:
                return
            accepted.append(conn)

    thread = threading.Thread(target=accept, daemon=True)
    thread.start()
    yield listener.getsockname()[1], accepted
    listener.close()
    for conn in accepted:
        conn.close()


def _pool(port: int) -> urllib3.HTTPConnectionPool:
    manager = urllib3.PoolManager()
    manager.pool_classes_by_scheme = deadline_pools(manager.pool_classes_by_scheme)
    return manager.connection_from_url(f"http://127.0.0.1:{port}")


def test_enter_and_exit_set_and_clear_the_thread_deadline() -> None:
    deadline = RequestDeadline(30)
    with deadline as entered:
        assert entered is deadline
        assert request_deadline._CALL.deadline is deadline
    assert request_deadline._CALL.deadline is None
    assert not deadline.expired.is_set()


def test_attach_registers_a_duplicate_and_exit_closes_it() -> None:
    a, b = socket.socketpair()
    with RequestDeadline(30) as deadline:
        deadline.attach(a)
        assert len(deadline._sockets) == 1
        handle = deadline._sockets[0]
    assert deadline._sockets == []
    assert handle.fileno() == -1
    assert a.fileno() != -1
    a.close()
    b.close()


def test_expiry_shuts_down_registered_sockets() -> None:
    a, b = socket.socketpair()
    with RequestDeadline(0.05) as deadline:
        deadline.attach(a)
        assert deadline.expired.wait(5)
        assert b.recv(10) == b""
    a.close()
    b.close()


def test_attach_after_expiry_shuts_down_at_once() -> None:
    a, b = socket.socketpair()
    with RequestDeadline(0.01) as deadline:
        assert deadline.expired.wait(5)
        deadline.attach(a)
        assert b.recv(10) == b""
    a.close()
    b.close()


def test_shut_ignores_an_already_closed_socket() -> None:
    a, b = socket.socketpair()
    a.close()
    RequestDeadline._shut(a)
    b.close()


def test_new_conn_registers_with_the_thread_deadline(server) -> None:
    port, _ = server
    pool = _pool(port)
    with RequestDeadline(30) as deadline:
        conn = pool._new_conn()
        conn.connect()
        try:
            assert len(deadline._sockets) == 1
        finally:
            conn.close()


def test_new_conn_without_a_deadline_registers_nothing(server) -> None:
    port, _ = server
    conn = _pool(port)._new_conn()
    conn.close()
    assert getattr(request_deadline._CALL, "deadline", None) is None


def test_deadline_ends_a_read_the_server_never_answers(server) -> None:
    port, _ = server
    pool = _pool(port)
    start = time.monotonic()
    with RequestDeadline(0.3) as deadline, pytest.raises(urllib3.exceptions.HTTPError):
        pool.request("GET", "/", timeout=urllib3.Timeout(connect=2, read=20), retries=False)
    assert deadline.expired.is_set()
    assert time.monotonic() - start < 10


def test_deadline_pools_reuses_subclasses() -> None:
    manager = urllib3.PoolManager()
    first = deadline_pools(manager.pool_classes_by_scheme)
    second = deadline_pools(manager.pool_classes_by_scheme)
    assert first == second
    assert set(first) == set(manager.pool_classes_by_scheme)
