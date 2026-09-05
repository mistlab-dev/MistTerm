#!/usr/bin/env python3
"""MistTerm Windows GUI 测试脚本的共享辅助。"""

from __future__ import annotations

import base64
import ctypes
import os
import subprocess
import time
from pathlib import Path

import paramiko
from pywinauto.keyboard import send_keys

from gui_screen import client_rect, screenshot

user32 = ctypes.windll.user32

REMOTE_FILE = "gui_e2e_upload.txt"
LOCAL_TEST_SESSION = "Local Test SSH"
# GUI 自动化默认超时（秒）：连接失败时尽快报错，避免长时间空等。
GUI_WINDOW_TIMEOUT_SEC = 25.0
GUI_CONNECT_WAIT_SEC = 10.0


def _env(name: str, default: str = "") -> str:
    val = os.environ.get(name, "").strip()
    return val if val else default


def _env_int(name: str, default: int) -> int:
    raw = os.environ.get(name, "").strip()
    if not raw:
        return default
    try:
        return int(raw)
    except ValueError:
        return default


def ssh_is_localhost() -> bool:
    return SSH_HOST in ("127.0.0.1", "localhost", "::1")


def reload_ssh_test_config() -> None:
    """从 MISTTERM_TEST_SSH_* 刷新模块级 SSH 配置（供脚本 import 后调用）。"""
    global SSH_HOST, SSH_USER, SSH_PASS, SSH_PORT, SSH_SFTP_ROOT, LOCAL_TEST_SESSION
    if os.environ.get("MISTTERM_GUI_LOCAL_SSH", "").strip() in ("1", "true", "yes"):
        use_local_openssh_test_config()
        return
    SSH_HOST = _env("MISTTERM_TEST_SSH_HOST", "127.0.0.1")
    SSH_PORT = _env_int("MISTTERM_TEST_SSH_PORT", 22)
    SSH_PASS = _env("MISTTERM_TEST_SSH_PASSWORD", "mistterm123")
    if _env("MISTTERM_TEST_SSH_USER"):
        SSH_USER = _env("MISTTERM_TEST_SSH_USER")
    elif ssh_is_localhost():
        SSH_USER = "mistterm_test"
    else:
        SSH_USER = "root"
    if _env("MISTTERM_TEST_SSH_SFTP_ROOT"):
        SSH_SFTP_ROOT = _env("MISTTERM_TEST_SSH_SFTP_ROOT").replace("\\", "/")
    elif ssh_is_localhost():
        SSH_SFTP_ROOT = f"C:/Users/{SSH_USER}/mistterm_sftp"
    else:
        SSH_SFTP_ROOT = "/tmp/mistterm_sftp"
    LOCAL_TEST_SESSION = _env("MISTTERM_TEST_SSH_SESSION", LOCAL_TEST_SESSION)
    if not _env("MISTTERM_TEST_SSH_SESSION"):
        LOCAL_TEST_SESSION = "Local Test SSH" if ssh_is_localhost() else "Linux Test SSH"


def use_local_openssh_test_config() -> None:
    """GUI 本地联调：固定 mistterm_test@127.0.0.1，忽略远程 MISTTERM_TEST_SSH_*。"""
    global SSH_HOST, SSH_USER, SSH_PASS, SSH_PORT, SSH_SFTP_ROOT, LOCAL_TEST_SESSION
    SSH_HOST = "127.0.0.1"
    SSH_USER = "mistterm_test"
    SSH_PASS = "mistterm123"
    SSH_PORT = 22
    SSH_SFTP_ROOT = "C:/Users/mistterm_test/mistterm_sftp"
    LOCAL_TEST_SESSION = "Local Test SSH"


SSH_HOST = "127.0.0.1"
SSH_USER = "mistterm_test"
SSH_PASS = "mistterm123"
SSH_PORT = 22
SSH_SFTP_ROOT = "C:/Users/mistterm_test/mistterm_sftp"
reload_ssh_test_config()


