//! macOS 系统菜单栏（NSMenu），通过 [muda](https://github.com/tauri-apps/muda) 接入。
//!
//! - **Mist**：应用级
//! - **终端**：会话与连接
//! - **编辑**：终端剪贴板 + 搜索（无系统预置复制/粘贴，避免重复项）
//! - **视图**：布局、右侧面板、主题
//! - **团队**：登录、成员、云端同步
//! - **工具**：片段、历史、凭证与日志
//! - **帮助**：内嵌文档与关于

use super::macos_app_name::APP_DISPLAY_NAME;
use crate::i18n::{menu, UiLanguage};
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
    BatchExec,
    CredentialPanel,
    TeamAccount,
    TeamMembers,
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
    _terminal_menu: Submenu,
    _edit_menu: Submenu,
    _view_menu: Submenu,
    _team_menu: Submenu,
    _tools_menu: Submenu,
    _help_menu: Submenu,
    about: MenuItem,
    preferences: MenuItem,
    quit: MenuItem,
    new_session: MenuItem,
    new_tab: MenuItem,
    import_ssh: MenuItem,
    close_tab: MenuItem,
    disconnect: MenuItem,
    reconnect: MenuItem,
    copy_terminal: MenuItem,
    paste_terminal: MenuItem,
    select_terminal: MenuItem,
    terminal_search: MenuItem,
    toggle_sidebar: MenuItem,
    toggle_maximize: MenuItem,
    sftp_panel: CheckMenuItem,
    fragment_panel: CheckMenuItem,
    monitor_panel: CheckMenuItem,
    theme_submenu: Submenu,
    theme_stored_names: Vec<String>,
    theme_items: Vec<CheckMenuItem>,
    fragments: MenuItem,
    quick_fragments: MenuItem,
    command_history: MenuItem,
    batch_exec: MenuItem,
    credentials: MenuItem,
    team_sign_in: MenuItem,
    team_members: MenuItem,
    cloud: MenuItem,
    session_logs: MenuItem,
    help_guide: MenuItem,
    help_spec: MenuItem,
    help_keys: MenuItem,
    help_folder: MenuItem,
    help_about: MenuItem,
    last_lang: Option<UiLanguage>,
}

