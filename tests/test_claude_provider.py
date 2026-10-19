"""Tests for loading Claude Code sessions through the provider."""

from __future__ import annotations

import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from src.data.agents.claude import ClaudeProvider
from tests.helpers import write_jsonl

SID_NAMED = "aaaaaaaa-0000-0000-0000-000000000001"
SID_PLAIN = "bbbbbbbb-0000-0000-0000-000000000002"
PROJECT = "/p/my_proj.x"
SLUG = "-p-my-proj-x"


class ClaudeProviderTest(unittest.TestCase):
    """Load sessions from a fake ~/.claude tree."""

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        env = mock.patch.dict(os.environ, {"CLAUDE_DATA_DIR": str(self.root)})
        env.start()
        self.addCleanup(env.stop)
        self.addCleanup(self._tmp.cleanup)

        write_jsonl(self.root / "history.jsonl", [
            {"display": "first", "timestamp": 1000, "project": PROJECT, "sessionId": SID_NAMED},
            {"display": "second", "timestamp": 2000, "project": PROJECT, "sessionId": SID_NAMED},
            {"display": "other", "timestamp": 3000, "project": PROJECT, "sessionId": SID_PLAIN},
        ])
        write_jsonl(self.root / "projects" / SLUG / f"{SID_NAMED}.jsonl", [
            {"type": "custom-title", "customTitle": "old-name", "sessionId": SID_NAMED},
            {"type": "ai-title", "aiTitle": "Auto title", "sessionId": SID_NAMED},
            {"type": "custom-title", "customTitle": "new-name", "sessionId": SID_NAMED},
            # a /rename title wins over a later AI title
            {"type": "ai-title", "aiTitle": "Newer auto title", "sessionId": SID_NAMED},
            {"type": "user", "todos": [{"content": "write tests"}]},
        ])
        # without /rename, the latest AI title is the name
        write_jsonl(self.root / "projects" / SLUG / f"{SID_PLAIN}.jsonl", [
            {"type": "ai-title", "aiTitle": "Old auto title", "sessionId": SID_PLAIN},
            {"type": "ai-title", "aiTitle": "Auto title", "sessionId": SID_PLAIN},
        ])

    def test_load_sessions(self) -> None:
        sessions = {s.session_id: s for s in ClaudeProvider().load_sessions()}

        self.assertEqual(set(sessions), {SID_NAMED, SID_PLAIN})
        named = sessions[SID_NAMED]
        self.assertEqual(named.agent, "claude")
        self.assertEqual(named.name, "new-name")
        self.assertEqual(named.commands, ["first", "second"])
        self.assertEqual(named.latest_command, "second")
        self.assertEqual(sessions[SID_PLAIN].name, "Auto title")

    def test_load_detail_reads_todos_for_path_with_special_chars(self) -> None:
        provider = ClaudeProvider()
        session = next(s for s in provider.load_sessions() if s.session_id == SID_NAMED)

        provider.load_detail(session)

        self.assertEqual(session.todos, ["write tests"])

    def test_missing_history_gives_no_sessions(self) -> None:
        (self.root / "history.jsonl").unlink()
        self.assertEqual(ClaudeProvider().load_sessions(), [])

    def test_resume_command(self) -> None:
        session = ClaudeProvider().load_sessions()[0]
        self.assertEqual(ClaudeProvider().resume_command(session), ["claude", "--resume", session.session_id])


if __name__ == "__main__":
    unittest.main()
