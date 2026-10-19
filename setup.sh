#!/usr/bin/env bash
# build and install both command names without changing shell profiles

set -euo pipefail

if [[ $# -gt 1 || "${1:-}" == -* ]]; then
    printf 'Usage: %s [bin-directory]\n' "$0" >&2
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
LEGACY_LAUNCHER="$SCRIPT_DIR/agents-sessions"
BIN_DIR="${1:-$HOME/.local/bin}"

if ! command -v cargo >/dev/null 2>&1; then
    printf 'Error: Rust and Cargo are required to build this tool. Install them from https://rustup.rs.\n' >&2
    exit 1
fi
if ! command -v cc >/dev/null 2>&1; then
    printf 'Error: A C compiler (cc) is required to build bundled SQLite.\n' >&2
    exit 1
fi

mkdir -p "$BIN_DIR"
BIN_DIR="$(cd "$BIN_DIR" && pwd)"

METADATA="$BIN_DIR/.agents-sessions-install"
MANAGED=0
if [[ -e "$METADATA" || -L "$METADATA" ]]; then
    if [[ -f "$METADATA" && ! -L "$METADATA" && -f "$BIN_DIR/agents-sessions" && ! -L "$BIN_DIR/agents-sessions" ]]; then
        expected="$(printf 'agents-sessions-v1\n%s' "$(cksum < "$BIN_DIR/agents-sessions")")"
        if [[ "$(cat "$METADATA")" == "$expected" ]]; then
            MANAGED=1
        fi
    fi
    if [[ "$MANAGED" == 0 ]]; then
        printf 'Error: Refusing to overwrite an unrecognized or modified installation: %s\n' "$METADATA" >&2
        exit 1
    fi
fi

# check both paths, including broken links, before building or replacing either
for name in agents-sessions sessions; do
    destination="$BIN_DIR/$name"
    if [[ -e "$destination" || -L "$destination" ]]; then
        if [[ -L "$destination" ]]; then
            target="$(readlink "$destination")"
            if [[ "$target" == "$LEGACY_LAUNCHER" ]]; then
                continue
            fi
            if [[ "$MANAGED" == 1 && "$name" == sessions && "$target" == agents-sessions ]]; then
                continue
            fi
        elif [[ "$MANAGED" == 1 && "$name" == agents-sessions && -f "$destination" ]]; then
            continue
        fi
        printf 'Error: Refusing to overwrite existing path: %s\n' "$destination" >&2
        exit 1
    fi
done

# stage on the destination filesystem so each replacement is atomic
STAGE="$(mktemp -d "$BIN_DIR/.agents-sessions-stage.XXXXXX")"
INSTALLING=0
COMMITTED=0
cleanup() {
    status=$?
    trap - EXIT INT TERM
    if [[ "$INSTALLING" == 1 && "$COMMITTED" == 0 ]]; then
        for name in agents-sessions sessions .agents-sessions-install; do
            previous="$STAGE/previous-$name"
            if [[ -e "$previous" || -L "$previous" ]]; then
                mv -f "$previous" "$BIN_DIR/$name"
            else
                rm -f "$BIN_DIR/$name"
            fi
        done
    fi
    rm -rf "$STAGE"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# keep Cargo's temporary install path out of its user-facing PATH advice
PATH="$STAGE/build/bin:$PATH" cargo install --path "$SCRIPT_DIR" --locked --root "$STAGE/build" --bin agents-sessions
cp "$STAGE/build/bin/agents-sessions" "$STAGE/new-agents-sessions"
chmod 755 "$STAGE/new-agents-sessions"
ln -s agents-sessions "$STAGE/new-sessions"
printf 'agents-sessions-v1\n%s\n' "$(cksum < "$STAGE/new-agents-sessions")" > "$STAGE/new-metadata"

for name in agents-sessions sessions .agents-sessions-install; do
    destination="$BIN_DIR/$name"
    if [[ -e "$destination" || -L "$destination" ]]; then
        cp -P "$destination" "$STAGE/previous-$name"
    fi
done

INSTALLING=1
mv -f "$STAGE/new-agents-sessions" "$BIN_DIR/agents-sessions"
mv -f "$STAGE/new-sessions" "$BIN_DIR/sessions"
mv -f "$STAGE/new-metadata" "$METADATA"
COMMITTED=1

printf 'Installed agents-sessions and sessions in %s.\n' "$BIN_DIR"
case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *)
        printf '\nAdd this line to your shell profile and run it in your current shell:\n'
        printf 'export PATH=%q:"$PATH"\n' "$BIN_DIR"
        ;;
esac