impl NativeAppMenu {
    pub fn install(
        theme_display_names: &[String],
        theme_stored_names: &[String],
        lang: UiLanguage,
    ) -> muda::Result<Self> {
        let l = menu::labels(lang);
        super::macos_app_name::set_application_display_name();

        let root = Menu::new();

        let app_menu = Submenu::new(APP_DISPLAY_NAME, true);
        let about = MenuItem::with_id("mistterm.app.about", l.about, true, None);
        let preferences = MenuItem::with_id(
            "mistterm.app.preferences",
            l.preferences,
            true,
            None,
        );
        let quit = MenuItem::with_id(
            "mistterm.app.quit",
            l.quit,
            true,
            None,
        );
        app_menu.append(&about)?;
        app_menu.append(&PredefinedMenuItem::separator())?;
        app_menu.append(&preferences)?;
        app_menu.append(&PredefinedMenuItem::separator())?;
        app_menu.append(&quit)?;

        // ── 终端 ──
        let terminal_menu = Submenu::new(l.terminal_menu, true);
        let new_session = MenuItem::with_id(
            "mistterm.terminal.new_session",
            l.new_session,
            true,
            None,
        );
        let new_tab = MenuItem::with_id(
            "mistterm.terminal.new_tab",
            l.new_tab,
            true,
            None,
        );
        let import_ssh =
            MenuItem::with_id("mistterm.terminal.import_ssh", l.import_ssh, true, None);
        let close_tab = MenuItem::with_id(
            "mistterm.terminal.close_tab",
            l.close_tab,
            true,
            None,
        );
        let disconnect = MenuItem::with_id(
            "mistterm.terminal.disconnect",
            l.disconnect,
            true,
            None,
        );
        let reconnect =
            MenuItem::with_id("mistterm.terminal.reconnect", l.reconnect, true, None);
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
        let edit = Submenu::new(l.edit_menu, true);
        edit.append(&PredefinedMenuItem::undo(Some(l.undo)))?;
        edit.append(&PredefinedMenuItem::redo(Some(l.redo)))?;
        edit.append(&PredefinedMenuItem::separator())?;
        let copy_terminal = MenuItem::with_id(
            "mistterm.edit.copy_terminal",
            l.copy,
            true,
            None,
        );
        let paste_terminal = MenuItem::with_id(
            "mistterm.edit.paste_terminal",
            l.paste,
            true,
            None,
        );
        let select_terminal = MenuItem::with_id(
            "mistterm.edit.select_all_terminal",
            l.select_all,
            true,
            None,
        );
        edit.append(&copy_terminal)?;
        edit.append(&paste_terminal)?;
        edit.append(&select_terminal)?;
        edit.append(&PredefinedMenuItem::separator())?;
        let terminal_search = MenuItem::with_id(
            "mistterm.edit.find",
            l.find_in_terminal,
            true,
            None,
        );
        edit.append(&terminal_search)?;

        // ── 视图：布局 / 面板 / 外观 ──
        let view = Submenu::new(l.view_menu, true);
        let toggle_sidebar =
            MenuItem::with_id("mistterm.view.toggle_sidebar", l.collapse_sidebar, true, None);
        let toggle_maximize =
            MenuItem::with_id("mistterm.view.toggle_maximize", l.maximize_window, true, None);
        let sftp_panel = CheckMenuItem::with_id(
            "mistterm.view.panel.sftp",
            l.sftp_panel,
            true,
            false,
            None,
        );
        let fragment_panel = CheckMenuItem::with_id(
            "mistterm.view.panel.fragments",
            l.fragment_panel,
            true,
            false,
            None,
        );
        let monitor_panel = CheckMenuItem::with_id(
            "mistterm.view.panel.monitor",
            l.monitor_panel,
            true,
            false,
            None,
        );
        let (theme_submenu, theme_items) =
            build_theme_submenu(theme_display_names, l.theme_menu)?;
        view.append(&toggle_sidebar)?;
        view.append(&toggle_maximize)?;
        view.append(&PredefinedMenuItem::separator())?;
        view.append(&sftp_panel)?;
        view.append(&fragment_panel)?;
        view.append(&monitor_panel)?;
        view.append(&PredefinedMenuItem::separator())?;
        view.append(&theme_submenu)?;

        // ── 团队 ──
        let team_menu = Submenu::new(l.team_menu, true);
        let team_sign_in = MenuItem::with_id(
            "mistterm.team.sign_in",
            l.team_sign_in,
            true,
            None,
        );
        let team_members = MenuItem::with_id(
            "mistterm.team.members",
            l.team_members,
            true,
            None,
        );
        let cloud = MenuItem::with_id("mistterm.team.cloud", l.cloud_sync, true, None);
        team_menu.append(&team_sign_in)?;
        team_menu.append(&team_members)?;
        team_menu.append(&PredefinedMenuItem::separator())?;
        team_menu.append(&cloud)?;

        // ── 工具 ──
        let tools = Submenu::new(l.tools_menu, true);
        let fragments =
            MenuItem::with_id("mistterm.tools.fragments", l.fragment_library, true, None);
        let quick_fragments = MenuItem::with_id(
            "mistterm.tools.quick_fragments",
            l.quick_fragments,
            true,
            None,
        );
        let command_history = MenuItem::with_id(
            "mistterm.tools.command_history",
            l.command_history,
            true,
            None,
        );
        let batch_exec =
            MenuItem::with_id("mistterm.tools.batch_exec", l.batch_exec, true, None);
        let credentials =
            MenuItem::with_id("mistterm.tools.credentials", l.credentials, true, None);
        let session_logs =
            MenuItem::with_id("mistterm.tools.session_logs", l.session_logs, true, None);
        tools.append(&fragments)?;
        tools.append(&quick_fragments)?;
        tools.append(&command_history)?;
        tools.append(&batch_exec)?;
        tools.append(&PredefinedMenuItem::separator())?;
        tools.append(&credentials)?;
        tools.append(&PredefinedMenuItem::separator())?;
        tools.append(&session_logs)?;

        // ── 帮助 ──
        let help = Submenu::new(l.help_menu, true);
        let help_guide = MenuItem::with_id("mistterm.help.guide", l.help_guide, true, None);
        let help_spec = MenuItem::with_id("mistterm.help.spec", l.help_spec, true, None);
        let help_keys = MenuItem::with_id("mistterm.help.shortcuts", l.help_shortcuts, true, None);
        let help_folder =
            MenuItem::with_id("mistterm.help.open_docs", l.help_open_docs, true, None);
        let help_about = MenuItem::with_id("mistterm.help.about", l.help_about, true, None);
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
        root.append(&team_menu)?;
        root.append(&tools)?;
        root.append(&help)?;

        root.init_for_nsapp();
        super::macos_app_name::fix_menu_bar_application_title();
        let _ = help.set_as_help_menu_for_nsapp();

        Ok(Self {
            _root: root,
            _terminal_menu: terminal_menu,
            _edit_menu: edit,
            _view_menu: view,
            _team_menu: team_menu,
            _tools_menu: tools,
            _help_menu: help,
            about,
            preferences,
            quit,
            new_session,
            new_tab,
            import_ssh,
            close_tab,
            disconnect,
            reconnect,
            copy_terminal,
            paste_terminal,
            select_terminal,
            terminal_search,
            toggle_sidebar,
            toggle_maximize,
            sftp_panel,
            fragment_panel,
            monitor_panel,
            theme_submenu,
            theme_stored_names: theme_stored_names.to_vec(),
            theme_items,
            fragments,
            quick_fragments,
            command_history,
            batch_exec,
            credentials,
            team_sign_in,
            team_members,
            cloud,
            session_logs,
            help_guide,
            help_spec,
            help_keys,
            help_folder,
            help_about,
            last_lang: Some(lang),
        })
    }

