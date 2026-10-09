"""MistTerm GUI 自动化快捷键（须与 src/ui/app.rs 一致，MISTTERM_GUI_AUTOMATION=1 时生效）。"""

from __future__ import annotations

import time

from pywinauto.keyboard import send_keys

# 关闭「新建会话」弹窗。禁止 Ctrl+Shift+Esc——Windows 系统快捷键会打开任务管理器。
CLOSE_NEW_SESSION = "+^{BACKSPACE}"
TOGGLE_SFTP = "+^s"
OPEN_FRAGMENT_LIBRARY_NEW = "+^{F7}"  # 打开命令片段库并进入新建表单
FRAGMENT_LIBRARY_CLOSE = "+^{F5}"  # 关闭命令片段库
FRAGMENT_LIBRARY_SAVE = "+^{F6}"  # 保存片段库表单
FRAGMENT_SHORTCUT_CAPTURE = "+^{F8}"  # 进入快捷键录制
SFTP_UPLOAD = "+^{F9}"
SFTP_DOWNLOAD = "+^{F10}"


def dismiss_new_session_dialog(*, repeats: int = 2, pause: float = 0.45) -> None:
    for _ in range(repeats):
        send_keys(CLOSE_NEW_SESSION)
        time.sleep(pause)
    send_keys("{ESC}")
    time.sleep(0.2)
