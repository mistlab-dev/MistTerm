#!/usr/bin/env python3
"""Offline end-to-end tests for the MistTerm auto-updater.

Everything runs on this machine; nothing is published and no real server is
contacted. The script:

1. generates a throw-away TEST minisign key pair (never trusted by release builds);
2. builds `Mist` + `mist` twice with `--features update-test`
   (MIST_BUILD_VERSION=1.91.0 = "new", 1.90.0 = "old"), embedding the test public key;
3. packages "new" exactly like the release workflow (tar.gz on Linux, portable zip on
   Windows), writes `latest.json` with scripts/gen-update-manifest.py and signs it;
4. serves a fake release server on 127.0.0.1 (static files + an "SPA" route that
   answers every path with an HTML home page, like mistlab.dev does today);
5. installs "old" into a temp dir and runs the CLI / GUI against good and broken
   manifests: happy path, HTML page, 404, refused connection, bad signature, wrong
   trusted comment, checksum mismatch, first-URL-fails fallback, read-only folder,
   concurrent-update lock, stale manifest, rollback and roll-forward.

Usage:
    python3 scripts/update-e2e/run_e2e.py              # build + run
    python3 scripts/update-e2e/run_e2e.py --skip-build # reuse binaries from a previous run
    python3 scripts/update-e2e/run_e2e.py --no-gui     # skip the Xvfb GUI check
    python3 scripts/update-e2e/run_e2e.py --static-cli # Linux static (musl) `mist` only; needs
                                                       # cargo-zigbuild + the x86_64-unknown-linux-musl target
"""

from __future__ import annotations

import argparse
import contextlib
import functools
import http.server
import json
import os
import pathlib
import shutil
import socket
import stat
import subprocess
import sys
import tarfile
import threading
import time
import zipfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
IS_WIN = os.name == "nt"
OLD, NEW, STALE = "1.90.0", "1.91.0", "1.80.0"
EXE = ".exe" if IS_WIN else ""
# Release layout: the CLI is `mist-cli.exe` + `mist.cmd` on Windows (Mist.exe/mist.exe collide on NTFS).
PROGRAMS = ["Mist.exe", "mist-cli.exe", "mist.cmd"] if IS_WIN else ["Mist", "mist"]
CLI = "mist-cli.exe" if IS_WIN else "mist"
PLATFORM_KEY = "windows-x86_64-portable" if IS_WIN else "linux-x86_64"
ASSET = "Mist-windows-x86_64.zip" if IS_WIN else "Mist-linux-x86_64.tar.gz"
EXIT_AVAILABLE = 10
# --static-cli: the static (musl) Linux CLI package `mist-cli-linux-x86_64.tar.gz` (only `mist`).
STATIC = False
STATIC_TARGET = "x86_64-unknown-linux-musl"

RESULTS: list[tuple[str, bool, str]] = []


def log(msg: str) -> None:
    print(f"[e2e] {msg}", flush=True)


def run(cmd, env=None, check=True, cwd=ROOT, timeout=None) -> subprocess.CompletedProcess:
    log("$ " + " ".join(str(c) for c in cmd))
    return subprocess.run(cmd, env=env, cwd=cwd, check=check, timeout=timeout,
                          capture_output=False, text=True)


def capture(cmd, env=None, timeout=120) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, env=env, capture_output=True, text=True, timeout=timeout,
                          stdin=subprocess.DEVNULL)


# ---------------------------------------------------------------- build

def feature_list() -> str:
    """`update-test` plus any extra features (e.g. MIST_E2E_CARGO_FEATURES=vendored-openssl on Windows CI)."""
    extra = os.environ.get("MIST_E2E_CARGO_FEATURES", "").strip()
    return "update-test" + ("," + extra if extra else "")


def target_root() -> pathlib.Path:
    return pathlib.Path(os.environ.get("CARGO_TARGET_DIR") or ROOT / "target")


def profile_dir(profile: str) -> pathlib.Path:
    """Where cargo puts binaries/examples for this run (musl target dir in --static-cli mode)."""
    return target_root() / STATIC_TARGET / profile if STATIC else target_root() / profile


