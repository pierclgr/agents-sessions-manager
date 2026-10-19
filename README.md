# Agents Sessions Manager

A terminal tool for browsing, resuming, and deleting coding agent sessions across all your projects. Supported agents: [Claude Code](https://docs.anthropic.com/en/docs/claude-code), [Codex](https://github.com/openai/codex), [OpenCode](https://opencode.ai) and [Pi](https://github.com/earendil-works/pi-mono). Provides both an interactive full-screen TUI and non-interactive CLI subcommands.

Built with **Python 3.8+** using only the standard library (zero third-party dependencies). Works on **macOS** and **Linux**.

## Features

- **Interactive TUI** -- Full-screen curses interface with two-pane layout: session list on the left, session details on the right
- **Multi-agent** -- Shows the sessions of Claude Code, Codex, OpenCode and Pi in one list, and resumes each one with its own agent
- **SELECT mode** -- Browse all sessions and resume any of them with a single keystroke
- **DELETE mode** -- Batch-select sessions (or select all of them) and delete them with all their data
- **Agent filter** -- Show or hide the sessions of each agent with keys `1`-`4`, in both modes
- **Agent colors** -- Each agent name has its own color in the list and in the filter bar: Claude orange, Codex light blue, OpenCode light green, Pi light magenta (red, cyan, green and magenta when the terminal cannot define custom colors)
- **Session names** -- Shows the name of a session, either the one you set (e.g. `/rename`) or the one the agent generated; sessions without a name show their ID
- **CLI subcommands** -- Non-interactive `ls`, `launch`, and `delete` commands for scripting and quick access
- **Cross-project view** -- Aggregates sessions from every project into a single list, sorted by most recent activity
- **Lazy detail loading** -- Per-session data (todos, commands) is loaded only when highlighted, keeping startup fast
- **Graceful degradation** -- Hides the right pane on narrow terminals, hides the banner on short terminals, falls back to 256-color when true-color is unavailable

## Prerequisites

- **Bash** 4.0+
- **Python** 3.8+
- At least one supported agent installed: **Claude Code**, **Codex**, **OpenCode** or **Pi**
- To delete Codex and OpenCode sessions, their CLI (`codex`, `opencode`) must be on your `PATH`
- A terminal with ANSI color support (24-bit true-color preferred; 256-color fallback available)

## Installation

```bash
# Clone the repository
git clone https://github.com/pierclgr/agents-sessions-manager.git
cd agents-sessions-manager

# Install both command names
./setup.sh
```

Setup installs `agents-sessions` and its alias `sessions` in `~/.local/bin`, linked to this repository. Both commands work from any directory and accept the same subcommands. No `sudo` is needed. Keep the repository in place; updates to it apply to both commands.

Setup can be run again and refuses to overwrite unrelated files. Python 3.8+ with curses support must already be installed.

If the install directory is not in your `PATH`, setup prints the line to add to your shell profile (`~/.zshrc` or `~/.bashrc`). Run that line in your current shell too. For the default location:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

To use a different install directory, pass it to setup: `./setup.sh /path/to/bin`.

## Usage

### Interactive TUI

```bash
agents-sessions
```

Launches the full-screen session browser in SELECT mode.

SELECT mode uses a muted orange (`#A36847`) accent with white text (gray when custom colors are unavailable), distinct from Claude's orange. DELETE mode keeps its dark reddish accent.

#### TUI Layout

```
┌──────────────────────────────────┬──────────────────────┐
│  ASCII Banner                    │                      │
├──────────────────────────────────┤                      │
│  Agent filter: [x] 1 Claude ...  │                      │
├──────────────────────────────────┤  Session details     │
│  Session list (60%)              │  (40%)               │
│                                  │                      │
│  - Agent                         │  - Agent             │
│  - Project name                  │  - Full project path │
│  - Session name (or ID)          │  - Name (if any)     │
│  - Modified date                 │  - Full session ID   │
│  - Latest command                │  - Recent commands   │
│                                  │  - Todos (Claude)    │
├──────────────────────────────────┴──────────────────────┤
│  Status bar: mode + keybindings                         │
└─────────────────────────────────────────────────────────┘
```

#### SELECT mode (default)

| Key              | Action                        |
| ---------------- | ----------------------------- |
| `Up` / `k`       | Move cursor up                |
| `Down` / `j`     | Move cursor down              |
| `1` - `4`        | Show/hide Claude, Codex, OpenCode, Pi sessions |
| `Enter`          | Resume the highlighted session |
| `D`              | Switch to DELETE mode          |
| `Esc` / `Q`      | Quit                          |

#### DELETE mode

| Key              | Action                               |
| ---------------- | ------------------------------------ |
| `Up` / `k`       | Move cursor up                       |
| `Down` / `j`     | Move cursor down                     |
| `1` - `4`        | Show/hide Claude, Codex, OpenCode, Pi sessions (hidden sessions are unselected) |
| `Tab` / `Space`  | Toggle selection on current session  |
| `Shift+Tab`      | Select all sessions from the last one toggled with `Tab`/`Space` to the cursor |
| `A`              | Select all shown sessions (press again to clear the selection) |
| `Enter`          | Delete selected sessions (with confirmation; `Esc` cancels) |
| `D`              | Switch back to SELECT mode           |
| `Esc` / `Q`      | Quit                                 |

Both `q` and `Q` work. During delete confirmation, `Esc` and `Q` cancel the prompt.

### CLI Subcommands

```bash
# List the sessions of all agents (tab-separated, suitable for piping)
agents-sessions ls

# Resume a session of any agent by ID (supports prefix matching)
agents-sessions launch <SESSION_ID>

# Delete a session of any agent by ID (with confirmation prompt)
agents-sessions delete <SESSION_ID>

# Show help
agents-sessions --help
```

#### `ls` output example

```
Agent     Session ID  Name          Project Directory         Last message     Created           Last Modified
claude    221e3acb    data-reader   /Users/you/my-project     /exit            2026-02-07 12:03  2026-02-07 12:06
codex     01a154ad                  /Users/you/other-project  Fix the bug in…  2026-02-06 09:15  2026-02-06 10:30
```

## Project Structure

```
agents-sessions-manager/
├── agents-sessions                # Bash entry point (routes subcommands to Python)
├── setup.sh                       # Installs agents-sessions and sessions
├── src/
│   ├── commands/                  # Non-interactive subcommand handlers
│   │   ├── ls.py                  #   List sessions
│   │   ├── launch.py              #   Resume a session
│   │   └── delete.py              #   Delete a session
│   ├── data/                      # Data access layer
│   │   ├── models.py              #   Session dataclass
│   │   ├── history.py             #   Parse Claude history.jsonl
│   │   ├── session.py             #   Parse Claude per-session JSONL (name, todos)
│   │   ├── delete.py              #   Claude deletion operations
│   │   └── agents/                #   One provider per agent (load, delete, resume)
│   │       ├── base.py            #     AgentProvider interface + CLI runner
│   │       ├── claude.py          #     Claude Code
│   │       ├── codex.py           #     Codex
│   │       ├── opencode.py        #     OpenCode
│   │       └── pi.py              #     Pi
│   ├── tui/                       # Full-screen curses TUI
│   │   ├── app.py                 #   Main loop, state management, colors
│   │   ├── filter_bar.py          #   Agent filter bar renderer
│   │   ├── left_pane.py           #   Scrollable session list renderer
│   │   ├── right_pane.py          #   Session detail renderer
│   │   └── status_bar.py          #   Mode indicator + keybindings bar
│   └── utils/                     # Shared utilities
│       ├── paths.py               #   Agent data directory / path resolution
│       ├── formatting.py          #   Date/time formatting, text truncation
│       ├── jsonl.py               #   Atomic removal of JSONL lines
│       └── sqlite.py              #   Read-only SQLite queries
├── tests/                         # Unit tests (unittest)
└── README.md                      # This file
```

## Environment Variables

| Variable          | Description                                      | Default        |
| ----------------- | ------------------------------------------------ | -------------- |
| `CLAUDE_DATA_DIR` | Override the Claude Code data directory           | `~/.claude/`   |
| `CODEX_HOME`      | Codex data directory (same variable as Codex)     | `~/.codex/`    |
| `XDG_DATA_HOME`   | Parent of the OpenCode data directory (same as OpenCode) | `~/.local/share/` |
| `PI_CODING_AGENT_SESSION_DIR` | Pi sessions directory (same as Pi)    | `~/.pi/agent/sessions/` |
| `PI_CODING_AGENT_DIR` | Pi data directory, used when `PI_CODING_AGENT_SESSION_DIR` is not set | `~/.pi/agent/` |

## How It Works

Each agent has a provider (`src/data/agents/`) that loads, deletes and resumes its sessions. The tool only lists top-level sessions with at least one prompt: subagent/child sessions and archived sessions are hidden. Agent databases are always opened read-only.

| Agent | Sessions source | Name | Resume | Delete |
| ----- | --------------- | ---- | ------ | ------ |
| Claude Code | `~/.claude/history.jsonl`, grouped by `sessionId` | `/rename` title, otherwise the generated title, in `~/.claude/projects/<slug>/<id>.jsonl` | `claude --resume <ID>` | see below |
| Codex | `threads` table of `~/.codex/state_<N>.sqlite`, prompts from `~/.codex/history.jsonl` | `name` column | `codex resume <ID>` | `codex delete --force <ID>`, then the session's lines are removed from `~/.codex/history.jsonl` |
| OpenCode | `session_v2` and `session_message` tables of `~/.local/share/opencode/opencode.db` | session title (except the default `New session - <date>`) | `opencode --session <ID>` | `opencode session delete <ID>` (also deletes child sessions and messages) |
| Pi | `~/.pi/agent/sessions/--<path>--/<timestamp>_<ID>.jsonl` | `/name` (`session_info` entry) | `pi --session <file>` | the session file (and its project folder, if empty) |

All resume commands run from the session's project directory, via `os.execvp`. When a session is highlighted in the TUI, Claude's per-session JSONL file is parsed to extract todos.

**Claude Code session deletion** removes:

1. The session's lines in `history.jsonl` (atomic rewrite: write to temp file, then rename), and the `paste-cache/` files that only those lines use.
2. All files/directories under `~/.claude/` whose name contains the session ID (transcript, subagent transcripts, `file-history/`, `session-env/`, ...).
3. The session's plan file `plans/<slug>.md`, unless another session uses the same plan.
4. The project folder under `projects/`, only if it is now empty: the project memory (`memory/`) is kept.

Background sessions (started with `claude --bg`) are first removed with `claude rm <ID>`, which also cleans the daemon state. If this fails, the session is not deleted at all.

## Running Tests

The tests use only the standard library (`unittest`) and build fake agent data in temporary directories:

```bash
python3 -m unittest discover -s tests -t .
```

## Disclaimer
This project is not affiliated with Claude Code, Codex, OpenCode or Pi, or with the companies behind them (Anthropic, OpenAI, ...), neither built by their teams.
