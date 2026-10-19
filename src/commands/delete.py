"""Handler for the 'delete' subcommand.

Looks up a session by ID across all agents, asks for confirmation, then
removes all its data with the agent's provider.
"""

from __future__ import annotations

import sys

from src.data.agents import PROVIDERS, load_all_sessions
from src.utils.formatting import format_datetime, truncate


def run(session_id: str) -> None:
    """Delete a session of any agent and all its data.

    Args:
        session_id: Full or partial session UUID to look up.
    """
    sessions = load_all_sessions()

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
    print(f"  Agent:       {match.agent}")
    if match.name:
        print(f"  Name:        {match.name}")
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

    # Delete all session data with the agent's own rules
    errors = PROVIDERS[match.agent].delete_sessions([match])
    if errors:
        for error in errors:
            print(f"Error: {error}", file=sys.stderr)
        sys.exit(1)

    print(f"Session '{match.session_id}' deleted.")


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Error: 'delete' requires a session ID argument.", file=sys.stderr)
        sys.exit(1)
    run(sys.argv[1])
