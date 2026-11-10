#!/usr/bin/env python3
"""Sync one published MistTerm release onto the mistlab.dev download mirror.

Public layout (clients and the website already use these paths):

    v<version>/            immutable installers, checksums, then latest.json
    stable/latest.json     channel pointer the app reads first
    latest/                website download buttons (download.js mirror base)

Order is fixed: versioned installers, verify, versioned manifest, verify,
and only then the stable pointer and the website alias. A tag older than the
public stable manifest is never promoted. Installer bytes already published
for the live version are not replaced when the size differs.

The deploy key is read from the environment (`MIST_DEPLOY_KEY`) and written to
a mode-600 file. It is never printed. `--self-test` and `--dry-run` do not
upload. `--dry-run` still reads the public URLs so the plan matches production.

    python3 scripts/sync-release-mirror.py --self-test
    python3 scripts/sync-release-mirror.py --version 1.2.3 --dist dist --dry-run
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.parse
from dataclasses import dataclass
from pathlib import Path

PUBLIC_BASE = "https://mistlab.dev/downloads/mistterm"
DEFAULT_HOST = "85.137.247.166"
DEFAULT_USER = "mist-deploy"
REMOTE_ROOT = "/var/www/mistlab/downloads/mistterm"
SEMVER_RE = re.compile(r"^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$")
MANIFEST_NAMES = ("latest.json", "latest.json.minisig")
CHECKSUM_NAMES = ("SHA256SUMS", "SHA256SUMS.minisig")
SKIP_IN_INSTALLERS = set(MANIFEST_NAMES) | set(CHECKSUM_NAMES)


class MirrorError(Exception):
    """A sync step failed before or during publication. Safe to retry."""


@dataclass(frozen=True)
class LocalRelease:
    version: str
    installers: dict[str, int]
    checksums: dict[str, int]
    manifests: dict[str, int]

    def size_of(self, name: str) -> int:
        for group in (self.installers, self.checksums, self.manifests):
            if name in group:
                return group[name]
        raise KeyError(name)


@dataclass(frozen=True)
class Phase:
    name: str
    subdir: str
    uploads: tuple[str, ...]
    checks: tuple[str, ...]
    expect_version: bool
    promotion: bool


def version_key(version: str) -> tuple:
    match = SEMVER_RE.match(version)
    if not match:
        raise MirrorError(f"bad version {version!r}")
    pre = match.group(4)
    # A final release sorts after its prerelease. Numeric parts, not text order,
    # so 1.3.15 is newer than 1.3.9.
    return (
        int(match.group(1)),
        int(match.group(2)),
        int(match.group(3)),
        0 if pre else 1,
        pre or "",
    )


def version_subdir(version: str) -> str:
    return f"v{version}"


def rel_path(subdir: str, name: str) -> str:
    return f"{subdir}/{name}"


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_local(dist: Path, version: str) -> LocalRelease:
    """Reject a partial or inconsistent dist before any network call."""
    if not SEMVER_RE.match(version):
        raise MirrorError(f"bad version {version!r}")
    manifest_path = dist / "latest.json"
    if not manifest_path.is_file():
        raise MirrorError(f"missing {manifest_path}")
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise MirrorError(f"latest.json is not json: {exc}") from exc
    if manifest.get("version") != version:
        raise MirrorError(
            f"latest.json version {manifest.get('version')!r} != {version}"
        )
    platforms = manifest.get("platforms")
    if not isinstance(platforms, dict) or not platforms:
        raise MirrorError("latest.json has no platforms")

    def require_file(name: str) -> Path:
        path = dist / name
        if not path.is_file() or path.stat().st_size == 0:
            raise MirrorError(f"missing {name}")
        return path

    require_file("latest.json.minisig")
    sums_path = require_file("SHA256SUMS")
    require_file("SHA256SUMS.minisig")

    sums: dict[str, str] = {}
    for raw in sums_path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line:
            continue
        parts = line.split()
        if len(parts) != 2 or not re.fullmatch(r"[0-9a-fA-F]{64}", parts[0]):
            raise MirrorError(f"bad SHA256SUMS line: {raw!r}")
        sums[parts[1].lstrip("*")] = parts[0].lower()
    if not sums:
        raise MirrorError("SHA256SUMS is empty")

    digests: dict[str, str] = {}

    def digest(name: str) -> str:
        if name not in digests:
            digests[name] = sha256_file(dist / name)
        return digests[name]

    installers: dict[str, int] = {}
    for name, expected in sorted(sums.items()):
        if name in SKIP_IN_INSTALLERS:
            continue
        path = dist / name
        if not path.is_file() or path.stat().st_size == 0:
            raise MirrorError(f"partial release: missing {name}")
        if digest(name) != expected:
            raise MirrorError(f"partial release: SHA256SUMS mismatch for {name}")
        installers[name] = path.stat().st_size
    if not installers:
        raise MirrorError("SHA256SUMS lists no installers")

    for key, entry in platforms.items():
        if not isinstance(entry, dict) or not entry.get("name"):
            raise MirrorError(f"{key}: bad platform entry")
        name = entry["name"]
        if name not in installers:
            raise MirrorError(f"partial release: {name} missing from SHA256SUMS")
        if str(entry.get("sha256", "")).lower() != digest(name):
            raise MirrorError(f"latest.json sha256 mismatch for {name}")
        if entry.get("size") != installers[name]:
            raise MirrorError(f"latest.json size mismatch for {name}")

    return LocalRelease(
        version=version,
        installers=installers,
        checksums={
            "SHA256SUMS": (dist / "SHA256SUMS").stat().st_size,
            "SHA256SUMS.minisig": (dist / "SHA256SUMS.minisig").stat().st_size,
        },
        manifests={
            "latest.json": manifest_path.stat().st_size,
            "latest.json.minisig": (dist / "latest.json.minisig").stat().st_size,
        },
    )


def classify_stable(stable: str | None, version: str) -> str:
    """Return newer, live, or older (including nothing published yet)."""
    if stable is None:
        return "older"
    if not SEMVER_RE.match(stable):
        raise MirrorError(f"public stable version is not semver: {stable!r}")
    current = version_key(stable)
    target = version_key(version)
    if current > target:
        return "newer"
    if current < target:
        return "older"
    return "live"


def select_uploads(
    names: tuple[str, ...],
    subdir: str,
    local: LocalRelease,
    remote_sizes: dict[str, int | None],
    mode: str,
) -> tuple[str, ...]:
    """Choose files to send. `mode` is immutable, replace, or always."""
    uploads: list[str] = []
    for name in names:
        remote = remote_sizes[rel_path(subdir, name)]
        local_size = local.size_of(name)
        if mode != "always" and remote == local_size:
            continue
        if remote is None or mode in ("replace", "always"):
            uploads.append(name)
            continue
        raise MirrorError(
            f"refuse to overwrite published {rel_path(subdir, name)}: "
            f"remote size {remote} != local {local_size}"
        )
    return tuple(uploads)


def plan_sync(
    local: LocalRelease,
    remote_sizes: dict[str, int | None],
    stable_version: str | None,
) -> list[Phase]:
    """Build phases. Raises before any upload when a live file would be clobbered."""
    relation = classify_stable(stable_version, local.version)
    version_dir = version_subdir(local.version)
    installer_names = tuple(sorted(local.installers))
    checksum_names = tuple(CHECKSUM_NAMES)
    archive_names = installer_names + checksum_names
    manifest_names = tuple(MANIFEST_NAMES)
    archive_mode = "replace" if relation == "older" else "immutable"
    manifest_mode = "immutable" if relation == "newer" else "always"

    def phase(
        name: str,
        subdir: str,
        names: tuple[str, ...],
        mode: str,
        *,
        expect_version: bool,
        promotion: bool,
    ) -> Phase:
        return Phase(
            name=name,
            subdir=subdir,
            uploads=select_uploads(names, subdir, local, remote_sizes, mode),
            checks=names,
            expect_version=expect_version,
            promotion=promotion,
        )

    phases = [
        phase(
            "versioned-installers",
            version_dir,
            archive_names,
            archive_mode,
            expect_version=False,
            promotion=False,
        ),
        phase(
            "versioned-manifest",
            version_dir,
            manifest_names,
            manifest_mode,
            expect_version=True,
            promotion=False,
        ),
    ]
    if relation == "newer":
        return phases
    phases.append(
        phase(
            "stable-manifest",
            "stable",
            manifest_names,
            "always",
            expect_version=True,
            promotion=True,
        )
    )
    phases.append(
        phase(
            "website-installers",
            "latest",
            archive_names,
            "replace",
            expect_version=False,
            promotion=True,
        )
    )
    phases.append(
        phase(
            "website-manifest",
            "latest",
            manifest_names,
            "always",
            expect_version=True,
            promotion=True,
        )
    )
    return phases


def execute(
    phases: list[Phase],
    local: LocalRelease,
    transport: "Transport",
    *,
    dry_run: bool,
    verify_attempts: int = 5,
    retry_sleep: float = 2.0,
    sleep=time.sleep,
) -> None:
    """Run phases in order. A failed check stops the run before later promotions."""
    promotion_checked = False
    for phase in phases:
        if not dry_run and phase.promotion and not promotion_checked:
            promotion_checked = True
            current = transport.fetch_stable_version()
            if current is not None and version_key(current) > version_key(local.version):
                raise MirrorError(
                    f"stable moved to {current} while syncing {local.version}; "
                    "not promoting"
                )
        uploaded = ", ".join(phase.uploads) if phase.uploads else "-"
        print(f"mirror phase {phase.name}: upload [{uploaded}]", flush=True)
        if dry_run:
            continue
        if phase.uploads:
            transport.put_tree(phase.subdir, phase.uploads)
        for name in phase.checks:
            expect = local.size_of(name)
            rel = rel_path(phase.subdir, name)
            got = _retry_size(transport, rel, expect, verify_attempts, retry_sleep, sleep)
            if got != expect:
                raise MirrorError(
                    f"{rel} size {got} != local {expect}; not continuing"
                )
        if phase.expect_version:
            rel = rel_path(phase.subdir, "latest.json")
            got_version = _retry_version(
                transport, rel, local.version, verify_attempts, retry_sleep, sleep
            )
            if got_version != local.version:
                raise MirrorError(
                    f"{rel} version {got_version!r} != {local.version}; not promoting"
                )


def _retry_size(transport, rel, expect, attempts, delay, sleep) -> int | None:
    got = None
    for attempt in range(attempts):
        got = transport.head_size(rel)
        if got == expect:
            return got
        if attempt + 1 < attempts:
            sleep(delay)
    return got


def _retry_version(transport, rel, expect, attempts, delay, sleep) -> str | None:
    got = None
    for attempt in range(attempts):
        try:
            got = transport.read_version(rel)
        except MirrorError:
            got = None
        if got == expect:
            return got
        if attempt + 1 < attempts:
            sleep(delay)
    return got


def run_sync(
    local: LocalRelease,
    transport: "Transport",
    *,
    dry_run: bool,
    key_present: bool,
    verify_attempts: int = 5,
    retry_sleep: float = 2.0,
    sleep=time.sleep,
) -> list[Phase]:
    if not dry_run and not key_present:
        raise MirrorError(
            "MIST_DEPLOY_KEY is not set. Put the existing deploy private key in "
            "the release environment (same place as the minisign secrets). "
            "The mirror was not updated."
        )
    stable = transport.fetch_stable_version()
    print(f"public stable: {stable or '(none)'}; syncing {local.version}", flush=True)
    relation = classify_stable(stable, local.version)
    subdirs = [version_subdir(local.version)]
    if relation != "newer":
        subdirs.extend(["stable", "latest"])
    remote_sizes: dict[str, int | None] = {}
    names = tuple(sorted(local.installers)) + CHECKSUM_NAMES + MANIFEST_NAMES
    for subdir in subdirs:
        for name in names:
            rel = rel_path(subdir, name)
            remote_sizes[rel] = transport.head_size(rel)
    phases = plan_sync(local, remote_sizes, stable)
    execute(
        phases,
        local,
        transport,
        dry_run=dry_run,
        verify_attempts=verify_attempts,
        retry_sleep=retry_sleep,
        sleep=sleep,
    )
    if relation == "newer":
        print(
            f"stable stays at {stable}; {local.version} was not promoted",
            flush=True,
        )
    return phases


class Transport:
    def fetch_stable_version(self) -> str | None:
        raise NotImplementedError

    def head_size(self, rel: str) -> int | None:
        raise NotImplementedError

    def read_version(self, rel: str) -> str:
        raise NotImplementedError

    def put_tree(self, subdir: str, names: tuple[str, ...]) -> None:
        raise NotImplementedError


def require_endpoint(user: str, host: str) -> None:
    if not re.fullmatch(r"[a-z_][a-z0-9_-]{0,31}", user):
        raise MirrorError("bad mirror user")
    if not re.fullmatch(r"[A-Za-z0-9](?:[A-Za-z0-9._-]{0,252})?", host):
        raise MirrorError("bad mirror host")


def rsync_argv(
    *,
    key_path: Path,
    known_hosts: Path,
    user: str,
    host: str,
    source_dir: Path,
    remote_root: str,
) -> list[str]:
    """Copy the directory named `source_dir` into the mirror root.

    The source must be the directory itself (no trailing slash) so rsync
    creates `v<version>/` when that directory is new. A trailing slash would
    copy the contents and require the destination to exist already.
    """
    require_endpoint(user, host)
    source = str(source_dir)
    if source.endswith("/") or source.endswith(os.sep):
        raise MirrorError(f"rsync source must not have a trailing slash: {source}")
    ssh = (
        f"ssh -i {key_path} -o IdentitiesOnly=yes -o BatchMode=yes "
        f"-o StrictHostKeyChecking=yes -o UserKnownHostsFile={known_hosts} "
        f"-o ConnectTimeout=20"
    )
    remote = f"{user}@{host}:{remote_root.rstrip('/')}/"
    return ["rsync", "-az", "--timeout=120", "-e", ssh, source, remote]


def ssh_keyscan_argv(host: str) -> list[str]:
    return ["ssh-keyscan", "-T", "10", "-H", host]


def install_private_key(secret: str, path: Path) -> None:
    text = secret.strip()
    if "PRIVATE KEY" not in text:
        raise MirrorError(
            "MIST_DEPLOY_KEY is set but does not look like a private key"
        )
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as handle:
        handle.write(text + "\n")


def content_length(header_text: str) -> int | None:
    blocks = [block for block in re.split(r"\r?\n\r?\n", header_text.strip()) if block]
    last = blocks[-1] if blocks else header_text
    found = re.findall(r"(?im)^content-length:\s*(\d+)\s*$", last)
    if not found:
        return None
    return int(found[-1])


def version_from_body(body: bytes, url: str) -> str:
    try:
        doc = json.loads(body.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise MirrorError(f"{url} is not a JSON manifest") from exc
    version = doc.get("version") if isinstance(doc, dict) else None
    if not isinstance(version, str):
        raise MirrorError(f"{url} has no version")
    return version


class HttpRsyncTransport(Transport):
    def __init__(
        self,
        *,
        dist: Path,
        host: str,
        user: str,
        key_path: Path,
        known_hosts: Path,
        public_base: str = PUBLIC_BASE,
        remote_root: str = REMOTE_ROOT,
        runner=subprocess.run,
    ) -> None:
        self.dist = dist
        self.host = host
        self.user = user
        self.key_path = key_path
        self.known_hosts = known_hosts
        self.public_base = public_base.rstrip("/")
        self.remote_root = remote_root
        self.runner = runner

    def fetch_stable_version(self) -> str | None:
        return self._read_version("stable/latest.json", missing_ok=True)

    def head_size(self, rel: str) -> int | None:
        code, headers, _body = self._curl(self._url(rel), head=True)
        if code == 404:
            return None
        if code != 200:
            raise MirrorError(f"HEAD {rel} returned HTTP {code}")
        size = content_length(headers)
        if size is None:
            raise MirrorError(f"HEAD {rel} returned no Content-Length")
        return size

    def read_version(self, rel: str) -> str:
        version = self._read_version(rel, missing_ok=False)
        if version is None:
            raise MirrorError(f"missing {rel}")
        return version

    def put_tree(self, subdir: str, names: tuple[str, ...]) -> None:
        stage_root = Path(tempfile.mkdtemp(prefix="mist-mirror-"))
        try:
            folder = stage_root / subdir
            folder.mkdir()
            for name in names:
                src = self.dist / name
                dest = folder / name
                try:
                    os.link(src, dest)
                except OSError:
                    shutil.copy2(src, dest)
            command = rsync_argv(
                key_path=self.key_path,
                known_hosts=self.known_hosts,
                user=self.user,
                host=self.host,
                source_dir=folder,
                remote_root=self.remote_root,
            )
            proc = self.runner(
                command,
                check=False,
                capture_output=True,
                text=True,
                timeout=600,
            )
            if proc.returncode != 0:
                detail = (proc.stderr or proc.stdout or "").strip().splitlines()
                tail = detail[-1] if detail else f"exit {proc.returncode}"
                raise MirrorError(f"rsync {subdir} failed: {tail}")
        finally:
            shutil.rmtree(stage_root, ignore_errors=True)

    def _read_version(self, rel: str, *, missing_ok: bool) -> str | None:
        url = self._url(rel)
        code, _headers, body = self._curl(url, head=False)
        if code == 404 and missing_ok:
            return None
        if code != 200:
            raise MirrorError(f"GET {rel} returned HTTP {code}")
        return version_from_body(body, url)

    def _url(self, rel: str) -> str:
        return f"{self.public_base}/{rel}"

    def _curl(self, url: str, *, head: bool) -> tuple[int, str, bytes]:
        with tempfile.TemporaryDirectory(prefix="mist-mirror-curl-") as tmp:
            header_path = Path(tmp) / "headers"
            body_path = Path(tmp) / "body"
            command = [
                "curl",
                "-sS",
                "-L",
                "--max-redirs",
                "3",
                "--max-time",
                "30",
                "--retry",
                "2",
                "-D",
                str(header_path),
                "-o",
                str(body_path),
                "-w",
                "%{http_code}",
                url,
            ]
            if head:
                command.insert(1, "-I")
            proc = self.runner(command, check=False, capture_output=True, text=True, timeout=60)
            if proc.returncode != 0:
                detail = (proc.stderr or "").strip().splitlines()
                tail = detail[-1] if detail else f"exit {proc.returncode}"
                raise MirrorError(f"request failed for {url}: {tail}")
            try:
                code = int((proc.stdout or "0").strip() or "0")
            except ValueError as exc:
                raise MirrorError(f"request failed for {url}: bad status") from exc
            headers = header_path.read_text(encoding="utf-8", errors="replace")
            body = body_path.read_bytes()
            return code, headers, body


def prepare_ssh(secret: str, host: str, work: Path, runner=subprocess.run) -> tuple[Path, Path]:
    require_endpoint("mist-deploy", host)
    key_path = work / "id_ed25519"
    install_private_key(secret, key_path)
    proc = runner(
        ssh_keyscan_argv(host),
        check=False,
        capture_output=True,
        text=True,
        timeout=30,
    )
    if proc.returncode != 0 or not proc.stdout.strip():
        raise MirrorError(f"ssh-keyscan failed for {host}")
    known = work / "known_hosts"
    fd = os.open(known, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as handle:
        handle.write(proc.stdout if proc.stdout.endswith("\n") else proc.stdout + "\n")
    return key_path, known


def report(error: MirrorError) -> int:
    message = str(error)
    if os.environ.get("GITHUB_ACTIONS"):
        print(f"::error title=Mirror sync failed::{message}")
    else:
        print(f"error: {message}", file=sys.stderr)
    return 1


def self_test() -> int:
    _test_version_order()
    _test_partial_release_does_not_touch_remote()
    _test_checksum_mismatch_does_not_touch_remote()
    _test_missing_key_does_not_touch_remote()
    _test_dry_run_plans_without_upload()
    _test_older_stable_promotes_in_order()
    _test_live_size_mismatch_refuses_overwrite()
    _test_newer_stable_is_not_promoted()
    _test_newer_stable_refuses_different_bytes()
    _test_verify_failure_stops_before_promotion()
    _test_stable_race_does_not_downgrade()
    _test_live_retry_repairs_website_alias()
    _test_rsync_command_hides_key_and_creates_directory()
    _test_content_length_parser()
    _test_http_transport_with_fake_curl()
    _test_workflow_contract()
    print("sync-release-mirror self-test: ok")
    return 0


def _sample_release(tmp: Path, *, omit: str | None = None, bad_sum: bool = False) -> LocalRelease:
    installer = b"installer-bytes"
    extra = b"dmg-bytes"
    (tmp / "pkg.bin").write_bytes(installer)
    (tmp / "Extra.dmg").write_bytes(extra)
    lines = [
        f"{hashlib.sha256(installer).hexdigest()}  pkg.bin",
        f"{hashlib.sha256(extra).hexdigest()}  Extra.dmg",
    ]
    if bad_sum:
        lines[0] = f"{'ab' * 32}  pkg.bin"
    (tmp / "SHA256SUMS").write_text("\n".join(lines) + "\n", encoding="utf-8")
    (tmp / "SHA256SUMS.minisig").write_text("checksum-sig\n", encoding="utf-8")
    manifest = {
        "version": "1.2.3",
        "platforms": {
            "linux-test": {
                "name": "pkg.bin",
                "size": len(installer),
                "sha256": hashlib.sha256(installer).hexdigest(),
            }
        },
    }
    (tmp / "latest.json").write_text(json.dumps(manifest), encoding="utf-8")
    (tmp / "latest.json.minisig").write_text("manifest-sig\n", encoding="utf-8")
    if omit:
        (tmp / omit).unlink()
    if bad_sum or omit:
        return None  # caller asserts the failure
    return load_local(tmp, "1.2.3")


class FakeTransport(Transport):
    def __init__(
        self,
        stable: str | None,
        sizes: dict[str, int | None] | None = None,
        *,
        stable_after_puts: str | None = None,
        lie_about_size: str | None = None,
    ) -> None:
        self.stable = stable
        self.stable_after_puts = stable_after_puts
        self.sizes = dict(sizes or {})
        self.bodies: dict[str, str] = {}
        self.ops: list[tuple] = []
        self.puts = 0
        self.lie_about_size = lie_about_size

    def fetch_stable_version(self) -> str | None:
        self.ops.append(("stable",))
        if self.stable_after_puts is not None and self.puts:
            return self.stable_after_puts
        return self.stable

    def head_size(self, rel: str) -> int | None:
        self.ops.append(("head", rel))
        if self.lie_about_size == rel and self.puts:
            return 1
        return self.sizes.get(rel)

    def read_version(self, rel: str) -> str:
        self.ops.append(("get", rel))
        if rel not in self.bodies:
            raise MirrorError(f"missing {rel}")
        return json.loads(self.bodies[rel])["version"]

    def put_tree(self, subdir: str, names: tuple[str, ...]) -> None:
        self.ops.append(("put", subdir, names))
        self.puts += 1
        for name in names:
            rel = rel_path(subdir, name)
            self.sizes[rel] = None  # filled by the test via bind()
        self._bind_uploads(subdir, names)

    def bind(self, local: LocalRelease) -> "FakeTransport":
        self._local = local
        return self

    def _bind_uploads(self, subdir: str, names: tuple[str, ...]) -> None:
        local = getattr(self, "_local", None)
        if local is None:
            return
        for name in names:
            rel = rel_path(subdir, name)
            self.sizes[rel] = local.size_of(name)
            if name == "latest.json":
                self.bodies[rel] = json.dumps({"version": local.version})


def _expect_error(fn, snippet: str) -> None:
    try:
        fn()
    except MirrorError as exc:
        if snippet not in str(exc):
            raise AssertionError(f"expected {snippet!r} in {exc}") from exc
        return
    raise AssertionError(f"expected error containing {snippet!r}")


def _test_version_order() -> None:
    assert version_key("1.3.9") < version_key("1.3.15")
    assert version_key("1.3.15") < version_key("1.10.0")
    assert version_key("1.3.15-rc1") < version_key("1.3.15")
    assert classify_stable("1.3.9", "1.3.15") == "older"
    assert classify_stable("1.3.15", "1.3.15") == "live"
    assert classify_stable("1.4.0", "1.3.15") == "newer"
    assert classify_stable(None, "1.3.15") == "older"


def _test_partial_release_does_not_touch_remote() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        _write_base(root)
        (root / "Extra.dmg").unlink()
        remote = FakeTransport("1.2.0")
        _expect_error(lambda: load_local(root, "1.2.3"), "missing Extra.dmg")
        assert remote.ops == []


def _test_checksum_mismatch_does_not_touch_remote() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        _sample_release(root, bad_sum=True)
        remote = FakeTransport("1.2.0")
        _expect_error(lambda: load_local(root, "1.2.3"), "SHA256SUMS mismatch")
        assert remote.ops == []


def _test_missing_key_does_not_touch_remote() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        local = _write_base(root)
        remote = FakeTransport("1.2.0").bind(local)
        _expect_error(
            lambda: run_sync(local, remote, dry_run=False, key_present=False, verify_attempts=1),
            "MIST_DEPLOY_KEY is not set",
        )
        assert remote.ops == []


def _test_dry_run_plans_without_upload() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        local = _write_base(Path(tmp))
        remote = FakeTransport(None).bind(local)
        phases = run_sync(
            local,
            remote,
            dry_run=True,
            key_present=False,
            verify_attempts=1,
            retry_sleep=0,
        )
        assert [phase.name for phase in phases] == [
            "versioned-installers",
            "versioned-manifest",
            "stable-manifest",
            "website-installers",
            "website-manifest",
        ]
        assert "latest.json" not in phases[0].uploads
        assert "Extra.dmg" in phases[0].checks
        assert ("put", "stable", MANIFEST_NAMES) not in remote.ops
        assert not any(op[0] == "put" for op in remote.ops)


def _test_older_stable_promotes_in_order() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        local = _write_base(Path(tmp))
        # One versioned installer is a short partial upload and may be replaced
        # because this version is not stable yet. The website alias is old too.
        remote = FakeTransport("1.2.0").bind(local)
        remote.sizes["v1.2.3/pkg.bin"] = 2
        remote.sizes["latest/pkg.bin"] = 2
        phases = run_sync(
            local, remote, dry_run=False, key_present=True, verify_attempts=1, retry_sleep=0
        )
        puts = [op for op in remote.ops if op[0] == "put"]
        assert [op[1] for op in puts] == [
            "v1.2.3",
            "v1.2.3",
            "stable",
            "latest",
            "latest",
        ]
        assert puts[0][2] == ("Extra.dmg", "pkg.bin", "SHA256SUMS", "SHA256SUMS.minisig")
        assert "latest.json" not in puts[0][2]
        assert puts[1][2] == MANIFEST_NAMES
        assert puts[2][2] == MANIFEST_NAMES
        assert "pkg.bin" in puts[3][2] and "latest.json" not in puts[3][2]
        assert puts[4][2] == MANIFEST_NAMES
        assert phases[-1].promotion is True
        # Idempotent retry: sizes now match, so installers are not sent again,
        # but the pointer files are refreshed.
        remote.ops.clear()
        run_sync(local, remote, dry_run=False, key_present=True, verify_attempts=1, retry_sleep=0)
        puts = [op for op in remote.ops if op[0] == "put"]
        assert [op[1] for op in puts] == ["v1.2.3", "stable", "latest"]
        assert puts[0][2] == MANIFEST_NAMES
        assert "pkg.bin" not in puts[2][2]


def _test_live_size_mismatch_refuses_overwrite() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        local = _write_base(Path(tmp))
        remote = FakeTransport("1.2.3").bind(local)
        for name, size in {**local.installers, **local.checksums, **local.manifests}.items():
            remote.sizes[f"v1.2.3/{name}"] = size
        remote.sizes["v1.2.3/Extra.dmg"] = local.installers["Extra.dmg"] + 5
        _expect_error(
            lambda: run_sync(
                local, remote, dry_run=False, key_present=True, verify_attempts=1
            ),
            "refuse to overwrite published v1.2.3/Extra.dmg",
        )
        assert not any(op[0] == "put" for op in remote.ops)


def _test_newer_stable_is_not_promoted() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        local = _write_base(Path(tmp))
        remote = FakeTransport("1.2.4").bind(local)
        for name, size in {**local.installers, **local.checksums, **local.manifests}.items():
            remote.sizes[f"v1.2.3/{name}"] = size
        remote.bodies["v1.2.3/latest.json"] = json.dumps({"version": "1.2.3"})
        phases = run_sync(
            local, remote, dry_run=False, key_present=True, verify_attempts=1, retry_sleep=0
        )
        assert [phase.name for phase in phases] == [
            "versioned-installers",
            "versioned-manifest",
        ]
        assert all(not phase.promotion for phase in phases)
        assert not any(op[0] == "put" for op in remote.ops)
        assert "stable/latest.json" not in remote.sizes or remote.sizes.get("stable/latest.json") is None


def _test_newer_stable_refuses_different_bytes() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        local = _write_base(Path(tmp))
        remote = FakeTransport("9.0.0").bind(local)
        remote.sizes["v1.2.3/pkg.bin"] = 1
        _expect_error(
            lambda: run_sync(
                local, remote, dry_run=False, key_present=True, verify_attempts=1
            ),
            "refuse to overwrite published v1.2.3/pkg.bin",
        )
        assert not any(op[0] == "put" for op in remote.ops)


def _test_verify_failure_stops_before_promotion() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        local = _write_base(Path(tmp))
        remote = FakeTransport("1.0.0", lie_about_size="v1.2.3/pkg.bin").bind(local)
        _expect_error(
            lambda: run_sync(
                local, remote, dry_run=False, key_present=True, verify_attempts=1
            ),
            "v1.2.3/pkg.bin size",
        )
        assert not any(op[0] == "put" and op[1] == "stable" for op in remote.ops)
        assert not any(op[0] == "put" and op[1] == "latest" for op in remote.ops)


def _test_stable_race_does_not_downgrade() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        local = _write_base(Path(tmp))
        remote = FakeTransport("1.0.0", stable_after_puts="9.9.9").bind(local)
        _expect_error(
            lambda: run_sync(
                local, remote, dry_run=False, key_present=True, verify_attempts=1
            ),
            "stable moved to 9.9.9",
        )
        assert any(op[0] == "put" and op[1] == "v1.2.3" for op in remote.ops)
        assert not any(op[0] == "put" and op[1] in ("stable", "latest") for op in remote.ops)


def _test_live_retry_repairs_website_alias() -> None:
    """Stable is already this version, but the website alias is a short partial upload."""
    with tempfile.TemporaryDirectory() as tmp:
        local = _write_base(Path(tmp))
        remote = FakeTransport("1.2.3").bind(local)
        for name, size in {**local.installers, **local.checksums, **local.manifests}.items():
            remote.sizes[f"v1.2.3/{name}"] = size
        remote.sizes["latest/pkg.bin"] = 1
        phases = run_sync(
            local, remote, dry_run=False, key_present=True, verify_attempts=1, retry_sleep=0
        )
        puts = [op for op in remote.ops if op[0] == "put"]
        assert not any(op[1] == "v1.2.3" and "pkg.bin" in op[2] for op in puts)
        alias = [op for op in puts if op[1] == "latest" and "pkg.bin" in op[2]]
        assert len(alias) == 1
        assert "latest.json" not in alias[0][2]
        assert any(op[1] == "stable" for op in puts)
        assert phases[-1].name == "website-manifest"


def _test_rsync_command_hides_key_and_creates_directory() -> None:
    secret = "SUPER-SECRET-KEY-MATERIAL"
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        key = root / "id_ed25519"
        known = root / "known_hosts"
        source = root / "v1.2.3"
        source.mkdir()
        argv = rsync_argv(
            key_path=key,
            known_hosts=known,
            user="mist-deploy",
            host="203.0.113.10",
            source_dir=source,
            remote_root=REMOTE_ROOT,
        )
    blob = " ".join(argv)
    assert secret not in blob
    assert argv[0] == "rsync"
    assert "--timeout=120" in argv
    assert "BatchMode=yes" in blob
    assert "IdentitiesOnly=yes" in blob
    assert argv[-2] == str(source)
    assert not argv[-2].endswith("/")
    assert argv[-1] == "mist-deploy@203.0.113.10:/var/www/mistlab/downloads/mistterm/"
    _expect_error(
        lambda: rsync_argv(
            key_path=key,
            known_hosts=known,
            user="mist-deploy",
            host="bad host",
            source_dir=source,
            remote_root=REMOTE_ROOT,
        ),
        "bad mirror host",
    )
    assert ssh_keyscan_argv("203.0.113.10") == ["ssh-keyscan", "-T", "10", "-H", "203.0.113.10"]
    _expect_error(
        lambda: install_private_key("ssh-ed25519 AAAA public only", Path(tmp) / "nope"),
        "does not look like a private key",
    )


def _test_content_length_parser() -> None:
    headers = (
        "HTTP/1.1 301 Moved\r\nContent-Length: 0\r\n\r\n"
        "HTTP/1.1 200 OK\r\nContent-Length: 42\r\n\r\n"
    )
    assert content_length(headers) == 42
    body = json.dumps({"version": "1.2.3"}).encode()
    assert version_from_body(body, "https://example.test/latest.json") == "1.2.3"
    _expect_error(lambda: version_from_body(b"<html>", "https://example.test/x"), "not a JSON")


def _test_http_transport_with_fake_curl() -> None:
    """Exercise the real curl/rsync transport with a scripted subprocess, not the network."""

    def runner(command, check=False, capture_output=False, text=False, timeout=None):
        del check, capture_output, text, timeout
        runner.calls.append(list(command))
        if command[0] == "rsync":
            return subprocess.CompletedProcess(command, 0, stdout="", stderr="")
        if command[0] != "curl":
            raise AssertionError(command[0])
        header_path = Path(command[command.index("-D") + 1])
        body_path = Path(command[command.index("-o") + 1])
        if getattr(runner, "fail", False):
            return subprocess.CompletedProcess(command, 22, stdout="", stderr="curl: (22) HTTP 500")
        if getattr(runner, "status", 200) == 404:
            header_path.write_text("HTTP/1.1 404 Not Found\r\n\r\n", encoding="utf-8")
            body_path.write_bytes(b"")
            return subprocess.CompletedProcess(command, 0, stdout="404", stderr="")
        if "-I" in command:
            header_path.write_text("HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\n", encoding="utf-8")
            body_path.write_bytes(b"")
        else:
            header_path.write_text("HTTP/1.1 200 OK\r\n\r\n", encoding="utf-8")
            body_path.write_bytes(b'{"version":"1.2.3"}')
        return subprocess.CompletedProcess(command, 0, stdout="200", stderr="")

    runner.calls = []
    with tempfile.TemporaryDirectory() as tmp:
        dist = Path(tmp)
        (dist / "pkg.bin").write_bytes(b"abc")
        transport = HttpRsyncTransport(
            dist=dist,
            host="203.0.113.10",
            user="mist-deploy",
            key_path=dist / "key",
            known_hosts=dist / "known",
            public_base="https://mirror.example/mistterm",
            runner=runner,
        )
        assert transport.fetch_stable_version() == "1.2.3"
        assert transport.head_size("v1.2.3/pkg.bin") == 9
        assert transport.read_version("v1.2.3/latest.json") == "1.2.3"
        assert all(call[0] == "curl" for call in runner.calls)
        assert "https://mirror.example/mistterm/stable/latest.json" in runner.calls[0]
        runner.status = 404
        assert transport.head_size("v1.2.3/missing.bin") is None
        runner.status = 200
        runner.fail = True
        _expect_error(lambda: transport.head_size("v1.2.3/pkg.bin"), "HTTP 500")
        runner.fail = False
        transport.put_tree("v1.2.3", ("pkg.bin",))
    rsync_calls = [call for call in runner.calls if call[0] == "rsync"]
    assert len(rsync_calls) == 1
    blob = " ".join(rsync_calls[0])
    assert "BEGIN OPENSSH" not in blob
    assert rsync_calls[0][-1] == "mist-deploy@203.0.113.10:/var/www/mistlab/downloads/mistterm/"
    assert rsync_calls[0][-2].endswith("/v1.2.3")


def _test_workflow_contract() -> None:
    root = Path(__file__).resolve().parents[1]
    workflow = (root / ".github/workflows/build.yml").read_text(encoding="utf-8")
    mirror = workflow.split("\n  mirror:\n", 1)[1]
    required = (
        "environment: release",
        "needs: release",
        "vars.MIST_MIRROR_ENABLED != 'false'",
        "MIST_DEPLOY_KEY: ${{ secrets.MIST_DEPLOY_KEY }}",
        "scripts/sync-release-mirror.py",
        "contents: read",
    )
    for snippet in required:
        if snippet not in mirror:
            raise AssertionError(f"mirror job missing {snippet}")
    if 'if [ -z "${{ secrets.MIST_DEPLOY_KEY }}" ]' in workflow:
        raise AssertionError("deploy key is still interpolated into a shell test")
    if "Mirror to mistlab.dev (notice only)" in workflow:
        raise AssertionError("notice-only mirror job is still present")
    if (root / ".github/workflows/mirror-smoke.yml").exists():
        raise AssertionError("temporary mirror smoke workflow is still present")
    preflight = workflow.split("\n  platform:\n", 1)[0]
    if "sync-release-mirror.py --self-test" not in preflight:
        raise AssertionError("preflight does not run the mirror self-test")


def _write_base(tmp: Path, *, bad_sum: bool = False) -> LocalRelease:
    loaded = _sample_release(tmp, bad_sum=bad_sum)
    if loaded is None:
        raise AssertionError("expected a loadable sample release")
    return loaded


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--version", help="release version X.Y.Z (a leading v is accepted)")
    parser.add_argument("--dist", type=Path, help="directory containing the published release files")
    parser.add_argument("--dry-run", action="store_true", help="read the public mirror and print the plan")
    parser.add_argument("--host", default=os.environ.get("MIST_MIRROR_HOST") or DEFAULT_HOST)
    parser.add_argument("--user", default=DEFAULT_USER)
    parser.add_argument("--public-base", default=PUBLIC_BASE)
    args = parser.parse_args(argv)
    if args.self_test:
        return self_test()
    if not args.version or args.dist is None:
        parser.error("--version and --dist are required")
    version = args.version[1:] if args.version.startswith("v") else args.version
    try:
        require_endpoint(args.user, args.host)
        local = load_local(args.dist, version)
        if args.dry_run:
            transport: Transport = HttpRsyncTransport(
                dist=args.dist,
                host=args.host,
                user=args.user,
                key_path=Path("/dev/null"),
                known_hosts=Path("/dev/null"),
                public_base=args.public_base,
            )
            run_sync(local, transport, dry_run=True, key_present=False, sleep=time.sleep)
            print("dry-run only; nothing was uploaded")
            return 0
        secret = os.environ.get("MIST_DEPLOY_KEY", "")
        if not secret.strip():
            raise MirrorError(
                "MIST_DEPLOY_KEY is not set. Put the existing deploy private key in "
                "the release environment (same place as the minisign secrets). "
                "The mirror was not updated."
            )
        with tempfile.TemporaryDirectory(prefix="mist-mirror-ssh-") as tmp:
            key_path, known = prepare_ssh(secret, args.host, Path(tmp))
            transport = HttpRsyncTransport(
                dist=args.dist,
                host=args.host,
                user=args.user,
                key_path=key_path,
                known_hosts=known,
                public_base=args.public_base,
            )
            run_sync(local, transport, dry_run=False, key_present=True)
        print(f"mirror sync ok: {version}")
        return 0
    except MirrorError as exc:
        return report(exc)


if __name__ == "__main__":
    sys.exit(main())
