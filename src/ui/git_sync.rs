//! Git 同步 UI 组件
//!
//! 提供 Git 仓库状态显示和操作界面

use eframe::egui;
use std::path::PathBuf;

use crate::core::session::{sessions_json_for_git_export, SessionConfig};
use crate::sync::{GitRepo, RepoStatus};
use crate::i18n::UiLanguage;
use std::fs;
use crate::ui::chrome;
use crate::ui::icons::IconId;
use crate::ui::layout_util;
use crate::ui::theme::Theme;

fn inset_section<R>(ui: &mut egui::Ui, theme: &Theme, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::none()
        .fill(theme.color_subtle_inset_fill())
        .rounding(theme.radius_list_item())
        .inner_margin(egui::Margin::symmetric(
            theme.spacing_search_input_x(),
            theme.spacing_search_input_y(),
        ))
        .show(ui, |ui| {
            layout_util::set_width_to_available(ui);
            add(ui)
        })
        .inner
}

/// Git 同步面板
pub struct GitSyncPanel {
    /// 仓库路径
    repo_path: String,
    /// Git 仓库实例
    repo: Option<GitRepo>,
    /// 仓库状态
    status: Option<RepoStatus>,
    /// 提交信息
    commit_message: String,
    /// 选中的文件列表（用于暂存；待接入暂存 UI）
    _staged_files: Vec<String>,
    /// 状态消息
    status_message: String,
    /// 错误消息
    error_message: String,
    /// 是否显示克隆对话框
    show_clone_dialog: bool,
    /// 克隆 URL
    clone_url: String,
    /// 克隆目标路径
    clone_path: String,
    /// 分支名
    branch: String,
    /// 远程 URL
    remote_url: String,
    /// 最后提交信息（待展示）
    _last_commit: String,
    /// 操作状态
    operation_status: OperationStatus,
    /// Pull 成功后待合并 `sessions.json`
    pending_sessions_merge: bool,
    /// 本帧 SidePanel 占位矩形（Foreground 重绘用）
    last_panel_slot_rect: Option<egui::Rect>,
    /// [`show`] 起始同步，用于 `open_repo` 等分支的用户提示文案
    ui_lang_last: UiLanguage,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OperationStatus {
    Idle,
    Loading,
    Success(String),
    Error(String),
}

impl GitSyncPanel {
    #[inline]
    pub fn is_clone_dialog_open(&self) -> bool {
        self.show_clone_dialog
    }

    pub fn new() -> Self {
        Self {
            repo_path: String::new(),
            repo: None,
            status: None,
            commit_message: String::new(),
            _staged_files: Vec::new(),
            status_message: String::new(),
            error_message: String::new(),
            show_clone_dialog: false,
            clone_url: String::new(),
            clone_path: String::new(),
            branch: String::new(),
            remote_url: String::new(),
            _last_commit: String::new(),
            operation_status: OperationStatus::Idle,
            pending_sessions_merge: false,
            last_panel_slot_rect: None,
            ui_lang_last: UiLanguage::default(),
        }
    }

    pub fn set_panel_slot_rect(&mut self, rect: egui::Rect) {
        self.last_panel_slot_rect = Some(rect);
    }

    pub fn panel_slot_rect(&self) -> Option<egui::Rect> {
        self.last_panel_slot_rect
    }

    pub fn clear_panel_slot_rect(&mut self) {
        self.last_panel_slot_rect = None;
    }

    pub fn take_pending_sessions_merge(&mut self) -> bool {
        std::mem::take(&mut self.pending_sessions_merge)
    }

    pub fn sessions_json_path(&self) -> PathBuf {
        PathBuf::from(&self.repo_path).join("sessions.json")
    }

    #[inline]
    fn loc(&self) -> crate::i18n::Locale {
        crate::i18n::Locale::from(self.ui_lang_last)
    }

    /// 设置仓库路径并打开
    pub fn set_repo_path(&mut self, path: &str) {
        self.repo_path = path.to_string();
        self.open_repo();
    }

