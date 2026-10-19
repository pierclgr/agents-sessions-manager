"""Tests for the Claude Code delete operations (no session data left behind)."""

from __future__ import annotations

import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from src.data.agents.claude import ClaudeProvider
from src.data.delete import delete_session_files, delete_sessions_from_history, find_background_job
from src.utils.paths import find_session_jsonl
from tests.helpers import completed, make_session, read_jsonl, write_jsonl

# session A and B share a plan slug; C is alone in another project; D is a background session
SID_A = "aaaaaaaa-0000-0000-0000-000000000001"
SID_B = "bbbbbbbb-0000-0000-0000-000000000002"
SID_C = "cccccccc-0000-0000-0000-000000000003"
SID_D = "dddddddd-0000-0000-0000-000000000004"

# Claude Code turns every non-alphanumeric char of the path into "-"
PROJECT_AB = "/p/my_proj.x"
SLUG_AB = "-p-my-proj-x"
PROJECT_C = "/p/other"
SLUG_C = "-p-other"


class ClaudeDeleteTest(unittest.TestCase):
    """Build a fake ~/.claude tree and check what each delete removes."""

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        env = mock.patch.dict(os.environ, {"CLAUDE_DATA_DIR": str(self.root)})
        env.start()
        self.addCleanup(env.stop)
        self.addCleanup(self._tmp.cleanup)

        write_jsonl(self.root / "history.jsonl", [
            {"display": "a1", "timestamp": 1000, "project": PROJECT_AB, "sessionId": SID_A,
             "pastedContents": {"1": {"id": "1", "type": "text", "contentHash": "hashonlya"}}},
            {"display": "a2", "timestamp": 2000, "project": PROJECT_AB, "sessionId": SID_A,
             "pastedContents": {"1": {"id": "1", "type": "text", "contentHash": "hashshared"}}},
            {"display": "b1", "timestamp": 3000, "project": PROJECT_AB, "sessionId": SID_B},
            {"display": "c1", "timestamp": 4000, "project": PROJECT_C, "sessionId": SID_C,
             "pastedContents": {"1": {"id": "1", "type": "text", "contentHash": "hashshared"}}},
            {"display": "d1", "timestamp": 5000, "project": PROJECT_C, "sessionId": SID_D},
        ])
        projects = self.root / "projects"
        write_jsonl(projects / SLUG_AB / f"{SID_A}.jsonl", [{"type": "user", "slug": "shared-slug"}])
        write_jsonl(projects / SLUG_AB / f"{SID_B}.jsonl", [{"type": "user", "slug": "shared-slug"}])
        write_jsonl(projects / SLUG_AB / SID_A / "subagents" / "agent-1.jsonl", [{"type": "user"}])
        (projects / SLUG_AB / "memory").mkdir(parents=True)
        (projects / SLUG_AB / "memory" / "MEMORY.md").write_text("project memory")
        write_jsonl(projects / SLUG_C / f"{SID_C}.jsonl", [{"type": "user", "slug": "c-slug"}])
        write_jsonl(projects / SLUG_C / f"{SID_D}.jsonl", [{"type": "user"}])

        for plan in ("shared-slug", "c-slug"):
            (self.root / "plans").mkdir(exist_ok=True)
            (self.root / "plans" / f"{plan}.md").write_text("plan")
        (self.root / "paste-cache").mkdir()
        for paste_hash in ("hashonlya", "hashshared"):
            (self.root / "paste-cache" / f"{paste_hash}.txt").write_text("pasted")
        (self.root / "file-history" / SID_A).mkdir(parents=True)
        (self.root / "session-env" / SID_A).mkdir(parents=True)

        (self.root / "jobs" / SID_D[:8]).mkdir(parents=True)
        (self.root / "jobs" / SID_D[:8] / "state.json").write_text(json.dumps({"sessionId": SID_D}))

    def test_find_session_jsonl_with_special_chars_in_path(self) -> None:
        found = find_session_jsonl(SID_A, claude_dir=self.root)
        self.assertEqual(found, self.root / "projects" / SLUG_AB / f"{SID_A}.jsonl")
        self.assertIsNone(find_session_jsonl("missing", claude_dir=self.root))

    def test_history_lines_and_unused_pastes_removed(self) -> None:
        removed = delete_sessions_from_history({SID_A}, claude_dir=self.root)

        self.assertEqual(removed, 2)
        remaining_ids = {entry["sessionId"] for entry in read_jsonl(self.root / "history.jsonl")}
        self.assertNotIn(SID_A, remaining_ids)
        self.assertFalse((self.root / "paste-cache" / "hashonlya.txt").exists())
        # still used by session C
        self.assertTrue((self.root / "paste-cache" / "hashshared.txt").exists())

    def test_session_files_removed_and_shared_plan_kept(self) -> None:
        delete_session_files(SID_A, claude_dir=self.root)

        self.assertFalse((self.root / "projects" / SLUG_AB / f"{SID_A}.jsonl").exists())
        self.assertFalse((self.root / "projects" / SLUG_AB / SID_A).exists())
        self.assertFalse((self.root / "file-history" / SID_A).exists())
        self.assertFalse((self.root / "session-env" / SID_A).exists())
        # session B still uses the same plan
        self.assertTrue((self.root / "plans" / "shared-slug.md").exists())

    def test_plan_removed_with_last_session_and_memory_kept(self) -> None:
        delete_session_files(SID_A, claude_dir=self.root)
        delete_session_files(SID_B, claude_dir=self.root)

        self.assertFalse((self.root / "plans" / "shared-slug.md").exists())
        self.assertTrue((self.root / "projects" / SLUG_AB / "memory" / "MEMORY.md").exists())

    def test_empty_project_dir_removed(self) -> None:
        delete_session_files(SID_C, claude_dir=self.root)
        delete_session_files(SID_D, claude_dir=self.root)

        self.assertFalse((self.root / "plans" / "c-slug.md").exists())
        self.assertFalse((self.root / "projects" / SLUG_C).exists())

    def test_find_background_job(self) -> None:
        self.assertEqual(find_background_job(SID_D, claude_dir=self.root), SID_D[:8])
        self.assertIsNone(find_background_job(SID_A, claude_dir=self.root))

    def test_provider_runs_claude_rm_for_background_session(self) -> None:
        session = make_session("claude", SID_D)
        with mock.patch("src.data.agents.base.subprocess.run", return_value=completed()) as run:
            errors = ClaudeProvider().delete_sessions([session])

        self.assertEqual(errors, [])
        self.assertEqual(run.call_args[0][0], ["claude", "rm", SID_D[:8]])
        self.assertFalse((self.root / "projects" / SLUG_C / f"{SID_D}.jsonl").exists())

    def test_provider_skips_session_when_claude_rm_fails(self) -> None:
        sessions = [make_session("claude", SID_D), make_session("claude", SID_C)]
        with mock.patch("src.data.agents.base.subprocess.run", return_value=completed(1, "unpushed commits")):
            errors = ClaudeProvider().delete_sessions(sessions)

        self.assertEqual(len(errors), 1)
        self.assertIn("unpushed commits", errors[0])
        # D is kept entirely, C is deleted
        self.assertTrue((self.root / "projects" / SLUG_C / f"{SID_D}.jsonl").exists())
        remaining_ids = {entry["sessionId"] for entry in read_jsonl(self.root / "history.jsonl")}
        self.assertIn(SID_D, remaining_ids)
        self.assertNotIn(SID_C, remaining_ids)
        self.assertFalse((self.root / "projects" / SLUG_C / f"{SID_C}.jsonl").exists())


if __name__ == "__main__":
    unittest.main()
