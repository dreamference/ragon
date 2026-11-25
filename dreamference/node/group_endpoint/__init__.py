"""The group endpoint: one address for a group of nodes."""

from dreamference.node.group_endpoint.load import load_header, load_snapshot
from dreamference.node.group_endpoint.settings import GroupEndpointSettings

__all__ = ["GroupEndpointSettings", "load_header", "load_snapshot"]
