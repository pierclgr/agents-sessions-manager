"""Handler for the 'delete' subcommand.

Looks up a session by ID, asks for confirmation, then removes all its
data from history.jsonl and the filesystem.
"""

from __future__ import annotations

import sys

from src.data import delete_session_files, delete_sessions_from_history, load_sessions
from src.utils.formatting import format_datetime, truncate


def run(session_id: str) -> None:
    """Delete a Claude Code session and all its data.

    Args:
        session_id: Full or partial session UUID to look up.
    """
    sessions = load_sessions()

    # Find matching session (exact or prefix match)
    match = None
    for session in sessions:
        if session.session_id == session_id or session.session_id.startswith(session_id):
            match = session
            break

    if match is None:
        print(f"Error: Session '{session_id}' not found.", file=sys.stderr)
        sys.exit(1)

    # Show session info and ask for confirmation
    print(f"  Session ID:  {match.session_id}")
    print(f"  Project:     {match.project_path}")
    print(f"  Created:     {format_datetime(match.created_at)}")
    print(f"  Last active: {format_datetime(match.last_active_at)}")
    print(f"  Last command: {truncate(match.latest_command.strip(), 60)}")
    print()

    try:
        answer = input("Delete this session? [y/N] ").strip().lower()
    except (EOFError, KeyboardInterrupt):
        print()
        print("Aborted.")
        sys.exit(0)

    if answer != "y":
        print("Aborted.")
        sys.exit(0)

    # Remove from history.jsonl (atomic rewrite)
    delete_sessions_from_history({match.session_id})

    # Delete session files and directories
    delete_session_files(match.session_id)

    print(f"Session '{match.session_id}' deleted.")


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Error: 'delete' requires a session ID argument.", file=sys.stderr)
        sys.exit(1)
    run(sys.argv[1])
