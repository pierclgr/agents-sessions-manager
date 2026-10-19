"""Main TUI application loop.

Manages curses setup/teardown, state (mode, cursor, selection, agent
filter), input dispatch, and the render loop. Coordinates filter bar,
left pane, right pane, and status bar renderers.
"""

from __future__ import annotations

import curses
import os
import sys
from pathlib import Path
from typing import Dict, List, Optional, Set, Tuple

from src.data import Session
from src.data.agents import PROVIDERS, load_all_sessions
from src.tui import filter_bar, left_pane, right_pane, status_bar

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

# Agent colors (R, G, B), in PROVIDERS order. Claude keeps the SELECT accent orange
_AGENT_COLORS: Dict[str, Tuple[int, int, int]] = {
    "claude": _SELECT_COLOR,
    "codex": (135, 206, 250),     # light blue
    "opencode": (144, 238, 144),  # light green
    "pi": (255, 128, 255),        # light magenta
}

# Built-in colors used when the terminal cannot define custom colors
_AGENT_FALLBACK_COLORS: Dict[str, int] = {
    "claude": curses.COLOR_RED,  # same as the SELECT accent fallback
    "codex": curses.COLOR_CYAN,
    "opencode": curses.COLOR_GREEN,
    "pi": curses.COLOR_MAGENTA,
}

# One color pair per agent, after the fixed pairs above
_AGENT_PAIRS: Dict[str, int] = {
    agent: _PAIR_MARKER + number for number, agent in enumerate(_AGENT_COLORS, start=1)
}

# Custom color IDs for the agents, after the IDs 20-22 used by _init_colors
_AGENT_FIRST_COLOR_ID = 23

# ASCII art banner (small figlet font, sans-serif)
_BANNER = [
    "   _   ___ ___ _  _ _____ ___   ___ ___ ___ ___ ___ ___  _  _ ___   __  __   _   _  _   _   ___ ___ ___ ",
    "  /_\\ / __| __| \\| |_   _/ __| / __| __/ __/ __|_ _/ _ \\| \\| / __| |  \\/  | /_\\ | \\| | /_\\ / __| __| _ \\",
    " / _ \\ (_ | _|| .` | | | \\__ \\ \\__ \\ _|\\__ \\__ \\| | (_) | .` \\__ \\ | |\\/| |/ _ \\| .` |/ _ \\ (_ | _||   /",
    "/_/ \\_\\___|___|_|\\_| |_| |___/ |___/___|___/___/___\\___/|_|\\_|___/ |_|  |_/_/ \\_\\_|\\_/_/ \\_\\___|___|_|_\\",
]
_BANNER_HEIGHT = len(_BANNER) + 1  # +1 for blank line after banner

# Keys 1-4 toggle the agents, in PROVIDERS order
_FILTER_KEYS: Dict[int, str] = {ord(str(number)): agent for number, agent in enumerate(PROVIDERS, start=1)}


class _State:
    """Mutable state container for the TUI.

    `all_sessions` holds every loaded session; `sessions` holds only the
    sessions of the agents enabled in `agent_filter`. The cursor and the
    selection refer to `sessions`.
    """

    def __init__(self, sessions: List[Session]) -> None:
        self.all_sessions = sessions
        self.agent_filter: Set[str] = set(PROVIDERS)
        self.sessions: List[Session] = list(sessions)
        self.mode = MODE_SELECT
        self.cursor = 0
        self.scroll_offset = 0
        # Session keys: (agent, session_id)
        self.selected: Set[Tuple[str, str]] = set()
        self.message: Optional[str] = None
        self.confirming = False
        # Track which session was last enriched to avoid re-reading
        self._enriched_key: Optional[Tuple[str, str]] = None


def _apply_filter(state: _State) -> None:
    """Rebuild the visible session list from the agent filter.

    Clamps the cursor and drops hidden sessions from the selection, so a
    session that is not visible can never be deleted.
    """
    state.sessions = [s for s in state.all_sessions if s.agent in state.agent_filter]
    visible_keys = {s.key for s in state.sessions}
    state.selected &= visible_keys
    state.cursor = min(state.cursor, max(0, len(state.sessions) - 1))
    state.scroll_offset = 0


