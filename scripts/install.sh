#!/usr/bin/env bash
# Librarian — system-wide installer.
#
# Idempotent. Re-running upgrades the binary in place and refreshes the index.
# Requires root for: copying the binary, creating /etc/librarian, /var/lib/librarian,
# the librarian group, the pacman hook, and the /etc/profile.d wrapper.

set -euo pipefail

# Resolve repo root regardless of where this script was invoked from.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

BIN_SRC="$REPO_ROOT/target/release/librarian"
BIN_DST="/usr/local/bin/librarian"
ETC_DIR="/etc/librarian"
VAR_DIR="/var/lib/librarian"
PACMAN_HOOK_DIR="/etc/pacman.d/hooks"
PROFILE_D="/etc/profile.d"
HOOK_SRC="$REPO_ROOT/system/pacman-hook/librarian.hook"
WRAPPERS_SRC="$REPO_ROOT/system/shell-wrappers/librarian-wrappers.sh"
SOURCES_TEMPLATE="$REPO_ROOT/system/config-templates/sources.toml"
ENV_TEMPLATE="$REPO_ROOT/system/config-templates/env.example"
README_TEMPLATE="$REPO_ROOT/system/config-templates/README.md"

red()    { printf '\033[31m%s\033[0m\n' "$*" >&2; }
green()  { printf '\033[32m%s\033[0m\n' "$*"; }
yellow() { printf '\033[33m%s\033[0m\n' "$*"; }
info()   { printf '  %s\n' "$*"; }

require_root() {
    if [ "$(id -u)" -ne 0 ]; then
        red "install.sh must run as root (sudo $0)"
        exit 1
    fi
}

build_if_missing() {
    if [ ! -x "$BIN_SRC" ]; then
        yellow "Release binary not found at $BIN_SRC — building now (this may take a few minutes)."
        # Build as the invoking user, not as root, to keep cargo's cache out of /root.
        local sudo_user="${SUDO_USER:-$USER}"
        sudo -u "$sudo_user" sh -c "cd '$REPO_ROOT' && cargo build --release"
    fi
    if [ ! -x "$BIN_SRC" ]; then
        red "Build failed — $BIN_SRC still missing."
        exit 1
    fi
}

ensure_group() {
    if ! getent group librarian >/dev/null; then
        info "Creating 'librarian' group"
        groupadd --system librarian
    fi
}

install_binary() {
    info "Installing binary → $BIN_DST"
    install -m 0755 -o root -g root "$BIN_SRC" "$BIN_DST"
}

install_dirs() {
    # /etc/librarian must be world-readable so non-root users (and any local AI)
    # can read sources.toml. Only .env carries secrets and gets 0640 below.
    info "Creating $ETC_DIR (root:librarian, 0755)"
    install -d -m 0755 -o root -g librarian "$ETC_DIR"

    info "Creating $VAR_DIR (root:librarian, 0775 — world-readable manifest, group-writable DB)"
    install -d -m 0775 -o root -g librarian "$VAR_DIR"
}

add_user_to_group() {
    # Add the invoking user to the librarian group so they can run `librarian refresh`
    # themselves without sudo (writes to /var/lib/librarian/index.db with group perm).
    local target_user="${SUDO_USER:-}"
    if [ -n "$target_user" ] && [ "$target_user" != "root" ]; then
        if ! id -nG "$target_user" | tr ' ' '\n' | grep -qx librarian; then
            info "Adding $target_user to the librarian group"
            usermod -aG librarian "$target_user"
            yellow "  → log out / back in (or run \`newgrp librarian\`) for the group to take effect"
        fi
    fi
}

install_config_templates() {
    if [ ! -f "$ETC_DIR/sources.toml" ]; then
        info "Installing sources.toml template → $ETC_DIR/sources.toml"
        install -m 0644 -o root -g librarian "$SOURCES_TEMPLATE" "$ETC_DIR/sources.toml"
    else
        info "$ETC_DIR/sources.toml already exists — leaving as-is"
    fi

    if [ ! -f "$ETC_DIR/.env" ]; then
        info "Installing .env template → $ETC_DIR/.env (fill in Bright Data creds before \`librarian sync\`)"
        install -m 0640 -o root -g librarian "$ENV_TEMPLATE" "$ETC_DIR/.env"
    else
        info "$ETC_DIR/.env already exists — leaving as-is"
    fi

    # README is always replaced — it's documentation that ships with the binary, not user state.
    info "Installing AI-facing README → $ETC_DIR/README.md"
    install -m 0644 -o root -g librarian "$README_TEMPLATE" "$ETC_DIR/README.md"
}

install_pacman_hook() {
    if [ ! -d "$PACMAN_HOOK_DIR" ]; then
        info "Creating $PACMAN_HOOK_DIR"
        install -d -m 0755 "$PACMAN_HOOK_DIR"
    fi
    info "Installing pacman hook → $PACMAN_HOOK_DIR/librarian.hook"
    install -m 0644 -o root -g root "$HOOK_SRC" "$PACMAN_HOOK_DIR/librarian.hook"
}

install_shell_wrappers() {
    info "Installing shell wrappers → $PROFILE_D/librarian-wrappers.sh"
    install -m 0644 -o root -g root "$WRAPPERS_SRC" "$PROFILE_D/librarian-wrappers.sh"
}

initial_scan() {
    info "Running initial full scan (refresh + manifest dump)…"
    LIBRARIAN_DB="$VAR_DIR/index.db" "$BIN_DST" refresh
    LIBRARIAN_DB="$VAR_DIR/index.db" "$BIN_DST" dump --format text --out "$VAR_DIR/manifest.txt"
    # DB group-writable so librarian-group members can run `librarian refresh` themselves;
    # manifest world-readable so any AI / user can grep it.
    chgrp librarian "$VAR_DIR/index.db" "$VAR_DIR/manifest.txt" 2>/dev/null || true
    chmod 0664 "$VAR_DIR/index.db" 2>/dev/null || true
    chmod 0644 "$VAR_DIR/manifest.txt" 2>/dev/null || true
    # Clean up any WAL sidecars from older installs.
    rm -f "$VAR_DIR/index.db-wal" "$VAR_DIR/index.db-shm" 2>/dev/null || true
}

main() {
    require_root
    build_if_missing
    ensure_group
    install_binary
    install_dirs
    install_config_templates
    install_pacman_hook
    install_shell_wrappers
    add_user_to_group
    initial_scan

    green "Librarian installed."
    info "Binary    : $BIN_DST"
    info "Database  : $VAR_DIR/index.db"
    info "Manifest  : $VAR_DIR/manifest.txt   (world-readable; AI/LLM access point)"
    info "Config    : $ETC_DIR/sources.toml   (edit to add custom build paths)"
    info "Creds     : $ETC_DIR/.env           (Bright Data — fill in before \`librarian sync\`)"
    info "AI Doc    : $ETC_DIR/README.md      (point local LLMs at this — full how-to-query)"
    info ""
    info "Open a new shell (or \`source $PROFILE_D/librarian-wrappers.sh\`) to pick up the wrappers."
    info "Try: librarian stats   |   librarian search nmap   |   librarian list --category cracker"
}

main "$@"
