"""Agent filter bar renderer for the TUI.

Draws a single line above the session list showing which agents are
shown, and the number key that toggles each of them. Each agent name
uses the agent color.
"""

from __future__ import annotations

import curses
from typing import Dict, List, Set, Tuple


def draw(stdscr: curses.window, labels: Dict[str, str], active: Set[str],
         pair: int, agent_pairs: Dict[str, int], width: int, y: int) -> None:
    """Render the filter bar at the given row.

    Args:
        stdscr: The curses window to draw on.
        labels: Agent key -> display name, in toggle-key order (1, 2, ...).
        active: Agent keys whose sessions are shown.
        pair: Color pair for the text.
        agent_pairs: Agent key -> color pair for the agent name.
        width: Terminal width in columns.
        y: Row number where the filter bar should be drawn.
    """
    normal_attr = curses.color_pair(pair) | curses.A_BOLD

    # Build the line as (text, attr) segments, so each agent name has its own color
    segments: List[Tuple[str, int]] = [(" Agents: ", normal_attr)]
    for number, (agent, label) in enumerate(labels.items(), start=1):
        if number > 1:
            segments.append(("  ", normal_attr))
        marker = "[x]" if agent in active else "[ ]"
        segments.append((f"{marker} {number} ", normal_attr))
        segments.append((label, curses.color_pair(agent_pairs[agent]) | curses.A_BOLD))

    # Draw the segments, truncated to the width, then pad the rest of the line
    column = 0
    try:
        for text, attr in segments:
            text = text[:width - column]
            if not text:
                break
            stdscr.addstr(y, column, text, attr)
            column += len(text)
        if column < width:
            stdscr.addstr(y, column, " " * (width - column), normal_attr)
    except curses.error:
        pass