    /// 打开仓库
    fn open_repo(&mut self) {
        let path = PathBuf::from(&self.repo_path);
        match GitRepo::open(&path) {
            Ok(repo) => {
                self.branch = repo.branch().to_string();
                self.remote_url = repo.remote_url().to_string();
                self.repo = Some(repo);
                self.refresh_status();
                self.status_message = self
                    .loc()
                    .tr("Repository opened", "仓库已打开")
                    .to_string();
                self.error_message.clear();
            }
            Err(e) => {
                self.repo = None;
                self.status = None;
                self.error_message = format!(
                    "{}{}",
                    self.loc().tr("Could not open repository: ", "打开仓库失败："),
                    e
                );
                self.status_message.clear();
            }
        }
    }

    /// 刷新仓库状态
    fn refresh_status(&mut self) {
        if let Some(ref repo) = self.repo {
            match repo.status() {
                Ok(status) => {
                    self.status = Some(status.clone());
                    self.status_message = if status.is_dirty {
                        self.loc()
                            .tr("Working tree has uncommitted changes", "有未提交的更改")
                            .to_string()
                    } else {
                        self.loc()
                            .tr("Working tree clean", "工作区干净")
                            .to_string()
                    };
                }
                Err(e) => {
                    self.error_message = format!(
                        "{}{}",
                        self.loc().tr("Failed to read repo status: ", "获取状态失败："),
                        e
                    );
                }
            }
        }
    }

    /// 执行 Pull 操作
    fn pull(&mut self) {
        if let Some(ref repo) = self.repo {
            self.operation_status = OperationStatus::Loading;
            match repo.pull() {
                Ok(()) => {
                    self.pending_sessions_merge = true;
                    self.operation_status = OperationStatus::Success(
                        self.loc().tr("Pull succeeded", "拉取成功").to_string(),
                    );
                    self.refresh_status();
                }
                Err(e) => {
                    self.operation_status = OperationStatus::Error(format!(
                        "{}{}",
                        self.loc().tr("Pull failed: ", "拉取失败："),
                        e
                    ));
                }
            }
        }
    }

    /// 执行 Push 操作
    fn push(&mut self) {
        if let Some(ref repo) = self.repo {
            self.operation_status = OperationStatus::Loading;
            match repo.push() {
                Ok(()) => {
                    self.operation_status = OperationStatus::Success(
                        self.loc().tr("Push succeeded", "推送成功").to_string(),
                    );
                    self.refresh_status();
                }
                Err(e) => {
                    self.operation_status = OperationStatus::Error(format!(
                        "{}{}",
                        self.loc().tr("Push failed: ", "推送失败："),
                        e
                    ));
                }
            }
        }
    }

    /// 执行 Commit 操作
    /// 将 MistTerm 会话写入当前仓库根目录的 `sessions.json`（密码占位，可安全 push）。
    pub fn export_redacted_sessions(&mut self, sessions: &[SessionConfig]) {
        if self.repo.is_none() {
            self.error_message = self
                .loc()
                .tr("Open a Git repository first", "请先打开 Git 仓库")
                .to_string();
            return;
        }
        let dest = PathBuf::from(&self.repo_path).join("sessions.json");
        match sessions_json_for_git_export(sessions) {
            Ok(json) => match fs::write(&dest, json) {
                Ok(()) => {
                    self.status_message = format!(
                        "{}{}",
                        self.loc()
                            .tr("Wrote redacted sessions to ", "已写入脱敏 sessions 至 "),
                        dest.display()
                    );
                    self.error_message.clear();
                    self.refresh_status();
                }
                Err(e) => {
                    self.error_message = format!(
                        "{}{}",
                        self.loc().tr("Write failed: ", "写入失败："),
                        e
                    );
                }
            },
            Err(e) => self.error_message = e,
        }
    }

