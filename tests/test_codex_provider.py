"""Tests for the Codex provider."""

from __future__ import annotations

import os
import sqlite3
import tempfile
import unittest
from pathlib import Path
from typing import List, Tuple
from unittest import mock

from src.data.agents.codex import CodexProvider
from tests.helpers import completed, read_jsonl, write_jsonl

T_NAMED = "01a00000-0000-7000-8000-000000000001"
T_PLAIN = "01a00000-0000-7000-8000-000000000002"
T_SUBAGENT = "01a00000-0000-7000-8000-000000000003"
T_ARCHIVED = "01a00000-0000-7000-8000-000000000004"
T_EMPTY = "01a00000-0000-7000-8000-000000000005"
T_OTHER = "01a00000-0000-7000-8000-000000000009"

# (id, cwd, name, first_user_message, created_at_ms, updated_at_ms, archived, thread_source)
_ThreadRow = Tuple[str, str, object, str, int, int, int, object]


def _create_state_db(path: Path, rows: List[_ThreadRow]) -> None:
    """Create a minimal Codex state DB with a threads table."""
    connection = sqlite3.connect(path)
    connection.execute(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, cwd TEXT NOT NULL, name TEXT,"
        " first_user_message TEXT NOT NULL DEFAULT '', created_at_ms INTEGER,"
        " updated_at_ms INTEGER, archived INTEGER NOT NULL DEFAULT 0, thread_source TEXT)"
    )
    connection.executemany("INSERT INTO threads VALUES (?, ?, ?, ?, ?, ?, ?, ?)", rows)
    connection.commit()
    connection.close()


class CodexProviderTest(unittest.TestCase):
    """Load and delete sessions from a fake ~/.codex tree."""

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        env = mock.patch.dict(os.environ, {"CODEX_HOME": str(self.root)})
        env.start()
        self.addCleanup(env.stop)
        self.addCleanup(self._tmp.cleanup)

        _create_state_db(self.root / "state_5.sqlite", [
            (T_NAMED, "/p/a", "my-thread", "first", 1000, 9000, 0, "user"),
            (T_PLAIN, "/p/b", None, "only prompt", 2000, 8000, 0, None),
            (T_SUBAGENT, "/p/a", None, "sub", 3000, 7000, 0, "subagent"),
            (T_ARCHIVED, "/p/a", None, "old", 4000, 6000, 1, "user"),
            (T_EMPTY, "/p/a", None, "", 5000, 5000, 0, "user"),
        ])
        # an older state DB version must be ignored
        _create_state_db(self.root / "state_4.sqlite", [
            (T_OTHER, "/p/old", None, "stale", 1, 1, 0, "user"),
        ])
        write_jsonl(self.root / "history.jsonl", [
            {"session_id": T_NAMED, "ts": 1, "text": "first"},
            {"session_id": T_OTHER, "ts": 2, "text": "keep me"},
            {"session_id": T_NAMED, "ts": 3, "text": "second"},
        ])

    def test_load_sessions(self) -> None:
        sessions = {s.session_id: s for s in CodexProvider().load_sessions()}

        self.assertEqual(set(sessions), {T_NAMED, T_PLAIN})
        named = sessions[T_NAMED]
        self.assertEqual(named.agent, "codex")
        self.assertEqual(named.name, "my-thread")
        self.assertEqual(named.project_path, "/p/a")
        self.assertEqual(named.commands, ["first", "second"])
        self.assertEqual(named.latest_command, "second")
        self.assertEqual(named.created_at.timestamp(), 1.0)
        self.assertEqual(named.last_active_at.timestamp(), 9.0)
        # no prompt history: fall back to the first user message
        plain = sessions[T_PLAIN]
        self.assertIsNone(plain.name)
        self.assertEqual(plain.commands, ["only prompt"])

    def test_missing_dir_gives_no_sessions(self) -> None:
        with mock.patch.dict(os.environ, {"CODEX_HOME": str(self.root / "missing")}):
            self.assertEqual(CodexProvider().load_sessions(), [])

    def test_delete_runs_cli_and_cleans_history(self) -> None:
        session = next(s for s in CodexProvider().load_sessions() if s.session_id == T_NAMED)
        with mock.patch("src.data.agents.base.subprocess.run", return_value=completed()) as run:
            errors = CodexProvider().delete_sessions([session])

        self.assertEqual(errors, [])
        self.assertEqual(run.call_args[0][0], ["codex", "delete", "--force", T_NAMED])
        remaining_ids = [entry["session_id"] for entry in read_jsonl(self.root / "history.jsonl")]
        self.assertEqual(remaining_ids, [T_OTHER])

    def test_failed_delete_keeps_history(self) -> None:
        session = next(s for s in CodexProvider().load_sessions() if s.session_id == T_NAMED)
        with mock.patch("src.data.agents.base.subprocess.run", return_value=completed(1, "fork references it")):
            errors = CodexProvider().delete_sessions([session])

        self.assertEqual(len(errors), 1)
        self.assertIn("fork references it", errors[0])
        remaining_ids = [entry["session_id"] for entry in read_jsonl(self.root / "history.jsonl")]
        self.assertEqual(remaining_ids.count(T_NAMED), 2)

    def test_resume_command(self) -> None:
        session = CodexProvider().load_sessions()[0]
        self.assertEqual(CodexProvider().resume_command(session), ["codex", "resume", session.session_id])


if __name__ == "__main__":
    unittest.main()
