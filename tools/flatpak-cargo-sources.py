#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-or-later
"""Write cargo-sources.json for flatpak-builder from Cargo.lock (standard library only).

    python3 tools/flatpak-cargo-sources.py [-o packaging/flatpak/cargo-sources.json]

flatpak-builder builds without network access, so every crate has to be a source of the manifest.
This lists each crates.io crate of Cargo.lock as a tarball with the checksum Cargo.lock records
for it, unpacked into `cargo/vendor/<name>-<version>`, plus the `cargo/config` that makes cargo
use that directory instead of the registry (the manifest sets CARGO_HOME to the `cargo` directory
of the build). It does the same job as flatpak-builder-tools' flatpak-cargo-generator.py for a
lock file that only has crates.io dependencies, and refuses anything else.
"""
import argparse
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"
CONFIG = """[source.vendored-sources]
directory = "cargo/vendor"

[source.crates-io]
replace-with = "vendored-sources"
"""


def read_packages(lock: Path) -> list:
    """The [[package]] tables of Cargo.lock (a flat list of `key = "value"` lines, so a small reader
    does; it keeps this script independent of the Python version of the build image)."""
    packages = []
    for block in re.split(r"^\[\[package\]\]\s*$", lock.read_text("utf-8"), flags=re.M)[1:]:
        fields = dict(re.findall(r'^(name|version|source|checksum) = "([^"]*)"\s*$', block, re.M))
        if "name" not in fields or "version" not in fields:
            sys.exit(f"cannot read a package of {lock.name}")
        packages.append(fields)
    return packages


def sources(lock: Path) -> list:
    packages = read_packages(lock)
    out = []
    for pkg in sorted(packages, key=lambda p: (p["name"], p["version"])):
        source = pkg.get("source")
        if source is None:
            continue  # a workspace member
        if source != REGISTRY:
            sys.exit(f"{pkg['name']} {pkg['version']}: unsupported source {source}")
        name, version, checksum = pkg["name"], pkg["version"], pkg.get("checksum")
        if not checksum:
            sys.exit(f"{name} {version}: no checksum in {lock.name}")
        dest = f"cargo/vendor/{name}-{version}"
        out.append(
            {
                "type": "archive",
                "archive-type": "tar-gzip",
                "url": f"https://static.crates.io/crates/{name}/{name}-{version}.crate",
                "sha256": checksum,
                "dest": dest,
            }
        )
        out.append(
            {
                "type": "inline",
                "contents": json.dumps({"package": checksum, "files": {}}),
                "dest": dest,
                "dest-filename": ".cargo-checksum.json",
            }
        )
    out.append(
        {"type": "inline", "contents": CONFIG, "dest": "cargo", "dest-filename": "config"}
    )
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("-o", "--output", default=str(ROOT / "packaging/flatpak/cargo-sources.json"))
    ap.add_argument("--lock", default=str(ROOT / "Cargo.lock"))
    args = ap.parse_args()
    data = sources(Path(args.lock))
    Path(args.output).write_text(json.dumps(data, indent=2) + "\n", "utf-8")
    print(f"wrote {args.output}: {(len(data) - 1) // 2} crates")
    return 0


if __name__ == "__main__":
    sys.exit(main())
