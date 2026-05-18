//! macOS 系统菜单栏（NSMenu），通过 [muda](https://github.com/tauri-apps/muda) 接入。
//!
//! - **Mist**：应用级
//! - **终端**：会话与连接
//! - **编辑**：终端剪贴板 + 搜索（无系统预置复制/粘贴，避免重复项）
//! - **视图**：布局、右侧面板、主题
//! - **工具**：片段、历史、凭证与日志
//! - **帮助**：内嵌文档与关于

use super::macos_app_name::APP_DISPLAY_NAME;
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{
    CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
};

/// 系统菜单项激活后映射的动作（由 `MistTermApp` 执行）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacMenuAction {
    ImportSsh,
    NewSession,
    NewTab,
    Preferences,
    CloseTab,
    DisconnectSsh,
    ReconnectTab,
    Quit,
    CopyTerminal,
    PasteToTerminal,
    SelectAllTerminal,
    TerminalSearch,
    ToggleSidebar,
    ToggleMaximize,
    ToggleSftp,
    ToggleFragmentSidebar,
    ToggleMonitorPanel,
    Theme(usize),
    FragmentLibrary,
    QuickFragmentSelector,
    CommandHistory,
    CredentialPanel,
    CloudSync,
    SessionLogBrowser,
    HelpUserGuide,
    HelpFunctionalSpec,
    HelpShortcuts,
    HelpRevealDocsFolder,
    About,
}

/// 持有 muda 菜单句柄与条目 id，供事件分发与状态同步。
pub struct NativeAppMenu {
    _root: Menu,
    import_ssh: MenuItem,
    toggle_sidebar: MenuItem,
    toggle_maximize: MenuItem,
    sftp_panel: CheckMenuItem,
    fragment_panel: CheckMenuItem,
    monitor_panel: CheckMenuItem,
    theme_items: Vec<CheckMenuItem>,
}

