#!/usr/bin/env bash
# Copies the cross-compiled binaries and helper scripts to a folder on the
# Windows disk, where they can run as normal Windows programs.
set -euo pipefail
out="${DRIVELEAF_WINDOWS_OUT:-/mnt/c/dev/driveleaf/out}"
root="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$out"
for exe in "$root"/target/x86_64-pc-windows-msvc/release/*.exe; do
    # Copy through a temp name so a running binary is never half-overwritten.
    cp "$exe" "$out/$(basename "$exe").new"
    mv -f "$out/$(basename "$exe").new" "$out/$(basename "$exe")"
done
cp "$root"/scripts/*.ps1 "$root"/fixtures/build.ps1 "$out"/
ls -la "$out"
