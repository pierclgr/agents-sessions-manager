"""Data models for coding agent sessions."""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime
from typing import List, Optional, Tuple


@dataclass
class Session:
    """Represents a single coding agent session (Claude Code, Codex, OpenCode or Pi)."""

    agent: str
    session_id: str
    project_path: str
    created_at: datetime
    last_active_at: datetime
    latest_command: str
    commands: List[str]
    name: Optional[str] = None
    todos: List[str] = field(default_factory=list)

    @property
    def key(self) -> Tuple[str, str]:
        """Unique key across agents: (agent, session_id)."""
        return (self.agent, self.session_id)
