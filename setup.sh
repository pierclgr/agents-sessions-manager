#!/usr/bin/env bash
# Install both command names for the current user, without changing shell profiles.

set -euo pipefail

if [[ $# -gt 1 || "${1:-}" == -* ]]; then
    printf 'Usage: %s [bin-directory]\n' "$0" >&2
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
LAUNCHER="$SCRIPT_DIR/agents-sessions"
BIN_DIR="${1:-$HOME/.local/bin}"

if ! command -v python3 >/dev/null 2>&1; then
    printf 'Error: Python 3.8+ is required; python3 was not found on PATH.\n' >&2
    exit 1
fi

python3 - <<'PY'
import sys

if sys.version_info < (3, 8):
    sys.exit("Error: Python 3.8+ is required; found " + sys.version.split()[0] + ".")
try:
    import curses
except ImportError:
    sys.exit("Error: Python curses support is required.")
PY

mkdir -p "$BIN_DIR"
BIN_DIR="$(cd "$BIN_DIR" && pwd)"

# Check both paths first, including broken links, to avoid a partial installation.
for name in agents-sessions sessions; do
    destination="$BIN_DIR/$name"
    if [[ -e "$destination" || -L "$destination" ]]; then
        if [[ ! -L "$destination" || "$(readlink "$destination")" != "$LAUNCHER" ]]; then
            printf 'Error: Refusing to overwrite existing path: %s\n' "$destination" >&2
            exit 1
        fi
    fi
done

chmod +x "$LAUNCHER"
for name in agents-sessions sessions; do
    destination="$BIN_DIR/$name"
    if [[ ! -L "$destination" ]]; then
        ln -s "$LAUNCHER" "$destination"
    fi
done

printf 'Installed agents-sessions and sessions in %s.\n' "$BIN_DIR"
case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *)
        printf '\nAdd this line to your shell profile and run it in your current shell:\n'
        printf 'export PATH=%q:"$PATH"\n' "$BIN_DIR"
        ;;
esac
