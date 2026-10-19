"""File operations for deleting Claude Code sessions.

Handles atomic rewrite of history.jsonl, deletion of per-session files
and directories, and searching for session artifacts under ~/.claude/.
Also removes the data linked to a session by content rather than by
name: its plan file and the paste-cache files of its prompts.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import sys
from pathlib import Path
from typing import List, Optional, Set

from src.utils.jsonl import remove_jsonl_lines
from src.utils.paths import HISTORY_FILE, find_session_jsonl, get_claude_dir

# Plan slugs and paste hashes become file names: only allow safe characters
_SAFE_NAME = re.compile(r"[A-Za-z0-9_-]+")


def find_session_artifacts(session_id: str, claude_dir: Optional[Path] = None) -> List[Path]:
    """Find all files and directories under the Claude data dir whose name contains the session ID.

    Excludes history.jsonl itself (that is handled separately by the rewrite).

    Args:
        session_id: The session UUID to search for.
        claude_dir: Optional override for the Claude data directory.

    Returns:
        List of Path objects matching the session ID, sorted for consistent ordering.
    """
    root = claude_dir if claude_dir is not None else get_claude_dir()
    matches: List[Path] = []

    for dirpath, dirnames, filenames in os.walk(root):
        for name in filenames:
            if session_id in name and name != "history.jsonl":
                matches.append(Path(dirpath) / name)
        for name in dirnames:
            if session_id in name:
                matches.append(Path(dirpath) / name)

    matches.sort()
    return matches


def find_background_job(session_id: str, claude_dir: Optional[Path] = None) -> Optional[str]:
    """Find the background job (started with `claude --bg`) that runs a session.

    Args:
        session_id: The session UUID.
        claude_dir: Optional override for the Claude data directory.

    Returns:
        The job short ID (its folder name under jobs/), or None if the
        session is not a background session.
    """
    root = claude_dir if claude_dir is not None else get_claude_dir()

    for state_path in sorted((root / "jobs").glob("*/state.json")):
        try:
            with open(state_path, encoding="utf-8") as file:
                state = json.load(file)
        except (OSError, json.JSONDecodeError):
            continue
        if isinstance(state, dict) and state.get("sessionId") == session_id:
            return state_path.parent.name

    return None


def delete_sessions_from_history(session_ids: Set[str], claude_dir: Optional[Path] = None) -> int:
    """Remove all history.jsonl lines whose sessionId is in the given set.

    Also deletes the paste-cache files used only by the removed lines.

    Args:
        session_ids: Set of session UUIDs to remove.
        claude_dir: Optional override for the Claude data directory.

    Returns:
        Number of lines removed.

    Raises:
        OSError: If history.jsonl cannot be rewritten (it stays untouched).
    """
    root = claude_dir if claude_dir is not None else get_claude_dir()
    history_path = root / HISTORY_FILE

    removed = remove_jsonl_lines(history_path, "sessionId", session_ids)
    _delete_unused_pastes(removed, history_path, root)

    return len(removed)


def delete_session_files(session_id: str, claude_dir: Optional[Path] = None) -> List[Path]:
    """Delete all files and directories associated with a session.

    Removes:
    1. The per-session .jsonl file under projects/<slug>/
    2. The per-session directory under projects/<slug>/
    3. Any other files/dirs under ~/.claude/ whose name contains the session ID
    4. The session plan file plans/<slug>.md, if no other session uses it
    5. The project directory itself if it is now empty (project memory is kept)

    Only deletes under the Claude data directory — never traverses outside it.

    Args:
        session_id: The session UUID.
        claude_dir: Optional override for the Claude data directory.

    Returns:
        List of paths that were successfully deleted.
    """
    root = claude_dir if claude_dir is not None else get_claude_dir()
    deleted: List[Path] = []

    jsonl_path = find_session_jsonl(session_id, claude_dir=root)
    # Read the plan slug before the transcript is gone
    plan_slug = _read_plan_slug(jsonl_path) if jsonl_path is not None else None

    if jsonl_path is not None:
        # 1. Delete the primary .jsonl file
        try:
            jsonl_path.unlink()
            deleted.append(jsonl_path)
        except OSError as exc:
            print(f"Warning: Could not delete {jsonl_path}: {exc}", file=sys.stderr)

        # 2. Delete the per-session directory (same path without .jsonl extension)
        session_dir = jsonl_path.with_suffix("")
        if session_dir.is_dir():
            try:
                shutil.rmtree(session_dir)
                deleted.append(session_dir)
            except OSError as exc:
                print(f"Warning: Could not delete {session_dir}: {exc}", file=sys.stderr)

    # 3. Search for any other artifacts matching the session ID
    artifacts = find_session_artifacts(session_id, claude_dir=root)
    for artifact in artifacts:
        if artifact in deleted:
            continue

        # Safety: only delete under the Claude data directory
        try:
            artifact.resolve().relative_to(root.resolve())
        except ValueError:
            continue

        try:
            if artifact.is_dir():
                shutil.rmtree(artifact)
            else:
                artifact.unlink()
            deleted.append(artifact)
        except OSError as exc:
            print(f"Warning: Could not delete {artifact}: {exc}", file=sys.stderr)

    # 4. Delete the plan file, unless another session (e.g. a fork) still uses the same slug
    if plan_slug is not None and not _plan_slug_in_use(plan_slug, root):
        plan_path = root / "plans" / f"{plan_slug}.md"
        if plan_path.exists():
            try:
                plan_path.unlink()
                deleted.append(plan_path)
            except OSError as exc:
                print(f"Warning: Could not delete {plan_path}: {exc}", file=sys.stderr)

    # 5. Remove the project directory only if nothing is left (e.g. keep memory/)
    if jsonl_path is not None:
        project_dir = jsonl_path.parent
        if project_dir.is_dir() and not any(project_dir.iterdir()):
            try:
                project_dir.rmdir()
                deleted.append(project_dir)
            except OSError as exc:
                print(f"Warning: Could not delete {project_dir}: {exc}", file=sys.stderr)

    return deleted


def _read_plan_slug(jsonl_path: Path) -> Optional[str]:
    """Return the session slug, which is also the name of its plan file.

    Args:
        jsonl_path: The session transcript.

    Returns:
        The slug, or None if the transcript has none or cannot be read.
    """
    try:
        file = open(jsonl_path, encoding="utf-8", errors="replace")
    except OSError:
        return None

    with file:
        for line in file:
            if '"slug"' not in line:
                continue
            try:
                slug = json.loads(line).get("slug")
            except (json.JSONDecodeError, AttributeError):
                continue
            if isinstance(slug, str) and _SAFE_NAME.fullmatch(slug):
                return slug

    return None


def _plan_slug_in_use(slug: str, root: Path) -> bool:
    """Check if any remaining session transcript has the given slug.

    Args:
        slug: The plan slug to look for.
        root: The Claude data directory.

    Returns:
        True if at least one transcript still uses the slug.
    """
    # Claude Code writes compact JSON, so the key/value pair appears as-is
    marker = f'"slug":"{slug}"'
    for transcript in (root / "projects").glob("*/*.jsonl"):
        try:
            with open(transcript, encoding="utf-8", errors="replace") as file:
                if any(marker in line for line in file):
                    return True
        except OSError:
            continue
    return False


def _delete_unused_pastes(removed_entries: List[dict], history_path: Path, root: Path) -> None:
    """Delete the paste-cache files of removed history lines, if no kept line uses them.

    Args:
        removed_entries: The history entries that were removed.
        history_path: The rewritten history.jsonl.
        root: The Claude data directory.
    """
    hashes: Set[str] = set()
    for entry in removed_entries:
        pasted = entry.get("pastedContents")
        if not isinstance(pasted, dict):
            continue
        for item in pasted.values():
            if isinstance(item, dict) and isinstance(item.get("contentHash"), str):
                hashes.add(item["contentHash"])

    if not hashes:
        return

    # Keep pastes that the remaining history lines still reference
    try:
        with open(history_path, encoding="utf-8", errors="replace") as file:
            for line in file:
                hashes = {paste_hash for paste_hash in hashes if paste_hash not in line}
    except FileNotFoundError:
        pass
    except OSError:
        # Cannot tell which pastes are still used: keep them all
        return

    for paste_hash in hashes:
        if not _SAFE_NAME.fullmatch(paste_hash):
            continue
        paste_path = root / "paste-cache" / f"{paste_hash}.txt"
        if paste_path.exists():
            try:
                paste_path.unlink()
            except OSError as exc:
                print(f"Warning: Could not delete {paste_path}: {exc}", file=sys.stderr)
