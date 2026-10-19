"""Tests for the TUI state: agent filter, select all and agent colors."""

from __future__ import annotations

import curses
import unittest
from unittest import mock

from src.data.agents import PROVIDERS
from src.tui import app
from src.tui.app import (
    _AGENT_COLORS,
    _AGENT_FALLBACK_COLORS,
    _AGENT_PAIRS,
    _DELETE_COLOR,
    _SELECT_COLOR,
    MODE_DELETE,
    MODE_SELECT,
    _handle_delete_key,
    _handle_select_key,
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

    def test_space_toggles_like_tab(self) -> None:
        self.state.mode = MODE_DELETE
        _handle_delete_key(ord(" "), self.state, None)
        self.assertEqual(self.state.selected, {("claude", "c1")})

        _handle_delete_key(ord(" "), self.state, None)
        self.assertEqual(self.state.selected, set())

    def test_shift_tab_selects_from_last_toggle_to_cursor(self) -> None:
        self.state.mode = MODE_DELETE
        _handle_delete_key(ord("\t"), self.state, None)
        self.state.cursor = 2

        _handle_delete_key(curses.KEY_BTAB, self.state, None)

        self.assertEqual(self.state.selected, {("claude", "c1"), ("codex", "x1"), ("claude", "c2")})

    def test_shift_tab_range_works_upwards(self) -> None:
        self.state.mode = MODE_DELETE
        self.state.cursor = 3
        _handle_delete_key(ord(" "), self.state, None)
        self.state.cursor = 1

        _handle_delete_key(curses.KEY_BTAB, self.state, None)

        self.assertEqual(self.state.selected, {("codex", "x1"), ("claude", "c2"), ("pi", "p1")})

    def test_shift_tab_without_toggle_selects_only_current(self) -> None:
        self.state.mode = MODE_DELETE
        self.state.cursor = 1

        _handle_delete_key(curses.KEY_BTAB, self.state, None)

        self.assertEqual(self.state.selected, {("codex", "x1")})

    def test_leaving_delete_mode_forgets_the_range_start(self) -> None:
        self.state.mode = MODE_DELETE
        _handle_delete_key(ord("\t"), self.state, None)
        _handle_delete_key(ord("d"), self.state, None)
        _handle_select_key(ord("d"), self.state, None)
        self.state.cursor = 2

        _handle_delete_key(curses.KEY_BTAB, self.state, None)

        self.assertEqual(self.state.selected, {("claude", "c2")})

    def test_d_toggles_between_select_and_delete_mode(self) -> None:
        _handle_select_key(ord("d"), self.state, None)
        self.assertEqual(self.state.mode, MODE_DELETE)

        self.state.selected = {("claude", "c1")}
        _handle_delete_key(ord("D"), self.state, None)
        self.assertEqual(self.state.mode, MODE_SELECT)
        self.assertEqual(self.state.selected, set())

    def test_s_key_does_nothing(self) -> None:
        self.assertIsNone(_handle_select_key(ord("s"), self.state, None))
        self.assertEqual(self.state.mode, MODE_SELECT)

        self.state.mode = MODE_DELETE
        _handle_delete_key(ord("s"), self.state, None)
        self.assertEqual(self.state.mode, MODE_DELETE)

    def _run_main(self, keys: str, empty: bool = False) -> _State:
        """Run the input loop with fake keys and no terminal or data writes."""
        window = mock.Mock()
        window.getch.side_effect = [ord(key) for key in keys]
        sessions = [] if empty else self.sessions
        with mock.patch.object(curses, "curs_set"), \
             mock.patch.object(app, "_init_colors"), \
             mock.patch.object(app, "_render") as render, \
             mock.patch.object(app, "load_all_sessions", return_value=sessions), \
             mock.patch.object(app, "_do_delete") as delete:
            app._main(window)

        self.assertEqual(window.getch.call_count, len(keys))
        delete.assert_not_called()
        return render.call_args[0][1]

    def test_escape_and_q_exit_in_both_modes_and_empty_lists(self) -> None:
        for key in ("\x1b", "q", "Q"):
            for prefix in ("", "d", "124", "d124"):
                with self.subTest(key=key, prefix=prefix):
                    self._run_main(prefix + key)
            with self.subTest(key=key, empty=True):
                self._run_main(key, empty=True)

    def test_quit_keys_cancel_delete_confirmation(self) -> None:
        for key in ("\x1b", "q", "Q"):
            with self.subTest(key=key):
                state = self._run_main("d \n" + key + "\x1b")
                self.assertFalse(state.confirming)
                self.assertEqual(state.selected, {("claude", "c1")})

    def test_every_agent_has_a_color(self) -> None:
        self.assertEqual(list(_AGENT_COLORS), list(PROVIDERS))
        self.assertEqual(list(_AGENT_FALLBACK_COLORS), list(PROVIDERS))
        # one distinct color pair per agent
        self.assertEqual(len(set(_AGENT_PAIRS.values())), len(PROVIDERS))

    def test_select_accent_is_muted_orange_and_other_colors_stay_the_same(self) -> None:
        self.assertEqual(_SELECT_COLOR, (163, 104, 71))
        self.assertEqual(_DELETE_COLOR, (55, 6, 3))
        self.assertEqual(_AGENT_COLORS["claude"], (202, 124, 94))


if __name__ == "__main__":
    unittest.main()
