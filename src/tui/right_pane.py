"""Right pane renderer for the TUI.

Draws session details: agent, project path, session name (if any), full
session ID, latest commands, and todos. Updates reactively when the
cursor moves.
"""

from __future__ import annotations

import curses
from typing import List

from src.data.models import Session
from src.utils.formatting import format_datetime


def _draw_line(stdscr: curses.window, row: int, col: int, text: str,
               width: int, attr: int) -> int:
    """Draw a single line, wrapping or truncating to fit. Returns next row."""
    text = text[:width]
    try:
        stdscr.addstr(row, col, text.ljust(width), attr)
    except curses.error:
        pass
    return row + 1


def draw(stdscr: curses.window, session: Session, pair: int,
         x: int, y: int, width: int, height: int) -> None:
    """Render the detail view for the highlighted session.

    Args:
        stdscr: The curses window to draw on.
        session: The session to display details for.
        pair: Color pair for normal text.
        x: Starting column of the pane.
        y: Starting row of the pane.
        width: Width of the pane in columns.
        height: Height of the pane in rows.
    """
    attr = curses.color_pair(pair)
    content_width = width - 2  # 1-char padding on each side
    col = x + 1
    row = y
    max_row = y + height

    def _line(text: str = "") -> None:
        """Draw one line and advance the row counter."""
        nonlocal row
        if row >= max_row:
            return
        row = _draw_line(stdscr, row, col, text, content_width, attr)

    def _clear_remaining() -> None:
        """Clear any unused rows in the pane."""
        nonlocal row
        while row < max_row:
            _line()

    # Section: Agent
    _line("Agent:")
    _line(f"  {session.agent}")
    _line()

    # Section: Project
    _line("Project:")
    _line(f"  {session.project_path}")
    _line()

    # Section: Name (only for named sessions)
    if session.name:
        _line("Name:")
        _line(f"  {session.name}")
        _line()

    # Section: Session ID
    _line("Session ID:")
    _line(f"  {session.session_id}")
    _line()

    # Section: Dates
    _line("Created:")
    _line(f"  {format_datetime(session.created_at)}")
    _line()
    _line("Last active:")
    _line(f"  {format_datetime(session.last_active_at)}")
    _line()

    # Section: Latest commands (up to 5, most recent first)
    _line("Latest commands:")
    recent_commands: List[str] = session.commands[-5:] if session.commands else []
    recent_commands = list(reversed(recent_commands))
    if recent_commands:
        for i, cmd in enumerate(recent_commands, 1):
            cmd_text = cmd.strip()
            # truncate to fit pane width with numbering prefix
            max_cmd = content_width - 6
            if len(cmd_text) > max_cmd > 0:
                cmd_text = cmd_text[:max_cmd - 1] + "\u2026"
            _line(f"  {i}. {cmd_text}")
    else:
        _line("  (none)")
    _line()

    # Section: Todos
    _line("Todos:")
    if session.todos:
        for todo in session.todos:
            max_todo = content_width - 4
            if len(todo) > max_todo > 0:
                todo = todo[:max_todo - 1] + "\u2026"
            _line(f"  - {todo}")
    else:
        _line("  (none)")

    _clear_remaining()