    fn commit(&mut self) {
        if let Some(ref repo) = self.repo {
            if self.commit_message.is_empty() {
                self.operation_status = OperationStatus::Error(
                    self.loc()
                        .tr("Enter a commit message", "请输入提交信息")
                        .to_string(),
                );
                return;
            }
            self.operation_status = OperationStatus::Loading;
            // 先添加所有更改
            if let Err(e) = repo.add_all() {
                self.operation_status = OperationStatus::Error(format!(
                    "{}{}",
                    self.loc().tr("Stage files failed: ", "添加文件失败："),
                    e
                ));
                return;
            }
            match repo.commit(&self.commit_message, None, None) {
                Ok(_) => {
                    self.operation_status = OperationStatus::Success(
                        self.loc().tr("Commit succeeded", "提交成功").to_string(),
                    );
                    self.commit_message.clear();
                    self.refresh_status();
                }
                Err(e) => {
                    self.operation_status = OperationStatus::Error(format!(
                        "{}{}",
                        self.loc().tr("Commit failed: ", "提交失败："),
                        e
                    ));
                }
            }
        }
    }

    /// 初始化新仓库
    fn init_repo(&mut self) {
        let path = PathBuf::from(&self.repo_path);
        match GitRepo::init(&path) {
            Ok(repo) => {
                self.repo = Some(repo);
                self.status_message = self
                    .loc()
                    .tr("Repository initialized", "仓库初始化成功")
                    .to_string();
                self.error_message.clear();
                self.refresh_status();
            }
            Err(e) => {
                self.error_message = format!(
                    "{}{}",
                    self.loc().tr("Init failed: ", "初始化失败："),
                    e
                );
            }
        }
    }

    /// 克隆仓库
    fn clone_repo(&mut self) {
        if self.clone_url.is_empty() || self.clone_path.is_empty() {
            self.error_message = self
                .loc()
                .tr("Enter clone URL and destination path", "请输入克隆 URL 和目标路径")
                .to_string();
            return;
        }
        self.operation_status = OperationStatus::Loading;
        let path = PathBuf::from(&self.clone_path);
        match GitRepo::clone(&self.clone_url, &path) {
            Ok(repo) => {
                self.repo = Some(repo);
                self.repo_path = self.clone_path.clone();
                self.operation_status = OperationStatus::Success(
                    self.loc().tr("Clone succeeded", "克隆成功").to_string(),
                );
                self.show_clone_dialog = false;
                self.refresh_status();
            }
            Err(e) => {
                self.operation_status = OperationStatus::Error(format!(
                    "{}{}",
                    self.loc().tr("Clone failed: ", "克隆失败："),
                    e
                ));
            }
        }
    }

