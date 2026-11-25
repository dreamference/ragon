"""Tests for the group endpoint's [node] settings."""

import pytest

from dreamference.node.group_endpoint.settings import GroupEndpointSettings


def test_defaults_leave_the_endpoint_off():
    for config in (None, {}, {"node": {}}):
        s = GroupEndpointSettings.from_config(config)
        assert (s.group, s.backends, s.endpoint_only, s.queue_wait_s) == ("serve", (), False, 60)
        assert not s.endpoint_on


def test_values_parse_flat_or_under_node():
    table = {
        "group": "hold-only",
        "backends": ["a1", "b-2", "a1"],
        "endpoint_only": True,
        "queue_wait_s": 5,
    }
    for config in (table, {"node": table}):
        s = GroupEndpointSettings.from_config(config)
        assert s.group == "hold-only" and s.backends == ("a1", "b-2")
        assert s.endpoint_only and s.queue_wait_s == 5 and s.endpoint_on


def test_paired_is_accepted():
    s = GroupEndpointSettings.from_config({"backends": "paired"})
    assert s.backends == "paired" and s.endpoint_on


@pytest.mark.parametrize("bad", ["", "-x", "a b", "a/b", "x" * 65, 3, None])
def test_bad_node_ids_are_refused(bad):
    with pytest.raises(ValueError, match="backends"):
        GroupEndpointSettings.from_config({"backends": [bad]})


@pytest.mark.parametrize(
    "table",
    [
        {"backends": "all"},
        {"group": "primary"},
        {"endpoint_only": "yes"},
        {"queue_wait_s": -1},
        {"queue_wait_s": "60"},
        {"queue_wait_s": True},
        {"queue_wait_s": 99999},
    ],
)
def test_bad_values_are_refused(table):
    with pytest.raises(ValueError):
        GroupEndpointSettings.from_config(table)


def test_no_limit_setting_exists():
    assert not any("limit" in f for f in GroupEndpointSettings.__dataclass_fields__)
