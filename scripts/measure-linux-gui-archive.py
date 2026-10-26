#!/usr/bin/env python3
"""List binaries inside a Linux GUI release archive and reject a bundled `mist`.

The desktop tarball `Mist-linux-x86_64.tar.gz` must contain `Mist` and must not
contain a second CLI named `mist`. That CLI is published separately as
`mist-cli-linux-*.tar.gz` (see docs/release/PACKAGE_SIZE.md).

Usage:
    python3 scripts/measure-linux-gui-archive.py dist/Mist-linux-x86_64.tar.gz
    python3 scripts/measure-linux-gui-archive.py --self-test
"""

from __future__ import annotations

import argparse
import io
import sys
import tarfile
import tempfile
from pathlib import Path


def members(path: Path) -> list[tarfile.TarInfo]:
    with tarfile.open(path, "r:gz") as tf:
        return [m for m in tf.getmembers() if m.isfile()]


def report(path: Path) -> int:
    files = members(path)
    print(f"{path}  archive_bytes={path.stat().st_size}")
    print(f"{'uncompressed':>14}  name")
    basenames: list[str] = []
    for m in files:
        base = Path(m.name).name
        basenames.append(base)
        print(f"{m.size:14d}  {m.name}")
    if "Mist" not in basenames:
        print(f"error: {path.name} has no Mist binary", file=sys.stderr)
        return 1
    if "mist" in basenames:
        print(
            f"error: {path.name} still contains `mist`. "
            "Ship the CLI only as mist-cli-linux-*.tar.gz.",
            file=sys.stderr,
        )
        return 1
    return 0


def self_test() -> int:
    with tempfile.TemporaryDirectory() as tmp:
        dist = Path(tmp)

        def write(name: str, entries: list[tuple[str, bytes]]) -> Path:
            buf = io.BytesIO()
            with tarfile.open(fileobj=buf, mode="w:gz") as tf:
                for member, data in entries:
                    info = tarfile.TarInfo(member)
                    info.size = len(data)
                    tf.addfile(info, io.BytesIO(data))
            path = dist / name
            path.write_bytes(buf.getvalue())
            return path

        good = write(
            "Mist-linux-x86_64.tar.gz",
            [("Mist-linux-x86_64/Mist", b"gui"), ("Mist-linux-x86_64/README.md", b"doc")],
        )
        if report(good) != 0:
            raise SystemExit("good archive rejected")
        bad = write("bad.tar.gz", [("Mist-linux-x86_64/Mist", b"gui"), ("Mist-linux-x86_64/mist", b"cli")])
        if report(bad) == 0:
            raise SystemExit("archive containing mist was accepted")
    print("measure-linux-gui-archive self-test: ok")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("archive", nargs="?", type=Path)
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()
    if args.self_test:
        return self_test()
    if not args.archive:
        ap.error("archive path is required")
    return report(args.archive)


if __name__ == "__main__":
    sys.exit(main())
