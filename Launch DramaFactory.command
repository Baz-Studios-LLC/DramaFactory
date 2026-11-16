#!/bin/bash
# Double-click in Finder to build changed sources and open the local Mac app.
set -euo pipefail
cd -- "$(dirname -- "$0")"
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:$PATH"

on_error() {
    result=$?
    printf '\nDramaFactory could not start. The error is shown above.\n'
    if [ -t 0 ]; then
        printf 'Press Return to close this window. '
        read -r _ || true
    fi
    exit "$result"
}
trap on_error ERR

app="$PWD/target/DramaFactory.app"
executable="$app/Contents/MacOS/DramaFactory"
if [ ! -x "$executable" ] || [ "${1:-}" = "--build" ] ||
    [ -n "$(find src workers branding scripts/bundle-macos.sh Cargo.toml Cargo.lock -type f -newer "$executable" -print -quit 2>/dev/null)" ]; then
    printf 'Building DramaFactory v002…\n'
    if ! command -v cargo >/dev/null 2>&1; then
        printf 'Rust is required to build. Install it from https://rustup.rs and try again.\n' >&2
        false
    fi
    ./scripts/bundle-macos.sh
fi
printf '\nOpening DramaFactory v002.\n'
printf 'If it is already running, quit and reopen it to load a rebuilt version.\n'
/usr/bin/open "$app"