impl NativeAppMenu {
    pub fn install(theme_names: &[String]) -> muda::Result<Self> {
        super::macos_app_name::set_application_display_name();

        let root = Menu::new();

        let app_menu = Submenu::new(APP_DISPLAY_NAME, true);
        let about = MenuItem::with_id("mistterm.app.about", "关于 Mist", true, None);
        let preferences = MenuItem::with_id(
            "mistterm.app.preferences",
            "偏好设置…",
            true,
            Some(Accelerator::new(Some(Modifiers::SUPER), Code::Comma)),
        );
        let quit = MenuItem::with_id(
            "mistterm.app.quit",
            "退出 Mist",
            true,
            Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyQ)),
        );
        app_menu.append(&about)?;
        app_menu.append(&PredefinedMenuItem::separator())?;
        app_menu.append(&preferences)?;
        app_menu.append(&PredefinedMenuItem::separator())?;
        app_menu.append(&quit)?;

        // ── 终端 ──
        let terminal_menu = Submenu::new("终端", true);
        let new_session = MenuItem::with_id(
            "mistterm.terminal.new_session",
            "新建会话",
            true,
            Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyN)),
        );
        let new_tab = MenuItem::with_id(
            "mistterm.terminal.new_tab",
            "新建标签",
            true,
            Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyT)),
        );
        let import_ssh =
            MenuItem::with_id("mistterm.terminal.import_ssh", "导入 SSH 配置", true, None);
        let close_tab = MenuItem::with_id(
            "mistterm.terminal.close_tab",
            "关闭标签",
            true,
            Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyW)),
        );
        let disconnect = MenuItem::with_id(
            "mistterm.terminal.disconnect",
            "断开 SSH（保留输出）",
            true,
            None,
        );
        let reconnect =
            MenuItem::with_id("mistterm.terminal.reconnect", "重连当前标签", true, None);
        terminal_menu.append(&new_session)?;
        terminal_menu.append(&new_tab)?;
        terminal_menu.append(&import_ssh)?;
        terminal_menu.append(&PredefinedMenuItem::separator())?;
        terminal_menu.append(&close_tab)?;
        terminal_menu.append(&PredefinedMenuItem::separator())?;
        terminal_menu.append(&disconnect)?;
        terminal_menu.append(&reconnect)?;

        // ── 编辑 ──
        // 勿使用 PredefinedMenuItem::copy/paste/select_all：与下方终端项重复，
        // 且 macOS 会在标准 Edit 项后注入 AutoFill / 听写 / 表情等系统菜单。
        let edit = Submenu::new("编辑", true);
        edit.append(&PredefinedMenuItem::undo(Some("撤销")))?;
        edit.append(&PredefinedMenuItem::redo(Some("重做")))?;
        edit.append(&PredefinedMenuItem::separator())?;
        let copy_terminal = MenuItem::with_id(
            "mistterm.edit.copy_terminal",
            "复制",
            true,
            Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyC)),
        );
        let paste_terminal = MenuItem::with_id(
            "mistterm.edit.paste_terminal",
            "粘贴",
            true,
            Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyV)),
        );
        let select_terminal = MenuItem::with_id(
            "mistterm.edit.select_all_terminal",
            "全选",
            true,
            Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyA)),
        );
        edit.append(&copy_terminal)?;
        edit.append(&paste_terminal)?;
        edit.append(&select_terminal)?;
        edit.append(&PredefinedMenuItem::separator())?;
        let terminal_search = MenuItem::with_id(
            "mistterm.edit.find",
            "在终端中搜索",
            true,
            Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyF)),
        );
        edit.append(&terminal_search)?;

        // ── 视图：布局 / 面板 / 外观 ──
        let view = Submenu::new("视图", true);
        let toggle_sidebar =
            MenuItem::with_id("mistterm.view.toggle_sidebar", "折叠侧边栏", true, None);
        let toggle_maximize =
            MenuItem::with_id("mistterm.view.toggle_maximize", "最大化窗口", true, None);
        let sftp_panel = CheckMenuItem::with_id(
            "mistterm.view.panel.sftp",
            "SFTP 文件浏览器",
            true,
            false,
            None,
        );
        let fragment_panel = CheckMenuItem::with_id(
            "mistterm.view.panel.fragments",
            "命令片段侧栏",
            true,
            false,
            None,
        );
        let monitor_panel = CheckMenuItem::with_id(
            "mistterm.view.panel.monitor",
            "系统监控",
            true,
            false,
            None,
        );
        let (theme_submenu, theme_items) = build_theme_submenu(theme_names)?;
        view.append(&toggle_sidebar)?;
        view.append(&toggle_maximize)?;
        view.append(&PredefinedMenuItem::separator())?;
        view.append(&sftp_panel)?;
        view.append(&fragment_panel)?;
        view.append(&monitor_panel)?;
        view.append(&PredefinedMenuItem::separator())?;
        view.append(&theme_submenu)?;

        // ── 工具 ──
        let tools = Submenu::new("工具", true);
        let fragments =
            MenuItem::with_id("mistterm.tools.fragments", "命令片段库…", true, None);
        let quick_fragments = MenuItem::with_id(
            "mistterm.tools.quick_fragments",
            "快速片段选择器",
            true,
            Some(Accelerator::new(
                Some(Modifiers::SUPER | Modifiers::SHIFT),
                Code::KeyJ,
            )),
        );
        let command_history = MenuItem::with_id(
            "mistterm.tools.command_history",
            "命令历史…",
            true,
            Some(Accelerator::new(Some(Modifiers::CONTROL), Code::KeyR)),
        );
        let credentials =
            MenuItem::with_id("mistterm.tools.credentials", "凭证管理", true, None);
        let cloud = MenuItem::with_id("mistterm.tools.cloud", "云端同步", true, None);
        let session_logs =
            MenuItem::with_id("mistterm.tools.session_logs", "浏览会话日志…", true, None);
        tools.append(&fragments)?;
        tools.append(&quick_fragments)?;
        tools.append(&command_history)?;
        tools.append(&PredefinedMenuItem::separator())?;
        tools.append(&credentials)?;
        tools.append(&cloud)?;
        tools.append(&PredefinedMenuItem::separator())?;
        tools.append(&session_logs)?;

        // ── 帮助 ──
        let help = Submenu::new("帮助", true);
        let help_guide = MenuItem::with_id("mistterm.help.guide", "快速入门…", true, None);
        let help_spec = MenuItem::with_id("mistterm.help.spec", "功能规格（系统打开）", true, None);
        let help_keys = MenuItem::with_id("mistterm.help.shortcuts", "键盘快捷键…", true, None);
        let help_folder =
            MenuItem::with_id("mistterm.help.open_docs", "在 Finder 中打开文档文件夹", true, None);
        let help_about = MenuItem::with_id("mistterm.help.about", "关于 Mist", true, None);
        help.append(&help_guide)?;
        help.append(&help_spec)?;
        help.append(&help_keys)?;
        help.append(&PredefinedMenuItem::separator())?;
        help.append(&help_folder)?;
        help.append(&PredefinedMenuItem::separator())?;
        help.append(&help_about)?;

        root.append(&app_menu)?;
        root.append(&terminal_menu)?;
        root.append(&edit)?;
        root.append(&view)?;
        root.append(&tools)?;
        root.append(&help)?;

        root.init_for_nsapp();
        super::macos_app_name::fix_menu_bar_application_title();
        let _ = help.set_as_help_menu_for_nsapp();

        Ok(Self {
            _root: root,
            import_ssh,
            toggle_sidebar,
            toggle_maximize,
            sftp_panel,
            fragment_panel,
            monitor_panel,
            theme_items,
        })
    }

    pub fn sync(
        &mut self,
        ssh_import_enabled: bool,
        sidebar_collapsed: bool,
        window_maximized: bool,
        show_sftp_panel: bool,
        show_fragment_panel: bool,
        show_monitor_panel: bool,
        theme_index: usize,
    ) {
        let _ = self.import_ssh.set_enabled(ssh_import_enabled);
        let _ = self.sftp_panel.set_checked(show_sftp_panel);
        let _ = self.fragment_panel.set_checked(show_fragment_panel);
        let _ = self.monitor_panel.set_checked(show_monitor_panel);
        for (i, item) in self.theme_items.iter().enumerate() {
            let _ = item.set_checked(i == theme_index);
        }
        let sidebar_label = if sidebar_collapsed {
            "展开侧边栏"
        } else {
            "折叠侧边栏"
        };
        let _ = self.toggle_sidebar.set_text(sidebar_label);
        let maximize_label = if window_maximized {
            "还原窗口大小"
        } else {
            "最大化窗口"
        };
        let _ = self.toggle_maximize.set_text(maximize_label);
    }

    pub fn drain_actions(&self) -> Vec<MacMenuAction> {
        let mut out = Vec::new();
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if let Some(action) = action_for_id(event.id.as_ref()) {
                out.push(action);
            }
        }
        out
    }
}

