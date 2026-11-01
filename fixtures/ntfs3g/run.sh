#!/usr/bin/env bash
# Builds a real NTFS image with mkntfs and ntfs-3g, then checks the parser
# against it file by file. Needs FUSE but no root. Set NTFS3G_PREFIX when
# ntfs-3g is unpacked outside the system paths.
set -euo pipefail

# ntfs-3g only mounts as root. A private user namespace provides that without
# any real privilege, and the mount disappears with the namespace.
if [ "$(id -u)" != 0 ]; then
    exec unshare --user --map-root-user --mount bash "$0" "$@"
fi

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
if [ -n "${NTFS3G_PREFIX:-}" ]; then
    export LD_LIBRARY_PATH="$NTFS3G_PREFIX/usr/lib/x86_64-linux-gnu"
    export PATH="$NTFS3G_PREFIX/usr/sbin:$NTFS3G_PREFIX/usr/bin:$PATH"
fi
work="$(mktemp -d)"
mount_point="$work/mnt"
mkdir -p "$mount_point"
cleanup() {
    umount "$mount_point" 2>/dev/null || true
    rm -rf "$work"
}
trap cleanup EXIT

truncate -s 512M "$work/volume.img"
mkntfs -F -Q -q -c 4096 -L driveleaf "$work/volume.img" 2>/dev/null
cc -shared -fPIC -o "$work/nosetgroups.so" "$here/nosetgroups.c"
LD_PRELOAD="$work/nosetgroups.so" \
    ntfs-3g "$work/volume.img" "$mount_point" -o streams_interface=xattr,no_detach &
daemon=$!
for _ in $(seq 100); do
    mountpoint -q "$mount_point" && break
    kill -0 "$daemon"
    sleep 0.1
done
mountpoint -q "$mount_point"
timeout 300 python3 "$here/populate.py" "$mount_point" "$work/expected.json"
umount "$mount_point"
wait "$daemon"

cargo build --quiet --release -p driveleaf-cli --manifest-path "$root/Cargo.toml"
cli="$root/target/release/driveleaf-cli"
"$cli" mft "$work/volume.img" > "$work/report.json"
"$cli" dump "$work/volume.img" > "$work/dump.tsv"
python3 "$here/verify.py" "$work/expected.json" "$work/dump.tsv" "$work/report.json"
