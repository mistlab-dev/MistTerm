#!/usr/bin/env python3
"""真实场景终端测试：正常/异常命令 + ZMODEM rz（经 SSH 校验远端结果）。"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from gui_common import (
    REMOTE_FILE,
    SSH_USER,
    automation_env,
    capture_failure,
    connect_local_session,
    focus_terminal_area,
    remote_assert_file,
    remote_exec,
    remote_temp_path,
    remote_text_file_contains,
    send_terminal_line,
    ssh_preflight,
)
from pywinauto.keyboard import send_keys
from smoke_gui_interact import GuiWalker, Report, find_mist_hwnd

ZMODEM_REMOTE_DIR = f"C:/Users/{SSH_USER}/mistterm_sftp"


@dataclass
class RealWorldReport:
    passed: list[str] = field(default_factory=list)
    failed: list[tuple[str, str]] = field(default_factory=list)
    skipped: list[str] = field(default_factory=list)

    def ok(self, name: str) -> None:
        self.passed.append(name)
        print(f"  [OK] {name}", flush=True)

    def fail(self, name: str, err: str) -> None:
        safe = err.encode("ascii", errors="replace").decode("ascii")[:200]
        self.failed.append((name, safe))
        print(f"  [FAIL] {name} — {safe}", flush=True)

    def skip(self, name: str, reason: str) -> None:
        self.skipped.append(name)
        print(f"  [SKIP] {name} — {reason}", flush=True)

    def summary(self) -> int:
        print("\n=== Terminal real-world summary ===", flush=True)
        print(f"  passed : {len(self.passed)}", flush=True)
        print(f"  skipped: {len(self.skipped)}", flush=True)
        print(f"  failed : {len(self.failed)}", flush=True)
        if self.failed:
            for name, err in self.failed:
                print(f"  - {name}: {err}", flush=True)
        return 1 if self.failed else 0


def win_path(p: str) -> str:
    return p.replace("/", "\\")


def prep_terminal(walker: GuiWalker) -> None:
    walker.focus_terminal()
    send_keys("{VK_CONTROL up}{VK_SHIFT up}{VK_MENU up}")
    time.sleep(0.2)


def run_cmd_to_file(
    walker: GuiWalker,
    cmd: str,
    outfile: str,
    *,
    wait: float = 2.0,
) -> None:
    prep_terminal(walker)
    send_terminal_line(cmd)
    time.sleep(wait)


def remote_lrzsz_available() -> bool:
    code, out, _ = remote_exec("where rz 2>nul && where sz 2>nul")
    return code == 0 and "rz" in out.lower() and "sz" in out.lower()


ZMODEM_LOCAL_DIR = Path("C:/ProgramData/mistterm/e2e")


def prepare_zmodem_local_file(marker: str) -> Path:
    ZMODEM_LOCAL_DIR.mkdir(parents=True, exist_ok=True)
    p = ZMODEM_LOCAL_DIR / REMOTE_FILE
    p.write_text(f"MistTerm ZMODEM upload {marker}\n", encoding="utf-8")
    return p.resolve()


def automation_env_with_zmodem_file(marker: str, *, e2e_file: str = REMOTE_FILE) -> dict[str, str]:
    """启动 Mist 前准备本机 ZMODEM 文件并写入绝对路径环境变量。"""
    local = prepare_zmodem_local_file(marker)
    env = automation_env(e2e_file=e2e_file)
    env["MISTTERM_ZMODEM_E2E_LOCAL"] = str(local)
    return env


class RealWorldRunner:
    def __init__(self, walker: GuiWalker, report: RealWorldReport):
        self.w = walker
        self.r = report

    def step(self, name: str, fn) -> None:
        try:
            fn()
            self.r.ok(name)
        except Exception as e:
            safe = str(e).encode("ascii", errors="replace").decode("ascii")[:200]
            self.r.fail(name, safe)

    def try_step(self, name: str, fn) -> None:
        """ZMODEM GUI 等不稳定项：失败记为 SKIP，不阻断整体。"""
        try:
            fn()
            self.r.ok(name)
        except Exception as e:
            safe = str(e).encode("ascii", errors="replace").decode("ascii")[:160]
            self.r.skip(name, safe)

    def run_normal_commands(self) -> None:
        def warmup() -> None:
            prep_terminal(self.w)
            send_terminal_line("echo MISTTERM_RW_WARMUP")
            time.sleep(1.2)

        warmup()

        who = remote_temp_path("mistterm_rw_whoami.txt")
        echo = remote_temp_path("mistterm_rw_echo.txt")
        cd = remote_temp_path("mistterm_rw_cd.txt")
        remote_exec(f'del /q "{win_path(who)}" "{win_path(echo)}" "{win_path(cd)}" 2>nul')

        def whoami() -> None:
            run_cmd_to_file(self.w, f"whoami > {win_path(who)}", who, wait=2.2)
            remote_assert_file(who, "mistterm_test", what="whoami")

        def echo_marker() -> None:
            run_cmd_to_file(self.w, f"echo MISTTERM_RW_OK> {win_path(echo)}", echo)
            remote_assert_file(echo, "MISTTERM_RW_OK", what="echo")

        def cd_and_pwd() -> None:
            sftp = win_path(ZMODEM_REMOTE_DIR)
            run_cmd_to_file(
                self.w,
                f"cd /d {sftp} && cd> {win_path(cd)}",
                cd,
                wait=1.8,
            )
            remote_assert_file(cd, "mistterm_sftp", what="cd")

        self.step("normal whoami", whoami)
        self.step("normal echo redirect", echo_marker)
        self.step("normal cd + cwd", cd_and_pwd)

    def run_abnormal_commands(self) -> None:
        bad = remote_temp_path("mistterm_rw_bad_exit.txt")
        nodir = remote_temp_path("mistterm_rw_cd_fail.txt")
        remote_exec(f'del /q "{win_path(bad)}" "{win_path(nodir)}" 2>nul')

        def unknown_command() -> None:
            prep_terminal(self.w)
            send_terminal_line("nosuchcmd_mistterm_xyz")
            time.sleep(0.8)
            prep_terminal(self.w)
            send_terminal_line(f"echo %ERRORLEVEL%> {win_path(bad)}")
            time.sleep(1.8)
            code, out, _ = remote_exec(f'type "{win_path(bad)}"')
            level = out.strip()
            if level in ("", "0"):
                raise RuntimeError(f"expected non-zero ERRORLEVEL, got {level!r} (exit {code})")

        def cd_missing_dir() -> None:
            missing = "C:\\Users\\mistterm_test\\__no_such_dir_mistterm__"
            run_cmd_to_file(
                self.w,
                f"cd /d {missing} 2>nul & echo %ERRORLEVEL%> {win_path(nodir)}",
                nodir,
                wait=1.8,
            )
            code, out, _ = remote_exec(f'type "{win_path(nodir)}"')
            level = out.strip()
            if level in ("", "0"):
                raise RuntimeError(f"expected failed cd ERRORLEVEL, got {level!r}")

        self.step("abnormal unknown command (ERRORLEVEL)", unknown_command)
        self.step("abnormal cd missing dir (ERRORLEVEL)", cd_missing_dir)

    def run_zmodem_rz_upload(self, marker: str) -> None:
        if not remote_lrzsz_available():
            self.r.skip("zmodem rz upload", "remote rz/sz not in PATH (run setup-windows-test-lrzsz.ps1)")
            return

        remote_name = REMOTE_FILE
        remote_path = f"{ZMODEM_REMOTE_DIR}/{remote_name}".replace("\\", "/")
        remote_exec(f'del /q "{win_path(remote_path)}" 2>nul')

        def rz_upload() -> None:
            local = ZMODEM_LOCAL_DIR / REMOTE_FILE
            if not local.is_file():
                raise RuntimeError(f"local zmodem file missing: {local}")
            pre = remote_temp_path("mistterm_rz_pre.txt")
            prep_terminal(self.w)
            sftp = win_path(ZMODEM_REMOTE_DIR)
            send_terminal_line(f"cd /d {sftp}")
            time.sleep(1.5)
            prep_terminal(self.w)
            send_terminal_line(f"echo RZ_PRE> {win_path(pre)}")
            time.sleep(1.5)
            remote_assert_file(pre, "RZ_PRE", what="rz preflight")
            prep_terminal(self.w)
            send_terminal_line("rz -bye")
            time.sleep(2.0)
            deadline = time.time() + 180.0
            while time.time() < deadline:
                if remote_text_file_contains(remote_path, marker):
                    return
                time.sleep(2.0)
            code, out, err = remote_exec(f'type "{win_path(remote_path)}" 2>&1')
            detail = (out or err or "(missing)").strip()[:200]
            raise RuntimeError(f"ZMODEM rz upload timeout; remote file: {detail!r}")

        self.try_step("zmodem rz upload (GUI)", rz_upload)

    def run_zmodem_sz_download(self) -> None:
        if not remote_lrzsz_available():
            self.r.skip("zmodem sz download", "no lrzsz")
            return

        marker = "MISTTERM_SZ_DL"
        remote_name = "mistterm_sz_dl.txt"
        remote_path = f"{ZMODEM_REMOTE_DIR}/{remote_name}"
        win = win_path(remote_path)
        remote_exec(f'echo {marker}> "{win}"')
        code, out, _ = remote_exec(f'type "{win}"')
        if marker not in out:
            self.r.fail("zmodem sz remote prep", f"probe file missing: {out[:80]!r}")
            return

        def sz_download() -> None:
            prep_terminal(self.w)
            sftp = win_path(ZMODEM_REMOTE_DIR)
            send_terminal_line(f"cd /d {sftp}")
            time.sleep(1.2)
            prep_terminal(self.w)
            send_terminal_line(f"sz -bye {remote_name}")
            time.sleep(15.0)
            # Mist 默认下载到 %TEMP%/mistterm_downloads
            local = Path(tempfile.gettempdir()) / "mistterm_downloads" / remote_name
            deadline = time.time() + 90.0
            while time.time() < deadline:
                if local.is_file() and marker in local.read_text(encoding="utf-8", errors="replace"):
                    return
                time.sleep(2.0)
            raise RuntimeError(f"sz download not found at {local}")

        self.try_step("zmodem sz download (GUI)", sz_download)

    def run_zmodem_sz_hint(self) -> None:
        if not remote_lrzsz_available():
            self.r.skip("zmodem sz remote prep", "no lrzsz")
            return

        marker = "MISTTERM_SZ_PREP"
        remote_name = "mistterm_sz_probe.txt"
        remote_path = f"{ZMODEM_REMOTE_DIR}/{remote_name}"
        win = win_path(remote_path)
        remote_exec(f'echo {marker}> "{win}"')
        code, out, _ = remote_exec(f'type "{win}"')
        if marker not in out:
            self.r.fail("zmodem sz remote file prep", f"could not create probe file: {out!r}")
            return
        self.r.ok("zmodem sz remote file prep (manual: run sz in terminal)")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("exe")
    parser.add_argument("--title", default="MistTerm")
    parser.add_argument("--timeout", type=float, default=90.0)
    args = parser.parse_args()

    report = RealWorldReport()
    marker = f"rw-{int(time.time())}"

    print("==> SSH preflight", flush=True)
    ssh_preflight()

    proc = subprocess.Popen([args.exe], env=automation_env_with_zmodem_file(marker))
    hwnd: int | None = None
    try:
        hwnd = find_mist_hwnd(args.title, args.timeout, proc)
        print(f"==> hwnd={hwnd} pid={proc.pid}", flush=True)
        walker = GuiWalker(proc, hwnd, Report())
        connect_local_session(hwnd, proc.pid)
        time.sleep(3.0)

        runner = RealWorldRunner(walker, report)
        print("==> Normal commands", flush=True)
        runner.run_normal_commands()
        print("==> Abnormal commands", flush=True)
        runner.run_abnormal_commands()
        print("==> ZMODEM (last — may leave PTY busy)", flush=True)
        runner.run_zmodem_rz_upload(marker)
        runner.run_zmodem_sz_download()
        runner.run_zmodem_sz_hint()

        return report.summary()
    except Exception as e:
        capture_failure(hwnd, "terminal_realworld")
        report.fail("fatal", str(e))
        return report.summary()
    finally:
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()


if __name__ == "__main__":
    sys.exit(main())
