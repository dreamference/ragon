"""Usage per user, day and client kind, and the unauthenticated counts (AUTH §9, §10).

The authenticated rows live in `AccountsStore.usage`. Every gate counts the model requests it
serves without credentials, per day, client kind and source address; a secondary sends its counts
to the primary once a day, and the primary keeps them beside its own, per node id.
"""

import re
import time
from typing import Any, Final

from dreamference.accounts.accounts_error import AccountsError
from dreamference.accounts.accounts_store import AccountsStore

SCHEMA: Final[str] = """
CREATE TABLE IF NOT EXISTS unauthenticated_usage (
    day TEXT NOT NULL, node_id TEXT NOT NULL, client_kind TEXT NOT NULL, source TEXT NOT NULL,
    requests INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (day, node_id, client_kind, source)
);
"""
DAY_FORMAT: Final[re.Pattern[str]] = re.compile(r"^\d{4}-\d{2}-\d{2}$")
MAX_ROWS: Final[int] = 500
MAX_FIELD: Final[int] = 200


class AccountsUsage:
    """Usage reads and the unauthenticated counts over one `AccountsStore`."""

    def __init__(self, store: AccountsStore) -> None:
        """Creates the unauthenticated table if missing.

        Args:
            store: The account store.
        """
        self.store = store
        store.db.executescript(SCHEMA)

    def for_user(self, username: str, days: int = 7) -> list[dict[str, Any]]:
        """One user's rows of `AccountsStore.usage`."""
        return [row for row in self.store.usage(days) if row["username"] == username]

    @classmethod
    def today(cls) -> str:
        """Today's UTC date."""
        return time.strftime("%Y-%m-%d", time.gmtime())

    def add_unauthenticated(self, node_id: str, day: str, counts: list[dict[str, Any]]) -> int:
        """Stores one node's counts for one day; a repeat send replaces, never doubles.

        Args:
            node_id: The sending node (the primary's own id for its own gate).
            day: The UTC date, `YYYY-MM-DD`.
            counts: Items with `client_kind`, `source` and `requests`.

        Returns:
            int: The number of rows stored.

        Raises:
            AccountsError: When the day or an item is malformed.
        """
        if not DAY_FORMAT.match(day):
            raise AccountsError("The day must be YYYY-MM-DD.")
        if len(counts) > MAX_ROWS:
            raise AccountsError("Too many rows.")
        rows = []
        for item in counts:
            kind, source, requests = (
                item.get("client_kind") if isinstance(item, dict) else None,
                item.get("source") if isinstance(item, dict) else None,
                item.get("requests") if isinstance(item, dict) else None,
            )
            if (
                not isinstance(kind, str)
                or not isinstance(source, str)
                or not isinstance(requests, int)
                or isinstance(requests, bool)
                or requests < 0
                or len(kind) > MAX_FIELD
                or len(source) > MAX_FIELD
            ):
                raise AccountsError("Each count needs client_kind, source and requests.")
            rows.append((day, node_id, kind, source, requests))
        db = self.store.db
        db.execute(
            "DELETE FROM unauthenticated_usage WHERE day = ? AND node_id = ?", (day, node_id)
        )
        db.executemany(
            "INSERT INTO unauthenticated_usage (day, node_id, client_kind, source, requests)"
            " VALUES (?, ?, ?, ?, ?) ON CONFLICT(day, node_id, client_kind, source)"
            " DO UPDATE SET requests = requests + excluded.requests",
            rows,
        )
        db.commit()
        return len(rows)

    def unauthenticated(self, days: int = 7) -> list[dict[str, Any]]:
        """The unauthenticated counts of the last N days, oldest first."""
        cutoff = time.strftime("%Y-%m-%d", time.gmtime(time.time() - max(days, 1) * 86400))
        rows = self.store.db.execute(
            "SELECT day, node_id, client_kind, source, requests FROM unauthenticated_usage"
            " WHERE day >= ? ORDER BY day, node_id, client_kind, source",
            (cutoff,),
        ).fetchall()
        return [dict(r) for r in rows]
