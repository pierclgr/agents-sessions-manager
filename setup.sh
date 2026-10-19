#!/usr/bin/env bash
# build and install both command names, then add their directory to PATH

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

SHELL_NAME="${SHELL:-}"
SHELL_NAME="${SHELL_NAME##*/}"
printf -v PATH_ENTRY 'export PATH=%q:"$PATH"' "$BIN_DIR"
PROFILE_ADDED=0
PROFILE_EXISTS=0
PROFILE_FAILED=0
PROFILE_SOURCE_FILE=""

add_path_entry() {
    local profile="$1"

    if [[ -f "$profile" ]] && grep -Fqx -- "$PATH_ENTRY" "$profile"; then
        return 1
    fi
    if [[ -e "$profile" || -L "$profile" ]]; then
        if ! [[ -f "$profile" ]]; then
            return 2
        fi
        if ! grep -Fq -- "$BIN_DIR" "$profile"; then
            if { if [[ -s "$profile" ]]; then printf '\n'; fi; printf '%s\n' "$PATH_ENTRY"; } >> "$profile" 2>/dev/null; then
                return 0
            fi
            return 2
        fi
        return 1
    fi
    if printf '%s\n' "$PATH_ENTRY" > "$profile" 2>/dev/null; then
        return 0
    fi
    return 2
}

record_path_entry() {
    local profile="$1"
    local status=0
    add_path_entry "$profile" || status=$?
    case "$status" in
        0)
            PROFILE_ADDED=1
            PROFILE_SOURCE_FILE="$profile"
            printf 'Added the install directory to %s.\n' "$profile"
            ;;
        1)
            PROFILE_EXISTS=1
            PROFILE_SOURCE_FILE="$profile"
            ;;
        *)
            PROFILE_FAILED=1
            ;;
    esac
}

case "$SHELL_NAME" in
    zsh)
        SHELL_RC_DIR="${ZDOTDIR:-$HOME}"
        record_path_entry "$SHELL_RC_DIR/.zshrc"
        ;;
    bash)
        BASH_LOGIN_PROFILE="$HOME/.profile"
        for candidate in "$HOME/.bash_profile" "$HOME/.bash_login" "$HOME/.profile"; do
            if [[ -f "$candidate" ]]; then
                BASH_LOGIN_PROFILE="$candidate"
                break
            fi
        done
        if [[ -n "$BASH_LOGIN_PROFILE" ]] && grep -Eq '(^|[[:space:]])(\.|source)[[:space:]][^#]*\.bashrc' "$BASH_LOGIN_PROFILE"; then
            record_path_entry "$HOME/.bashrc"
        else
            if [[ -n "$BASH_LOGIN_PROFILE" ]]; then
                record_path_entry "$HOME/.bashrc"
            fi
            record_path_entry "$BASH_LOGIN_PROFILE"
        fi
        ;;
    *)
        PROFILE_FAILED=1
        ;;
esac

if [[ "$PROFILE_ADDED" == 1 && "$PROFILE_FAILED" == 0 ]]; then
    printf 'Run `source %q` or open a new terminal to use the commands.\n' "$PROFILE_SOURCE_FILE"
elif [[ "$PROFILE_ADDED" == 1 ]]; then
    printf 'Added the install directory to %s.\n' "$PROFILE_SOURCE_FILE"
    printf 'Add this line to any shell startup file that setup could not update:\n%s\n' "$PATH_ENTRY"
elif [[ "$PROFILE_EXISTS" == 1 && "$PROFILE_FAILED" == 0 ]]; then
    printf 'The install directory is already in %s. Run `source %q` or open a new terminal.\n' "$PROFILE_SOURCE_FILE" "$PROFILE_SOURCE_FILE"
else
    printf '\nAdd this line to your shell profile to put the install directory on PATH:\n'
    printf '%s\n' "$PATH_ENTRY"
fi
