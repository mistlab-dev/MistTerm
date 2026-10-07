#!/usr/bin/env python3
"""Fail when the release version is not the same everywhere.

Checks that these all carry the same X.Y.Z:
  * Cargo.toml            [package] version
  * Info.plist            CFBundleShortVersionString and CFBundleVersion
  * the git tag (vX.Y.Z)  when --tag is given (CI passes it on tag builds)

A mismatch would make an installed build report the wrong version, and the
auto-updater would then offer the same "new" version again and again.

Usage:
    python3 scripts/check-version-consistency.py
    python3 scripts/check-version-consistency.py --tag v1.2.0
    python3 scripts/check-version-consistency.py --self-test
"""

from __future__ import annotations

import argparse
import pathlib
import plistlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SEMVER_RE = re.compile(r"^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$")


def cargo_version(text: str) -> str:
    in_package = False
    for raw in text.splitlines():
        line = raw.strip()
        if line.startswith("["):
            in_package = line == "[package]"
            continue
        if in_package:
            m = re.match(r'^version\s*=\s*"([^"]+)"', line)
            if m:
                return m.group(1)
    raise ValueError("Cargo.toml: [package] version not found")


def plist_versions(data: bytes) -> tuple[str, str]:
    plist = plistlib.loads(data)
    return (
        str(plist.get("CFBundleShortVersionString", "")),
        str(plist.get("CFBundleVersion", "")),
    )


def check(cargo_text: str, plist_data: bytes, tag: str | None) -> list[str]:
    errors: list[str] = []
    cargo = cargo_version(cargo_text)
    short, bundle = plist_versions(plist_data)
    if not SEMVER_RE.match(cargo):
        errors.append(f"Cargo.toml version {cargo!r} is not X.Y.Z")
    if short != cargo:
        errors.append(f"Info.plist CFBundleShortVersionString {short!r} != Cargo.toml {cargo!r}")
    if bundle != cargo:
        errors.append(f"Info.plist CFBundleVersion {bundle!r} != Cargo.toml {cargo!r}")
    if tag is not None:
        tag = tag.removeprefix("refs/tags/")
        if not tag.startswith("v"):
            errors.append(f"tag {tag!r} must look like vX.Y.Z")
        elif tag[1:] != cargo:
            errors.append(f"tag {tag!r} != Cargo.toml version {cargo!r} (expected v{cargo})")
    return errors


def self_test() -> int:
    cargo = '[package]\nname = "x"\nversion = "1.2.0"\n\n[dependencies]\nversion = "9"\n'

    def plist(short: str, bundle: str) -> bytes:
        return plistlib.dumps({"CFBundleShortVersionString": short, "CFBundleVersion": bundle})

    assert check(cargo, plist("1.2.0", "1.2.0"), None) == []
    assert check(cargo, plist("1.2.0", "1.2.0"), "v1.2.0") == []
    assert check(cargo, plist("1.2.0", "1.2.0"), "refs/tags/v1.2.0") == []
    assert len(check(cargo, plist("1.1.9", "1.2.0"), None)) == 1
    assert len(check(cargo, plist("1.2.0", "1.1.9"), None)) == 1
    assert len(check(cargo, plist("1.2.0", "1.2.0"), "v1.2.1")) == 1
    assert len(check(cargo, plist("1.2.0", "1.2.0"), "1.2.0")) == 1
    print("check-version-consistency self-test: ok")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--tag", help="release tag, e.g. v1.2.0 or refs/tags/v1.2.0")
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()
    if args.self_test:
        return self_test()
    errors = check(
        (ROOT / "Cargo.toml").read_text(encoding="utf-8"),
        (ROOT / "Info.plist").read_bytes(),
        args.tag,
    )
    if errors:
        for e in errors:
            print(f"::error::version mismatch: {e}")
        print("Bump Cargo.toml and Info.plist together, then tag vX.Y.Z on main.")
        return 1
    print(f"version consistency ok: {cargo_version((ROOT / 'Cargo.toml').read_text(encoding='utf-8'))}"
          + (f" (tag {args.tag})" if args.tag else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main())
