"""Handler for the 'ls' subcommand.

Prints a tab-separated list of all sessions to stdout, sorted by
latest modification date (most recent first).
"""

from __future__ import annotations

import sys

from src.data.agents import load_all_sessions
from src.utils.formatting import format_datetime, truncate


def run() -> None:
    """Print the sessions of all agents as an aligned table to stdout."""
    sessions = load_all_sessions()

    if not sessions:
        print("No sessions found.", file=sys.stderr)
        return

    headers = ["Agent", "Session ID", "Name", "Project Directory", "Last message", "Created", "Last Modified"]

    rows = []
    for session in sessions:
        rows.append([
            session.agent,
            session.session_id[:8],
            session.name or "",
            session.project_path,
            truncate(session.latest_command.strip(), 50),
            format_datetime(session.created_at),
            format_datetime(session.last_active_at),
        ])

    # Compute column widths from headers and data
    col_widths = [len(h) for h in headers]
    for row in rows:
        for i, cell in enumerate(row):
            col_widths[i] = max(col_widths[i], len(cell))

    # Print header and rows with consistent padding
    fmt = "\t".join(f"{{:<{w}}}" for w in col_widths)
    print(fmt.format(*headers))
    for row in rows:
        print(fmt.format(*row))


if __name__ == "__main__":
    run()