def cargo_cmd(profile: str, *what: str) -> list[str]:
    flags = ["--release"] if profile == "release" else []
    if STATIC:
        # Same flags as the release workflow's cli-linux job, plus update-test.
        return ["cargo", "zigbuild", *flags, "--target", STATIC_TARGET, "--no-default-features",
                "--features", "update-test,vendored-openssl", *what]
    return ["cargo", "build", *flags, "--features", feature_list(), *what]


def cargo_env(extra: dict) -> dict:
    env = os.environ.copy()
    env.update(extra)
    return env


def build_variant(version: str, pubkey: str, out_dir: pathlib.Path, profile: str) -> None:
    env = cargo_env({"MIST_BUILD_VERSION": version, "MIST_UPDATE_TEST_PUBKEY": pubkey})
    env.pop("MIST_DIST_CHANNEL", None)
    target_dir = profile_dir(profile)
    out_dir.mkdir(parents=True, exist_ok=True)
    if STATIC:
        run(cargo_cmd(profile, "--bin", "mist"), env=env)
        shutil.copy2(target_dir / "mist", out_dir / "mist")
        return
    # Build the GUI first and copy it out: on Windows Mist.exe and mist.exe are the same path.
    run(cargo_cmd(profile, "--bin", "Mist"), env=env)
    shutil.copy2(target_dir / f"Mist{EXE}", out_dir / f"Mist{EXE}")
    run(cargo_cmd(profile, "--bin", "mist"), env=env)
    if IS_WIN:
        shutil.copy2(target_dir / "mist.exe", out_dir / "mist-cli.exe")
        (out_dir / "mist.cmd").write_text('@echo off\r\n"%~dp0mist-cli.exe" %*\r\n', encoding="ascii")
    else:
        shutil.copy2(target_dir / "mist", out_dir / "mist")


def package_new(new_dir: pathlib.Path, dist: pathlib.Path) -> pathlib.Path:
    dist.mkdir(parents=True, exist_ok=True)
    top = "Mist-windows-x86_64" if IS_WIN else ASSET.removesuffix(".tar.gz")
    out = dist / ASSET
    if IS_WIN:
        with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as zf:
            for p in PROGRAMS:
                zf.write(new_dir / p, f"{top}/{p}")
            zf.writestr(f"{top}/README.md", "test package\n")
    else:
        with tarfile.open(out, "w:gz") as tf:
            for p in PROGRAMS:
                tf.add(new_dir / p, f"{top}/{p}")
            data = b"test package\n"
            info = tarfile.TarInfo(f"{top}/README.md")
            info.size = len(data)
            import io
            tf.addfile(info, io.BytesIO(data))
    return out


# ---------------------------------------------------------------- signing / manifests

class Signer:
    def __init__(self, work: pathlib.Path, profile: str):
        self.dir = work / "keys"
        run(cargo_cmd(profile, "--example", "update_test_sign"))
        self.tool = profile_dir(profile) / "examples" / f"update_test_sign{EXE}"
        out = capture([str(self.tool), "keygen", str(self.dir)])
        if out.returncode != 0:
            raise SystemExit(out.stderr)
        self.pubkey = out.stdout.strip()
        self.key = self.dir / "test.key"

    def sign(self, file: pathlib.Path, trusted_comment: str) -> None:
        out = capture([str(self.tool), "sign", str(self.key), str(file), trusted_comment])
        if out.returncode != 0:
            raise SystemExit(out.stderr)


def gen_manifest(dist: pathlib.Path, out: pathlib.Path, version: str, github_base: str,
                 mirror_base: str | None = None) -> dict:
    cmd = [sys.executable, str(ROOT / "scripts" / "gen-update-manifest.py"), "--version", version,
           "--dist", str(dist), "--out", str(out), "--require", PLATFORM_KEY,
           "--github-base", github_base, "--pub-date", "2026-10-05T12:00:00Z" if version != STALE
           else "2026-09-01T12:00:00Z"]
    if mirror_base:
        cmd += ["--mirror-base", mirror_base, "--url-order", "github,mirror"]
    if not IS_WIN:
        cmd += ["--notes-en", str(dist / "notes-en.md"), "--notes-zh", str(dist / "notes-zh.md")]
    out_p = capture(cmd)
    if out_p.returncode != 0:
        raise SystemExit(out_p.stdout + out_p.stderr)
    return json.loads(out.read_text(encoding="utf-8"))


