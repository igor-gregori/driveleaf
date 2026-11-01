#!/usr/bin/env python3
"""Fills a mounted ntfs-3g volume with the exactness cases, then records what
the file system itself reports, so the MFT parser can be checked against it."""

import json
import os
import random
import sys

MIB = 1024 * 1024


def write(path, size):
    with open(path, "wb") as out:
        out.write(b"x" * size)


def nested_tree(root, rng):
    for top in range(6):
        for sub in range(5):
            folder = f"{root}/projects/top{top}/sub{sub}"
            os.makedirs(folder)
            for index in range(60):
                size = rng.choice([0, 10, 300, 600, 900, 5000, 70000, 300000])
                write(f"{folder}/file{index}.bin", size)


def unicode_names(root):
    folder = f"{root}/nomes"
    os.makedirs(folder)
    for name in ["relatório final.txt", "写真.jpg", "emoji 🌿 leaf.txt", "x" * 200 + ".long"]:
        write(f"{folder}/{name}", 1234)


def hard_links(root):
    folder = f"{root}/links"
    os.makedirs(f"{folder}/a")
    os.makedirs(f"{folder}/b")
    write(f"{folder}/a/original.bin", 123456)
    os.link(f"{folder}/a/original.bin", f"{folder}/b/second.bin")
    os.link(f"{folder}/a/original.bin", f"{folder}/b/third.bin")
    # Enough names to overflow the record into extension records.
    write(f"{folder}/many.bin", 4096)
    for index in range(400):
        os.link(f"{folder}/many.bin", f"{folder}/a/many-{index:04d}-with-a-rather-long-name.bin")


def sparse_files(root):
    folder = f"{root}/sparse"
    os.makedirs(folder)
    with open(f"{folder}/hole.bin", "wb") as out:
        out.seek(50 * MIB)
        out.write(b"end")


def compressed_files(root):
    folder = f"{root}/compressed"
    os.makedirs(folder)
    os.setxattr(folder, "system.ntfs_attrib_be", (0x800).to_bytes(4, "big"))
    with open(f"{folder}/zeros.txt", "wb") as out:
        out.write(b"compress me " * 500_000)


def alternate_streams(root):
    folder = f"{root}/streams"
    os.makedirs(folder)
    write(f"{folder}/carrier.txt", 2000)
    os.setxattr(f"{folder}/carrier.txt", "user.Zone.Identifier", b"[ZoneTransfer]\nZoneId=3\n")
    # Large enough to leave the record; Linux caps an xattr at 64 KiB.
    os.setxattr(f"{folder}/carrier.txt", "user.big", b"y" * 60_000)


def fragmented_files(root):
    # Interleaved synced appends scatter both files, which spills their run
    # lists out of the base record.
    folder = f"{root}/fragmented"
    os.makedirs(folder)
    block = b"z" * 4096
    with open(f"{folder}/one.bin", "wb") as one, open(f"{folder}/two.bin", "wb") as two:
        for _ in range(1500):
            for out in (one, two):
                out.write(block)
                out.flush()
                os.fsync(out.fileno())


def deleted_files(root):
    folder = f"{root}/deleted"
    os.makedirs(folder)
    for index in range(200):
        write(f"{folder}/gone{index}.bin", 2000)
    for index in range(0, 200, 2):
        os.remove(f"{folder}/gone{index}.bin")


def snapshot(root):
    entries = []
    pending = [root]
    while pending:
        folder = pending.pop()
        parent = os.lstat(folder).st_ino
        with os.scandir(folder) as listing:
            for entry in listing:
                info = entry.stat(follow_symlinks=False)
                streams = {}
                is_dir = entry.is_dir(follow_symlinks=False)
                if not is_dir:
                    for name in os.listxattr(entry.path, follow_symlinks=False):
                        if name.startswith("user."):
                            value = os.getxattr(entry.path, name, follow_symlinks=False)
                            streams[name] = len(value)
                entries.append(
                    {
                        "record": info.st_ino,
                        "parent": parent,
                        "name": entry.name,
                        "path": os.path.relpath(entry.path, root),
                        "dir": is_dir,
                        "size": info.st_size,
                        "blocks": info.st_blocks,
                        "links": info.st_nlink,
                        "streams": streams,
                    }
                )
                if is_dir:
                    pending.append(entry.path)
    volume = os.statvfs(root)
    used = (volume.f_blocks - volume.f_bfree) * volume.f_frsize
    return {"entries": entries, "used_bytes": used, "cluster_bytes": volume.f_frsize}


def main():
    root, output = sys.argv[1], sys.argv[2]
    rng = random.Random(42)
    nested_tree(root, rng)
    unicode_names(root)
    hard_links(root)
    sparse_files(root)
    compressed_files(root)
    alternate_streams(root)
    fragmented_files(root)
    deleted_files(root)
    os.sync()
    with open(output, "w", encoding="utf-8") as out:
        json.dump(snapshot(root), out)


if __name__ == "__main__":
    main()
