<h2 align="center">Agents Sessions Manager</h2>

<p align="center"><em>Browse, resume, and delete coding agent sessions</em></p>

<p align="center">
  <a href="https://www.repostatus.org/#active"><img
    src="https://flat.badgen.net/badge/repo%20status/Active/green"
    alt="Project Status: Active – The project has reached a stable, usable
    state and is being actively developed."></a>
</p>

A terminal tool for browsing, resuming, and deleting coding agent sessions
across all your projects. Supported agents: [Claude Code](https://docs.anthropic.com/en/docs/claude-code),
[Codex](https://github.com/openai/codex), [OpenCode](https://opencode.ai),
and [Pi](https://github.com/earendil-works/pi-mono). Provides both an
interactive full-screen TUI and non-interactive CLI subcommands.

Built with **Rust**, using Ratatui and Crossterm for the terminal interface.
Works on **macOS** and **Linux**. SQLite is bundled into the executable.

## Features

- **Interactive TUI** -- Full-screen terminal interface with two-pane layout: session list on the left, session details on the right
- **Multi-agent** -- Shows the sessions of Claude Code, Codex, OpenCode and Pi in one list, and resumes each one with its own agent
- **SELECT mode** -- Browse all sessions and resume any of them with a single keystroke
- **DELETE mode** -- Batch-select sessions (or select all of them) and delete them with all their data
- **Agent filter** -- Press `1`-`4` to show only the sessions of one agent; press the same key again to show all agents, in both modes
- **Directory filter** -- Starts with sessions from the launch directory.
  Press `D` to toggle between CWD and all directories, in both modes
- **Agent colors** -- Each agent name has its own color in the list and in the filter bar: Claude orange, Codex light blue, OpenCode light green, Pi light magenta (red, cyan, green and magenta when the terminal cannot define custom colors)
- **Session names** -- Shows the name of a session, either the one you set (e.g. `/rename`) or the one the agent generated; sessions without a name show their ID
- **CLI subcommands** -- Non-interactive `ls`, `launch`, and `delete` commands for scripting and quick access
- **Cross-project view** -- Aggregates sessions from every project into a single list, sorted by most recent activity
- **Lazy detail loading** -- Per-session data (todos, commands) is loaded only when highlighted, keeping startup fast
- **Graceful degradation** -- Hides the right pane on narrow terminals, hides the banner on short terminals, falls back to 256-color when true-color is unavailable

## Prerequisites

- **Rust and Cargo**, stable toolchain, to build and install
- A **C compiler** (`cc`) to build bundled SQLite
- **Bash** to run the installer
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

Install [Rust and Cargo](https://rustup.rs) before running setup. On macOS,
the C compiler comes with the Xcode Command Line Tools (`xcode-select --install`).
On Linux, install your distribution's C build tools.

Setup builds a release executable using the versions in `Cargo.lock`. It
installs `agents-sessions` and its alias `sessions` in `~/.local/bin`. Both
commands work from any directory and accept the same subcommands. No `sudo`
is needed. The installed commands work after you move or remove the checkout.
Running them does not require Rust, Cargo, or Bash.

Setup adds the install directory to a detected zsh or Bash startup file when
it is missing. It updates `.zshrc` (respecting `ZDOTDIR`) or the appropriate
Bash startup file. If the shell is not recognized or the file cannot be
updated, setup prints the line to add manually. Open a new terminal or source
the updated file to use the commands in the current session.

Run setup again after updating the source. It updates its own installation
and refuses to overwrite unrelated or modified files. A small checksum record
(`.agents-sessions-install`) identifies the executable installed by setup.
Links created by the previous installer for this checkout can be upgraded.

For the default install directory, the PATH entry looks like this:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

To use a different install directory, pass it to setup: `./setup.sh /path/to/bin`.

## Usage

### Interactive TUI

```bash
agents-sessions
```

Launches the full-screen session browser in SELECT mode, showing sessions
from the exact launch directory (CWD). Subdirectories are excluded. Paths
through symlinks to the same directory also match. Press `D` to show sessions
from all directories, then press it again to return to CWD. Each launch
starts in CWD view with all agents enabled; directory and agent filters work
together.

SELECT mode uses a muted orange (`#A36847`) accent with white text (gray when custom colors are unavailable), distinct from Claude's orange. DELETE mode keeps its dark reddish accent.

#### TUI Layout

```
┌──────────────────────────────────┬──────────────────────┐
│  ASCII Banner                    │                      │
├──────────────────────────────────┤                      │
│  D: CWD | Agents: [x] 1 Claude    │                      │
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
| `D`              | Toggle CWD / all directories   |
| `Enter`          | Resume the highlighted session |
| `S`              | Switch to DELETE mode          |
| `Esc` / `Q`      | Quit                          |

#### DELETE mode

| Key              | Action                               |
| ---------------- | ------------------------------------ |
| `Up` / `k`       | Move cursor up                       |
| `Down` / `j`     | Move cursor down                     |
| `1` - `4`        | Show/hide Claude, Codex, OpenCode, Pi sessions (hidden sessions are unselected) |
| `D`              | Toggle CWD / all; unselect hidden sessions |
| `Tab` / `Space`  | Toggle selection on current session  |
| `Shift+Tab`      | Select all sessions from the last one toggled with `Tab`/`Space` to the cursor |
| `A`              | Select all shown sessions (press again to clear the selection) |
| `Enter`          | Delete selected sessions (with confirmation; `Esc` cancels) |
| `S`              | Switch back to SELECT mode           |
| `Esc` / `Q`      | Quit                                 |

`D`, `S`, and `Q` also work in lowercase. The directory and mode toggles
work when no sessions are shown. During delete confirmation, `Esc`, `Q`,
and the toggle keys cancel the prompt without changing the filters or mode.

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
├── Cargo.toml                     # Package and library dependencies
├── Cargo.lock                     # Locked dependency versions
├── setup.sh                       # Builds and installs both commands
├── src/
│   ├── main.rs                    # Executable entry point
│   ├── lib.rs                     # Shared application modules
│   ├── commands.rs                # CLI routing, listing, resume, confirmation
│   ├── data.rs                    # Session types and data-directory overrides
│   ├── data/
│   │   ├── agents.rs              # Provider interface and registry
│   │   ├── agents/                # Claude, Codex, OpenCode, and Pi providers
│   │   └── delete.rs              # Claude artifact and shared-file cleanup
│   ├── tui.rs                     # State, rendering, input, terminal lifecycle
│   └── utils.rs                   # Formatting, files, SQLite, command runner
├── tests/                         # Rust tests with isolated agent data
├── .github/workflows/ci.yml        # macOS and Linux checks
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

All resume commands replace the current process from the session's project
directory, using Unix `exec`. The terminal is restored before the agent starts.
When a session is highlighted in the TUI, Claude's per-session JSONL file is
parsed to extract todos.

**Claude Code session deletion** removes:

1. The session's lines in `history.jsonl` (atomic rewrite: write to temp file, then rename), and the `paste-cache/` files that only those lines use.
2. All files/directories under `~/.claude/` whose name contains the session ID (transcript, subagent transcripts, `file-history/`, `session-env/`, ...).
3. The session's plan file `plans/<slug>.md`, unless another session uses the same plan.
4. The project folder under `projects/`, only if it is now empty: the project memory (`memory/`) is kept.

Background sessions (started with `claude --bg`) are first removed with `claude rm <ID>`, which also cleans the daemon state. If this fails, the session is not deleted at all.

## Running Tests

The tests build fake agent data in temporary directories. Agent commands are
replaced with test commands. Terminal tests use a pseudo-terminal and verify
that terminal settings are restored after quitting, cancellation, and launch.

```bash
cargo test --locked
```

Run the same checks as CI:

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

To run from the checkout without installing:

```bash
cargo run -- ls
```

### Conventions

Use `rustfmt`, typed session and agent models, and small modules with clear
responsibilities. Pass data paths and command runners into the data layer.
Keep state changes separate from terminal I/O. Tests must use temporary data
and must not read or delete real sessions.

Use snake_case for source files and UPPER_SNAKE_CASE for documentation names.
Start code comments with a lowercase letter and omit the ending period.

Build output is stored in `target/` and is not committed. Keep `Cargo.lock`
committed so installation and CI use the same dependency versions.

## Updating

From a checkout, update the source and run the installer again:

```bash
git pull
./setup.sh
```

## Disclaimer
This project is not affiliated with Claude Code, Codex, OpenCode or Pi, or with the companies behind them (Anthropic, OpenAI, ...), neither built by their teams.
