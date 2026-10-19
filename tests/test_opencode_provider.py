"""Tests for the OpenCode provider."""

from __future__ import annotations

import json
import os
import sqlite3
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from src.data.agents.opencode import OpenCodeProvider
from tests.helpers import completed

S_NAMED = "ses_named"
S_DEFAULT = "ses_default"
S_CHILD = "ses_child"
S_ARCHIVED = "ses_archived"
S_EMPTY = "ses_empty"


def _create_db(path: Path) -> None:
    """Create a minimal OpenCode v2 DB with sessions and messages."""
    path.parent.mkdir(parents=True)
    connection = sqlite3.connect(path)
    connection.execute(
        "CREATE TABLE session_v2 (id TEXT PRIMARY KEY, parent_id TEXT, directory TEXT NOT NULL,"
        " title TEXT, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, time_archived INTEGER)"
    )
    connection.execute(
        "CREATE TABLE session_message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, type TEXT NOT NULL,"
        " seq INTEGER NOT NULL, data TEXT NOT NULL)"
    )
    connection.executemany("INSERT INTO session_v2 VALUES (?, ?, ?, ?, ?, ?, ?)", [
        (S_NAMED, None, "/p/a", "My work", 1000, 9000, None),
        (S_DEFAULT, None, "/p/b", "New session - 2026-09-07T15:43:45.423Z", 2000, 8000, None),
        (S_CHILD, S_NAMED, "/p/a", "Child work", 3000, 7000, None),
        (S_ARCHIVED, None, "/p/a", "Old work", 4000, 6000, 5000),
        (S_EMPTY, None, "/p/a", "", 5000, 5000, None),
    ])
    messages = [
        ("m1", S_NAMED, "user", 2, {"text": "second"}),
        ("m2", S_NAMED, "user", 1, {"text": "first"}),
        ("m3", S_NAMED, "assistant", 3, {"text": "reply"}),
        ("m4", S_DEFAULT, "user", 1, {"text": "hello"}),
        ("m5", S_CHILD, "user", 1, {"text": "child prompt"}),
        ("m6", S_ARCHIVED, "user", 1, {"text": "archived prompt"}),
    ]
    connection.executemany(
        "INSERT INTO session_message VALUES (?, ?, ?, ?, ?)",
        [(mid, sid, kind, seq, json.dumps(data)) for mid, sid, kind, seq, data in messages],
    )
    connection.commit()
    connection.close()


class OpenCodeProviderTest(unittest.TestCase):
    """Load and delete sessions from a fake OpenCode data dir."""

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        env = mock.patch.dict(os.environ, {"XDG_DATA_HOME": str(self.root)})
        env.start()
        self.addCleanup(env.stop)
        self.addCleanup(self._tmp.cleanup)
        _create_db(self.root / "opencode" / "opencode.db")

    def test_load_sessions(self) -> None:
        sessions = {s.session_id: s for s in OpenCodeProvider().load_sessions()}

        self.assertEqual(set(sessions), {S_NAMED, S_DEFAULT})
        named = sessions[S_NAMED]
        self.assertEqual(named.agent, "opencode")
        self.assertEqual(named.name, "My work")
        self.assertEqual(named.project_path, "/p/a")
        self.assertEqual(named.commands, ["first", "second"])
        self.assertEqual(named.latest_command, "second")
        self.assertEqual(named.created_at.timestamp(), 1.0)
        self.assertEqual(named.last_active_at.timestamp(), 9.0)
        # the default title is not a user-set name
        self.assertIsNone(sessions[S_DEFAULT].name)

    def test_missing_db_gives_no_sessions(self) -> None:
        with mock.patch.dict(os.environ, {"XDG_DATA_HOME": str(self.root / "missing")}):
            self.assertEqual(OpenCodeProvider().load_sessions(), [])

    def test_delete_runs_cli(self) -> None:
        session = next(s for s in OpenCodeProvider().load_sessions() if s.session_id == S_NAMED)
        with mock.patch("src.data.agents.base.subprocess.run", return_value=completed()) as run:
            errors = OpenCodeProvider().delete_sessions([session])

        self.assertEqual(errors, [])
        self.assertEqual(run.call_args[0][0], ["opencode", "session", "delete", S_NAMED])

    def test_failed_delete_returns_error(self) -> None:
        session = next(s for s in OpenCodeProvider().load_sessions() if s.session_id == S_NAMED)
        with mock.patch("src.data.agents.base.subprocess.run", return_value=completed(1, "not found")):
            errors = OpenCodeProvider().delete_sessions([session])

        self.assertEqual(len(errors), 1)
        self.assertIn("not found", errors[0])

    def test_resume_command(self) -> None:
        session = OpenCodeProvider().load_sessions()[0]
        self.assertEqual(OpenCodeProvider().resume_command(session), ["opencode", "--session", session.session_id])


if __name__ == "__main__":
    unittest.main()