def trusted(m: dict) -> str:
    return f"{m['product']} {m['channel']} {m['version']} {m['pub_date']}"


def write_variant(www: pathlib.Path, name: str, manifest: dict, signer: Signer,
                  tc: str | None = None, mutate_after_sign=None) -> None:
    d = www / name
    d.mkdir(parents=True, exist_ok=True)
    f = d / "latest.json"
    f.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    signer.sign(f, tc or trusted(manifest))
    if mutate_after_sign:
        mutate_after_sign(f)


# ---------------------------------------------------------------- fake server

HOME_HTML = b"<!doctype html><html><head><title>MistLab</title></head><body>home</body></html>"


class Handler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, fmt, *args):  # quiet
        pass

    def do_GET(self):
        if self.path.startswith("/spa/"):
            # mistlab.dev today: any unknown path returns the home page with 200.
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(HOME_HTML)))
            self.end_headers()
            self.wfile.write(HOME_HTML)
            return
        return super().do_GET()


@contextlib.contextmanager
def serve(www: pathlib.Path):
    handler = functools.partial(Handler, directory=str(www))
    httpd = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    t = threading.Thread(target=httpd.serve_forever, daemon=True)
    t.start()
    try:
        yield httpd.server_address[1]
    finally:
        httpd.shutdown()


def closed_port() -> int:
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


# ---------------------------------------------------------------- scenarios

def program_version(path: pathlib.Path) -> str:
    out = capture([str(path), "--version"])
    parts = out.stdout.split()
    return parts[-1] if parts else f"<exit {out.returncode}: {out.stderr.strip()}>"


def install_versions(install: pathlib.Path) -> dict[str, str]:
    return {p: program_version(install / p) for p in PROGRAMS if not p.endswith(".cmd")}


def expect(name: str, ok: bool, detail: str = "") -> None:
    RESULTS.append((name, ok, detail))
    log(("PASS " if ok else "FAIL ") + name + (f" — {detail}" if detail and not ok else ""))


class Env:
    def __init__(self, work: pathlib.Path, install: pathlib.Path):
        self.work = work
        self.install = install
        self.n = 0

    def fresh_home(self) -> pathlib.Path:
        self.n += 1
        h = self.work / f"home-{self.n}"
        shutil.rmtree(h, ignore_errors=True)
        h.mkdir(parents=True)
        return h

    def cli(self, args: list[str], urls: list[str], home: pathlib.Path) -> subprocess.CompletedProcess:
        env = os.environ.copy()
        env["MIST_UPDATE_MANIFEST_URL"] = ",".join(urls)
        env["MIST_UPDATE_HOME"] = str(home)
        env.pop("MIST_DISABLE_UPDATE_CHECK", None)
        cp = capture([str(self.install / CLI), *args], env=env, timeout=300)
        log(f"  mist {' '.join(args)} -> exit {cp.returncode}")
        for line in (cp.stdout + cp.stderr).strip().splitlines()[-6:]:
            log(f"    | {line}")
        return cp


def reset_install(old_dir: pathlib.Path, install: pathlib.Path) -> None:
    if install.exists():
        make_writable(install)
        shutil.rmtree(install)
    install.mkdir(parents=True)
    for p in PROGRAMS:
        shutil.copy2(old_dir / p, install / p)


def make_writable(path: pathlib.Path) -> None:
    for p in [path, *path.rglob("*")]:
        with contextlib.suppress(OSError):
            os.chmod(p, os.stat(p).st_mode | stat.S_IWUSR)


