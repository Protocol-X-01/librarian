# Librarian shell wrappers — trigger a background refresh after package-manager
# install/upgrade/uninstall operations. Sourced by interactive shells via
# /etc/profile.d/librarian-wrappers.sh (installed by scripts/install.sh).
#
# Design notes:
#   - Each wrapper calls `command <pm> "$@"` so the user gets pm's normal output.
#   - The refresh runs in the background (`&`) and is `--quiet`, so the shell
#     prompt returns immediately. Stderr is dropped to avoid clutter.
#   - Wrappers only fire on a successful pm invocation (rc == 0).
#   - Non-interactive shells (CI, scripts) don't source /etc/profile.d, so the
#     lazy stale-check in the next `librarian` query is the safety net.
#   - Pacman / yay are NOT wrapped here — they're covered by the pacman hook
#     installed at /etc/pacman.d/hooks/librarian.hook.

# Skip everything if the librarian binary isn't on PATH.
command -v librarian >/dev/null 2>&1 || return 0

__librarian_bg_refresh() {
    # $1 = source name to refresh. Runs in background, drops all output.
    (librarian refresh --source "$1" --quiet >/dev/null 2>&1 &) 2>/dev/null
}

__librarian_first_arg_matches() {
    # $1 = pattern (extended regex)  $2..$N = actual command args
    local pat="$1"; shift
    [[ "$1" =~ $pat ]]
}

if command -v cargo >/dev/null 2>&1; then
cargo() {
    command cargo "$@"; local rc=$?
    [ $rc -eq 0 ] && __librarian_first_arg_matches '^(install|uninstall|update)$' "$@" \
        && __librarian_bg_refresh cargo
    return $rc
}
fi

if command -v rustup >/dev/null 2>&1; then
rustup() {
    command rustup "$@"; local rc=$?
    [ $rc -eq 0 ] && __librarian_first_arg_matches '^(toolchain|component|install|update|default)$' "$@" \
        && __librarian_bg_refresh rustup
    return $rc
}
fi

if command -v foundryup >/dev/null 2>&1; then
foundryup() {
    command foundryup "$@"; local rc=$?
    [ $rc -eq 0 ] && __librarian_bg_refresh foundry
    return $rc
}
fi

if command -v pip >/dev/null 2>&1; then
pip() {
    command pip "$@"; local rc=$?
    [ $rc -eq 0 ] && __librarian_first_arg_matches '^(install|uninstall)$' "$@" \
        && __librarian_bg_refresh pip
    return $rc
}
fi

if command -v pip3 >/dev/null 2>&1; then
pip3() {
    command pip3 "$@"; local rc=$?
    [ $rc -eq 0 ] && __librarian_first_arg_matches '^(install|uninstall)$' "$@" \
        && __librarian_bg_refresh pip
    return $rc
}
fi

if command -v pip2 >/dev/null 2>&1; then
pip2() {
    command pip2 "$@"; local rc=$?
    [ $rc -eq 0 ] && __librarian_first_arg_matches '^(install|uninstall)$' "$@" \
        && __librarian_bg_refresh pip2
    return $rc
}
fi

if command -v pipx >/dev/null 2>&1; then
pipx() {
    command pipx "$@"; local rc=$?
    [ $rc -eq 0 ] && __librarian_first_arg_matches '^(install|uninstall|upgrade|reinstall|inject)$' "$@" \
        && __librarian_bg_refresh pipx
    return $rc
}
fi

if command -v uv >/dev/null 2>&1; then
uv() {
    command uv "$@"; local rc=$?
    [ $rc -eq 0 ] && [[ "$1" == "tool" && "$2" =~ ^(install|uninstall|upgrade)$ ]] \
        && __librarian_bg_refresh uv
    return $rc
}
fi

if command -v conda >/dev/null 2>&1; then
conda() {
    command conda "$@"; local rc=$?
    [ $rc -eq 0 ] && [[ "$1" =~ ^(install|remove|update|create|env)$ ]] \
        && __librarian_bg_refresh conda
    return $rc
}
fi

if command -v npm >/dev/null 2>&1; then
npm() {
    command npm "$@"; local rc=$?
    if [ $rc -eq 0 ]; then
        # Only react to global ops (-g / --global) to avoid spamming on project installs.
        local has_global=0
        for a in "$@"; do
            [[ "$a" == "-g" || "$a" == "--global" ]] && has_global=1 && break
        done
        if [ $has_global -eq 1 ] && [[ "$1" =~ ^(install|i|uninstall|remove|rm|update|up)$ ]]; then
            __librarian_bg_refresh npm
        fi
    fi
    return $rc
}
fi

if command -v yarn >/dev/null 2>&1; then
yarn() {
    command yarn "$@"; local rc=$?
    [ $rc -eq 0 ] && [[ "$1" == "global" ]] && __librarian_bg_refresh yarn
    return $rc
}
fi

if command -v pnpm >/dev/null 2>&1; then
pnpm() {
    command pnpm "$@"; local rc=$?
    if [ $rc -eq 0 ]; then
        local has_global=0
        for a in "$@"; do
            [[ "$a" == "-g" || "$a" == "--global" ]] && has_global=1 && break
        done
        [ $has_global -eq 1 ] && [[ "$1" =~ ^(add|install|i|remove|rm|update|up)$ ]] \
            && __librarian_bg_refresh pnpm
    fi
    return $rc
}
fi

if command -v bun >/dev/null 2>&1; then
bun() {
    command bun "$@"; local rc=$?
    [ $rc -eq 0 ] && [[ "$1" == "add" || "$1" == "install" || "$1" == "remove" || "$1" == "uninstall" ]] \
        && __librarian_bg_refresh bun
    return $rc
}
fi
