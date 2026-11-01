#!/usr/bin/env python3
"""Compares `driveleaf-cli dump` output with what ntfs-3g reported for the same
volume. ntfs-3g exposes the MFT record number as the inode, so every file can
be matched one to one."""

import collections
import csv
import json
import sys

# Metafiles and reserved records sit below this number and ntfs-3g hides them.
FIRST_USER_RECORD = 24
RECORD_BYTES = 1024


def load_dump(path):
    files = collections.defaultdict(
        lambda: {"logical": 0, "streams": 0, "allocated": 0, "links": 0, "directory": False}
    )
    names = collections.defaultdict(set)
    with open(path, "rb") as dump:
        for line in dump:
            fields = line.rstrip(b"\n").split(b"\t")
            if fields[0] == b"F":
                record, flags, links, logical, streams, allocated = map(int, fields[1:7])
                file = files[record]
                file["logical"] += logical
                file["streams"] += streams
                file["allocated"] += allocated
                if not flags & 2:
                    file["links"] = links
                    file["directory"] = bool(flags & 1)
            else:
                record, parent = int(fields[1]), int(fields[2])
                names[record].add((parent, fields[3].decode("utf-8")))
    return files, names


def check_tree(expected, tree_path, problems):
    """Every path must exist in the tree, and folders must add up."""
    with open(tree_path, encoding="utf-8", newline="") as tree_file:
        rows = {row["path"].lstrip("\\"): row for row in csv.DictReader(tree_file)}

    for entry in expected["entries"]:
        path = entry["path"].replace("/", "\\")
        row = rows.get(path)
        if row is None:
            problems.append(f"{path} is missing from the tree")
        elif not entry["dir"] and int(row["logical"]) != entry["size"]:
            problems.append(f"{path}: tree says {row['logical']}, disk says {entry['size']}")

    # Folders without hard links must equal the sum of what they hold.
    for top in ("projects", "sparse", "compressed", "fragmented", "deleted", "nomes"):
        wanted = sum(
            entry["size"]
            for entry in expected["entries"]
            if not entry["dir"] and entry["path"].split("/")[0] == top
        )
        row = rows.get(top)
        if row is None:
            problems.append(f"{top} is missing from the tree")
        elif int(row["logical"]) != wanted:
            problems.append(f"{top}: folder total {row['logical']}, files add up to {wanted}")
    return len(rows)


def main():
    with open(sys.argv[1], encoding="utf-8") as expected_file:
        expected = json.load(expected_file)
    files, names = load_dump(sys.argv[2])
    report = json.load(open(sys.argv[3], encoding="utf-8"))

    problems = []
    by_record = collections.defaultdict(list)
    for entry in expected["entries"]:
        by_record[entry["record"]].append(entry)

    extra_streams = 0
    for record, entries in by_record.items():
        entry = entries[0]
        file = files.get(record)
        if file is None:
            problems.append(f"record {record} ({entry['name']}) is missing")
            continue
        wanted_names = {(item["parent"], item["name"]) for item in entries}
        if wanted_names - names[record]:
            problems.append(f"record {record}: names differ: {wanted_names ^ names[record]}")
        if file["directory"] != entry["dir"]:
            problems.append(f"record {record} ({entry['name']}): directory flag differs")
        if entry["dir"]:
            continue
        if file["logical"] != entry["size"]:
            problems.append(
                f"record {record} ({entry['name']}): logical {file['logical']} != {entry['size']}"
            )
        if file["links"] != entry["links"]:
            problems.append(
                f"record {record} ({entry['name']}): links {file['links']} != {entry['links']}"
            )
        streams = sum(entry["streams"].values())
        if file["streams"] != streams:
            problems.append(
                f"record {record} ({entry['name']}): streams {file['streams']} != {streams}"
            )
        # A resident file lives inside its record and owns no clusters, while
        # ntfs-3g rounds st_blocks up from its size anyway.
        resident = file["allocated"] == 0 and entry["size"] < RECORD_BYTES
        allocated = 0 if resident else entry["blocks"] * 512
        extra_streams += file["allocated"] - allocated
        if file["allocated"] < allocated:
            problems.append(
                f"record {record} ({entry['name']}): allocated {file['allocated']} < {allocated}"
            )

    unknown = [
        record
        for record in files
        if record >= FIRST_USER_RECORD and record not in by_record and record not in (0,)
    ]
    tree_rows = check_tree(expected, sys.argv[4], problems)
    used = expected["used_bytes"]
    allocated_total = report["allocated_bytes"]
    gap = 100.0 * (allocated_total - used) / used

    print(f"records matched one to one: {len(by_record)}")
    print(f"names checked: {len(expected['entries'])}")
    print(f"tree rows: {tree_rows}")
    print(f"records the mount does not show: {len(unknown)} (metafile children)")
    print(f"allocated beyond st_blocks (streams and metadata): {extra_streams} bytes")
    print(f"allocated {allocated_total} vs volume used {used}: {gap:+.3f}%")
    print(f"corrupt {report['corrupt_records']}, malformed {report['malformed_records']}")
    print(f"extension records {report['extension_records']}")
    if problems:
        print(f"{len(problems)} problems:")
        for problem in problems[:20]:
            print(" ", problem)
        sys.exit(1)
    print("every file matches")


if __name__ == "__main__":
    main()
