"""The bounded log options every `docker run` Ragon builds carries (LOGGING §6.3, O4)."""

from __future__ import annotations

from typing import Final

MAX_SIZE: Final[str] = "10m"
MAX_FILE: Final[int] = 3
LABEL: Final[str] = "ragon.component"


class ContainerLogOptions:
    """Builds the `docker run` arguments that bound a container's log and core dumps."""

    @classmethod
    def args(
        cls, component: str, *, max_size: str = MAX_SIZE, max_file: int = MAX_FILE
    ) -> list[str]:
        """Returns the log-bounding arguments, to be placed right after `run`.

        Args:
            component: The component name, written as the `ragon.component` label.
            max_size: The size at which Docker rotates the container's log.
            max_file: How many rotated files Docker keeps.

        Returns:
            list[str]: Arguments with no value that begins with the container name or image, so
            they can be spliced anywhere between `run` and the image.
        """
        return [
            "--log-driver",
            "json-file",
            "--log-opt",
            f"max-size={max_size}",
            "--log-opt",
            f"max-file={max_file}",
            "--log-opt",
            "compress=true",
            "--ulimit",
            "core=0",
            "--label",
            f"{LABEL}={component}",
        ]
