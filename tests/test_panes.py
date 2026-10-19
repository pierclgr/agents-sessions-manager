"""Tests for the left and right pane renderers."""

from __future__ import annotations

import unittest
from typing import Dict, List, Tuple
from unittest import mock

from src.tui import left_pane, right_pane
from tests.helpers import make_session

SID_NAMED = "aaaaaaaa-0000-0000-0000-000000000001"
SID_PLAIN = "bbbbbbbb-0000-0000-0000-000000000002"


class _FakeWindow:
    """Record addstr calls so tests can read the drawn lines."""

    def __init__(self) -> None:
        self.cells: Dict[Tuple[int, int], str] = {}

    def addstr(self, y: int, x: int, text: str, attr: int = 0) -> None:
        self.cells[(y, x)] = text

    def lines(self) -> List[str]:
        """Return the drawn text of each row, in row order."""
        rows: Dict[int, List[Tuple[int, str]]] = {}
        for (y, x), text in self.cells.items():
            rows.setdefault(y, []).append((x, text))
        return ["".join(text for _, text in sorted(rows[y])).rstrip() for y in sorted(rows)]


@mock.patch("curses.color_pair", return_value=0)
class PanesTest(unittest.TestCase):
    """Check the columns and the name/ID rules."""

    def test_left_pane_columns_and_session_label(self, _color_pair: mock.Mock) -> None:
        sessions = [
            make_session("claude", SID_NAMED, name="data-reader"),
            make_session("codex", SID_PLAIN),
        ]
        window = _FakeWindow()

        left_pane.draw(window, sessions, 0, 0, "SELECT", set(), 1, 2, 3, x=0, y=0, width=120, height=5)

        header, named_row, plain_row = window.lines()[:3]
        self.assertEqual(header.split(), ["Agent", "Project", "Session", "Modified", "Last", "message"])
        self.assertIn("claude", named_row)
        self.assertIn("data-reader", named_row)
        self.assertNotIn(SID_NAMED[:8], named_row)
        self.assertIn("codex", plain_row)
        self.assertIn(SID_PLAIN[:8], plain_row)

    def test_left_pane_marks_selected_sessions(self, _color_pair: mock.Mock) -> None:
        sessions = [make_session("claude", SID_NAMED), make_session("codex", SID_NAMED)]
        window = _FakeWindow()

        left_pane.draw(window, sessions, 0, 0, "DELETE", {("codex", SID_NAMED)}, 1, 2, 3,
                       x=0, y=0, width=120, height=5)

        _, claude_row, codex_row = window.lines()[:3]
        self.assertTrue(claude_row.startswith("[ ]"))
        self.assertTrue(codex_row.startswith("[x]"))

    def test_right_pane_shows_name_and_id(self, _color_pair: mock.Mock) -> None:
        window = _FakeWindow()

        right_pane.draw(window, make_session("pi", SID_NAMED, name="data-reader"), 1, x=0, y=0, width=60, height=30)

        lines = [line.strip() for line in window.lines()]
        self.assertEqual(lines[lines.index("Agent:") + 1], "pi")
        self.assertEqual(lines[lines.index("Name:") + 1], "data-reader")
        self.assertEqual(lines[lines.index("Session ID:") + 1], SID_NAMED)

    def test_right_pane_without_name_shows_only_id(self, _color_pair: mock.Mock) -> None:
        window = _FakeWindow()

        right_pane.draw(window, make_session("pi", SID_PLAIN), 1, x=0, y=0, width=60, height=30)

        lines = [line.strip() for line in window.lines()]
        self.assertNotIn("Name:", lines)
        self.assertEqual(lines[lines.index("Session ID:") + 1], SID_PLAIN)


if __name__ == "__main__":
    unittest.main()
