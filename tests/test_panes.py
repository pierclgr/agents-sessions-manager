"""Tests for the left and right pane renderers."""

from __future__ import annotations

import unittest
from unittest import mock

from src.tui import left_pane, right_pane
from tests.helpers import FakeWindow, make_session

SID_NAMED = "aaaaaaaa-0000-0000-0000-000000000001"
SID_PLAIN = "bbbbbbbb-0000-0000-0000-000000000002"

ACCENT_PAIR = 1
NORMAL_PAIR = 2
MARKER_PAIR = 3
AGENT_PAIRS = {"claude": 7, "codex": 8}


# color_pair(n) returns n, so tests can read which pair drew each cell
@mock.patch("curses.color_pair", side_effect=lambda pair: pair)
class PanesTest(unittest.TestCase):
    """Check the columns, the name/ID rules and the agent colors."""

    def test_left_pane_columns_and_session_label(self, _color_pair: mock.Mock) -> None:
        sessions = [
            make_session("claude", SID_NAMED, name="data-reader"),
            make_session("codex", SID_PLAIN),
        ]
        window = FakeWindow()

        left_pane.draw(window, sessions, 0, 0, "SELECT", set(), ACCENT_PAIR, NORMAL_PAIR, MARKER_PAIR, AGENT_PAIRS,
                       x=0, y=0, width=120, height=5)

        header, named_row, plain_row = window.lines()[:3]
        self.assertEqual(header.split(), ["Agent", "Project", "Session", "Modified", "Last", "message"])
        self.assertIn("claude", named_row)
        self.assertIn("data-reader", named_row)
        self.assertNotIn(SID_NAMED[:8], named_row)
        self.assertIn("codex", plain_row)
        self.assertIn(SID_PLAIN[:8], plain_row)

    def test_left_pane_colors_agent_names(self, _color_pair: mock.Mock) -> None:
        sessions = [make_session("claude", SID_NAMED), make_session("codex", SID_PLAIN)]
        window = FakeWindow()

        left_pane.draw(window, sessions, 0, 0, "SELECT", set(), ACCENT_PAIR, NORMAL_PAIR, MARKER_PAIR, AGENT_PAIRS,
                       x=0, y=0, width=120, height=5)

        # the highlighted row keeps the accent color, also on the agent name
        self.assertEqual(window.attr_at(1, 0), ACCENT_PAIR)
        # other rows: agent name in its color, the rest in the normal color
        for column in range(len("codex")):
            self.assertEqual(window.attr_at(2, column), AGENT_PAIRS["codex"])
        self.assertEqual(window.attr_at(2, len("Agent") + 2), NORMAL_PAIR)

    def test_left_pane_marks_selected_sessions(self, _color_pair: mock.Mock) -> None:
        sessions = [make_session("claude", SID_NAMED), make_session("codex", SID_NAMED)]
        window = FakeWindow()

        left_pane.draw(window, sessions, 0, 0, "DELETE", {("codex", SID_NAMED)}, ACCENT_PAIR, NORMAL_PAIR,
                       MARKER_PAIR, AGENT_PAIRS, x=0, y=0, width=120, height=5)

        _, claude_row, codex_row = window.lines()[:3]
        self.assertTrue(claude_row.startswith("[ ]"))
        self.assertTrue(codex_row.startswith("[x]"))
        # the agent name starts after the 4-char marker
        self.assertEqual(window.attr_at(2, 4), AGENT_PAIRS["codex"])

    def test_right_pane_shows_name_and_id(self, _color_pair: mock.Mock) -> None:
        window = FakeWindow()

        right_pane.draw(window, make_session("pi", SID_NAMED, name="data-reader"), 1, x=0, y=0, width=60, height=30)

        lines = [line.strip() for line in window.lines()]
        self.assertEqual(lines[lines.index("Agent:") + 1], "pi")
        self.assertEqual(lines[lines.index("Name:") + 1], "data-reader")
        self.assertEqual(lines[lines.index("Session ID:") + 1], SID_NAMED)

    def test_right_pane_without_name_shows_only_id(self, _color_pair: mock.Mock) -> None:
        window = FakeWindow()

        right_pane.draw(window, make_session("pi", SID_PLAIN), 1, x=0, y=0, width=60, height=30)

        lines = [line.strip() for line in window.lines()]
        self.assertNotIn("Name:", lines)
        self.assertEqual(lines[lines.index("Session ID:") + 1], SID_PLAIN)


if __name__ == "__main__":
    unittest.main()
