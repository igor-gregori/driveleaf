#!/usr/bin/env bash
# Cross-compiles the release binaries for Windows from WSL and copies them to the
# Windows disk. Official releases come from native Windows CI runners instead.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root/apps/desktop"
npm run tauri build -- --runner cargo-xwin --target x86_64-pc-windows-msvc --no-bundle
cd "$root"
cargo xwin build --release --target x86_64-pc-windows-msvc -p driveleaf-cli -p engine-compare
bash scripts/publish-local.sh
