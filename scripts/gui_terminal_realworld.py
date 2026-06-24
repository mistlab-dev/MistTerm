#!/usr/bin/env python3
"""真实场景终端测试：正常/异常命令 + ZMODEM rz/sz（经 SSH 校验远端结果）。"""

from __future__ import annotations

import argparse
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from gui_common import (
    LOCAL_TEST_SESSION,
    REMOTE_FILE,
    SSH_USER,
    automation_env,
    capture_failure,
    connect_local_session,
    remote_assert_file,
    remote_exec,
    remote_temp_path,
    remote_text_file_contains,
    remote_zmodem_dir,
    send_terminal_line,
    ssh_is_localhost,
    ssh_preflight,
)
from pywinauto.keyboard import send_keys
from smoke_gui_interact import GuiWalker, Report, find_mist_hwnd

UI_STEP_SEC = 10.0
OP_TIMEOUT_SEC = 30.0
POLL_INTERVAL_SEC = 0.5

ZMODEM_LOCAL_DIR = Path("C:/ProgramData/mistterm/e2e")


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


def shell_path(p: str) -> str:
    return win_path(p) if ssh_is_localhost() else p.replace("\\", "/")


def prep_terminal(walker: GuiWalker) -> None:
    walker.focus_terminal()
    send_keys("{VK_CONTROL up}{VK_SHIFT up}{VK_MENU up}")
    time.sleep(0.15)


def wait_until(deadline: float, fn, *, poll: float = POLL_INTERVAL_SEC) -> None:
    while time.time() < deadline:
        if fn():
            return
        time.sleep(poll)
    raise RuntimeError(f"timeout after {OP_TIMEOUT_SEC:.0f}s")


def run_cmd_to_file(
    walker: GuiWalker,
    cmd: str,
    outfile: str,
    *,
    wait: float = 1.2,
) -> None:
    prep_terminal(walker)
    send_terminal_line(cmd)
    time.sleep(min(wait, UI_STEP_SEC))


def remote_lrzsz_available() -> bool:
    if ssh_is_localhost():
        code, out, _ = remote_exec("where rz 2>nul && where sz 2>nul", timeout=UI_STEP_SEC)
    else:
        code, out, _ = remote_exec("command -v rz && command -v sz", timeout=UI_STEP_SEC)
    return code == 0 and "rz" in out.lower() and "sz" in out.lower()


def remote_rm(*paths: str) -> None:
    if not paths:
        return
    if ssh_is_localhost():
        quoted = " ".join(f'"{win_path(p)}"' for p in paths)
        remote_exec(f"del /q {quoted} 2>nul")
    else:
        quoted = " ".join(f"{p!r}" for p in paths)
        remote_exec(f"rm -f {quoted}")


def term_redirect(outfile: str, inner: str) -> str:
    p = shell_path(outfile)
    if ssh_is_localhost():
        return f"{inner}> {p}"
    return f"{inner} > {p}"


def term_cd(directory: str) -> str:
    d = shell_path(directory)
    if ssh_is_localhost():
        return f"cd /d {d}"
    return f"cd {d}"


def term_exit_code_to(outfile: str) -> str:
    p = shell_path(outfile)
    if ssh_is_localhost():
        return f"echo %ERRORLEVEL%> {p}"
    return f"echo $? > {p}"


def prepare_zmodem_local_file(marker: str) -> Path:
    ZMODEM_LOCAL_DIR.mkdir(parents=True, exist_ok=True)
    p = ZMODEM_LOCAL_DIR / REMOTE_FILE
    p.write_text(f"MistTerm ZMODEM upload {marker}\n", encoding="utf-8")
    return p.resolve()


def automation_env_with_zmodem_file(marker: str, *, e2e_file: str = REMOTE_FILE) -> dict[str, str]:
    local = prepare_zmodem_local_file(marker)
    env = automation_env(e2e_file=e2e_file)
    env["MISTTERM_ZMODEM_E2E_LOCAL"] = str(local)
    return env


def cancel_pty(walker: GuiWalker) -> None:
    prep_terminal(walker)
    send_keys("^c")
    time.sleep(0.5)


def ensure_remote_zmodem_dir() -> None:
    if ssh_is_localhost():
        return
    root = remote_zmodem_dir()
    remote_exec(f"mkdir -p {root!r}")


