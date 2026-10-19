"""Registry of the supported coding agents."""

from __future__ import annotations

from typing import Dict, List

from src.data.agents.base import AgentProvider
from src.data.agents.claude import ClaudeProvider
from src.data.agents.codex import CodexProvider
from src.data.models import Session

# Order matters: it sets the filter keys 1-4 in the TUI
PROVIDERS: Dict[str, AgentProvider] = {
    provider.key: provider
    for provider in (ClaudeProvider(), CodexProvider())
}


def load_all_sessions() -> List[Session]:
    """Load the sessions of all agents.

    Returns:
        All sessions, sorted by last activity, most recent first.
    """
    sessions: List[Session] = []
    for provider in PROVIDERS.values():
        sessions.extend(provider.load_sessions())
    sessions.sort(key=lambda s: s.last_active_at, reverse=True)
    return sessions
