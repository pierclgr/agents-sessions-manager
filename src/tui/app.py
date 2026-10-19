"""Main TUI application loop.

Manages curses setup/teardown, state (mode, cursor, selection),
input dispatch, and the render loop. Coordinates left pane, right
pane, and status bar renderers.
"""

from __future__ import annotations

import curses
import os
import sys
from pathlib import Path
from typing import List, Optional, Set

from src.data import (
    Session,
    delete_session_files,
    delete_sessions_from_history,
    load_session_detail,
    load_sessions,
)
from src.tui import left_pane, right_pane, status_bar

# Mode constants
MODE_SELECT = "SELECT"
MODE_DELETE = "DELETE"

# Accent colors (R, G, B)
_SELECT_COLOR = (202, 124, 94)
_DELETE_COLOR = (55, 6, 3)

# Color pair IDs
_PAIR_SELECT_ACCENT = 1
_PAIR_DELETE_ACCENT = 2
_PAIR_NORMAL = 3
_PAIR_DIVIDER_SELECT = 4
_PAIR_DIVIDER_DELETE = 5
_PAIR_MARKER = 6

# ASCII art banner (small figlet font, sans-serif)
_BANNER = [
    "  ___ _      _  _   _ ___  ___    ___ ___  ___  ___   ___ ___ ___ ___ ___ ___  _  _   __  __   _   _  _   _   ___ ___ ___ ",
    " / __| |    /_\\| | | |   \\| __|  / __/ _ \\|   \\| __| / __| __/ __/ __|_ _/ _ \\| \\| | |  \\/  | /_\\ | \\| | /_\\ / __| __| _ \\",
    "| (__| |__ / _ \\ |_| | |) | _|  | (_| (_) | |) | _|  \\__ \\ _|\\__ \\__ \\| | (_) | .` | | |\\/| |/ _ \\| .` |/ _ \\ (_ | _||   /",
    " \\___|____/_/ \\_\\___/|___/|___|  \\___\\___/|___/|___| |___/___|___/___/___\\___/|_|\\_| |_|  |_/_/ \\_\\_|\\_/_/ \\_\\___|___|_|_\\",
]
_BANNER_HEIGHT = len(_BANNER) + 1  # +1 for blank line after banner


class _State:
    """Mutable state container for the TUI."""

    def __init__(self, sessions: List[Session]) -> None:
        self.sessions = sessions
        self.mode = MODE_SELECT
        self.cursor = 0
        self.scroll_offset = 0
        self.selected: Set[str] = set()
        self.message: Optional[str] = None
        self.confirming = False
        # Track which session was last enriched to avoid re-reading
        self._enriched_id: Optional[str] = None


