#!/usr/bin/env python3
"""Writes the notices that ship inside the app: every third-party package in the
Windows build and the web bundle, plus the font license the OFL requires."""

import json
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parent.parent
desktop = root / "apps/desktop"


def rust_packages():
    raw = subprocess.check_output(
        [
            "cargo",
            "metadata",
            "--format-version",
            "1",
            "--filter-platform",
            "x86_64-pc-windows-msvc",
        ],
        cwd=root,
    )
    data = json.loads(raw)
    nodes = {node["id"]: node for node in data["resolve"]["nodes"]}
    packages = {package["id"]: package for package in data["packages"]}
    stack = [id_ for id_, package in packages.items() if package["name"] == "driveleaf-desktop"]
    seen = set()
    while stack:
        current = stack.pop()
        if current in seen:
            continue
        seen.add(current)
        for dependency in nodes[current]["deps"]:
            if any(kind["kind"] is None for kind in dependency["dep_kinds"]):
                stack.append(dependency["pkg"])
    return sorted(
        (packages[id_]["name"], packages[id_]["version"], packages[id_]["license"] or "unknown")
        for id_ in seen
        if not packages[id_]["name"].startswith("driveleaf-")
    )


def web_packages():
    lock = json.loads((desktop / "package-lock.json").read_text(encoding="utf-8"))
    listed = []
    for path, entry in lock["packages"].items():
        if not path or entry.get("dev"):
            continue
        name = path.split("node_modules/")[-1]
        if "license" not in entry:
            manifest = desktop / path / "package.json"
            if manifest.exists():
                entry = {**entry, **json.loads(manifest.read_text(encoding="utf-8"))}
        listed.append((name, entry.get("version", "?"), entry.get("license", "unknown")))
    return sorted(set(listed))


def font_license():
    package = desktop / "node_modules/@fontsource-variable/geist/LICENSE"
    return package.read_text(encoding="utf-8")


def main():
    lines = [
        "Driveleaf includes third-party software, under these licenses.",
        "",
        "The full license text of each package comes with its source code on",
        "crates.io or npm. Packages under MPL-2.0 are used unmodified.",
        "",
        "Rust packages",
        "-------------",
    ]
    lines += [f"{name} {version}: {license}" for name, version, license in rust_packages()]
    lines += ["", "Web packages", "------------"]
    lines += [f"{name} {version}: {license}" for name, version, license in web_packages()]
    lines += ["", "Geist font", "----------", "", font_license()]
    target = desktop / "public/third-party-notices.txt"
    target.parent.mkdir(exist_ok=True)
    target.write_text("\n".join(lines), encoding="utf-8")
    print(f"wrote {target}")


if __name__ == "__main__":
    main()
