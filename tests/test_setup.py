"""Check installation and both command names without changing the user's setup."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from typing import Optional


REPOSITORY = Path(__file__).resolve().parents[1]


class SetupTest(unittest.TestCase):
    """Install into temporary folders and run commands outside the repository."""

    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.repository = self.root / "repository's folder"
        self.bin_directory = self.root / "local bin"
        self.work_directory = self.root / "other directory"
        self.repository.mkdir()
        self.work_directory.mkdir()

        for filename in ("agents-sessions", "setup.sh"):
            shutil.copy2(REPOSITORY / filename, self.repository / filename)

        # Tiny modules expose the selected route and arguments without reading sessions.
        for package in ("src", "src/commands", "src/tui"):
            directory = self.repository / package
            directory.mkdir(exist_ok=True)
            (directory / "__init__.py").touch()

        module_code = (
            "import json\n"
            "import sys\n"
            "from pathlib import Path\n"
            "print(json.dumps({'module': __package__ + '.' + Path(__file__).stem, "
            "'args': sys.argv[1:]}))\n"
        )
        for module in ("src/tui/app.py", "src/commands/ls.py",
                       "src/commands/launch.py", "src/commands/delete.py"):
            (self.repository / module).write_text(module_code, encoding="utf-8")

    def _run_setup(self, directory: Optional[Path] = None) -> subprocess.CompletedProcess:
        """Use an isolated destination instead of the real user's bin directory."""
        destination = self.bin_directory if directory is None else directory
        return subprocess.run(
            [str(self.repository / "setup.sh"), str(destination)],
            cwd=self.work_directory,
            capture_output=True,
            text=True,
            timeout=10,
        )

    def _run_command(self, name: str, *arguments: str) -> subprocess.CompletedProcess:
        """Find installed commands through PATH and preserve argument boundaries."""
        environment = os.environ.copy()
        environment["PATH"] = str(self.bin_directory) + os.pathsep + environment["PATH"]
        return subprocess.run(
            [name, *arguments],
            cwd=self.work_directory,
            env=environment,
            capture_output=True,
            text=True,
            timeout=10,
        )

    def test_installs_both_commands_with_all_routes(self) -> None:
        result = self._run_setup()
        self.assertEqual(result.returncode, 0, result.stderr)

        for name in ("agents-sessions", "sessions"):
            installed = self.bin_directory / name
            self.assertTrue(installed.is_symlink())
            self.assertEqual(installed.resolve(), self.repository / "agents-sessions")
            for arguments, module, forwarded in (
                ((), "src.tui.app", []),
                (("ls",), "src.commands.ls", []),
                (("launch", "session with spaces"), "src.commands.launch", ["session with spaces"]),
                (("delete", "session-id"), "src.commands.delete", ["session-id"]),
            ):
                with self.subTest(name=name, arguments=arguments):
                    command = self._run_command(name, *arguments)
                    self.assertEqual(command.returncode, 0, command.stderr)
                    self.assertEqual(json.loads(command.stdout), {"module": module, "args": forwarded})

            help_result = self._run_command(name, "--help")
            self.assertEqual(help_result.returncode, 0, help_result.stderr)
            self.assertIn("Usage:", help_result.stdout)

    def test_setup_can_be_run_again(self) -> None:
        for _ in range(2):
            result = self._run_setup()
            self.assertEqual(result.returncode, 0, result.stderr)
        command = self._run_command("sessions", "ls")
        self.assertEqual(command.returncode, 0, command.stderr)

    def test_refuses_conflicts_before_installing_either_command(self) -> None:
        for name in ("agents-sessions", "sessions"):
            for kind in ("file", "directory", "broken-link"):
                with self.subTest(name=name, kind=kind):
                    destination = self.root / (name + "-" + kind)
                    destination.mkdir()
                    existing = destination / name
                    if kind == "file":
                        existing.write_text("keep me", encoding="utf-8")
                    elif kind == "directory":
                        existing.mkdir()
                    else:
                        existing.symlink_to("missing-target")

                    result = self._run_setup(destination)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(str(existing), result.stderr)
                    if kind == "file":
                        self.assertEqual(existing.read_text(encoding="utf-8"), "keep me")
                    elif kind == "directory":
                        self.assertTrue(existing.is_dir())
                    else:
                        self.assertEqual(os.readlink(existing), "missing-target")

                    other = "sessions" if name == "agents-sessions" else "agents-sessions"
                    self.assertFalse((destination / other).exists())
                    self.assertFalse((destination / other).is_symlink())

    def test_printed_path_line_makes_both_commands_available(self) -> None:
        result = self._run_setup()
        self.assertEqual(result.returncode, 0, result.stderr)
        export_line = next(line for line in result.stdout.splitlines() if line.startswith("export PATH="))
        shell = subprocess.run(
            ["bash", "-c", "set -e\n" + export_line + "\ncommand -v agents-sessions\ncommand -v sessions"],
            cwd=self.work_directory,
            capture_output=True,
            text=True,
            timeout=10,
        )
        self.assertEqual(shell.returncode, 0, shell.stderr)
        self.assertEqual(shell.stdout.splitlines(), [
            str(self.bin_directory / "agents-sessions"),
            str(self.bin_directory / "sessions"),
        ])

    def test_launcher_follows_relative_and_chained_links(self) -> None:
        result = self._run_setup()
        self.assertEqual(result.returncode, 0, result.stderr)
        (self.bin_directory / "shortcut").symlink_to("sessions")
        command = self._run_command("shortcut", "ls")
        self.assertEqual(command.returncode, 0, command.stderr)
        self.assertEqual(json.loads(command.stdout), {"module": "src.commands.ls", "args": []})


if __name__ == "__main__":
    unittest.main()
