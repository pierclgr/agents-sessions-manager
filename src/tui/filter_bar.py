"""Agent filter bar renderer for the TUI.

Draws a single line above the session list showing which agents are
shown, and the number key that toggles each of them.
"""

from __future__ import annotations

import curses
from typing import Dict, Set


def draw(stdscr: curses.window, labels: Dict[str, str], active: Set[str],
         pair: int, width: int, y: int) -> None:
    """Render the filter bar at the given row.

    Args:
        stdscr: The curses window to draw on.
        labels: Agent key -> display name, in toggle-key order (1, 2, ...).
        active: Agent keys whose sessions are shown.
        pair: Color pair for the text.
        width: Terminal width in columns.
        y: Row number where the filter bar should be drawn.
    """
    items = []
    for number, (agent, label) in enumerate(labels.items(), start=1):
        marker = "[x]" if agent in active else "[ ]"
        items.append(f"{marker} {number} {label}")
    text = " Agents: " + "  ".join(items)

    # Pad or truncate to fill the full width
    text = text[:width].ljust(width)

    try:
        stdscr.addstr(y, 0, text, curses.color_pair(pair) | curses.A_BOLD)
    except curses.error:
        pass
