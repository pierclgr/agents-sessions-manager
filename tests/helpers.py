"""Shared helpers for building fake agent data in tests."""

from __future__ import annotations

import json
import subprocess
from datetime import datetime, timezone
from pathlib import Path
from typing import Dict, List, Optional, Tuple

from src.data.models import Session


class FakeWindow:
    """Record what curses draws: one (char, attr) per screen cell.

    Later calls overwrite earlier ones, like on a real screen.
    """

    def __init__(self) -> None:
        self.cells: Dict[Tuple[int, int], Tuple[str, int]] = {}

    def addstr(self, y: int, x: int, text: str, attr: int = 0) -> None:
        for offset, char in enumerate(text):
            self.cells[(y, x + offset)] = (char, attr)

    def lines(self) -> List[str]:
        """Return the drawn text of each row, in row order."""
        result: List[str] = []
        for y in sorted({row for row, _ in self.cells}):
            last_x = max(column for row, column in self.cells if row == y)
            text = "".join(self.cells.get((y, x), (" ", 0))[0] for x in range(last_x + 1))
            result.append(text.rstrip())
        return result

    def attr_at(self, y: int, x: int) -> int:
        """Return the attribute used to draw the cell at (y, x)."""
        return self.cells[(y, x)][1]


def write_jsonl(path: Path, entries: List[dict]) -> None:
    """Write entries as JSON lines, creating parent directories.

    Args:
        path: Destination file.
        entries: One dict per line.
    """
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w", encoding="utf-8") as file:
        for entry in entries:
            file.write(json.dumps(entry, separators=(",", ":")) + "\n")


def read_jsonl(path: Path) -> List[dict]:
    """Read a JSONL file into a list of dicts.

    Args:
        path: File to read.

    Returns:
        One dict per non-empty line.
    """
    with open(path, encoding="utf-8") as file:
        return [json.loads(line) for line in file if line.strip()]


def make_session(agent: str, session_id: str, name: Optional[str] = None,
                 minute: int = 0) -> Session:
    """Build a minimal Session for TUI and pane tests.

    Args:
        agent: Agent key, e.g. "claude".
        session_id: Session ID.
        name: Optional session name.
        minute: Minute of the last activity, used to order sessions.

    Returns:
        A Session with fixed dates and one command.
    """
    when = datetime(2026, 10, 19, 12, minute, tzinfo=timezone.utc)
    return Session(
        agent=agent,
        session_id=session_id,
        project_path="/p/proj",
        created_at=when,
        last_active_at=when,
        latest_command="hello",
        commands=["hello"],
        name=name,
    )


def completed(returncode: int = 0, stderr: str = "") -> subprocess.CompletedProcess:
    """Build a fake subprocess result.

    Args:
        returncode: Exit code of the fake command.
        stderr: Error output of the fake command.

    Returns:
        A CompletedProcess with empty stdout.
    """
    return subprocess.CompletedProcess(args=[], returncode=returncode, stdout="", stderr=stderr)