class RealWorldRunner:
    def __init__(self, walker: GuiWalker, report: RealWorldReport):
        self.w = walker
        self.r = report
        self.zmodem_dir = remote_zmodem_dir()

    def step(self, name: str, fn) -> None:
        try:
            fn()
            self.r.ok(name)
        except Exception as e:
            safe = str(e).encode("ascii", errors="replace").decode("ascii")[:200]
            self.r.fail(name, safe)

    def run_normal_commands(self) -> None:
        def warmup() -> None:
            prep_terminal(self.w)
            send_terminal_line("echo MISTTERM_RW_WARMUP")
            time.sleep(0.8)

        warmup()

        who = remote_temp_path("mistterm_rw_whoami.txt")
        echo = remote_temp_path("mistterm_rw_echo.txt")
        cd = remote_temp_path("mistterm_rw_cd.txt")
        remote_rm(who, echo, cd)

        expected_user = SSH_USER

        def whoami() -> None:
            run_cmd_to_file(self.w, term_redirect(who, "whoami"), who, wait=1.5)
            remote_assert_file(who, expected_user, what="whoami")

        def echo_marker() -> None:
            run_cmd_to_file(self.w, term_redirect(echo, "echo MISTTERM_RW_OK"), echo)
            remote_assert_file(echo, "MISTTERM_RW_OK", what="echo")

        def cd_and_pwd() -> None:
            ensure_remote_zmodem_dir()
            if ssh_is_localhost():
                run_cmd_to_file(
                    self.w,
                    f"{term_cd(self.zmodem_dir)} && cd> {shell_path(cd)}",
                    cd,
                    wait=1.2,
                )
                remote_assert_file(cd, "mistterm_sftp", what="cd")
            else:
                run_cmd_to_file(
                    self.w,
                    f"{term_cd(self.zmodem_dir)} && pwd > {shell_path(cd)}",
                    cd,
                    wait=1.2,
                )
                remote_assert_file(cd, "mistterm_sftp", what="cd")

        self.step("normal whoami", whoami)
        self.step("normal echo redirect", echo_marker)
        self.step("normal cd + cwd", cd_and_pwd)

    def run_abnormal_commands(self) -> None:
        bad = remote_temp_path("mistterm_rw_bad_exit.txt")
        nodir = remote_temp_path("mistterm_rw_cd_fail.txt")
        remote_rm(bad, nodir)

        def unknown_command() -> None:
            prep_terminal(self.w)
            if ssh_is_localhost():
                send_terminal_line("nosuchcmd_mistterm_xyz")
                time.sleep(0.5)
                prep_terminal(self.w)
                send_terminal_line(term_exit_code_to(bad))
            else:
                send_terminal_line(f"nosuchcmd_mistterm_xyz; {term_exit_code_to(bad)}")
            time.sleep(1.0)
            code, out, _ = remote_exec(f'type "{shell_path(bad)}"' if ssh_is_localhost() else f"cat {bad!r}", timeout=UI_STEP_SEC)
            level = out.strip()
            if level in ("", "0"):
                raise RuntimeError(f"expected non-zero exit, got {level!r} (exit {code})")

        def cd_missing_dir() -> None:
            if ssh_is_localhost():
                missing = "C:\\Users\\mistterm_test\\__no_such_dir_mistterm__"
                run_cmd_to_file(
                    self.w,
                    f"cd /d {missing} 2>nul & {term_exit_code_to(nodir)}",
                    nodir,
                    wait=1.2,
                )
            else:
                missing = "/tmp/__no_such_dir_mistterm__"
                run_cmd_to_file(
                    self.w,
                    f"cd {missing} 2>/dev/null; {term_exit_code_to(nodir)}",
                    nodir,
                    wait=1.2,
                )
            code, out, _ = remote_exec(
                f'type "{shell_path(nodir)}"' if ssh_is_localhost() else f"cat {nodir!r}",
                timeout=UI_STEP_SEC,
            )
            level = out.strip()
            if level in ("", "0"):
                raise RuntimeError(f"expected failed cd exit, got {level!r}")

        self.step("abnormal unknown command (exit code)", unknown_command)
        self.step("abnormal cd missing dir (exit code)", cd_missing_dir)

    def run_zmodem_rz_upload(self, marker: str) -> None:
        if not remote_lrzsz_available():
            self.r.skip("zmodem rz upload", "remote rz/sz not in PATH")
            return

        ensure_remote_zmodem_dir()
        remote_path = f"{self.zmodem_dir}/{REMOTE_FILE}".replace("\\", "/")
        remote_rm(remote_path)

        def rz_upload() -> None:
            local = ZMODEM_LOCAL_DIR / REMOTE_FILE
            if not local.is_file():
                raise RuntimeError(f"local zmodem file missing: {local}")
            pre = remote_temp_path("mistterm_rz_pre.txt")
            prep_terminal(self.w)
            send_terminal_line(term_cd(self.zmodem_dir))
            time.sleep(0.8)
            prep_terminal(self.w)
            send_terminal_line(term_redirect(pre, "echo RZ_PRE"))
            time.sleep(0.8)
            remote_assert_file(pre, "RZ_PRE", what="rz preflight")
            prep_terminal(self.w)
            send_terminal_line("rz -bye")
            deadline = time.time() + OP_TIMEOUT_SEC

            def done() -> bool:
                return remote_text_file_contains(remote_path, marker)

            wait_until(deadline, done)
            cancel_pty(self.w)

        self.step("zmodem rz upload (GUI)", rz_upload)

    def run_zmodem_sz_download(self) -> None:
        if not remote_lrzsz_available():
            self.r.skip("zmodem sz download", "no lrzsz")
            return

        cancel_pty(self.w)
        ensure_remote_zmodem_dir()

        marker = "MISTTERM_SZ_DL"
        remote_name = "mistterm_sz_dl.txt"
        remote_path = f"{self.zmodem_dir}/{remote_name}".replace("\\", "/")

        if ssh_is_localhost():
            remote_exec(f'echo {marker}> "{shell_path(remote_path)}"')
        else:
            remote_exec(f"echo {marker} > {remote_path!r}")
        code, out, _ = remote_exec(
            f'type "{shell_path(remote_path)}"' if ssh_is_localhost() else f"cat {remote_path!r}",
            timeout=UI_STEP_SEC,
        )
        if marker not in out:
            self.r.fail("zmodem sz remote prep", f"probe file missing: {out[:80]!r}")
            return

        def sz_download() -> None:
            prep_terminal(self.w)
            send_terminal_line(term_cd(self.zmodem_dir))
            time.sleep(0.8)
            prep_terminal(self.w)
            send_terminal_line(f"sz -bye {remote_name}")
            local = Path(tempfile.gettempdir()) / "mistterm_downloads" / remote_name
            deadline = time.time() + OP_TIMEOUT_SEC

            def done() -> bool:
                return local.is_file() and marker in local.read_text(
                    encoding="utf-8", errors="replace"
                )

            wait_until(deadline, done)
            cancel_pty(self.w)

        self.step("zmodem sz download (GUI)", sz_download)

    def run_zmodem_sz_hint(self) -> None:
        if not remote_lrzsz_available():
            self.r.skip("zmodem sz remote prep", "no lrzsz")
            return
        self.r.ok("zmodem sz remote file prep")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("exe")
    parser.add_argument("--title", default="MistTerm")
    parser.add_argument("--timeout", type=float, default=UI_STEP_SEC, help="窗口出现/连接 UI 超时（秒）")
    args = parser.parse_args()

    report = RealWorldReport()
    marker = f"rw-{int(time.time())}"

    print(f"==> SSH target: {SSH_USER}@{__import__('gui_common').SSH_HOST}", flush=True)
    print("==> SSH preflight", flush=True)
    ssh_preflight()

    proc = subprocess.Popen([args.exe], env=automation_env_with_zmodem_file(marker))
    hwnd: int | None = None
    try:
        hwnd = find_mist_hwnd(args.title, args.timeout, proc)
        print(f"==> hwnd={hwnd} pid={proc.pid}", flush=True)
        walker = GuiWalker(proc, hwnd, Report())
        print(f"==> Connect session: {LOCAL_TEST_SESSION}", flush=True)
        connect_local_session(hwnd, proc.pid, LOCAL_TEST_SESSION, wait=min(args.timeout, UI_STEP_SEC))
        time.sleep(1.0)

        runner = RealWorldRunner(walker, report)
        print("==> Normal commands", flush=True)
        runner.run_normal_commands()
        print("==> Abnormal commands", flush=True)
        runner.run_abnormal_commands()
        print("==> ZMODEM", flush=True)
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
