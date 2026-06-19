#!/usr/bin/env python3
"""GUI 集成测试功能覆盖清单（与 smoke / full_workflow / e2e 脚本对应）。"""

from __future__ import annotations

from dataclasses import dataclass, field

# id -> (描述, 负责脚本)
GUI_FEATURES: dict[str, tuple[str, str]] = {
    "session.connect": ("连接已有会话", "smoke|workflow|e2e|connect"),
    "session.new_dialog": ("新建会话对话框", "smoke|workflow"),
    "session.edit": ("编辑会话 Ctrl+E", "smoke|workflow"),
    "session.import_ssh": ("导入 SSH Config", "smoke|workflow"),
    "session.disconnect": ("断开 SSH", "smoke"),
    "session.reconnect": ("重连标签", "smoke"),
    "tab.new": ("新建标签 Ctrl+T", "smoke|workflow|e2e"),
    "tab.close": ("关闭标签", "smoke"),
    "tab.cycle": ("标签切换 Ctrl+Tab", "smoke"),
    "terminal.commands": ("终端命令", "workflow|e2e"),
    "terminal.find": ("终端查找 F3", "smoke|workflow"),
    "terminal.history": ("命令历史 Ctrl+R", "smoke|workflow"),
    "terminal.split_h": ("左右分屏 Ctrl+Shift+D", "smoke|workflow"),
    "terminal.split_v": ("上下分屏 Ctrl+Shift+U", "smoke|workflow"),
    "terminal.pane_focus": ("窗格切换 Alt+←/→", "smoke|workflow"),
    "sftp.toggle": ("SFTP 面板 Ctrl+Shift+S", "smoke|workflow|connect|e2e"),
    "sftp.upload": ("SFTP 上传 Ctrl+Shift+F9", "workflow|e2e"),
    "sftp.download": ("SFTP 下载 Ctrl+Shift+F10", "workflow|e2e"),
    "panel.monitor": ("监控面板", "smoke|workflow|connect"),
    "panel.port_forward": ("端口转发面板", "smoke|workflow"),
    "panel.snippets": ("命令片段面板", "smoke|workflow"),
    "panel.ai": ("AI 助手面板", "smoke|workflow"),
    "panel.ai_settings": ("AI 设置", "smoke|workflow"),
    "dialog.preferences": ("偏好设置 Ctrl+,", "smoke|workflow"),
    "dialog.about": ("关于 Ctrl+H", "smoke|workflow"),
    "dialog.fragment_lib": ("片段库", "smoke|workflow"),
    "dialog.batch_exec": ("批量执行", "smoke"),
    "dialog.credentials": ("凭证面板", "smoke"),
    "dialog.team": ("团队账户", "smoke"),
    "dialog.cloud_sync": ("云同步", "smoke"),
    "dialog.session_logs": ("会话日志", "smoke"),
    "dialog.help": ("帮助快速入门", "smoke|workflow"),
    "menu.edit": ("编辑菜单 Copy/Paste/Find", "smoke"),
    "menu.view": ("视图菜单（侧栏/最大化/主题）", "smoke"),
    "bar.bottom": ("底栏快捷按钮", "smoke|connect"),
    "bar.status_ctx": ("状态栏右键菜单", "smoke"),
    "shortcut.ai_send_sel": ("选中发送到 AI Ctrl+Shift+L", "smoke"),
    "automation.close_modal": ("关闭新建会话 Ctrl+Shift+Backspace", "smoke"),
}


@dataclass
class CoverageTracker:
    script: str
    covered: set[str] = field(default_factory=set)

    def mark(self, *feature_ids: str) -> None:
        self.covered.update(feature_ids)

    def mark_many(self, *feature_ids: str) -> None:
        self.mark(*feature_ids)

    def report(self) -> int:
        expected = {
            fid
            for fid, (_, owners) in GUI_FEATURES.items()
            if self.script in owners.split("|")
        }
        missing = sorted(expected - self.covered)
        extra = sorted(self.covered - set(GUI_FEATURES))
        print(f"\n=== GUI coverage ({self.script}) ===", flush=True)
        print(f"  expected: {len(expected)}", flush=True)
        print(f"  covered : {len(self.covered & expected)}", flush=True)
        if missing:
            print("  MISSING:", flush=True)
            for fid in missing:
                desc, _ = GUI_FEATURES[fid]
                print(f"    - {fid}: {desc}", flush=True)
        if extra:
            print("  unknown ids:", ", ".join(extra), flush=True)
        return 1 if missing else 0
