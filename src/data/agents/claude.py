"""Claude Code sessions: ~/.claude/history.jsonl and per-session transcripts."""

from __future__ import annotations

from typing import List

from src.data.agents.base import AgentProvider, run_command
from src.data.delete import delete_session_files, delete_sessions_from_history, find_background_job
from src.data.history import load_sessions
from src.data.models import Session
from src.data.session import load_session_detail


class ClaudeProvider(AgentProvider):
    """Sessions of Claude Code."""

    key = "claude"
    label = "Claude"

    def load_sessions(self) -> List[Session]:
        """Load sessions from history.jsonl. See AgentProvider.load_sessions."""
        return load_sessions()

    def load_detail(self, session: Session) -> Session:
        """Load the todos from the transcript. See AgentProvider.load_detail."""
        return load_session_detail(session)

    def delete_sessions(self, sessions: List[Session]) -> List[str]:
        """Delete history lines and all session files. See AgentProvider.delete_sessions.

        Background sessions (claude --bg) are first removed with `claude rm`,
        which also cleans the daemon state. If that fails, the session is
        skipped, so it is never deleted only in part.
        """
        errors: List[str] = []
        deletable: List[Session] = []

        for session in sessions:
            job_id = find_background_job(session.session_id)
            if job_id is not None:
                error = run_command(["claude", "rm", job_id])
                if error is not None:
                    errors.append(error)
                    continue
            deletable.append(session)

        if not deletable:
            return errors

        try:
            delete_sessions_from_history({session.session_id for session in deletable})
        except OSError as exc:
            # Keep the files: the sessions are still listed in history.jsonl
            errors.append(f"Could not rewrite history.jsonl: {exc}")
            return errors

        for session in deletable:
            delete_session_files(session.session_id)

        return errors

    def resume_command(self, session: Session) -> List[str]:
        """Build `claude --resume <id>`. See AgentProvider.resume_command."""
        return ["claude", "--resume", session.session_id]
