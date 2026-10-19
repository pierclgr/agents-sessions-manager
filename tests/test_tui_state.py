"""Tests for the TUI state: agent filter, select all and agent colors."""

from __future__ import annotations

import unittest

from src.data.agents import PROVIDERS
from src.tui.app import (
    _AGENT_COLORS,
    _AGENT_FALLBACK_COLORS,
    _AGENT_PAIRS,
    MODE_DELETE,
    _handle_delete_key,
    _State,
    _toggle_agent,
)
from tests.helpers import make_session


class TuiStateTest(unittest.TestCase):
    """Check filter and selection rules without a real terminal."""

    def setUp(self) -> None:
        self.sessions = [
            make_session("claude", "c1", minute=5),
            make_session("codex", "x1", minute=4),
            make_session("claude", "c2", minute=3),
            make_session("pi", "p1", minute=2),
        ]
        self.state = _State(self.sessions)

    def test_all_agents_shown_at_start(self) -> None:
        self.assertEqual(self.state.agent_filter, {"claude", "codex", "opencode", "pi"})
        self.assertEqual(self.state.sessions, self.sessions)

    def test_toggle_agent_hides_and_shows_sessions(self) -> None:
        _toggle_agent(self.state, "claude")
        self.assertEqual([s.session_id for s in self.state.sessions], ["x1", "p1"])

        _toggle_agent(self.state, "claude")
        self.assertEqual(self.state.sessions, self.sessions)

    def test_toggle_agent_clamps_cursor(self) -> None:
        self.state.cursor = 3
        _toggle_agent(self.state, "pi")
        self.assertEqual(self.state.cursor, 2)

        for agent in ("claude", "codex"):
            _toggle_agent(self.state, agent)
        self.assertEqual(self.state.sessions, [])
        self.assertEqual(self.state.cursor, 0)

    def test_toggle_agent_drops_hidden_sessions_from_selection(self) -> None:
        self.state.selected = {("claude", "c1"), ("codex", "x1")}
        _toggle_agent(self.state, "claude")
        self.assertEqual(self.state.selected, {("codex", "x1")})

    def test_select_all_toggles_visible_sessions(self) -> None:
        self.state.mode = MODE_DELETE
        _toggle_agent(self.state, "pi")

        _handle_delete_key(ord("A"), self.state, None)
        self.assertEqual(self.state.selected, {("claude", "c1"), ("codex", "x1"), ("claude", "c2")})

        _handle_delete_key(ord("a"), self.state, None)
        self.assertEqual(self.state.selected, set())

    def test_every_agent_has_a_color(self) -> None:
        self.assertEqual(list(_AGENT_COLORS), list(PROVIDERS))
        self.assertEqual(list(_AGENT_FALLBACK_COLORS), list(PROVIDERS))
        # one distinct color pair per agent
        self.assertEqual(len(set(_AGENT_PAIRS.values())), len(PROVIDERS))


if __name__ == "__main__":
    unittest.main()
