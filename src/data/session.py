"""Per-session JSONL enrichment."""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import List, Optional

from src.data.models import Session
from src.utils.paths import find_session_jsonl


def load_session_name(session_id: str, claude_dir: Optional[Path] = None) -> Optional[str]:
    """Read the user-set session name (set with /rename) from the per-session .jsonl file.

    Only lines containing "custom-title" are parsed, so large transcripts
    are scanned quickly. The last title wins. Auto-generated "ai-title"
    entries are ignored.

    Args:
        session_id: The session UUID.
        claude_dir: Optional override for the Claude data directory.

    Returns:
        The session name, or None if the session has none.
    """
    path = find_session_jsonl(session_id, claude_dir=claude_dir)
    if path is None:
        return None

    name: Optional[str] = None
    try:
        with open(path, encoding="utf-8", errors="replace") as file:
            for line in file:
                if '"custom-title"' not in line:
                    continue
                try:
                    entry = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if entry.get("type") == "custom-title":
                    name = entry.get("customTitle") or None
    except OSError:
        return None

    return name


def load_session_detail(session: Session) -> Session:
    """Read the per-session .jsonl file and enrich the Session object with todos.

    Returns the same Session object (mutated).
    If the file doesn't exist or can't be read, returns session unchanged.
    """
    path = find_session_jsonl(session.session_id)

    if path is None:
        return session

    try:
        file = open(path, encoding="utf-8")
    except (PermissionError, OSError):
        return session

    todos: List[str] = []

    with file:
        for line in file:
            line = line.strip()
            if not line:
                continue
            try:
                entry = json.loads(line)
            except json.JSONDecodeError:
                continue

            entry_todos = entry.get("todos")
            if not entry_todos or not isinstance(entry_todos, list):
                continue

            for item in entry_todos:
                if isinstance(item, str):
                    todos.append(item)
                elif isinstance(item, dict):
                    # Extract string representation from todo objects
                    text = item.get("content") or item.get("text") or item.get("title") or str(item)
                    todos.append(text)

    # Deduplicate while preserving order
    seen = set()
    unique_todos: List[str] = []
    for todo in todos:
        if todo not in seen:
            seen.add(todo)
            unique_todos.append(todo)

    session.todos = unique_todos
    return session