    /// 显示 Git 同步面板；`close_panel` 为 true 时由宿主关闭侧栏。
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        close_panel: &mut bool,
        sessions: &[SessionConfig],
    ) {
        self.ui_lang_last = crate::i18n::language(ui.ctx());
        let panel_w = ui.available_width();
        ui.set_max_width(panel_w);

        let prev_gap_y = ui.spacing().item_spacing.y;
        ui.spacing_mut().item_spacing.y = 0.0;
        theme.frame_right_dock_header_band().show(ui, |ui| {
            layout_util::set_width_to_available(ui);
            chrome::dock_header_horizontal(ui, theme, |ui| {
                chrome::panel_header_title_leading(
                    ui,
                    theme,
                    IconId::GitBranch,
                    crate::i18n::tr(ui.ctx(), "Git Sync", "Git 同步"),
                );
                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        if chrome::dock_close_icon_button(
                            ui,
                            theme,
                            crate::i18n::tr(ui.ctx(), "Close Git Sync", "关闭 Git 同步"),
                        )
                        .clicked()
                        {
                            *close_panel = true;
                        }
                        if chrome::panel_toolbar_icon_button(
                            ui,
                            theme,
                            IconId::Refresh,
                            crate::i18n::tr(ui.ctx(), "Refresh", "刷新"),
                        )
                        .clicked()
                        {
                            self.refresh_status();
                        }
                    },
                );
            });
        });
        chrome::right_dock_header_divider(ui, theme);
        ui.spacing_mut().item_spacing.y = prev_gap_y;
        ui.add_space(theme.spacing_xs());

        let scroll_h = layout_util::scroll_area_fill_height(ui, 120.0);
        egui::ScrollArea::vertical()
            .id_source("mistterm_git_sync_scroll")
            .auto_shrink([false; 2])
            .max_height(scroll_h)
            .show(ui, |ui| {
                layout_util::set_width_to_available(ui);
                let field_w = layout_util::finite_content_width_inset(
                    ui,
                    0.0,
                    200.0,
                    ui.available_width(),
                );

                inset_section(ui, theme, |ui| {
                    chrome::form_field_label(
                        ui,
                        theme,
                        crate::i18n::tr(ui.ctx(), "Repository path", "仓库路径"),
                    );
                    chrome::form_singleline_field(
                        ui,
                        theme,
                        egui::Id::new("git_sync_repo_path"),
                        &mut self.repo_path,
                        "/path/to/repo",
                        field_w,
                        false,
                    );
                    ui.add_space(theme.spacing_sm());
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(
                            theme.spacing_panel_gap(),
                            theme.spacing_panel_gap(),
                        );
                        if chrome::panel_toolbar_icon_button(
                            ui,
                            theme,
                            IconId::Folder,
                            crate::i18n::tr(ui.ctx(), "Open repository", "打开仓库"),
                        )
                        .clicked()
                            && !self.repo_path.is_empty()
                        {
                            self.open_repo();
                        }
                        if chrome::panel_toolbar_icon_button(
                            ui,
                            theme,
                            IconId::GitBranch,
                            crate::i18n::tr(ui.ctx(), "Initialize repository", "初始化仓库"),
                        )
                        .clicked()
                            && !self.repo_path.is_empty()
                        {
                            self.init_repo();
                        }
                        if chrome::panel_toolbar_icon_button(
                            ui,
                            theme,
                            IconId::Package,
                            crate::i18n::tr(ui.ctx(), "Clone…", "克隆…"),
                        )
                        .clicked()
                        {
                            self.show_clone_dialog = true;
                        }
                    });
                });

                if self.repo.is_some() {
                    ui.add_space(theme.spacing_md());
                    inset_section(ui, theme, |ui| {
                        ui.label(chrome::form_section_heading(
                            theme,
                            crate::i18n::tr(ui.ctx(), "Repository status", "仓库状态"),
                        ));
                        ui.add_space(theme.spacing_sm());
                        chrome::dock_label_value_row(
                            ui,
                            theme,
                            IconId::GitBranch,
                            crate::i18n::tr(ui.ctx(), "Branch", "分支"),
                            &self.branch,
                        );
                        ui.add_space(theme.spacing_xs());
                        let remote_display = if self.remote_url.is_empty() {
                            crate::i18n::tr(ui.ctx(), "Not configured", "未配置").to_string()
                        } else {
                            self.remote_url.clone()
                        };
                        chrome::dock_label_value_row(
                            ui,
                            theme,
                            IconId::Cloud,
                            crate::i18n::tr(ui.ctx(), "Remote", "远程"),
                            remote_display,
                        );
                        if let Some(ref status) = self.status {
                            ui.add_space(theme.spacing_xs());
                            let status_text = if status.is_dirty {
                                crate::i18n::tr(ui.ctx(), "Dirty", "有更改")
                            } else {
                                crate::i18n::tr(ui.ctx(), "Clean", "干净")
                            };
                            chrome::dock_label_value_row(
                                ui,
                                theme,
                                IconId::Dot,
                                crate::i18n::tr(ui.ctx(), "Status", "状态"),
                                status_text,
                            );
                        }
                    });

                    ui.add_space(theme.spacing_md());
                    inset_section(ui, theme, |ui| {
                        ui.label(chrome::form_section_heading(
                            theme,
                            crate::i18n::tr(ui.ctx(), "Actions", "操作"),
                        ));
                        ui.add_space(theme.spacing_sm());
                        let has_remote = !self.remote_url.trim().is_empty();
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(
                                theme.spacing_panel_gap(),
                                theme.spacing_panel_gap(),
                            );
                            ui.add_enabled_ui(has_remote, |ui| {
                                if chrome::panel_toolbar_icon_button(
                                    ui,
                                    theme,
                                    IconId::GitPull,
                                    crate::i18n::tr(ui.ctx(), "Pull from remote", "从远程拉取"),
                                )
                                .clicked()
                                {
                                    self.pull();
                                }
                                if chrome::panel_toolbar_icon_button(
                                    ui,
                                    theme,
                                    IconId::GitPush,
                                    crate::i18n::tr(ui.ctx(), "Push to remote", "推送到远程"),
                                )
                                .clicked()
                                {
                                    self.push();
                                }
                            });
                            if chrome::panel_toolbar_icon_button(
                                ui,
                                theme,
                                IconId::GitCommit,
                                crate::i18n::tr(ui.ctx(), "Commit changes", "提交更改"),
                            )
                            .clicked()
                            {
                                self.commit();
                            }
                        });
                        if !has_remote {
                            ui.add_space(theme.spacing_xs());
                            ui.label(
                                chrome::rich_caption(
                                    theme,
                                    crate::i18n::tr(
                                        ui.ctx(),
                                        "Configure remote URL to enable pull/push.",
                                        "配置远程 URL 后可拉取/推送。",
                                    ),
                                )
                                .weak(),
                            );
                        }
                        ui.add_space(theme.spacing_sm());
                        if chrome::panel_toolbar_icon_button(
                            ui,
                            theme,
                            IconId::Server,
                            crate::i18n::tr(
                                ui.ctx(),
                                "Write redacted sessions.json",
                                "写入脱敏 sessions.json",
                            ),
                        )
                        .clicked()
                        {
                            self.export_redacted_sessions(sessions);
                        }
                    });

                    ui.add_space(theme.spacing_md());
                    inset_section(ui, theme, |ui| {
                        chrome::form_field_label(
                            ui,
                            theme,
                            crate::i18n::tr(ui.ctx(), "Commit message", "提交信息"),
                        );
                        chrome::form_singleline_field(
                            ui,
                            theme,
                            egui::Id::new("git_sync_commit_msg"),
                            &mut self.commit_message,
                            crate::i18n::tr(ui.ctx(), "Enter commit message…", "输入提交信息…"),
                            field_w,
                            false,
                        );
                        ui.add_space(theme.spacing_sm());
                        if chrome::panel_toolbar_icon_button(
                            ui,
                            theme,
                            IconId::Check,
                            crate::i18n::tr(ui.ctx(), "Stage all", "暂存全部"),
                        )
                        .clicked()
                        {
                            if let Some(r) = self.repo.as_ref() {
                                if let Err(e) = r.add_all() {
                                    self.error_message = format!(
                                        "{}{}",
                                        self.loc().tr("Stage failed: ", "暂存失败："),
                                        e
                                    );
                                } else {
                                    self.status_message = self
                                        .loc()
                                        .tr("All changes staged", "已暂存所有更改")
                                        .to_string();
                                }
                            }
                        }
                    });
                } else {
                    ui.add_space(theme.spacing_md());
                    ui.label(
                        chrome::rich_caption(
                            theme,
                            crate::i18n::tr(
                                ui.ctx(),
                                "No Git repository open. Enter a path above or clone a new one.",
                                "尚未打开 Git 仓库。请在上方输入路径，或点击「克隆」。",
                            ),
                        )
                        .color(theme.text_tertiary()),
                    );
                }

                ui.add_space(theme.spacing_md());
                match &self.operation_status {
                    OperationStatus::Idle => {}
                    OperationStatus::Loading => {
                        chrome::busy_row(ui, theme, crate::i18n::tr(ui.ctx(), "Working…", "操作中…"));
                    }
                    OperationStatus::Success(msg) => {
                        crate::ui::icons::icon_label_row(
                            ui,
                            IconId::Check,
                            msg,
                            theme.size_icon_glyph(),
                            6.0,
                            |t| t.color(theme.green_color()),
                        );
                    }
                    OperationStatus::Error(msg) => {
                        crate::ui::icons::icon_label_row(
                            ui,
                            IconId::Cross,
                            msg,
                            theme.size_icon_glyph(),
                            6.0,
                            |t| t.color(theme.red_color()),
                        );
                    }
                }
                if !self.status_message.is_empty() {
                    ui.label(
                        egui::RichText::new(&self.status_message).color(theme.text_tertiary()),
                    );
                }
                if !self.error_message.is_empty() {
                    ui.colored_label(theme.red_color(), &self.error_message);
                }
            });

        if self.show_clone_dialog {
            let mut clone_open = self.show_clone_dialog;
            let mut cancel_clone = false;
            let modal_sz = layout_util::modal_clone_size(ui.ctx());
            crate::ui::chrome::modal_window("clone_repo_modal", theme, ui.ctx())
                .open(&mut clone_open)
                .default_pos(layout_util::modal_center_pos(ui.ctx(), modal_sz))
                .resizable(false)
                .fixed_size(modal_sz)
                .show(ui.ctx(), |ui| {
                    crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                        crate::ui::chrome::modal_header_title_only(
                            ui,
                            theme,
                            crate::i18n::tr(ui.ctx(), "Clone repository", "克隆仓库"),
                            crate::ui::chrome::modal_title_font_size(theme),
                        );
                        ui.set_min_width(layout_util::finite_content_width(ui));
                        let field_w =
                            layout_util::finite_content_width_inset(ui, 0.0, 280.0, ui.available_width());
                        crate::ui::chrome::form_field_label(ui, theme, crate::i18n::tr(ui.ctx(), "Clone URL", "克隆 URL"));
                        crate::ui::chrome::form_singleline_field(
                            ui,
                            theme,
                            egui::Id::new("git_clone_url"),
                            &mut self.clone_url,
                            "https://github.com/user/repo.git",
                            field_w,
                            false,
                        );
                        ui.add_space(theme.spacing_md());
                        crate::ui::chrome::form_field_label(ui, theme, crate::i18n::tr(ui.ctx(), "Destination path", "目标路径"));
                        crate::ui::chrome::form_singleline_field(
                            ui,
                            theme,
                            egui::Id::new("git_clone_path"),
                            &mut self.clone_path,
                            "/path/to/clone",
                            field_w,
                            false,
                        );
                        ui.add_space(theme.spacing_lg());
                        ui.horizontal(|ui| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if crate::ui::chrome::modal_primary_icon_button(
                                        ui,
                                        theme,
                                        crate::ui::icons::IconId::Package,
                                        crate::i18n::tr(ui.ctx(), "Clone", "克隆"),
                                    )
                                    .clicked()
                                    {
                                        self.clone_repo();
                                    }
                                    if crate::ui::chrome::modal_secondary_icon_button(
                                        ui,
                                        theme,
                                        crate::ui::icons::IconId::Cross,
                                        crate::i18n::tr(ui.ctx(), "Cancel", "取消"),
                                    )
                                    .clicked()
                                    {
                                        cancel_clone = true;
                                    }
                                },
                            );
                        });
                    });
                });
            if cancel_clone {
                clone_open = false;
                self.clone_url.clear();
                self.clone_path.clear();
            }
            self.show_clone_dialog = clone_open;
        }
    }
}

impl Default for GitSyncPanel {
    fn default() -> Self {
        Self::new()
    }
}