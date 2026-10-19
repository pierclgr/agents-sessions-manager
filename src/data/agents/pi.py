"""Pi sessions: one JSONL file per session in ~/.pi/agent/sessions/--<path>--/."""

from __future__ import annotations

import json
from pathlib import Path
from typing import List, Optional

from src.data.agents.base import AgentProvider
from src.data.models import Session
from src.utils.formatting import iso_to_datetime
from src.utils.paths import get_pi_sessions_dir


class PiProvider(AgentProvider):
    """Sessions of the Pi coding agent."""

    key = "pi"
    label = "Pi"

    def load_sessions(self) -> List[Session]:
        """Load sessions from the session files. See AgentProvider.load_sessions."""
        sessions: List[Session] = []
        for path in sorted(get_pi_sessions_dir().glob("*/*.jsonl")):
            session = _parse_session_file(path)
            if session is not None:
                sessions.append(session)
        return sessions

    def delete_sessions(self, sessions: List[Session]) -> List[str]:
        """Delete the session files. See AgentProvider.delete_sessions.

        The project folder is removed too when no session is left in it.
        """
        errors: List[str] = []
        for session in sessions:
            path = _find_session_file(session.session_id)
            if path is None:
                errors.append(f"Pi session file not found for {session.session_id}")
                continue
            try:
                path.unlink()
            except OSError as exc:
                errors.append(f"Could not delete {path}: {exc}")
                continue
            # Remove the project folder if it is now empty
            if not any(path.parent.iterdir()):
                try:
                    path.parent.rmdir()
                except OSError:
                    pass
        return errors

    def resume_command(self, session: Session) -> List[str]:
        """Build `pi --session <file>`. See AgentProvider.resume_command.

        The file path is unambiguous; the session ID is used only if the
        file cannot be found.
        """
        path = _find_session_file(session.session_id)
        return ["pi", "--session", str(path) if path is not None else session.session_id]


def _find_session_file(session_id: str) -> Optional[Path]:
    """Find the file of a session (<timestamp>_<session_id>.jsonl).

    Args:
        session_id: The session UUID.

    Returns:
        The session file, or None if it does not exist.
    """
    matches = sorted(get_pi_sessions_dir().glob(f"*/*_{session_id}.jsonl"))
    return matches[0] if matches else None


def _parse_session_file(path: Path) -> Optional[Session]:
    """Build a Session from a Pi session file.

    The first entry is the header ({"type": "session", "id", "cwd",
    "timestamp"}). User prompts are "message" entries with role "user".
    The name is the one of the latest "session_info" entry.

    Args:
        path: The session file.

    Returns:
        The session, or None if the file has no header, no user prompt,
        or cannot be read.
    """
    header: Optional[dict] = None
    commands: List[str] = []
    name: Optional[str] = None
    last_timestamp: Optional[str] = None

    try:
        with open(path, encoding="utf-8", errors="replace") as file:
            for line in file:
                try:
                    entry = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if not isinstance(entry, dict):
                    continue

                if header is None:
                    # The header must come first
                    if entry.get("type") != "session":
                        return None
                    header = entry
                    continue

                if isinstance(entry.get("timestamp"), str):
                    last_timestamp = entry["timestamp"]

                if entry.get("type") == "session_info":
                    name = entry.get("name") or None
                elif entry.get("type") == "message":
                    message = entry.get("message")
                    if isinstance(message, dict) and message.get("role") == "user":
                        text = _message_text(message.get("content"))
                        if text:
                            commands.append(text)
    except OSError:
        return None

    if header is None or not commands:
        return None

    try:
        created_at = iso_to_datetime(header["timestamp"])
        last_active_at = iso_to_datetime(last_timestamp) if last_timestamp else created_at
        return Session(
            agent=PiProvider.key,
            session_id=header["id"],
            project_path=header["cwd"],
            created_at=created_at,
            last_active_at=last_active_at,
            latest_command=commands[-1],
            commands=commands,
            name=name,
        )
    except (KeyError, TypeError, ValueError):
        # Header without id/cwd/timestamp or with a bad timestamp
        return None


def _message_text(content: object) -> str:
    """Extract the text of a message content (a string or a list of blocks).

    Args:
        content: The "content" of a Pi message.

    Returns:
        The text blocks joined by spaces. Empty for non-text content.
    """
    if isinstance(content, str):
        return content
    if not isinstance(content, list):
        return ""
    texts = [block.get("text", "") for block in content if isinstance(block, dict) and block.get("type") == "text"]
    return " ".join(text for text in texts if text)
