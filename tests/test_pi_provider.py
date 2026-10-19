"""Tests for the Pi provider."""

from __future__ import annotations

import os
import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path
from unittest import mock

from src.data.agents.pi import PiProvider
from tests.helpers import write_jsonl

U_NAMED = "01a07c39-3bb8-73e4-8137-000000000001"
U_PLAIN = "01a07c39-3bb8-73e4-8137-000000000002"
U_EMPTY = "01a07c39-3bb8-73e4-8137-000000000003"


def _user_message(text: object, timestamp: str) -> dict:
    """Build a Pi user message entry."""
    return {"type": "message", "id": "m", "timestamp": timestamp, "message": {"role": "user", "content": text}}


class PiProviderTest(unittest.TestCase):
    """Load and delete sessions from a fake Pi sessions dir."""

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        env = mock.patch.dict(os.environ, {"PI_CODING_AGENT_SESSION_DIR": str(self.root)})
        env.start()
        self.addCleanup(env.stop)
        self.addCleanup(self._tmp.cleanup)

        self.named_file = self.root / "--p-a--" / f"2026-09-07T14-15-20-761Z_{U_NAMED}.jsonl"
        write_jsonl(self.named_file, [
            {"type": "session", "version": 3, "id": U_NAMED, "timestamp": "2026-09-07T14:15:20.761Z", "cwd": "/p/a"},
            _user_message([{"type": "text", "text": "first"}], "2026-09-07T14:15:24.000Z"),
            {"type": "message", "id": "r", "timestamp": "2026-09-07T14:15:30.000Z",
             "message": {"role": "assistant", "content": [{"type": "text", "text": "reply"}]}},
            {"type": "session_info", "id": "i1", "timestamp": "2026-09-07T14:16:00.000Z", "name": "old name"},
            {"type": "session_info", "id": "i2", "timestamp": "2026-09-07T14:17:00.000Z", "name": "new name"},
            _user_message("second", "2026-09-07T14:18:00.000Z"),
        ])
        write_jsonl(self.root / "--p-b--" / f"2026-09-08T08-13-31-551Z_{U_PLAIN}.jsonl", [
            {"type": "session", "version": 3, "id": U_PLAIN, "timestamp": "2026-09-08T08:13:31.551Z", "cwd": "/p/b"},
            _user_message([{"type": "text", "text": "hello"}], "2026-09-08T08:13:40.000Z"),
        ])
        # no user message: hidden
        write_jsonl(self.root / "--p-b--" / f"2026-09-08T09-00-00-000Z_{U_EMPTY}.jsonl", [
            {"type": "session", "version": 3, "id": U_EMPTY, "timestamp": "2026-09-08T09:00:00.000Z", "cwd": "/p/b"},
        ])
        # no session header: skipped
        write_jsonl(self.root / "--p-b--" / "broken.jsonl", [_user_message("lost", "2026-09-08T09:00:00.000Z")])

    def test_load_sessions(self) -> None:
        sessions = {s.session_id: s for s in PiProvider().load_sessions()}

        self.assertEqual(set(sessions), {U_NAMED, U_PLAIN})
        named = sessions[U_NAMED]
        self.assertEqual(named.agent, "pi")
        self.assertEqual(named.name, "new name")
        self.assertEqual(named.project_path, "/p/a")
        self.assertEqual(named.commands, ["first", "second"])
        self.assertEqual(named.latest_command, "second")
        self.assertEqual(named.created_at, datetime(2026, 9, 7, 14, 15, 20, 761000, tzinfo=timezone.utc))
        self.assertEqual(named.last_active_at, datetime(2026, 9, 7, 14, 18, tzinfo=timezone.utc))
        self.assertIsNone(sessions[U_PLAIN].name)

    def test_missing_dir_gives_no_sessions(self) -> None:
        with mock.patch.dict(os.environ, {"PI_CODING_AGENT_SESSION_DIR": str(self.root / "missing")}):
            self.assertEqual(PiProvider().load_sessions(), [])

    def test_delete_removes_file(self) -> None:
        session = next(s for s in PiProvider().load_sessions() if s.session_id == U_NAMED)

        errors = PiProvider().delete_sessions([session])

        self.assertEqual(errors, [])
        self.assertFalse(self.named_file.exists())
        # it was the only session of the project: the folder is removed too
        self.assertFalse(self.named_file.parent.exists())

    def test_delete_missing_file_returns_error(self) -> None:
        session = next(s for s in PiProvider().load_sessions() if s.session_id == U_NAMED)
        self.named_file.unlink()

        errors = PiProvider().delete_sessions([session])

        self.assertEqual(len(errors), 1)

    def test_resume_command_uses_session_file(self) -> None:
        session = next(s for s in PiProvider().load_sessions() if s.session_id == U_NAMED)
        self.assertEqual(PiProvider().resume_command(session), ["pi", "--session", str(self.named_file)])


if __name__ == "__main__":
    unittest.main()
