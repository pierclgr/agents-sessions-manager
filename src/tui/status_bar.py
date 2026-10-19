"""Status bar renderer for the TUI.

Draws a single line at the bottom showing the current mode,
available keybindings, and contextual messages. Uses mode-dependent
accent color as background.
"""

from __future__ import annotations

import curses
from typing import Optional


# Key hint strings per mode
_SELECT_HINTS = (" MODE: SELECT  |  \u2191/\u2193 Navigate  |  1-4: Filter  |  Enter: Resume session"
                 "  |  D: Delete mode  |  ESC/Q: Quit")
_DELETE_HINTS = (" MODE: DELETE  |  \u2191/\u2193 Navigate  |  1-4: Filter  |  Tab/Space: Toggle select"
                 "  |  Shift+Tab: Select range  |  A: Select all  |  Enter: Delete selected  |  D: Select mode"
                 "  |  ESC/Q: Quit")


def draw(stdscr: curses.window, mode: str, accent_pair: int, width: int, y: int,
         message: Optional[str] = None) -> None:
    """Render the status bar at the given row.

    Args:
        stdscr: The curses window to draw on.
        mode: Current mode, "SELECT" or "DELETE".
        accent_pair: Curses color pair number for the accent background.
        width: Terminal width in columns.
        y: Row number where the status bar should be drawn.
        message: Optional override message (e.g. confirmation prompt).
    """
    if message is not None:
        text = " " + message
    elif mode == "DELETE":
        text = _DELETE_HINTS
    else:
        text = _SELECT_HINTS

    # Pad or truncate to fill the full width
    text = text[:width].ljust(width)

    try:
        stdscr.addstr(y, 0, text, curses.color_pair(accent_pair) | curses.A_BOLD)
    except curses.error:
        # Writing to the very last cell can raise on some terminals
        pass
