"""Read-only access to the SQLite databases of other agents."""

from __future__ import annotations

import sqlite3
from pathlib import Path
from typing import List, Tuple


def query_readonly(db_path: Path, sql: str) -> List[Tuple]:
    """Run a query on a SQLite database opened in read-only mode.

    The agents own their databases: opening them read-only guarantees the
    tool never changes them.

    Args:
        db_path: The database file.
        sql: The SELECT statement to run.

    Returns:
        All result rows.

    Raises:
        sqlite3.Error: If the database cannot be opened or queried.
    """
    connection = sqlite3.connect(db_path.as_uri() + "?mode=ro", uri=True)
    try:
        return connection.execute(sql).fetchall()
    finally:
        connection.close()
