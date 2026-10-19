"""Per-session JSONL enrichment."""

from __future__ import annotations

import json
import sys
from typing import List

from src.data.models import Session
from src.utils.paths import find_session_jsonl


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
