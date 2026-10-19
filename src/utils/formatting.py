"""Formatting helpers for timestamps and text display."""

from __future__ import annotations

from datetime import datetime, timezone


def ms_to_datetime(timestamp_ms: int) -> datetime:
    """Convert Unix milliseconds timestamp to local datetime."""
    return datetime.fromtimestamp(timestamp_ms / 1000, tz=timezone.utc).astimezone()


def iso_to_datetime(timestamp: str) -> datetime:
    """Convert an ISO 8601 UTC timestamp (e.g. '2026-09-07T14:15:20.761Z') to local datetime.

    Raises:
        ValueError: If the timestamp is not valid ISO 8601.
    """
    # fromisoformat() accepts the 'Z' suffix only from Python 3.11
    if timestamp.endswith("Z"):
        timestamp = timestamp[:-1] + "+00:00"
    return datetime.fromisoformat(timestamp).astimezone()


def format_datetime(dt: datetime) -> str:
    """Format datetime as 'YYYY-MM-DD HH:MM'."""
    return dt.strftime("%Y-%m-%d %H:%M")


def truncate(text: str, max_length: int) -> str:
    """Truncate text to max_length, appending '\u2026' if truncated."""
    if len(text) <= max_length:
        return text
    return text[: max_length - 1] + "\u2026"
