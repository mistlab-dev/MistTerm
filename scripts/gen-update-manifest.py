#!/usr/bin/env python3
"""Generate the auto-update manifest `latest.json` for a release.

The manifest lists, per platform, the file name, size, SHA-256 and download
URLs. CI signs it with minisign (`latest.json.minisig`); the client verifies
the signature with its built-in public key *before* parsing it, then checks
the trusted comment, which must be exactly:

    mistterm <channel> <version> <pub_date>

Download URL order is configurable (`--url-order`). Today GitHub comes first;
mirror URLs are only added when `--mirror-base` is given (the mistlab.dev
mirror upload is not enabled yet — see docs/release/AUTO_UPDATE.md).

Usage:
    python3 scripts/gen-update-manifest.py --version 1.2.0 --dist dist --out dist/latest.json
    python3 scripts/gen-update-manifest.py --trusted-comment-of dist/latest.json
    python3 scripts/gen-update-manifest.py --self-test
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import io
import json
import pathlib
import re
import sys
import tarfile
import tempfile

PRODUCT = "mistterm"
SCHEMA = 1
REPO = "mistlab-dev/MistTerm"
DOWNLOAD_PAGE = "https://mistlab.dev/download.html"
GLIBC_RE = re.compile(rb"GLIBC_(\d+)\.(\d+)")
SEMVER_RE = re.compile(r"^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$")


def platform_files(version: str) -> dict[str, dict]:
    """Platform key -> how the release file is named and handled."""
    return {
        "linux-x86_64": {"name": "Mist-linux-x86_64.tar.gz", "kind": "tar.gz", "auto_update": True},
        "windows-x86_64-setup": {
            "name": f"MistTerm-{version}-windows-x86_64-setup.exe",
            "kind": "inno-setup",
            "auto_update": True,
        },
        "windows-x86_64-portable": {"name": "Mist-windows-x86_64.zip", "kind": "zip", "auto_update": True},
        # macOS: notify only for now (the client also refuses to self-replace on macOS).
        "macos-universal": {"name": "Mist-macos-universal.tar.gz", "kind": "tar.gz", "auto_update": False},
    }


def sha256_of(path: pathlib.Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def read_sha256sums(path: pathlib.Path) -> dict[str, str]:
    sums: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        parts = line.strip().split()
        if len(parts) == 2:
            sums[parts[1].lstrip("*")] = parts[0].lower()
    return sums


def max_glibc_in_tarball(path: pathlib.Path, members=("Mist", "mist")) -> str | None:
    """Highest GLIBC_x.y symbol version referenced by the Linux binaries."""
    best: tuple[int, int] | None = None
    with tarfile.open(path, "r:gz") as tf:
        for m in tf.getmembers():
            if not m.isfile() or pathlib.PurePosixPath(m.name).name not in members:
                continue
            f = tf.extractfile(m)
            if f is None:
                continue
            for major, minor in GLIBC_RE.findall(f.read()):
                v = (int(major), int(minor))
                if best is None or v > best:
                    best = v
    return f"{best[0]}.{best[1]}" if best else None


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).replace(microsecond=0).strftime("%Y-%m-%dT%H:%M:%SZ")


def trusted_comment(manifest: dict) -> str:
    return f"{manifest['product']} {manifest['channel']} {manifest['version']} {manifest['pub_date']}"


def build_manifest(
    *,
    version: str,
    dist: pathlib.Path,
    channel: str = "stable",
    pub_date: str | None = None,
    github_base: str | None = None,
    mirror_base: str | None = None,
    url_order: str = "github,mirror",
    require: list[str] | None = None,
    notes_zh: str = "",
    notes_en: str = "",
    notes_url: str | None = None,
    min_glibc: str | None = None,
    macos_auto_update: bool = False,
) -> dict:
    if not SEMVER_RE.match(version):
        raise SystemExit(f"bad version {version!r}")
    tag = f"v{version}"
    github_base = (github_base or f"https://github.com/{REPO}/releases/download/{tag}").rstrip("/")
    bases = {"github": github_base}
    if mirror_base:
        bases["mirror"] = mirror_base.rstrip("/")
    order = [o.strip() for o in url_order.split(",") if o.strip()]
    for o in order:
        if o not in ("github", "mirror"):
            raise SystemExit(f"--url-order: unknown source {o!r}")
    for b in bases.values():
        if not b.startswith("https://") and not b.startswith("http://127.0.0.1"):
            raise SystemExit(f"download base must be https: {b}")

    sums_path = dist / "SHA256SUMS"
    sums = read_sha256sums(sums_path) if sums_path.is_file() else {}
    files = platform_files(version)
    required = require if require is not None else list(files)
    platforms: dict[str, dict] = {}
    for key, spec in files.items():
        path = dist / spec["name"]
        if not path.is_file():
            if key in required:
                raise SystemExit(f"missing release file for {key}: {path}")
            continue
        digest = sha256_of(path)
        if spec["name"] in sums and sums[spec["name"]] != digest:
            raise SystemExit(f"SHA256SUMS disagrees with {spec['name']}")
        entry: dict = {
            "kind": spec["kind"],
            "name": spec["name"],
            "size": path.stat().st_size,
            "sha256": digest,
            "auto_update": spec["auto_update"] if key != "macos-universal" else macos_auto_update,
            "urls": [f"{bases[o]}/{spec['name']}" for o in order if o in bases],
        }
        if key == "linux-x86_64":
            glibc = min_glibc or max_glibc_in_tarball(path)
            if glibc:
                entry["min_glibc"] = glibc
        if not entry["auto_update"]:
            entry["manual_url"] = DOWNLOAD_PAGE
        if not entry["urls"]:
            raise SystemExit("no download URLs (check --url-order / --mirror-base)")
        platforms[key] = entry

    return {
        "schema": SCHEMA,
        "product": PRODUCT,
        "channel": channel,
        "version": version,
        "pub_date": pub_date or utc_now(),
        "notes_url": notes_url or f"https://github.com/{REPO}/releases/tag/{tag}",
        "notes": {"zh": notes_zh, "en": notes_en},
        "platforms": platforms,
    }


def write_manifest(manifest: dict, out: pathlib.Path) -> None:
    out.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def self_test() -> int:
    with tempfile.TemporaryDirectory() as tmp:
        dist = pathlib.Path(tmp)
        # Fake Linux tarball whose binaries reference GLIBC_2.34 and GLIBC_2.39.
        buf = io.BytesIO()
        with tarfile.open(fileobj=buf, mode="w:gz") as tf:
            for name, data in [
                ("Mist-linux-x86_64/Mist", b"\x7fELF...GLIBC_2.34...GLIBC_2.39..."),
                ("Mist-linux-x86_64/mist", b"\x7fELF...GLIBC_2.17..."),
                ("Mist-linux-x86_64/README.md", b"GLIBC_9.99"),
            ]:
                info = tarfile.TarInfo(name)
                info.size = len(data)
                tf.addfile(info, io.BytesIO(data))
        (dist / "Mist-linux-x86_64.tar.gz").write_bytes(buf.getvalue())
        (dist / "Mist-macos-universal.tar.gz").write_bytes(b"mac")
        m = build_manifest(
            version="1.2.0",
            dist=dist,
            pub_date="2026-10-05T12:00:00Z",
            require=["linux-x86_64"],
            mirror_base="https://mistlab.dev/downloads/mistterm/v1.2.0",
        )
        linux = m["platforms"]["linux-x86_64"]
        assert linux["min_glibc"] == "2.39", linux
        assert linux["urls"][0].startswith("https://github.com/"), linux["urls"]
        assert linux["urls"][1].startswith("https://mistlab.dev/"), linux["urls"]
        assert m["platforms"]["macos-universal"]["auto_update"] is False
        assert m["platforms"]["macos-universal"]["manual_url"] == DOWNLOAD_PAGE
        assert "windows-x86_64-setup" not in m["platforms"]
        assert trusted_comment(m) == "mistterm stable 1.2.0 2026-10-05T12:00:00Z"
        m2 = build_manifest(version="1.2.0", dist=dist, require=["linux-x86_64"],
                            mirror_base="https://mistlab.dev/x", url_order="mirror,github")
        assert m2["platforms"]["linux-x86_64"]["urls"][0].startswith("https://mistlab.dev/")
        m3 = build_manifest(version="1.2.0", dist=dist, require=["linux-x86_64"])
        assert len(m3["platforms"]["linux-x86_64"]["urls"]) == 1  # mirror disabled
        (dist / "SHA256SUMS").write_text("00" * 32 + "  Mist-linux-x86_64.tar.gz\n")
        try:
            build_manifest(version="1.2.0", dist=dist, require=["linux-x86_64"])
            raise AssertionError("SHA256SUMS mismatch not detected")
        except SystemExit:
            pass
        try:
            build_manifest(version="1.2.0", dist=dist)  # windows files missing
            raise AssertionError("missing required file not detected")
        except SystemExit:
            pass
    print("gen-update-manifest self-test: ok")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--version", help="release version X.Y.Z (or use --tag)")
    ap.add_argument("--tag", help="release tag vX.Y.Z")
    ap.add_argument("--dist", type=pathlib.Path, help="directory with the release files")
    ap.add_argument("--out", type=pathlib.Path, help="output path (default <dist>/latest.json)")
    ap.add_argument("--channel", default="stable")
    ap.add_argument("--pub-date", help="RFC 3339 UTC, default now")
    ap.add_argument("--github-base", help="default https://github.com/<repo>/releases/download/v<version>")
    ap.add_argument("--mirror-base", help="mirror base URL; omit while the mirror is disabled")
    ap.add_argument("--url-order", default="github,mirror", help="download URL order, e.g. github,mirror")
    ap.add_argument("--require", help="comma-separated platform keys that must exist (default: all)")
    ap.add_argument("--notes-zh", type=pathlib.Path, help="Markdown release notes (Chinese)")
    ap.add_argument("--notes-en", type=pathlib.Path, help="Markdown release notes (English)")
    ap.add_argument("--notes-url")
    ap.add_argument("--min-glibc", help="override detected minimum glibc for Linux")
    ap.add_argument("--macos-auto-update", action="store_true", help="allow macOS one-click update (P6+)")
    ap.add_argument("--trusted-comment-of", type=pathlib.Path, help="print the trusted comment for a manifest")
    ap.add_argument("--self-test", action="store_true")
    a = ap.parse_args()

    if a.self_test:
        return self_test()
    if a.trusted_comment_of:
        print(trusted_comment(json.loads(a.trusted_comment_of.read_text(encoding="utf-8"))))
        return 0
    version = a.version or (a.tag[1:] if a.tag and a.tag.startswith("v") else None)
    if not version or not a.dist:
        ap.error("--version/--tag and --dist are required")
    read = lambda p: p.read_text(encoding="utf-8") if p and p.is_file() else ""
    manifest = build_manifest(
        version=version,
        dist=a.dist,
        channel=a.channel,
        pub_date=a.pub_date,
        github_base=a.github_base,
        mirror_base=a.mirror_base,
        url_order=a.url_order,
        require=[r.strip() for r in a.require.split(",")] if a.require else None,
        notes_zh=read(a.notes_zh),
        notes_en=read(a.notes_en),
        notes_url=a.notes_url,
        min_glibc=a.min_glibc,
        macos_auto_update=a.macos_auto_update,
    )
    out = a.out or (a.dist / "latest.json")
    write_manifest(manifest, out)
    print(f"wrote {out}")
    print(f"trusted comment: {trusted_comment(manifest)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
