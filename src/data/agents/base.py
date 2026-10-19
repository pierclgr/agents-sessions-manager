"""Common interface for the agents whose sessions the tool manages."""

from __future__ import annotations

import subprocess
from abc import ABC, abstractmethod
from typing import List, Optional

from src.data.models import Session


class AgentProvider(ABC):
    """Loads, deletes and resumes the sessions of one coding agent.

    Attributes:
        key: Short agent ID stored in Session.agent (e.g. "claude").
        label: Display name of the agent (e.g. "Claude").
    """

    key: str
    label: str

    @abstractmethod
    def load_sessions(self) -> List[Session]:
        """Load all resumable sessions of the agent.

        Returns:
            The sessions, in any order. Empty if the agent has no data.
        """

    def load_detail(self, session: Session) -> Session:
        """Load the data shown only in the detail view (e.g. todos).

        Args:
            session: The session to enrich.

        Returns:
            The same Session object. By default nothing is added.
        """
        return session

    @abstractmethod
    def delete_sessions(self, sessions: List[Session]) -> List[str]:
        """Delete sessions and all their data.

        Args:
            sessions: Sessions of this agent to delete.

        Returns:
            One error message per problem. Empty if all deletes succeeded.
        """

    @abstractmethod
    def resume_command(self, session: Session) -> List[str]:
        """Build the command that resumes a session.

        Args:
            session: The session to resume.

        Returns:
            The argv to exec from the session's project directory.
        """


def run_command(argv: List[str]) -> Optional[str]:
    """Run an agent CLI command without showing its output.

    Output is captured so it does not break the curses screen, and stdin
    is closed so a command can never wait for an answer.

    Args:
        argv: The command and its arguments.

    Returns:
        An error message if the command could not run or failed, else None.
    """
    command = " ".join(argv)
    try:
        result = subprocess.run(argv, capture_output=True, text=True, stdin=subprocess.DEVNULL)
    except OSError as exc:
        return f"{command}: {exc}"

    if result.returncode != 0:
        # The last output line usually holds the reason
        output_lines = (result.stderr or result.stdout).strip().splitlines()
        reason = output_lines[-1] if output_lines else f"exit code {result.returncode}"
        return f"{command}: {reason}"

    return None
