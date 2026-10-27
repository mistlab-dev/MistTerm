#!/usr/bin/env python3
"""Generate the auto-update manifest `latest.json` for a release.

The manifest lists, per platform, the file name, size, SHA-256 and download
URLs. CI signs it with minisign (`latest.json.minisig`); the client verifies
the signature with its built-in public key *before* parsing it, then checks
the trusted comment, which must be exactly:

    mistterm <channel> <version> <pub_date>

Download URL order is configurable (`--url-order`). Release CI passes
`--url-order mirror,github` and
`--mirror-base https://mistlab.dev/downloads/mistterm/v<version>` so clients
try mistlab.dev first. This script's default remains `github,mirror`, and
mirror URLs are omitted unless `--mirror-base` is set.
`--patch-urls` rewrites `urls` in an existing manifest without changing
`pub_date`, checksums, or the trusted comment
(`mistterm <channel> <version> <pub_date>`).

Usage:
    python3 scripts/gen-update-manifest.py --version 1.2.0 --dist dist --out dist/latest.json
    python3 scripts/gen-update-manifest.py --trusted-comment-of dist/latest.json
    python3 scripts/gen-update-manifest.py --patch-urls dist/latest.json --version 1.2.38 \\
        --mirror-base https://mistlab.dev/downloads/mistterm/v1.2.38 --url-order mirror,github
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
        # 只含命令行 `mist` 的静态（musl）包：不依赖 glibc，CentOS 7 这类老系统和 ARM 服务器也能用。
        # 静态版 `mist update` 只认这两个条目；桌面版和老客户端不会用到它们。
        "linux-x86_64-cli": {"name": "mist-cli-linux-x86_64.tar.gz", "kind": "tar.gz", "auto_update": True, "static": True},
        "linux-aarch64-cli": {"name": "mist-cli-linux-aarch64.tar.gz", "kind": "tar.gz", "auto_update": True, "static": True},
        "windows-x86_64-setup": {
            "name": f"MistTerm-{version}-windows-x86_64-setup.exe",
            "kind": "inno-setup",
            "auto_update": True,
        },
        "windows-x86_64-portable": {"name": "Mist-windows-x86_64.zip", "kind": "zip", "auto_update": True},
        # macOS one-click archives are app-only and single-arch (#127).
        # Primary download is Mist-macos-aarch64.dmg; x86_64 is optional.
        # The full Mist-macos-<arch>.tar.gz (+ bare Mist / mist-cli) stays a manual download.
        # auto_update is switched on by --macos-auto-update.
        "macos-aarch64": {
            "name": "Mist-macos-aarch64-update.tar.gz",
            "kind": "tar.gz",
            "auto_update": False,
            "macos": True,
        },
        "macos-x86_64": {
            "name": "Mist-macos-x86_64-update.tar.gz",
            "kind": "tar.gz",
            "auto_update": False,
            "macos": True,
        },
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


def max_glibc_in_tarball(path: pathlib.Path, members=("Mist",)) -> str | None:
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


def assert_gui_tarball_has_no_cli(path: pathlib.Path) -> None:
    """The Linux desktop archive must not ship a second `mist` next to `Mist`."""
    with tarfile.open(path, "r:gz") as tf:
        for member in tf.getmembers():
            if pathlib.PurePosixPath(member.name).name == "mist":
                raise SystemExit(
                    f"{path.name} contains `mist`. The Linux GUI package must not duplicate "
                    "mist-cli; command-line users get mist-cli-linux-*.tar.gz "
                    "(docs/release/PACKAGE_SIZE.md)."
                )


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).replace(microsecond=0).strftime("%Y-%m-%dT%H:%M:%SZ")


def trusted_comment(manifest: dict) -> str:
    return f"{manifest['product']} {manifest['channel']} {manifest['version']} {manifest['pub_date']}"


def resolve_bases(github_base: str | None, mirror_base: str | None, version: str) -> dict[str, str]:
    tag = f"v{version}"
    github = (github_base or f"https://github.com/{REPO}/releases/download/{tag}").rstrip("/")
    bases = {"github": github}
    if mirror_base:
        bases["mirror"] = mirror_base.rstrip("/")
    for base in bases.values():
        if not base.startswith("https://") and not base.startswith("http://127.0.0.1"):
            raise SystemExit(f"download base must be https: {base}")
    return bases


def ordered_urls(name: str, bases: dict[str, str], url_order: str) -> list[str]:
    """Download URLs for one file. Sources missing from `bases` are skipped."""
    order = [part.strip() for part in url_order.split(",") if part.strip()]
    for source in order:
        if source not in ("github", "mirror"):
            raise SystemExit(f"--url-order: unknown source {source!r}")
    urls: list[str] = []
    for source in order:
        if source not in bases:
            continue
        url = f"{bases[source]}/{name}"
        if url not in urls:
            urls.append(url)
    if not urls:
        raise SystemExit("no download URLs (check --url-order / --mirror-base)")
    return urls


def patch_manifest_urls(
    manifest: dict,
    *,
    version: str,
    github_base: str | None = None,
    mirror_base: str | None = None,
    url_order: str = "github,mirror",
) -> dict:
    """Replace each platform's `urls`. Every other field stays as-is."""
    if not SEMVER_RE.match(version):
        raise SystemExit(f"bad version {version!r}")
    if manifest.get("version") != version:
        raise SystemExit(f"manifest version {manifest.get('version')!r} != {version!r}")
    for field in ("product", "channel", "pub_date"):
        if not manifest.get(field):
            raise SystemExit(f"manifest missing {field}")
    platforms = manifest.get("platforms")
    if not isinstance(platforms, dict) or not platforms:
        raise SystemExit("manifest has no platforms")
    order = [part.strip() for part in url_order.split(",") if part.strip()]
    if "mirror" in order and not mirror_base:
        raise SystemExit("--mirror-base is required when --url-order includes mirror")
    bases = resolve_bases(github_base, mirror_base, version)
    for key, entry in platforms.items():
        if not isinstance(entry, dict) or not entry.get("name"):
            raise SystemExit(f"{key}: missing name")
        entry["urls"] = ordered_urls(entry["name"], bases, url_order)
    return manifest


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
    bases = resolve_bases(github_base, mirror_base, version)

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
            "auto_update": macos_auto_update if spec.get("macos") else spec["auto_update"],
            "urls": ordered_urls(spec["name"], bases, url_order),
        }
        if key == "linux-x86_64":
            assert_gui_tarball_has_no_cli(path)
            glibc = min_glibc or max_glibc_in_tarball(path)
            if glibc:
                entry["min_glibc"] = glibc
        if spec.get("static"):
            glibc = max_glibc_in_tarball(path, members=("mist",))
            if glibc:
                raise SystemExit(f"{spec['name']}: static CLI package must not need glibc (found GLIBC_{glibc})")
        if not entry["auto_update"]:
            entry["manual_url"] = DOWNLOAD_PAGE
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
                ("Mist-linux-x86_64/README.md", b"GLIBC_9.99"),
            ]:
                info = tarfile.TarInfo(name)
                info.size = len(data)
                tf.addfile(info, io.BytesIO(data))
        (dist / "Mist-linux-x86_64.tar.gz").write_bytes(buf.getvalue())
        (dist / "Mist-macos-aarch64-update.tar.gz").write_bytes(b"arm")
        (dist / "Mist-macos-x86_64-update.tar.gz").write_bytes(b"x64")
        for arch in ("x86_64", "aarch64"):
            buf = io.BytesIO()
            with tarfile.open(fileobj=buf, mode="w:gz") as tf:
                data = b"\x7fELF...static musl..."
                info = tarfile.TarInfo(f"mist-cli-linux-{arch}/mist")
                info.size = len(data)
                tf.addfile(info, io.BytesIO(data))
            (dist / f"mist-cli-linux-{arch}.tar.gz").write_bytes(buf.getvalue())
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
        assert "macos-universal" not in m["platforms"]
        for key, name, size in (
            ("macos-aarch64", "Mist-macos-aarch64-update.tar.gz", 3),
            ("macos-x86_64", "Mist-macos-x86_64-update.tar.gz", 3),
        ):
            mac = m["platforms"][key]
            assert mac["name"] == name and mac["size"] == size, mac
            assert mac["auto_update"] is False and mac["manual_url"] == DOWNLOAD_PAGE
        m_mac = build_manifest(version="1.2.0", dist=dist, require=["linux-x86_64"], macos_auto_update=True)
        assert m_mac["platforms"]["macos-aarch64"]["auto_update"] is True
        assert m_mac["platforms"]["macos-x86_64"]["auto_update"] is True
        assert "manual_url" not in m_mac["platforms"]["macos-aarch64"]
        # A GUI tarball that still embeds `mist` is rejected.
        dup = io.BytesIO()
        with tarfile.open(fileobj=dup, mode="w:gz") as tf:
            data = b"\x7fELF...GLIBC_2.39..."
            info = tarfile.TarInfo("Mist-linux-x86_64/mist")
            info.size = len(data)
            tf.addfile(info, io.BytesIO(data))
        good_gui = (dist / "Mist-linux-x86_64.tar.gz").read_bytes()
        (dist / "Mist-linux-x86_64.tar.gz").write_bytes(dup.getvalue())
        try:
            build_manifest(version="1.2.0", dist=dist, require=["linux-x86_64"])
            raise AssertionError("GUI package containing mist was accepted")
        except SystemExit:
            pass
        (dist / "Mist-linux-x86_64.tar.gz").write_bytes(good_gui)
        assert "windows-x86_64-setup" not in m["platforms"]
        for key in ("linux-x86_64-cli", "linux-aarch64-cli"):
            cli = m["platforms"][key]
            assert cli["kind"] == "tar.gz" and cli["auto_update"] is True, cli
            assert "min_glibc" not in cli, cli
            assert cli["urls"][0].endswith("/" + cli["name"]), cli
        # A "static" CLI package that actually links glibc must be rejected.
        good_cli = (dist / "mist-cli-linux-aarch64.tar.gz").read_bytes()
        buf = io.BytesIO()
        with tarfile.open(fileobj=buf, mode="w:gz") as tf:
            data = b"\x7fELF...GLIBC_2.17..."
            info = tarfile.TarInfo("mist-cli-linux-aarch64/mist")
            info.size = len(data)
            tf.addfile(info, io.BytesIO(data))
        (dist / "mist-cli-linux-aarch64.tar.gz").write_bytes(buf.getvalue())
        try:
            build_manifest(version="1.2.0", dist=dist, require=["linux-x86_64"])
            raise AssertionError("glibc-linked CLI package not detected")
        except SystemExit:
            pass
        (dist / "mist-cli-linux-aarch64.tar.gz").write_bytes(good_cli)
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
        # Patching an already published manifest only rewrites urls.
        src = json.loads(json.dumps(m))
        src["notes"] = {"zh": "保持不变", "en": "unchanged"}
        mac_name = src["platforms"]["macos-aarch64"]["name"]
        github_mac = f"https://github.com/{REPO}/releases/download/v1.2.0/{mac_name}"
        src["platforms"]["macos-aarch64"]["urls"] = [github_mac, github_mac, "https://example.invalid/extra"]
        frozen = json.loads(json.dumps(src))
        patched = patch_manifest_urls(
            src,
            version="1.2.0",
            mirror_base="https://mistlab.dev/downloads/mistterm/v1.2.0",
            url_order="mirror,github",
        )
        def without_urls(doc: dict) -> dict:
            doc = json.loads(json.dumps(doc))
            for entry in doc["platforms"].values():
                entry.pop("urls", None)
            return doc
        assert without_urls(patched) == without_urls(frozen)
        expect = [
            f"https://mistlab.dev/downloads/mistterm/v1.2.0/{mac_name}",
            github_mac,
        ]
        assert patched["platforms"]["macos-aarch64"]["urls"] == expect, patched["platforms"]["macos-aarch64"]
        assert trusted_comment(patched) == trusted_comment(frozen)
        again = patch_manifest_urls(
            json.loads(json.dumps(patched)),
            version="1.2.0",
            mirror_base="https://mistlab.dev/downloads/mistterm/v1.2.0",
            url_order="mirror,github",
        )
        assert again["platforms"]["macos-aarch64"]["urls"] == expect
        try:
            patch_manifest_urls(
                json.loads(json.dumps(patched)),
                version="9.9.9",
                mirror_base="https://mistlab.dev/downloads/mistterm/v9.9.9",
                url_order="mirror,github",
            )
            raise AssertionError("version mismatch not detected")
        except SystemExit:
            pass
        try:
            patch_manifest_urls(json.loads(json.dumps(patched)), version="1.2.0", url_order="mirror,github")
            raise AssertionError("patch without mirror base was accepted")
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
    ap.add_argument("--mirror-base", help="mirror base URL (release CI: https://mistlab.dev/downloads/mistterm/v<version>)")
    ap.add_argument("--url-order", default="github,mirror", help="download URL order; release CI uses mirror,github")
    ap.add_argument("--patch-urls", type=pathlib.Path, help="rewrite urls in an existing latest.json in place")
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
    if a.patch_urls:
        if not version:
            ap.error("--version/--tag is required with --patch-urls")
        manifest = json.loads(a.patch_urls.read_text(encoding="utf-8"))
        patched = patch_manifest_urls(
            manifest,
            version=version,
            github_base=a.github_base,
            mirror_base=a.mirror_base,
            url_order=a.url_order,
        )
        write_manifest(patched, a.patch_urls)
        print(f"patched {a.patch_urls}")
        print(f"trusted comment: {trusted_comment(patched)}")
        mac = patched["platforms"].get("macos-aarch64")
        if not mac:
            raise SystemExit("macos-aarch64 missing from manifest")
        print("macos-aarch64 urls:")
        for url in mac["urls"]:
            print(url)
        return 0
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
