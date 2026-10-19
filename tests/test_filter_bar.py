"""Tests for the agent filter bar renderer."""

from __future__ import annotations

import curses
import unittest
from unittest import mock

from src.tui import filter_bar
from tests.helpers import FakeWindow

NORMAL_PAIR = 2
AGENT_PAIRS = {"claude": 7, "codex": 8}
LABELS = {"claude": "Claude", "codex": "Codex"}


# color_pair(n) returns n, so tests can read which pair drew each cell
@mock.patch("curses.color_pair", side_effect=lambda pair: pair)
class FilterBarTest(unittest.TestCase):
    """Check the filter bar text and the agent label colors."""

    def test_text_and_label_colors(self, _color_pair: mock.Mock) -> None:
        window = FakeWindow()

        filter_bar.draw(window, LABELS, {"claude"}, NORMAL_PAIR, AGENT_PAIRS, width=80, y=0)

        line = window.lines()[0]
        self.assertEqual(line, " Agents: [x] 1 Claude  [ ] 2 Codex")
        # agent names in their color, the rest in the normal color
        for agent, label in LABELS.items():
            start = line.index(label)
            for column in range(start, start + len(label)):
                self.assertEqual(window.attr_at(0, column), AGENT_PAIRS[agent] | curses.A_BOLD)
        self.assertEqual(window.attr_at(0, line.index("[x]")), NORMAL_PAIR | curses.A_BOLD)

    def test_fills_and_truncates_to_width(self, _color_pair: mock.Mock) -> None:
        window = FakeWindow()

        filter_bar.draw(window, LABELS, set(), NORMAL_PAIR, AGENT_PAIRS, width=20, y=0)

        drawn_columns = sorted(column for _, column in window.cells)
        self.assertEqual(drawn_columns, list(range(20)))


if __name__ == "__main__":
    unittest.main()
