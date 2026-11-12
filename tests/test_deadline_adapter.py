"""A DeadlineAdapter's proxy pools open sockets that register with the call's deadline."""

import urllib3
import urllib3.connection

from dreamference.mcp_server.deadline_adapter import DeadlineAdapter


def test_a_proxy_manager_gets_pool_classes_whose_connections_register_with_the_deadline():
    manager = DeadlineAdapter().proxy_manager_for("http://proxy.example:3128")

    assert isinstance(manager, urllib3.ProxyManager)
    for scheme in ("http", "https"):
        pool = manager.pool_classes_by_scheme[scheme]
        assert issubclass(pool, urllib3.connectionpool.HTTPConnectionPool)
        assert pool.ConnectionCls is not urllib3.connection.HTTPConnection
        assert pool.ConnectionCls is not urllib3.connection.HTTPSConnection
