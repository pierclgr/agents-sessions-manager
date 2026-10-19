"""Path utilities for locating Claude Code data files."""

from __future__ import annotations

import os
from pathlib import Path
from typing import Optional

DEFAULT_CLAUDE_DIR = Path.home() / ".claude"
HISTORY_FILE = "history.jsonl"


def get_claude_dir() -> Path:
    """Return Claude data dir. Honors CLAUDE_DATA_DIR env var, defaults to ~/.claude/."""
    env = os.environ.get("CLAUDE_DATA_DIR")
    if env:
        return Path(env)
    return DEFAULT_CLAUDE_DIR


def get_history_path() -> Path:
    """Return full path to history.jsonl."""
    return get_claude_dir() / HISTORY_FILE


def find_session_jsonl(session_id: str, claude_dir: Optional[Path] = None) -> Optional[Path]:
    """Find the per-session transcript <claude_dir>/projects/<slug>/<session_id>.jsonl.

    The project folder name (slug) is not computed from the project path:
    Claude Code replaces every non-alphanumeric char and shortens long paths,
    so searching by session ID is the only reliable way.

    Args:
        session_id: The session UUID.
        claude_dir: Optional override for the Claude data directory.

    Returns:
        Path of the transcript, or None if it does not exist.
    """
    root = claude_dir if claude_dir is not None else get_claude_dir()
    matches = sorted((root / "projects").glob(f"*/{session_id}.jsonl"))
    return matches[0] if matches else None


def get_codex_dir() -> Path:
    """Return Codex data dir. Honors CODEX_HOME env var, defaults to ~/.codex/."""
    env = os.environ.get("CODEX_HOME")
    if env:
        return Path(env)
    return Path.home() / ".codex"


def get_opencode_db_path() -> Path:
    """Return OpenCode database path. Honors XDG_DATA_HOME, defaults to ~/.local/share/opencode/opencode.db."""
    env = os.environ.get("XDG_DATA_HOME")
    data_dir = Path(env) if env else Path.home() / ".local" / "share"
    return data_dir / "opencode" / "opencode.db"
