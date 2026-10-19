"""Helpers for JSONL (JSON Lines) files."""

from __future__ import annotations

import json
from pathlib import Path
from typing import List, Set


def remove_jsonl_lines(path: Path, id_key: str, ids: Set[str]) -> List[dict]:
    """Remove all lines whose `id_key` value is in `ids`.

    Uses an atomic temp-file-then-rename approach: writes the kept lines to
    a .tmp file in the same directory, then renames it over the original.
    Empty and malformed lines are kept as they are.

    Args:
        path: The JSONL file to rewrite.
        id_key: The JSON key holding the session ID (e.g. "sessionId").
        ids: Session IDs whose lines are removed.

    Returns:
        The removed entries. Empty if the file does not exist.

    Raises:
        OSError: If the file cannot be read or written. The original file
            stays untouched.
    """
    try:
        file = open(path, encoding="utf-8")
    except FileNotFoundError:
        return []

    kept_lines: List[str] = []
    removed: List[dict] = []

    with file:
        for line in file:
            stripped = line.strip()
            if not stripped:
                kept_lines.append(line)
                continue
            try:
                entry = json.loads(stripped)
            except json.JSONDecodeError:
                kept_lines.append(line)
                continue

            if isinstance(entry, dict) and entry.get(id_key) in ids:
                removed.append(entry)
            else:
                kept_lines.append(line)

    if not removed:
        return removed

    tmp_path = path.with_suffix(path.suffix + ".tmp")
    try:
        with open(tmp_path, "w", encoding="utf-8") as tmp_file:
            tmp_file.writelines(kept_lines)
        tmp_path.replace(path)
    except OSError:
        # Clean up temp file on failure; original remains untouched
        try:
            tmp_path.unlink()
        except OSError:
            pass
        raise

    return removed