    fn apply_locale(&self, lang: UiLanguage) {
        let l = menu::labels(lang);
        let _ = self.about.set_text(l.about);
        let _ = self.preferences.set_text(l.preferences);
        let _ = self.quit.set_text(l.quit);
        let _ = self._terminal_menu.set_text(l.terminal_menu);
        let _ = self.new_session.set_text(l.new_session);
        let _ = self.new_tab.set_text(l.new_tab);
        let _ = self.import_ssh.set_text(l.import_ssh);
        let _ = self.close_tab.set_text(l.close_tab);
        let _ = self.disconnect.set_text(l.disconnect);
        let _ = self.reconnect.set_text(l.reconnect);
        let _ = self._edit_menu.set_text(l.edit_menu);
        let _ = self.copy_terminal.set_text(l.copy);
        let _ = self.paste_terminal.set_text(l.paste);
        let _ = self.select_terminal.set_text(l.select_all);
        let _ = self.terminal_search.set_text(l.find_in_terminal);
        let _ = self._view_menu.set_text(l.view_menu);
        let _ = self.sftp_panel.set_text(l.sftp_panel);
        let _ = self.fragment_panel.set_text(l.fragment_panel);
        let _ = self.monitor_panel.set_text(l.monitor_panel);
        let _ = self.theme_submenu.set_text(l.theme_menu);
        let _ = self._tools_menu.set_text(l.tools_menu);
        let _ = self.fragments.set_text(l.fragment_library);
        let _ = self.quick_fragments.set_text(l.quick_fragments);
        let _ = self.command_history.set_text(l.command_history);
        let _ = self.batch_exec.set_text(l.batch_exec);
        let _ = self.credentials.set_text(l.credentials);
        let _ = self.team_sign_in.set_text(l.team_sign_in);
        let _ = self.team_members.set_text(l.team_members);
        let _ = self.cloud.set_text(l.cloud_sync);
        let _ = self.session_logs.set_text(l.session_logs);
        let _ = self._help_menu.set_text(l.help_menu);
        let _ = self.help_guide.set_text(l.help_guide);
        let _ = self.help_spec.set_text(l.help_spec);
        let _ = self.help_keys.set_text(l.help_shortcuts);
        let _ = self.help_folder.set_text(l.help_open_docs);
        let _ = self.help_about.set_text(l.help_about);
    }

    pub fn sync(
        &mut self,
        ctx: &eframe::egui::Context,
        lang: UiLanguage,
        ssh_import_enabled: bool,
        sidebar_collapsed: bool,
        window_maximized: bool,
        show_sftp_panel: bool,
        show_fragment_panel: bool,
        show_monitor_panel: bool,
        theme_index: usize,
    ) {
        if self.last_lang != Some(lang) {
            self.apply_locale(lang);
            self.last_lang = Some(lang);
        }
        for (i, stored) in self.theme_stored_names.iter().enumerate() {
            if let Some(item) = self.theme_items.get(i) {
                let label = crate::i18n::theme_display_name(ctx, stored);
                let _ = item.set_text(label.as_ref());
            }
        }
        let l = menu::labels(lang);
        let _ = self.import_ssh.set_enabled(ssh_import_enabled);
        let _ = self.sftp_panel.set_checked(show_sftp_panel);
        let _ = self.fragment_panel.set_checked(show_fragment_panel);
        let _ = self.monitor_panel.set_checked(show_monitor_panel);
        for (i, item) in self.theme_items.iter().enumerate() {
            let _ = item.set_checked(i == theme_index);
        }
        let sidebar_label = if sidebar_collapsed {
            l.expand_sidebar
        } else {
            l.collapse_sidebar
        };
        let _ = self.toggle_sidebar.set_text(sidebar_label);
        let maximize_label = if window_maximized {
            l.restore_window
        } else {
            l.maximize_window
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

fn build_theme_submenu(
    theme_names: &[String],
    title: &str,
) -> muda::Result<(Submenu, Vec<CheckMenuItem>)> {
    let submenu = Submenu::new(title, true);
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
        "mistterm.tools.batch_exec" => Some(MacMenuAction::BatchExec),
        "mistterm.tools.credentials" => Some(MacMenuAction::CredentialPanel),
        "mistterm.team.sign_in" | "mistterm.tools.team_account" => Some(MacMenuAction::TeamAccount),
        "mistterm.team.members" => Some(MacMenuAction::TeamMembers),
        "mistterm.team.cloud" | "mistterm.tools.cloud" => Some(MacMenuAction::CloudSync),
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
