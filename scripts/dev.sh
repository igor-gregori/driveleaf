#!/usr/bin/env bash
# Development loop from WSL: Vite runs here with hot reload, and the Windows app
# is cross-compiled and opened on the Windows side, pointed at the dev server.
# Changing Rust code means running this again; frontend edits reload by themselves.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
out="${DRIVELEAF_WINDOWS_OUT:-/mnt/c/dev/driveleaf/out}"
cd "$root/apps/desktop"

if ! ss -ltn 2>/dev/null | grep -q ':1420 '; then
    npm run dev >/tmp/driveleaf-vite.log 2>&1 &
    vite=$!
    trap 'kill "$vite" 2>/dev/null || true' EXIT
fi
until (exec 3<>/dev/tcp/127.0.0.1/1420) 2>/dev/null; do sleep 0.2; done

# A debug build loads the frontend from devUrl instead of the embedded files.
cargo xwin build --target x86_64-pc-windows-msvc -p driveleaf-desktop
mkdir -p "$out"
cp -f "$root/target/x86_64-pc-windows-msvc/debug/driveleaf-desktop.exe" "$out/driveleaf-dev.exe"
"$out/driveleaf-dev.exe"