fn build_theme_submenu(theme_names: &[String]) -> muda::Result<(Submenu, Vec<CheckMenuItem>)> {
    let submenu = Submenu::new("主题", true);
    let mut items = Vec::with_capacity(theme_names.len());
    for (i, name) in theme_names.iter().enumerate() {
        let id = format!("mistterm.view.theme.{i}");
        let item = CheckMenuItem::with_id(id, name.as_str(), true, false, None);
        submenu.append(&item)?;
        items.push(item);
    }
    Ok((submenu, items))
}

fn action_for_id(id: &str) -> Option<MacMenuAction> {
    match id {
        "mistterm.app.about" | "mistterm.help.about" => Some(MacMenuAction::About),
        "mistterm.app.preferences" => Some(MacMenuAction::Preferences),
        "mistterm.app.quit" => Some(MacMenuAction::Quit),
        "mistterm.terminal.import_ssh" | "mistterm.file.import_ssh" => Some(MacMenuAction::ImportSsh),
        "mistterm.terminal.new_session" | "mistterm.file.new_session" => Some(MacMenuAction::NewSession),
        "mistterm.terminal.new_tab" => Some(MacMenuAction::NewTab),
        "mistterm.terminal.close_tab" | "mistterm.file.close_tab" => Some(MacMenuAction::CloseTab),
        "mistterm.terminal.disconnect" | "mistterm.file.disconnect" => Some(MacMenuAction::DisconnectSsh),
        "mistterm.terminal.reconnect" | "mistterm.file.reconnect" => Some(MacMenuAction::ReconnectTab),
        "mistterm.edit.copy_terminal" => Some(MacMenuAction::CopyTerminal),
        "mistterm.edit.paste_terminal" => Some(MacMenuAction::PasteToTerminal),
        "mistterm.edit.select_all_terminal" => Some(MacMenuAction::SelectAllTerminal),
        "mistterm.edit.find" => Some(MacMenuAction::TerminalSearch),
        "mistterm.view.toggle_sidebar" => Some(MacMenuAction::ToggleSidebar),
        "mistterm.view.toggle_maximize" => Some(MacMenuAction::ToggleMaximize),
        "mistterm.view.panel.sftp" | "mistterm.view.sftp" => Some(MacMenuAction::ToggleSftp),
        "mistterm.view.panel.fragments" => Some(MacMenuAction::ToggleFragmentSidebar),
        "mistterm.view.panel.monitor" => Some(MacMenuAction::ToggleMonitorPanel),
        "mistterm.tools.fragments" => Some(MacMenuAction::FragmentLibrary),
        "mistterm.tools.quick_fragments" => Some(MacMenuAction::QuickFragmentSelector),
        "mistterm.tools.command_history" => Some(MacMenuAction::CommandHistory),
        "mistterm.tools.credentials" => Some(MacMenuAction::CredentialPanel),
        "mistterm.tools.cloud" => Some(MacMenuAction::CloudSync),
        "mistterm.tools.session_logs" => Some(MacMenuAction::SessionLogBrowser),
        "mistterm.help.guide" => Some(MacMenuAction::HelpUserGuide),
        "mistterm.help.spec" => Some(MacMenuAction::HelpFunctionalSpec),
        "mistterm.help.shortcuts" => Some(MacMenuAction::HelpShortcuts),
        "mistterm.help.open_docs" => Some(MacMenuAction::HelpRevealDocsFolder),
        _ if id.starts_with("mistterm.view.theme.") => {
            id.strip_prefix("mistterm.view.theme.")
                .and_then(|n| n.parse().ok())
                .map(MacMenuAction::Theme)
        }
        _ => None,
    }
}
