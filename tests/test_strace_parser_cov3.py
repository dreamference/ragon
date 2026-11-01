"""Branch tests for StraceParser: what counts as loopback, a git subcommand and a DNS query."""

import pytest

from dreamference.audit.strace_parser import StraceParser


@pytest.mark.parametrize(
    ("address", "loopback"),
    [
        ("127.0.0.1", True),
        ("::1", True),
        ("0.0.0.0", True),
        ("::ffff:127.0.0.1", True),
        ("::ffff:8.8.8.8", False),
        ("8.8.8.8", False),
        ("not-an-address", False),
        ("", False),
    ],
)
def test_loopback_is_judged_on_the_address_a_mapped_one_stands_for(address, loopback):
    assert StraceParser.is_loopback(address) is loopback


def test_git_subcommand_skips_options_and_the_values_they_take():
    assert StraceParser.git_subcommand(["-C", "/tmp/repo", "fetch", "origin"]) == "fetch"
    assert StraceParser.git_subcommand(["--no-pager", "-c", "a=b", "ls-remote"]) == "ls-remote"


def test_git_subcommand_is_none_without_one():
    assert StraceParser.git_subcommand([]) is None
    assert StraceParser.git_subcommand(["--version"]) is None
    assert StraceParser.git_subcommand(["-C"]) is None


def _query(name: bytes, flags: bytes = b"\x01\x00", qd: bytes = b"\x00\x01") -> bytes:
    return b"\x12\x34" + flags + qd + b"\x00\x00\x00\x00\x00\x00" + name


def test_dns_query_name_reads_a_name_and_lowercases_it():
    payload = _query(b"\x07Example\x03ORG\x00\x00\x01\x00\x01")
    assert StraceParser.dns_query_name(payload) == "example.org"


@pytest.mark.parametrize(
    "payload",
    [
        b"\x00" * 5,
        _query(b"\x03abc\x00", flags=b"\x81\x80"),
        _query(b"\x03abc\x00", qd=b"\x00\x02"),
        _query(b"\x40" + b"a" * 64 + b"\x00"),
        _query(b"\x09abc"),
        _query(b"\x03a b\x00"),
        _query(b"\x03abc"),
        _query(b"\x00"),
    ],
)
def test_dns_query_name_is_none_for_anything_that_is_not_a_plain_query(payload):
    assert StraceParser.dns_query_name(payload) is None
