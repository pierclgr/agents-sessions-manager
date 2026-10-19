"""Codex sessions: the threads table of ~/.codex/state_<N>.sqlite and ~/.codex/history.jsonl."""

from __future__ import annotations

import json
import sqlite3
import sys
from pathlib import Path
from typing import Dict, List, Optional

from src.data.agents.base import AgentProvider, run_command
from src.data.models import Session
from src.utils.formatting import ms_to_datetime
from src.utils.jsonl import remove_jsonl_lines
from src.utils.paths import get_codex_dir
from src.utils.sqlite import query_readonly

_STATE_DB_PREFIX = "state_"

# Top-level, non-archived threads with at least one user prompt (like the Codex resume picker)
_THREADS_QUERY = (
    "SELECT id, cwd, name, first_user_message, created_at_ms, updated_at_ms FROM threads"
    " WHERE archived = 0 AND thread_source IS NOT 'subagent' AND first_user_message != ''"
)


class CodexProvider(AgentProvider):
    """Sessions (threads) of the Codex CLI."""

    key = "codex"
    label = "Codex"

    def load_sessions(self) -> List[Session]:
        """Load threads from the newest state DB. See AgentProvider.load_sessions."""
        codex_dir = get_codex_dir()
        db_path = _find_state_db(codex_dir)
        if db_path is None:
            return []

        try:
            rows = query_readonly(db_path, _THREADS_QUERY)
        except sqlite3.Error as exc:
            print(f"Warning: Cannot read Codex sessions from {db_path}: {exc}", file=sys.stderr)
            return []

        prompts = _load_prompts(codex_dir / "history.jsonl")
        sessions: List[Session] = []
        for thread_id, cwd, name, first_message, created_ms, updated_ms in rows:
            # Prompt history can be missing (e.g. disabled): fall back to the first message
            commands = prompts.get(thread_id) or [first_message]
            sessions.append(
                Session(
                    agent=self.key,
                    session_id=thread_id,
                    project_path=cwd,
                    created_at=ms_to_datetime(created_ms),
                    last_active_at=ms_to_datetime(updated_ms),
                    latest_command=commands[-1],
                    commands=commands,
                    name=name or None,
                )
            )
        return sessions

    def delete_sessions(self, sessions: List[Session]) -> List[str]:
        """Delete with `codex delete --force`, then clean history.jsonl. See AgentProvider.delete_sessions.

        `codex delete` removes the rollout files, the DB rows, the name index
        and the subagent threads, but not the prompt history.
        """
        errors: List[str] = []
        deleted_ids = set()

        for session in sessions:
            error = run_command(["codex", "delete", "--force", session.session_id])
            if error is None:
                deleted_ids.add(session.session_id)
            else:
                errors.append(error)

        if deleted_ids:
            history_path = get_codex_dir() / "history.jsonl"
            try:
                remove_jsonl_lines(history_path, "session_id", deleted_ids)
            except OSError as exc:
                errors.append(f"Could not rewrite {history_path}: {exc}")

        return errors

    def resume_command(self, session: Session) -> List[str]:
        """Build `codex resume <id>`. See AgentProvider.resume_command."""
        return ["codex", "resume", session.session_id]


def _find_state_db(codex_dir: Path) -> Optional[Path]:
    """Find the newest Codex state DB (state_<version>.sqlite).

    Args:
        codex_dir: The Codex data directory.

    Returns:
        The DB with the highest version, or None if there is none.
    """
    versions: Dict[int, Path] = {}
    for path in codex_dir.glob(f"{_STATE_DB_PREFIX}*.sqlite"):
        version = path.stem[len(_STATE_DB_PREFIX):]
        if version.isdigit():
            versions[int(version)] = path
    return versions[max(versions)] if versions else None


def _load_prompts(history_path: Path) -> Dict[str, List[str]]:
    """Read the user prompts of each session from Codex's history.jsonl.

    Args:
        history_path: The Codex prompt history file.

    Returns:
        Prompts per session ID, oldest first. Empty if the file is missing.
    """
    prompts: Dict[str, List[str]] = {}
    try:
        file = open(history_path, encoding="utf-8", errors="replace")
    except OSError:
        return prompts

    with file:
        for line in file:
            try:
                entry = json.loads(line)
            except json.JSONDecodeError:
                continue
            if not isinstance(entry, dict):
                continue
            session_id = entry.get("session_id")
            text = entry.get("text")
            if session_id and text:
                prompts.setdefault(session_id, []).append(text)
    return prompts