def scale_for(cl: int, cr: int) -> float:
    return max(0.85, min(1.35, (cr - cl) / 1200.0))


def click(x: int, y: int, pause: float = 0.12) -> None:
    user32.SetCursorPos(int(x), int(y))
    user32.mouse_event(0x0002, 0, 0, 0, 0)
    user32.mouse_event(0x0004, 0, 0, 0, 0)
    time.sleep(pause)


def paste_field(text: str) -> None:
    send_keys("^a")
    time.sleep(0.05)
    send_keys(text, with_spaces=True, pause=0.02)


def clipboard_get() -> str:
    """读取系统剪贴板文本。短超时：与 Mist OpenClipboard 争用时勿长时间阻塞。"""
    try:
        proc = subprocess.run(
            ["powershell", "-NoProfile", "-Command", "Get-Clipboard -Raw"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=1.5,
        )
    except subprocess.TimeoutExpired:
        return ""
    if proc.returncode != 0:
        return ""
    return proc.stdout or ""


def clipboard_set(text: str) -> None:
    """写入系统剪贴板文本（PowerShell Base64，避免转义问题）。"""
    payload = base64.b64encode(text.encode("utf-16-le")).decode("ascii")
    ps = (
        f"$b=[Convert]::FromBase64String('{payload}'); "
        "$t=[System.Text.Encoding]::Unicode.GetString($b); "
        "Set-Clipboard -Value $t"
    )
    try:
        proc = subprocess.run(
            ["powershell", "-NoProfile", "-Command", ps],
            capture_output=True,
            text=True,
            encoding="utf-8",
            timeout=1.5,
        )
    except subprocess.TimeoutExpired as e:
        raise RuntimeError("Set-Clipboard timed out (clipboard lock)") from e
    if proc.returncode != 0:
        err = (proc.stderr or proc.stdout or "").strip()
        raise RuntimeError(f"Set-Clipboard failed: {err}")


def drag_select(x1: int, y1: int, x2: int, y2: int, pause: float = 0.15) -> None:
    user32.SetCursorPos(int(x1), int(y1))
    time.sleep(0.05)
    user32.mouse_event(0x0002, 0, 0, 0, 0)
    time.sleep(0.05)
    user32.SetCursorPos(int(x2), int(y2))
    time.sleep(pause)
    user32.mouse_event(0x0004, 0, 0, 0, 0)
    time.sleep(0.12)


def focus_terminal_area(hwnd: int, *, y_ratio: float = 0.58) -> None:
    """点击终端主体区域以获取键盘焦点。"""
    cl, ct, cr, cb = client_rect(hwnd)
    s = scale_for(cl, cr)
    x = cl + int((cr - cl) * 0.42)
    y = ct + int((cb - ct) * y_ratio)
    click(x, y, pause=0.28)


def remote_temp_path(name: str) -> str:
    if ssh_is_localhost():
        return f"C:/Users/{SSH_USER}/AppData/Local/Temp/{name}"
    return f"/tmp/{name}"


def remote_zmodem_dir() -> str:
    return SSH_SFTP_ROOT.replace("\\", "/")


def _ssh_connect(c: paramiko.SSHClient, *, timeout: float = 10.0) -> None:
    c.connect(
        SSH_HOST,
        SSH_PORT,
        SSH_USER,
        SSH_PASS,
        timeout=timeout,
        allow_agent=False,
        look_for_keys=False,
    )


def remote_cat(path: str) -> tuple[int, str]:
    """读取远端文本文件内容（Windows type / Linux cat）。"""
    p = path.replace("\\", "/")
    if ssh_is_localhost():
        code, out, err = remote_exec(f'type "{path.replace("/", chr(92))}" 2>&1')
    else:
        code, out, err = remote_exec(f"cat {p!r} 2>&1")
    return code, (out or err or "").strip()


def remote_exec(command: str, *, timeout: float = 15.0) -> tuple[int, str, str]:
    """经独立 SSH 会话执行 cmd 命令，返回 (exit_code, stdout, stderr)。"""
    c = paramiko.SSHClient()
    c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    try:
        _ssh_connect(c, timeout=timeout)
        _stdin, stdout, stderr = c.exec_command(command, timeout=timeout)
        out = stdout.read().decode("utf-8", errors="replace")
        err = stderr.read().decode("utf-8", errors="replace")
        code = stdout.channel.recv_exit_status()
        return code, out, err
    finally:
        c.close()


def remote_assert_file(path: str, marker: str, *, what: str) -> None:
    """断言远端文本文件包含 marker；失败时附带 type 输出便于排查。"""
    if remote_text_file_contains(path, marker):
        return
    code, detail = remote_cat(path)
    detail = detail[:240]
    raise RuntimeError(f"{what}: expected {marker!r} in {path}, got: {detail!r} (exit {code})")


def remote_text_file_contains(path: str, marker: str) -> bool:
    """经 SSH 读取远端文本文件是否包含 marker。"""
    c = paramiko.SSHClient()
    c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    try:
        _ssh_connect(c, timeout=10)
        sftp = c.open_sftp()
        try:
            with sftp.open(path.replace("\\", "/"), "r") as f:
                body = f.read().decode("utf-8", errors="replace")
                return marker in body
        except OSError:
            return False
    finally:
        c.close()


def send_terminal_line(text: str) -> None:
    """向终端发送一行命令（转义 pywinauto 特殊字符）。"""
    escaped = (
        text.replace("{", "{{")
        .replace("}", "}}")
        .replace("+", "{+}")
        .replace("^", "{^}")
        .replace("%", "{%}")
        .replace("~", "{~}")
    )
    send_keys(escaped + "{ENTER}", with_spaces=True, pause=0.02)


def ssh_preflight() -> None:
    """确认本地 sshd 可登录且 exec 输出正确。"""
    code, out, err = remote_exec("echo ok")
    got = out.strip()
    if code != 0 or got != "ok":
        msg = err.strip() or got or f"exit {code}"
        raise RuntimeError(
            f"SSH preflight failed for {SSH_USER}@{SSH_HOST}:{SSH_PORT}: {msg}. "
            f"Check MISTTERM_TEST_SSH_HOST / MISTTERM_TEST_SSH_PASSWORD."
        )
    print(f"  [SSH] {SSH_USER}@{SSH_HOST} exec echo ok -> {got!r}", flush=True)


def remote_paths(filename: str = REMOTE_FILE) -> list[str]:
    paths: list[str] = []
    if ssh_is_localhost():
        home_fwd = f"C:/Users/{SSH_USER}"
        home_bsl = home_fwd.replace("/", "\\")
        root_fwd = SSH_SFTP_ROOT.replace("\\", "/")
        root_bsl = SSH_SFTP_ROOT.replace("/", "\\")
        for p in (
            filename,
            f"{home_fwd}/{filename}",
            f"{root_fwd}/{filename}",
            f"mistterm_sftp/{filename}",
            f"{home_bsl}\\{filename}",
            f"{root_bsl}\\{filename}",
        ):
            if p not in paths:
                paths.append(p)
        return paths
    root = remote_zmodem_dir()
    return [
        filename,
        f"{root}/{filename}",
        f"/tmp/{filename}",
        f"/root/{filename}",
    ]


def remote_has_marker(marker: str, filename: str = REMOTE_FILE) -> bool:
    c = paramiko.SSHClient()
    c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    try:
        _ssh_connect(c, timeout=10)
        sftp = c.open_sftp()
        candidates = list(remote_paths(filename))
        try:
            cwd = sftp.normalize(".")
            cwd = cwd.replace("\\", "/")
            candidates.insert(0, f"{cwd}/{filename}")
            if ssh_is_localhost():
                candidates.insert(1, f"{cwd}\\{filename}")
        except OSError:
            pass
        seen: set[str] = set()
        for rp in candidates:
            if rp in seen:
                continue
            seen.add(rp)
            try:
                with sftp.open(rp, "r") as f:
                    if marker in f.read().decode("utf-8", errors="replace"):
                        return True
            except OSError:
                pass
    finally:
        c.close()
    return False


def automation_env(
    *,
    e2e_file: str = REMOTE_FILE,
    auto_connect: str | None = None,
) -> dict[str, str]:
    env = os.environ.copy()
    env["MISTTERM_GUI_AUTOMATION"] = "1"
    env["MISTTERM_E2E_FILE"] = e2e_file
    connect_name = auto_connect
    if connect_name is None and os.environ.get("MISTTERM_GUI_LOCAL_SSH", "").strip() in (
        "1",
        "true",
        "yes",
    ):
        connect_name = LOCAL_TEST_SESSION
    if connect_name:
        env["MISTTERM_AUTO_CONNECT"] = connect_name
    return env


WM_CLOSE = 0x0010


def stop_existing_mist_processes(*, title_sub: str = "Mist", grace_sec: float = 3.0) -> int:
    """关闭已存在的 MistTerm 窗口，避免 GUI 测试叠加多个实例。返回尝试关闭的窗口数。"""
    closed = 0
    try:
        from pywinauto import Desktop

        for w in Desktop(backend="uia").windows():
            title = w.window_text() or ""
            if title_sub not in title:
                continue
            try:
                hwnd = int(w.handle)
            except Exception:
                continue
            user32.PostMessageW(hwnd, WM_CLOSE, 0, 0)
            closed += 1
    except Exception:
        pass

    if closed:
        time.sleep(grace_sec)

    # 仍存活的进程：先 taskkill（WM_CLOSE），再 /F
    subprocess.run(
        ["taskkill", "/IM", "Mist.exe", "/T"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    time.sleep(0.4)
    subprocess.run(
        ["taskkill", "/F", "/IM", "Mist.exe", "/T"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if closed:
        print(f"  [cleanup] closed {closed} lingering Mist window(s)", flush=True)
    return closed


def graceful_stop_mist_process(
    proc: subprocess.Popen[bytes],
    hwnd: int | None = None,
    *,
    grace_sec: float = 4.0,
) -> None:
    """先 WM_CLOSE 关窗，超时后再 terminate/kill。"""
    if proc.poll() is not None:
        return
    if hwnd is not None:
        user32.PostMessageW(int(hwnd), WM_CLOSE, 0, 0)
    else:
        proc.terminate()
    deadline = time.time() + grace_sec
    while time.time() < deadline:
        if proc.poll() is not None:
            return
        time.sleep(0.12)
    if proc.poll() is None:
        proc.kill()
        try:
            proc.wait(timeout=3)
        except subprocess.TimeoutExpired:
            subprocess.run(
                ["taskkill", "/F", "/PID", str(proc.pid), "/T"],
                capture_output=True,
            )


def capture_failure(hwnd: int | None, label: str) -> Path | None:
    if hwnd is None:
        return None
    safe = "".join(c if c.isalnum() or c in "-_" else "_" for c in label)[:48]
    path = screenshot(hwnd, f"fail-{safe}", stable_name=f"fail_{safe}_{int(time.time() * 1000)}")
    print(f"    [失败截图] {path}", flush=True)
    return path


def count_local_ssh_established() -> int:
    """统计本机 OpenSSH (127.0.0.1 / ::1:22) 的 ESTABLISHED 会话数（每连接计 1）。"""
    try:
        out = subprocess.check_output(
            ["netstat", "-an"],
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=8,
        )
    except Exception:
        return 0
    clients: set[str] = set()
    for line in out.splitlines():
        upper = line.upper()
        if "ESTABLISHED" not in upper or ":22" not in line:
            continue
        parts = line.split()
        if len(parts) < 3:
            continue
        local, remote = parts[1], parts[2]
        if remote.endswith(":22") and ("127.0.0.1" in remote or "[::1]" in remote):
            clients.add(local)
        elif local.endswith(":22") and ("127.0.0.1" in local or "[::1]" in local):
            clients.add(remote)
    return len(clients)


def wait_session_connected(
    hwnd: int,
    *,
    wait: float = GUI_CONNECT_WAIT_SEC,
    session_name: str = LOCAL_TEST_SESSION,
    ssh_baseline: int | None = None,
    require_netstat: bool = True,
) -> None:
    """轮询 netstat + 终端探测命令，确认 MistTerm 终端已连上 SSH。"""
    if ssh_baseline is None:
        ssh_baseline = count_local_ssh_established()

    netstat_budget = min(wait * 0.55, 6.0)
    netstat_deadline = time.time() + netstat_budget
    saw_conn = False
    while time.time() < netstat_deadline:
        if count_local_ssh_established() > ssh_baseline:
            saw_conn = True
            break
        time.sleep(0.25)

    if require_netstat and not saw_conn:
        raise RuntimeError(
            f"连接「{session_name}」超时：未观察到 localhost:22 ESTABLISHED。"
            f"请确认 seed_local_test_session 已运行且 MISTTERM_AUTO_CONNECT 或侧栏点击已触发"
            f"（{SSH_USER}@{SSH_HOST}）。"
        )

    if saw_conn:
        time.sleep(1.2)

    # 勿向 MistTerm 终端键入探测命令(会污染滚动区、像用户误操作)。
    # 已有 ESTABLISHED 时，用独立 SSH 会话确认账号可执行即可。
    if saw_conn:
        try:
            code, out, err = remote_exec("echo ok")
            if code == 0 and out.strip() == "ok":
                return
            last_err = err.strip() or out.strip() or f"exit {code}"
        except Exception as e:
            last_err = str(e)
        # 独立会话失败不代表 MistTerm shell 未就绪：再轻量试一次终端(仅此兜底)
        pass

    probe_deadline = time.time() + max(3.0, wait - netstat_budget)
    last_err = locals().get("last_err") or "探测失败"
    for attempt in range(2):
        probe = f"MISTTERM_CONN_{int(time.time())}_{attempt}"
        probe_file = remote_temp_path("mistterm_conn_probe.txt")
        try:
            if ssh_is_localhost():
                win_path = probe_file.replace("/", "\\")
                # 独立 SSH 写探测文件，不碰 MistTerm PTY
                code, _, err = remote_exec(f'cmd /c "echo {probe}>{win_path}"')
            else:
                code, _, err = remote_exec(f"echo {probe} > {probe_file}")
            if code == 0 and remote_text_file_contains(probe_file, probe):
                # MistTerm：点一下终端区域确认窗口存活即可，不发命令
                focus_terminal_area(hwnd)
                time.sleep(0.2)
                return
            last_err = err.strip() or f"probe exit {code}"
        except Exception as e:
            last_err = str(e)
        time.sleep(0.5)
        if time.time() > probe_deadline:
            break

    raise RuntimeError(
        f"连接「{session_name}」超时 ({wait:.0f}s)：{last_err}。"
        f"请确认终端已获得焦点且 sshd 可执行命令（{SSH_USER}@{SSH_HOST}）。"
    )


def verify_session_connected(
    hwnd: int,
    *,
    wait: float = 4.0,
    session_name: str = LOCAL_TEST_SESSION,
) -> None:
    """已连接场景：跳过 netstat，仅发终端探测确认 shell 可用。"""
    wait_session_connected(
        hwnd,
        wait=wait,
        session_name=session_name,
        require_netstat=False,
    )


def _click_sidebar_session(hwnd: int, pid: int, name: str) -> None:
    """侧栏搜索并点击会话行。优先点搜索框再输入，避免键入漏进 PTY。"""
    from gui_automation_keys import dismiss_new_session_dialog
    from pywinauto import Application

    app = Application(backend="uia").connect(process=pid)
    win = app.window(handle=hwnd)
    win.set_focus()
    dismiss_new_session_dialog(repeats=1, pause=0.2)

    cl, ct, cr, cb = client_rect(hwnd)
    s = scale_for(cl, cr)
    # 活动栏 Server 图标：确保连接侧栏展开
    click(cl + int(24 * s), ct + int(28 * s), pause=0.25)
    time.sleep(0.2)
    # 侧栏搜索框：rail(~48) + 侧栏内偏上
    search_x = cl + int(48 * s) + int(120 * s)
    search_y = ct + int(52 * s)
    click(search_x, search_y, pause=0.3)
    time.sleep(0.25)
    for _ in range(20):
        send_keys("{BACKSPACE}")
    send_keys(name.replace(" ", "{SPACE}"), with_spaces=True, pause=0.02)
    time.sleep(0.55)

    clicked = False
    for ctrl in win.descendants():
        try:
            text = (ctrl.window_text() or "").strip()
        except Exception:
            continue
        if name not in text or len(text) > len(name) + 48:
            continue
        try:
            ctrl.click_input()
            clicked = True
            break
        except Exception:
            continue

    if not clicked:
        # 会话行大致在搜索框下方
        click(search_x, search_y + int(90 * s), pause=0.3)
    time.sleep(0.45)


def connect_local_session(
    hwnd: int,
    pid: int,
    name: str = LOCAL_TEST_SESSION,
    *,
    wait: float = GUI_CONNECT_WAIT_SEC,
    verify: bool = True,
) -> None:
    """侧栏搜索并点击会话行以连接本地测试 SSH。"""
    baseline = count_local_ssh_established()
    _click_sidebar_session(hwnd, pid, name)
    if verify:
        wait_session_connected(
            hwnd,
            wait=wait,
            session_name=name,
            ssh_baseline=baseline,
        )
    else:
        time.sleep(min(wait, 1.5))


def launch_mist_gui(
    exe: str | Path,
    *,
    session_name: str | None = None,
    e2e_file: str = REMOTE_FILE,
    window_timeout: float = GUI_WINDOW_TIMEOUT_SEC,
    connect_wait: float = GUI_CONNECT_WAIT_SEC,
    title_sub: str = "Mist",
    auto_connect: bool = True,
    extra_env: dict[str, str] | None = None,
) -> tuple[subprocess.Popen[bytes], int]:
    """清理旧进程 → 启动 Mist（MISTTERM_AUTO_CONNECT）→ 等待 SSH 连接就绪。"""
    from gui_automation_keys import dismiss_new_session_dialog
    from gui_screen import find_mist_window

    os.environ["MISTTERM_GUI_LOCAL_SSH"] = "1"
    reload_ssh_test_config()
    ssh_preflight()
    stop_existing_mist_processes(title_sub=title_sub)
    time.sleep(1.0)
    ssh_baseline = count_local_ssh_established()

    name = session_name or LOCAL_TEST_SESSION
    env = automation_env(
        e2e_file=e2e_file,
        auto_connect=name if auto_connect else None,
    )
    if extra_env:
        env.update(extra_env)

    proc = subprocess.Popen([str(exe)], env=env)
    hwnd = find_mist_window(proc, timeout=window_timeout, title_sub=title_sub)
    time.sleep(0.8)
    dismiss_new_session_dialog(repeats=1, pause=0.2)
    if auto_connect:
        wait_session_connected(
            hwnd,
            wait=connect_wait,
            session_name=name,
            ssh_baseline=ssh_baseline,
        )
    return proc, hwnd