@contextlib.contextmanager
def hold_update_lock(home: pathlib.Path):
    d = home / "updates"
    d.mkdir(parents=True, exist_ok=True)
    f = open(d / "update.lock", "a+b")
    try:
        if IS_WIN:
            import msvcrt
            f.seek(0)
            msvcrt.locking(f.fileno(), msvcrt.LK_NBLCK, 1)
        else:
            import fcntl
            fcntl.flock(f.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        yield
    finally:
        f.close()


def run_scenarios(e: Env, port: int, old_dir: pathlib.Path) -> None:
    base = f"http://127.0.0.1:{port}"
    good = f"{base}/good/latest.json"
    install = e.install

    # 1. Plain check finds the new version (exit 10, JSON says auto-install possible).
    reset_install(old_dir, install)
    h = e.fresh_home()
    cp = e.cli(["update", "--check", "--json"], [good], h)
    data = json.loads(cp.stdout or "{}") if cp.stdout.strip().startswith("{") else {}
    expect("check: new version found", cp.returncode == EXIT_AVAILABLE and data.get("latest") == NEW
           and data.get("auto_install") is True, cp.stdout + cp.stderr)
    state = json.loads((h / "update-state.json").read_text()) if (h / "update-state.json").exists() else {}
    expect("check: state records highest seen + last check",
           state.get("highest_seen_version") == NEW and state.get("last_check_unix"), str(state))

    # 2. Already up to date.
    cp = e.cli(["update", "--check"], [f"{base}/same/latest.json"], e.fresh_home())
    expect("check: same version is up to date", cp.returncode == 0, cp.stdout + cp.stderr)

    # 3. Primary returns an HTML home page (mistlab.dev today) -> falls back to GitHub-like source.
    cp = e.cli(["update", "--check"], [f"{base}/spa/downloads/mistterm/stable/latest.json", good], e.fresh_home())
    expect("check: HTML page on primary falls back to secondary", cp.returncode == EXIT_AVAILABLE, cp.stderr)

    # 4. Connection refused on primary -> fallback.
    cp = e.cli(["update", "--check"], [f"http://127.0.0.1:{closed_port()}/latest.json", good], e.fresh_home())
    expect("check: refused connection falls back", cp.returncode == EXIT_AVAILABLE, cp.stderr)

    # 5. Failures that must NOT be trusted.
    for name, url in [
        ("check: 404 everywhere is an error", f"{base}/missing/latest.json"),
        ("check: tampered manifest rejected (bad signature)", f"{base}/tampered/latest.json"),
        ("check: HTML served as JSON rejected", f"{base}/html/latest.json"),
        ("check: wrong trusted comment rejected", f"{base}/wrongtc/latest.json"),
        ("check: signed by another key rejected", f"{base}/otherkey/latest.json"),
    ]:
        cp = e.cli(["update", "--check"], [url], e.fresh_home())
        expect(name, cp.returncode == 1, cp.stdout + cp.stderr)

    # 6. Checksum mismatch: nothing installed.
    reset_install(old_dir, install)
    cp = e.cli(["update", "--yes"], [f"{base}/badsha/latest.json"], e.fresh_home())
    v = install_versions(install)
    expect("install: checksum mismatch keeps old version", cp.returncode == 1
           and set(v.values()) == {OLD} and not (install / ".mist-update-backup").exists(), f"{cp.stderr} {v}")

    # 7. Read-only folder: refuses, explains, keeps old version.
    if not IS_WIN and os.geteuid() != 0:
        reset_install(old_dir, install)
        os.chmod(install, 0o555)
        try:
            cp = e.cli(["update", "--yes"], [good], e.fresh_home())
        finally:
            os.chmod(install, 0o755)
        v = install_versions(install)
        expect("install: read-only folder refused with guidance",
               cp.returncode == EXIT_AVAILABLE and "sudo mist update" in cp.stdout and set(v.values()) == {OLD},
               f"{cp.stdout} {v}")

    # 8. Another update holds the lock.
    reset_install(old_dir, install)
    h = e.fresh_home()
    with hold_update_lock(h):
        cp = e.cli(["update", "--yes"], [good], h)
    v = install_versions(install)
    expect("install: concurrent update blocked by lock", cp.returncode == 1 and set(v.values()) == {OLD},
           f"{cp.stderr} {v}")

    # 9. Happy path with first download URL failing -> second URL (mirror) used.
    reset_install(old_dir, install)
    h = e.fresh_home()
    cp = e.cli(["update", "--yes"], [f"{base}/fallback/latest.json"], h)
    v = install_versions(install)
    expect("install: first URL fails, second URL works, all programs updated",
           cp.returncode == 0 and set(v.values()) == {NEW}, f"{cp.stdout}{cp.stderr} {v}")
    expect("install: backup of old version kept",
           (install / ".mist-update-backup" / CLI).is_file(), str(list(install.iterdir())))
    leftovers = [p.name for p in install.iterdir() if p.name.startswith(".mist-update-new-")]
    expect("install: no temp files left behind", not leftovers, str(leftovers))

    # 10. Now up to date against the same manifest.
    cp = e.cli(["update", "--check"], [good], h)
    expect("check after install: up to date", cp.returncode == 0, cp.stdout + cp.stderr)

    # 11. Stale (older) manifest after having seen 1.91.0 -> refused.
    cp = e.cli(["update", "--check"], [f"{base}/stale/latest.json"], h)
    expect("check: older manifest than seen before is refused", cp.returncode == 1, cp.stdout + cp.stderr)

    # 12. Rollback and roll forward.
    cp = e.cli(["update", "--rollback", "--yes"], [good], h)
    v = install_versions(install)
    expect("rollback: back to previous version", cp.returncode == 0 and set(v.values()) == {OLD}, f"{cp.stderr} {v}")
    cp = e.cli(["update", "--rollback", "--yes"], [good], h)
    v = install_versions(install)
    expect("rollback again: forward to the newer version", cp.returncode == 0 and set(v.values()) == {NEW},
           f"{cp.stderr} {v}")

    # 13. Non-interactive install without --yes does not install.
    reset_install(old_dir, install)
    cp = e.cli(["update"], [good], e.fresh_home())
    v = install_versions(install)
    expect("install: non-interactive without --yes does nothing", cp.returncode == EXIT_AVAILABLE
           and set(v.values()) == {OLD}, f"{cp.stderr} {v}")

    # 14. Environment kill switch.
    env_home = e.fresh_home()
    os.environ["MIST_DISABLE_UPDATE_CHECK"] = "1"
    try:
        env = os.environ.copy()
        env["MIST_UPDATE_MANIFEST_URL"] = good
        env["MIST_UPDATE_HOME"] = str(env_home)
        cp = capture([str(install / CLI), "update", "--check"], env=env)
    finally:
        del os.environ["MIST_DISABLE_UPDATE_CHECK"]
    expect("MIST_DISABLE_UPDATE_CHECK=1 turns checks off", cp.returncode == 1
           and not (env_home / "update-state.json").exists(), cp.stderr)


def static_scenarios(e: Env, port: int, old_dir: pathlib.Path) -> None:
    """Extra checks for the static CLI package (--static-cli)."""
    base = f"http://127.0.0.1:{port}"
    install = e.install

    # 15. A desktop `Mist` sitting next to the static `mist` (e.g. both in ~/.local/bin) is left alone.
    reset_install(old_dir, install)
    desktop = install / "Mist"
    desktop.write_text("#!/bin/sh\necho Mist 1.90.0\n")
    desktop.chmod(0o755)
    before = desktop.read_bytes()
    cp = e.cli(["update", "--yes"], [f"{base}/good/latest.json"], e.fresh_home())
    expect("static: only mist is replaced, a desktop Mist next to it is untouched",
           cp.returncode == 0 and program_version(install / "mist") == NEW and desktop.read_bytes() == before,
           f"{cp.stdout}{cp.stderr} mist={program_version(install / 'mist')}")

    # 16. Manifest without the CLI package: never fall back to the desktop (glibc) package.
    reset_install(old_dir, install)
    cp = e.cli(["update", "--yes"], [f"{base}/guionly/latest.json"], e.fresh_home())
    expect("static: desktop-only manifest is not installed by the static CLI",
           cp.returncode == EXIT_AVAILABLE and program_version(install / "mist") == OLD, cp.stdout + cp.stderr)


def gui_check(e: Env, port: int, old_dir: pathlib.Path) -> None:
    """Start the old GUI under Xvfb and confirm its background check runs and finds 1.91.0."""
    if IS_WIN or not shutil.which("xvfb-run"):
        log("skip GUI check (needs Linux + xvfb-run)")
        return
    reset_install(old_dir, e.install)
    h = e.fresh_home()
    fake_home = e.work / "gui-home"
    shutil.rmtree(fake_home, ignore_errors=True)
    fake_home.mkdir()
    env = os.environ.copy()
    env.update({
        "HOME": str(fake_home),
        "XDG_CONFIG_HOME": str(fake_home / ".config"),
        "XDG_DATA_HOME": str(fake_home / ".local/share"),
        "XDG_CACHE_HOME": str(fake_home / ".cache"),
        "MIST_UPDATE_MANIFEST_URL": f"http://127.0.0.1:{port}/good/latest.json",
        "MIST_UPDATE_HOME": str(h),
        "MIST_UPDATE_FIRST_CHECK_DELAY_SECS": "2",
        "MIST_LOG_FILE": str(e.work / "gui.log"),
        "RUST_LOG": "info",
        "LIBGL_ALWAYS_SOFTWARE": "1",
        "WGPU_BACKEND": "gl",
    })
    proc = subprocess.Popen(["xvfb-run", "-a", "-s", "-screen 0 1280x900x24", str(e.install / "Mist")],
                            env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                            start_new_session=True)
    state_file = h / "update-state.json"
    found = False
    deadline = time.time() + 90
    while time.time() < deadline and proc.poll() is None:
        if state_file.exists():
            with contextlib.suppress(Exception):
                st = json.loads(state_file.read_text())
                if st.get("highest_seen_version") == NEW:
                    found = True
                    break
        time.sleep(1)
    alive = proc.poll() is None
    with contextlib.suppress(Exception):
        os.killpg(proc.pid, 15)
    with contextlib.suppress(Exception):
        proc.wait(timeout=10)
    gui_log = (e.work / "gui.log").read_text(errors="replace") if (e.work / "gui.log").exists() else ""
    expect("GUI (Xvfb): background check runs and finds the new version", found,
           f"alive={alive}; log tail: {gui_log[-1500:]}")
    v = install_versions(e.install)
    expect("GUI: notify only — nothing installed without a click", set(v.values()) == {OLD}, str(v))


# ---------------------------------------------------------------- main

def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--work", type=pathlib.Path, default=ROOT / "target" / "update-e2e")
    ap.add_argument("--profile", choices=["release", "debug"], default="release")
    ap.add_argument("--skip-build", action="store_true")
    ap.add_argument("--no-gui", action="store_true")
    ap.add_argument("--static-cli", action="store_true",
                    help="test the static (musl) Linux CLI package instead of the desktop package")
    a = ap.parse_args()
    if a.static_cli:
        if IS_WIN:
            raise SystemExit("--static-cli is Linux only")
        global STATIC, PROGRAMS, PLATFORM_KEY, ASSET
        STATIC = True
        PROGRAMS = ["mist"]
        PLATFORM_KEY = "linux-x86_64-cli"
        ASSET = "mist-cli-linux-x86_64.tar.gz"
        a.no_gui = True
        if a.work == ROOT / "target" / "update-e2e":
            a.work = ROOT / "target" / "update-e2e-static"

    work: pathlib.Path = a.work.resolve()
    old_dir, new_dir = work / "bin-old", work / "bin-new"
    keyfile = work / "pubkey.txt"
    if a.skip_build and keyfile.exists() and (old_dir / CLI).exists() and (new_dir / CLI).exists():
        signer = Signer.__new__(Signer)
        signer.dir = work / "keys"
        signer.key = signer.dir / "test.key"
        signer.tool = profile_dir(a.profile) / "examples" / f"update_test_sign{EXE}"
        signer.pubkey = keyfile.read_text().strip()
    else:
        shutil.rmtree(work, ignore_errors=True)
        work.mkdir(parents=True)
        signer = Signer(work, a.profile)
        keyfile.write_text(signer.pubkey)
        build_variant(NEW, signer.pubkey, new_dir, a.profile)
        build_variant(OLD, signer.pubkey, old_dir, a.profile)

    for d, want in [(old_dir, OLD), (new_dir, NEW)]:
        got = program_version(d / CLI)
        if got != want:
            raise SystemExit(f"{d}: expected {want}, got {got}")

    www = work / "www"
    shutil.rmtree(www, ignore_errors=True)
    dist = www / "releases" / f"v{NEW}"
    package_new(new_dir, dist)
    (dist / "notes-en.md").write_text("## New\n- Test release\n", encoding="utf-8")
    (dist / "notes-zh.md").write_text("## 新功能\n- 测试版本\n", encoding="utf-8")
    mirror = www / "mirror" / f"v{NEW}"
    mirror.mkdir(parents=True)
    shutil.copy2(dist / ASSET, mirror / ASSET)

    other = Signer.__new__(Signer)
    other.dir, other.tool = work / "otherkey", signer.tool
    other.key = other.dir / "test.key"
    capture([str(signer.tool), "keygen", str(other.dir)])

    with serve(www) as port:
        base = f"http://127.0.0.1:{port}"
        gh = f"{base}/releases/v{NEW}"
        m = gen_manifest(dist, work / "good.json", NEW, gh, mirror_base=f"{base}/mirror/v{NEW}")
        write_variant(www, "good", m, signer)
        write_variant(www, "same", gen_manifest(dist, work / "same.json", OLD, gh), signer)
        write_variant(www, "stale", gen_manifest(dist, work / "stale.json", STALE, gh), signer)
        write_variant(www, "tampered", m, signer, mutate_after_sign=lambda f: f.write_text(
            f.read_text().replace(f'"size": {m["platforms"][PLATFORM_KEY]["size"]}',
                                  f'"size": {m["platforms"][PLATFORM_KEY]["size"] + 1}')))
        write_variant(www, "wrongtc", m, signer, tc=f"mistterm stable {OLD} {m['pub_date']}")
        write_variant(www, "otherkey", m, other)
        html_dir = www / "html"
        html_dir.mkdir()
        (html_dir / "latest.json").write_bytes(HOME_HTML)
        (html_dir / "latest.json.minisig").write_bytes(HOME_HTML)
        bad = json.loads(json.dumps(m))
        bad["platforms"][PLATFORM_KEY]["sha256"] = "0" * 64
        write_variant(www, "badsha", bad, signer)
        fb = json.loads(json.dumps(m))
        fb["platforms"][PLATFORM_KEY]["urls"] = [f"{base}/missing/{ASSET}", f"{base}/mirror/v{NEW}/{ASSET}"]
        write_variant(www, "fallback", fb, signer)

        if STATIC:
            # A manifest that only has the desktop package: the static CLI must not install it.
            gui_only = json.loads(json.dumps(m))
            desk = dict(gui_only["platforms"].pop(PLATFORM_KEY), name="Mist-linux-x86_64.tar.gz")
            desk["urls"] = [u.replace(ASSET, desk["name"]) for u in desk["urls"]]
            gui_only["platforms"] = {"linux-x86_64": desk}
            write_variant(www, "guionly", gui_only, signer)

        e = Env(work, work / "install")
        run_scenarios(e, port, old_dir)
        if STATIC:
            static_scenarios(e, port, old_dir)
        if not a.no_gui:
            gui_check(e, port, old_dir)

    failed = [r for r in RESULTS if not r[1]]
    print()
    print(f"update e2e: {len(RESULTS) - len(failed)} passed, {len(failed)} failed")
    for name, _, detail in failed:
        print(f"  FAIL {name}\n       {detail[:2000]}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