def _toggle_agent(state: _State, agent: str) -> None:
    """Show or hide the sessions of one agent."""
    state.agent_filter ^= {agent}
    _apply_filter(state)


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

        # Agent colors: agent fg, default bg
        for offset, (agent, rgb) in enumerate(_AGENT_COLORS.items()):
            color_id = _AGENT_FIRST_COLOR_ID + offset
            _set(color_id, *rgb)
            curses.init_pair(_AGENT_PAIRS[agent], color_id, -1)
    else:
        # Fallback: use built-in colors
        curses.init_pair(_PAIR_SELECT_ACCENT, curses.COLOR_WHITE, curses.COLOR_RED)
        curses.init_pair(_PAIR_DELETE_ACCENT, curses.COLOR_WHITE, curses.COLOR_RED)
        curses.init_pair(_PAIR_NORMAL, -1, -1)
        curses.init_pair(_PAIR_DIVIDER_SELECT, curses.COLOR_RED, -1)
        curses.init_pair(_PAIR_DIVIDER_DELETE, curses.COLOR_RED, -1)
        curses.init_pair(_PAIR_MARKER, curses.COLOR_RED, -1)
        for agent, color in _AGENT_FALLBACK_COLORS.items():
            curses.init_pair(_AGENT_PAIRS[agent], color, -1)


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

    if max_y < 4 or max_x < 20:
        # Terminal too small
        try:
            stdscr.addstr(0, 0, "Terminal too small")
        except curses.error:
            pass
        stdscr.noutrefresh()
        curses.doupdate()
        return

    # Layout: banner at top, agent filter bar, panes, status bar at bottom
    # Hide banner if terminal is too short
    show_banner = max_y > _BANNER_HEIGHT + 5
    banner_h = _BANNER_HEIGHT if show_banner else 0
    filter_y = banner_h
    content_y = banner_h + 1
    status_y = max_y - 1
    pane_height = max_y - 2 - banner_h

    # Draw banner
    if show_banner:
        _draw_banner(stdscr, max_x)

    # Draw agent filter bar
    agent_labels = {agent: provider.label for agent, provider in PROVIDERS.items()}
    filter_bar.draw(stdscr, agent_labels, state.agent_filter, _PAIR_NORMAL, _AGENT_PAIRS, max_x, filter_y)

    # Empty state (no sessions, or all hidden by the filter)
    if not state.sessions:
        try:
            msg = "No sessions found."
            msg_y = content_y + pane_height // 2
            stdscr.addstr(msg_y, max(0, (max_x - len(msg)) // 2), msg)
        except curses.error:
            pass
        status_bar.draw(stdscr, state.mode, _accent_pair(state.mode),
                        max_x, status_y, message=" 1-4: Filter  |  Q: Quit")
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
    if state._enriched_key != current_session.key:
        PROVIDERS[current_session.agent].load_detail(current_session)
        state._enriched_key = current_session.key

    # Draw left pane
    left_pane.draw(
        stdscr, state.sessions, state.cursor, state.scroll_offset,
        state.mode, state.selected,
        accent_pair=_accent_pair(state.mode),
        normal_pair=_PAIR_NORMAL,
        marker_pair=_PAIR_MARKER,
        agent_pairs=_AGENT_PAIRS,
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
        session_key = state.sessions[state.cursor].key
        if session_key in state.selected:
            state.selected.discard(session_key)
        else:
            state.selected.add(session_key)
    elif key in (ord("A"), ord("a")):
        # Select all visible sessions, or clear the selection if all are selected
        visible_keys = {s.key for s in state.sessions}
        if state.selected == visible_keys:
            state.selected.clear()
        else:
            state.selected = visible_keys
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

    argv = PROVIDERS[session.agent].resume_command(session)

    # Must restore terminal before exec
    curses.endwin()
    os.chdir(project_dir)
    os.execvp(argv[0], argv)
    # If execvp returns (shouldn't happen), we can't continue the TUI
    return True


def _do_delete(state: _State) -> None:
    """Delete all selected sessions, agent by agent, and refresh the list."""
    selected_sessions = [s for s in state.sessions if s.key in state.selected]

    errors: List[str] = []
    for agent, provider in PROVIDERS.items():
        agent_sessions = [s for s in selected_sessions if s.agent == agent]
        if agent_sessions:
            errors.extend(provider.delete_sessions(agent_sessions))

    # Refresh sessions
    state.all_sessions = load_all_sessions()
    state.selected.clear()
    state.mode = MODE_SELECT
    state._enriched_key = None
    _apply_filter(state)

    # Count what is really gone: a failed delete leaves the session listed
    remaining_keys = {s.key for s in state.all_sessions}
    deleted_count = sum(1 for s in selected_sessions if s.key not in remaining_keys)
    state.message = f"Deleted {deleted_count} session(s)."
    if errors:
        state.message += f" {len(errors)} error(s): {errors[0]}"


def _main(stdscr: curses.window) -> None:
    """Curses main function — runs inside curses.wrapper."""
    # Setup
    curses.curs_set(0)  # hide cursor
    stdscr.keypad(True)
    stdscr.timeout(100)  # 100ms timeout for responsive resize handling
    _init_colors()

    sessions = load_all_sessions()
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

        # Agent filter keys work in both modes, even when no session is visible
        if key in _FILTER_KEYS:
            _toggle_agent(state, _FILTER_KEYS[key])
            continue

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
