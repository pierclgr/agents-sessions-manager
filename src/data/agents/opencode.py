"""OpenCode sessions: the session_v2 and session_message tables of opencode.db."""

from __future__ import annotations

import json
import sqlite3
import sys
from typing import Dict, List

from src.data.agents.base import AgentProvider, run_command
from src.data.models import Session
from src.utils.formatting import ms_to_datetime
from src.utils.paths import get_opencode_db_path
from src.utils.sqlite import query_readonly

# OpenCode gives this title to sessions until it generates one: not a user-set name
_DEFAULT_TITLE_PREFIX = "New session - "

# Top-level (not child), non-archived sessions
_SESSIONS_QUERY = (
    "SELECT id, directory, title, time_created, time_updated FROM session_v2"
    " WHERE parent_id IS NULL AND time_archived IS NULL"
)
_PROMPTS_QUERY = "SELECT session_id, data FROM session_message WHERE type = 'user' ORDER BY session_id, seq"


class OpenCodeProvider(AgentProvider):
    """Sessions of OpenCode (v2 database)."""

    key = "opencode"
    label = "OpenCode"

    def load_sessions(self) -> List[Session]:
        """Load sessions from opencode.db. See AgentProvider.load_sessions."""
        db_path = get_opencode_db_path()
        if not db_path.is_file():
            return []

        try:
            rows = query_readonly(db_path, _SESSIONS_QUERY)
            prompt_rows = query_readonly(db_path, _PROMPTS_QUERY)
        except sqlite3.Error as exc:
            print(f"Warning: Cannot read OpenCode sessions from {db_path}: {exc}", file=sys.stderr)
            return []

        prompts: Dict[str, List[str]] = {}
        for session_id, data in prompt_rows:
            try:
                text = json.loads(data).get("text")
            except (json.JSONDecodeError, AttributeError):
                continue
            if text:
                prompts.setdefault(session_id, []).append(text)

        sessions: List[Session] = []
        for session_id, directory, title, created_ms, updated_ms in rows:
            commands = prompts.get(session_id)
            # Hide sessions without any prompt, like the other agents
            if not commands:
                continue
            is_named = bool(title) and not title.startswith(_DEFAULT_TITLE_PREFIX)
            sessions.append(
                Session(
                    agent=self.key,
                    session_id=session_id,
                    project_path=directory,
                    created_at=ms_to_datetime(created_ms),
                    last_active_at=ms_to_datetime(updated_ms),
                    latest_command=commands[-1],
                    commands=commands,
                    name=title if is_named else None,
                )
            )
        return sessions

    def delete_sessions(self, sessions: List[Session]) -> List[str]:
        """Delete with `opencode session delete`. See AgentProvider.delete_sessions.

        The command also deletes the child sessions and, through the DB
        foreign keys, all messages of the session.
        """
        errors: List[str] = []
        for session in sessions:
            error = run_command(["opencode", "session", "delete", session.session_id])
            if error is not None:
                errors.append(error)
        return errors

    def resume_command(self, session: Session) -> List[str]:
        """Build `opencode --session <id>`. See AgentProvider.resume_command."""
        return ["opencode", "--session", session.session_id]
