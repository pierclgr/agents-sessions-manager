"""Parse history.jsonl and build Session objects."""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Dict, List, Optional

from src.data.models import Session
from src.data.session import load_session_name
from src.utils.formatting import ms_to_datetime
from src.utils.paths import get_history_path


def load_sessions(claude_dir: Optional[Path] = None) -> List[Session]:
    """Read history.jsonl, aggregate by sessionId, return flat list sorted by last_active_at descending.

    Returns an empty list if the file is not found (Claude Code not used).
    Raises SystemExit with message if permission denied.
    Skips malformed lines with a warning to stderr.
    """
    history_path = get_history_path() if claude_dir is None else claude_dir / "history.jsonl"

    try:
        file = open(history_path, encoding="utf-8")
    except FileNotFoundError:
        return []
    except PermissionError:
        print(f"Error: Cannot read {history_path}. Check permissions.", file=sys.stderr)
        sys.exit(1)

    entries: Dict[str, List[dict]] = {}

    with file:
        for lineno, line in enumerate(file, start=1):
            line = line.strip()
            if not line:
                continue
            try:
                entry = json.loads(line)
            except json.JSONDecodeError:
                print(f"Warning: Skipping malformed JSON on line {lineno} of {history_path}", file=sys.stderr)
                continue

            session_id = entry.get("sessionId")
            timestamp = entry.get("timestamp")
            display = entry.get("display")
            project = entry.get("project")

            if not all((session_id, timestamp, display, project)):
                continue

            entries.setdefault(session_id, []).append(entry)

    sessions: List[Session] = []
    for session_id, group in entries.items():
        group.sort(key=lambda e: e["timestamp"])
        timestamps = [e["timestamp"] for e in group]
        sessions.append(
            Session(
                agent="claude",
                session_id=session_id,
                project_path=group[0]["project"],
                created_at=ms_to_datetime(min(timestamps)),
                last_active_at=ms_to_datetime(max(timestamps)),
                latest_command=group[-1]["display"],
                commands=[e["display"] for e in group],
                name=load_session_name(session_id, claude_dir=claude_dir),
                todos=[],
            )
        )

    sessions.sort(key=lambda s: s.last_active_at, reverse=True)
    return sessions