def _init_colors() -> None:
    """Initialize curses color pairs using true-color if available."""
    curses.start_color()
    curses.use_default_colors()

    # True-color support via init_color (requires can_change_color)
    if curses.can_change_color():
        # curses uses 0-1000 scale
        def _set(color_id: int, r: int, g: int, b: int) -> None:
            curses.init_color(color_id, r * 1000 // 255, g * 1000 // 255, b * 1000 // 255)

        _set(20, *_SELECT_COLOR)
        _set(21, *_DELETE_COLOR)
        _set(22, 255, 255, 255)  # white

        curses.init_pair(_PAIR_SELECT_ACCENT, 22, 20)   # white on select accent
        curses.init_pair(_PAIR_DELETE_ACCENT, 22, 21)    # white on delete accent
        curses.init_pair(_PAIR_NORMAL, -1, -1)           # default
        curses.init_pair(_PAIR_DIVIDER_SELECT, 20, -1)   # select accent fg, default bg
        curses.init_pair(_PAIR_DIVIDER_DELETE, 21, -1)    # delete accent fg, default bg
        curses.init_pair(_PAIR_MARKER, 21, -1)           # delete accent fg for markers
    else:
        # Fallback: use built-in colors
        curses.init_pair(_PAIR_SELECT_ACCENT, curses.COLOR_WHITE, curses.COLOR_RED)
        curses.init_pair(_PAIR_DELETE_ACCENT, curses.COLOR_WHITE, curses.COLOR_RED)
        curses.init_pair(_PAIR_NORMAL, -1, -1)
        curses.init_pair(_PAIR_DIVIDER_SELECT, curses.COLOR_RED, -1)
        curses.init_pair(_PAIR_DIVIDER_DELETE, curses.COLOR_RED, -1)
        curses.init_pair(_PAIR_MARKER, curses.COLOR_RED, -1)


def _accent_pair(mode: str) -> int:
    """Return the color pair ID for the current mode's accent."""
    return _PAIR_SELECT_ACCENT if mode == MODE_SELECT else _PAIR_DELETE_ACCENT


def _divider_pair(mode: str) -> int:
    """Return the color pair ID for the pane divider in the current mode."""
    return _PAIR_DIVIDER_SELECT if mode == MODE_SELECT else _PAIR_DIVIDER_DELETE


def _ensure_cursor_visible(state: _State, pane_height: int) -> None:
    """Adjust scroll_offset so the cursor is visible in the pane."""
    # Header row takes 1 line, so data rows = pane_height - 1
    data_height = pane_height - 1
    if state.cursor < state.scroll_offset:
        state.scroll_offset = state.cursor
    elif state.cursor >= state.scroll_offset + data_height:
        state.scroll_offset = state.cursor - data_height + 1


def _draw_banner(stdscr: curses.window, max_x: int) -> None:
    """Draw the ASCII art banner left-aligned at the top of the screen."""
    attr = curses.color_pair(_PAIR_DIVIDER_SELECT) | curses.A_BOLD
    for i, line in enumerate(_BANNER):
        truncated = line[:max_x - 1] if len(line) >= max_x else line
        try:
            stdscr.addstr(i, 0, truncated, attr)
        except curses.error:
            pass


def _draw_divider(stdscr: curses.window, col: int, y: int, height: int, pair: int) -> None:
    """Draw a vertical divider line between the two panes."""
    for row in range(height):
        try:
            stdscr.addstr(y + row, col, "\u2502", curses.color_pair(pair))
        except curses.error:
            pass


def _render(stdscr: curses.window, state: _State) -> None:
    """Perform a full render of the TUI."""
    stdscr.erase()
    max_y, max_x = stdscr.getmaxyx()

    if max_y < 3 or max_x < 20:
        # Terminal too small
        try:
            stdscr.addstr(0, 0, "Terminal too small")
        except curses.error:
            pass
        stdscr.noutrefresh()
        curses.doupdate()
        return

    # Layout: banner at top, status bar at bottom, panes in between
    # Hide banner if terminal is too short
    show_banner = max_y > _BANNER_HEIGHT + 5
    banner_h = _BANNER_HEIGHT if show_banner else 0
    content_y = banner_h
    status_y = max_y - 1
    pane_height = max_y - 1 - banner_h

    # Draw banner
    if show_banner:
        _draw_banner(stdscr, max_x)

    # Empty state
    if not state.sessions:
        try:
            msg = "No Claude Code sessions found."
            msg_y = content_y + pane_height // 2
            stdscr.addstr(msg_y, max(0, (max_x - len(msg)) // 2), msg)
        except curses.error:
            pass
        status_bar.draw(stdscr, state.mode, _accent_pair(state.mode),
                        max_x, status_y, message=" Q: Quit")
        stdscr.noutrefresh()
        curses.doupdate()
        return

    # Pane widths: left ~60%, divider 1 col, right ~40%
    left_width = max(20, int(max_x * 0.6))
    divider_col = left_width
    right_x = left_width + 1
    right_width = max_x - right_x

    # Hide right pane on narrow terminals
    show_right = max_x >= 60
    if not show_right:
        left_width = max_x

    _ensure_cursor_visible(state, pane_height)

    # Enrich the highlighted session lazily (for right pane detail)
    current_session = state.sessions[state.cursor]
    if state._enriched_id != current_session.session_id:
        load_session_detail(current_session)
        state._enriched_id = current_session.session_id

    # Draw left pane
    left_pane.draw(
        stdscr, state.sessions, state.cursor, state.scroll_offset,
        state.mode, state.selected,
        accent_pair=_accent_pair(state.mode),
        normal_pair=_PAIR_NORMAL,
        marker_pair=_PAIR_MARKER,
        x=0, y=content_y, width=left_width, height=pane_height,
    )

    if show_right:
        # Draw divider
        _draw_divider(stdscr, divider_col, content_y, pane_height, _divider_pair(state.mode))

        # Draw right pane
        right_pane.draw(
            stdscr, current_session, _PAIR_NORMAL,
            x=right_x, y=content_y, width=right_width, height=pane_height,
        )

    # Draw status bar
    status_bar.draw(stdscr, state.mode, _accent_pair(state.mode),
                    max_x, status_y, message=state.message)

    stdscr.noutrefresh()
    curses.doupdate()


def _handle_select_key(key: int, state: _State, stdscr: curses.window) -> Optional[str]:
    """Handle keypress in SELECT mode. Returns 'quit' or 'launch' or None."""
    if key in (curses.KEY_UP, ord("k")):
        if state.cursor > 0:
            state.cursor -= 1
    elif key in (curses.KEY_DOWN, ord("j")):
        if state.cursor < len(state.sessions) - 1:
            state.cursor += 1
    elif key in (ord("D"), ord("d")):
        state.mode = MODE_DELETE
        state.message = None
    elif key in (ord("Q"), ord("q")):
        return "quit"
    elif key in (curses.KEY_ENTER, ord("\n"), ord("\r")):
        return "launch"
    return None


def _handle_delete_key(key: int, state: _State, stdscr: curses.window) -> Optional[str]:
    """Handle keypress in DELETE mode. Returns 'quit' or None."""
    if key in (curses.KEY_UP, ord("k")):
        if state.cursor > 0:
            state.cursor -= 1
    elif key in (curses.KEY_DOWN, ord("j")):
        if state.cursor < len(state.sessions) - 1:
            state.cursor += 1
    elif key == ord("\t"):
        # Toggle selection on current session
        sid = state.sessions[state.cursor].session_id
        if sid in state.selected:
            state.selected.discard(sid)
        else:
            state.selected.add(sid)
    elif key in (ord("S"), ord("s")):
        state.mode = MODE_SELECT
        state.selected.clear()
        state.message = None
    elif key in (ord("Q"), ord("q")):
        return "quit"
    elif key in (curses.KEY_ENTER, ord("\n"), ord("\r")):
        if not state.selected:
            state.message = "No sessions selected."
        else:
            state.confirming = True
            state.message = f"Delete {len(state.selected)} session(s)? [y/N]"
    return None


def _do_launch(state: _State, stdscr: curses.window) -> bool:
    """Attempt to launch the highlighted session. Returns True if exec'd (never returns on success)."""
    session = state.sessions[state.cursor]
    project_dir = Path(session.project_path)

    if not project_dir.is_dir():
        state.message = f"Error: Directory '{session.project_path}' no longer exists."
        return False

    # Must restore terminal before exec
    curses.endwin()
    os.chdir(project_dir)
    os.execvp("claude", ["claude", "--resume", session.session_id])
    # If execvp returns (shouldn't happen), we can't continue the TUI
    return True


def _do_delete(state: _State) -> None:
    """Delete all selected sessions and refresh the list."""
    count = len(state.selected)

    # Remove from history.jsonl
    delete_sessions_from_history(state.selected)

    # Delete files for each session
    for session in state.sessions:
        if session.session_id in state.selected:
            delete_session_files(session.session_id)

    # Refresh sessions
    state.sessions = load_sessions()
    state.selected.clear()
    state.mode = MODE_SELECT
    state._enriched_id = None

    # Fix cursor position
    if state.cursor >= len(state.sessions):
        state.cursor = max(0, len(state.sessions) - 1)
    state.scroll_offset = 0

    state.message = f"Deleted {count} session(s)."


def _main(stdscr: curses.window) -> None:
    """Curses main function — runs inside curses.wrapper."""
    # Setup
    curses.curs_set(0)  # hide cursor
    stdscr.keypad(True)
    stdscr.timeout(100)  # 100ms timeout for responsive resize handling
    _init_colors()

    sessions = load_sessions()
    state = _State(sessions)

    while True:
        _render(stdscr, state)

        try:
            key = stdscr.getch()
        except curses.error:
            continue

        if key == -1:
            # Timeout — no input, just re-render (handles resize)
            continue

        if key == curses.KEY_RESIZE:
            stdscr.clear()
            continue

        # Handle confirmation prompt
        if state.confirming:
            if key in (ord("y"), ord("Y")):
                state.confirming = False
                state.message = None
                _do_delete(state)
            else:
                state.confirming = False
                state.message = None
            continue

        # Clear transient messages on any keypress
        state.message = None

        if not state.sessions:
            # Only Q works in empty state
            if key in (ord("Q"), ord("q")):
                break
            continue

        if state.mode == MODE_SELECT:
            action = _handle_select_key(key, state, stdscr)
            if action == "quit":
                break
            elif action == "launch":
                _do_launch(state, stdscr)
        elif state.mode == MODE_DELETE:
            action = _handle_delete_key(key, state, stdscr)
            if action == "quit":
                break


def run() -> None:
    """Entry point for the TUI. Sets up curses and runs the main loop."""
    # Ensure stderr is available for delete logging even under curses
    curses.wrapper(_main)


if __name__ == "__main__":
    run()
