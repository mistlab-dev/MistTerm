//! Git 同步 UI 组件
//!
//! 提供 Git 仓库状态显示和操作界面

use eframe::egui;
use std::path::PathBuf;

use crate::sync::{GitRepo, RepoStatus};
use crate::i18n::UiLanguage;
use crate::ui::layout_util;
use crate::ui::theme::Theme;

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
            ui_lang_last: UiLanguage::default(),
        }
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
    pub fn show(&mut self, ui: &mut egui::Ui, theme: &Theme, close_panel: &mut bool) {
        self.ui_lang_last = crate::i18n::language(ui.ctx());
        ui.vertical(|ui| {
            let trailing_w = crate::ui::chrome::panel_header_trailing_width_tools(
                ui,
                theme,
                &[crate::ui::chrome::PanelToolbarSpec {
                    icon: Some(crate::ui::icons::IconId::Refresh),
                    label: crate::i18n::tr(ui.ctx(), "Refresh", "刷新"),
                }],
            );
            let mut header_closed = false;
            theme.frame_panel_header_band().show(ui, |ui| {
                header_closed = crate::ui::chrome::dock_panel_title_row(
                    ui,
                    theme,
                    |ui| {
                        crate::ui::chrome::dock_title_row(
                            ui,
                            theme,
                            crate::ui::icons::IconId::GitBranch,
                            crate::i18n::tr(ui.ctx(), "Git Sync", "Git 同步"),
                        );
                    },
                    crate::i18n::tr(ui.ctx(), "Close Git Sync", "关闭 Git 同步"),
                    trailing_w,
                    |ui, theme| {
                        let closed = crate::ui::chrome::dock_close_icon_button(
                            ui,
                            theme,
                            crate::i18n::tr(ui.ctx(), "Close Git Sync", "关闭 Git 同步"),
                        )
                        .clicked();
                        if crate::ui::chrome::panel_toolbar_icon_button(
                            ui,
                            theme,
                            crate::ui::icons::IconId::Refresh,
                            crate::i18n::tr(ui.ctx(), "Refresh", "刷新"),
                        )
                        .clicked()
                        {
                            self.refresh_status();
                        }
                        closed
                    },
                );
            });
            if header_closed {
                *close_panel = true;
            }
            crate::ui::chrome::panel_header_divider(ui, theme);

            // 仓库路径设置
            ui.group(|ui| {
                crate::ui::chrome::form_field_label(ui, theme, crate::i18n::tr(ui.ctx(), "Repository path", "仓库路径"));
                let path_w = crate::ui::layout_util::finite_content_width_inset(
                    ui,
                    0.0,
                    200.0,
                    ui.available_width(),
                );
                crate::ui::chrome::form_singleline_field(
                    ui,
                    theme,
                    egui::Id::new("git_sync_repo_path"),
                    &mut self.repo_path,
                    "/path/to/repo",
                    path_w,
                    false,
                );
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_panel_gap();
                    if crate::ui::chrome::panel_action_icon_button_ex(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Folder,
                        crate::i18n::tr(ui.ctx(), "Open repository", "打开仓库"),
                        !self.repo_path.is_empty(),
                    )
                    .clicked()
                        && !self.repo_path.is_empty()
                    {
                        self.open_repo();
                    }
                    if crate::ui::chrome::panel_action_icon_button_ex(
                        ui,
                        theme,
                        crate::ui::icons::IconId::GitBranch,
                        crate::i18n::tr(ui.ctx(), "Initialize repository", "初始化仓库"),
                        !self.repo_path.is_empty(),
                    )
                    .clicked()
                        && !self.repo_path.is_empty()
                    {
                        self.init_repo();
                    }
                    if crate::ui::chrome::panel_action_icon_button(ui, theme, crate::ui::icons::IconId::Package, crate::i18n::tr(ui.ctx(), "Clone…", "克隆…"))
                        .clicked() {
                        self.show_clone_dialog = true;
                    }
                });
            });

            ui.add_space(theme.spacing_md());

            // 仓库状态显示（不显式借用 `repo`，避免与 `pull/commit` 等对 `&mut self` 的调用冲突）
            if self.repo.is_some() {
                ui.group(|ui| {
                    ui.label(egui::RichText::new(crate::i18n::tr(ui.ctx(), "Repository status", "仓库状态")).strong());
                    ui.horizontal(|ui| {
                        ui.label(crate::i18n::tr(ui.ctx(), "Branch:", "分支："));
                        ui.label(
                            egui::RichText::new(&self.branch).color(theme.accent_color()),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label(crate::i18n::tr(ui.ctx(), "Remote:", "远程："));
                        if self.remote_url.is_empty() {
                            ui.label(egui::RichText::new(crate::i18n::tr(ui.ctx(), "Not configured", "未配置")).color(theme.text_tertiary()));
                        } else {
                            ui.label(
                                egui::RichText::new(&self.remote_url).color(theme.green_color()),
                            );
                        }
                    });

                    // 状态指示
                    if let Some(ref status) = self.status {
                        ui.horizontal(|ui| {
                            ui.label(crate::i18n::tr(ui.ctx(), "Status:", "状态："));
                            if status.is_dirty {
                                crate::ui::icons::icon_label_row(
                                    ui,
                                    crate::ui::icons::IconId::Dot,
                                    crate::i18n::tr(ui.ctx(), "Dirty", "有更改"),
                                    10.0,
                                    4.0,
                                    |t| t.color(theme.amber_color()),
                                );
                            } else {
                                crate::ui::icons::icon_label_row(
                                    ui,
                                    crate::ui::icons::IconId::Dot,
                                    crate::i18n::tr(ui.ctx(), "Clean", "干净"),
                                    10.0,
                                    4.0,
                                    |t| t.color(theme.green_color()),
                                );
                            }
                        });
                    }
                });

                ui.add_space(theme.spacing_md());

                // 操作按钮
                ui.group(|ui| {
                    ui.label(egui::RichText::new(crate::i18n::tr(ui.ctx(), "Actions", "操作")).strong());
                    ui.horizontal(|ui| {
                        if crate::ui::chrome::panel_toolbar_icon_button(
                            ui,
                            theme,
                            crate::ui::icons::IconId::GitPull,
                            crate::i18n::tr(ui.ctx(), "Pull from remote", "从远程拉取更新"),
                        )
                        .clicked()
                        {
                            self.pull();
                        }
                        if crate::ui::chrome::panel_toolbar_icon_button(
                            ui,
                            theme,
                            crate::ui::icons::IconId::GitPush,
                            crate::i18n::tr(ui.ctx(), "Push to remote", "推送到远程"),
                        )
                        .clicked()
                        {
                            self.push();
                        }
                        if crate::ui::chrome::panel_toolbar_icon_button(
                            ui,
                            theme,
                            crate::ui::icons::IconId::GitCommit,
                            crate::i18n::tr(ui.ctx(), "Commit changes", "提交更改"),
                        )
                        .clicked()
                        {
                            self.commit();
                        }
                    });
                });

                ui.add_space(theme.spacing_md());

                // 提交信息输入
                ui.group(|ui| {
                    crate::ui::chrome::form_field_label(ui, theme, crate::i18n::tr(ui.ctx(), "Commit message", "提交信息"));
                    let msg_w = layout_util::finite_content_width_inset(ui, 0.0, 200.0, ui.available_width());
                    crate::ui::chrome::form_singleline_field(
                        ui,
                        theme,
                        egui::Id::new("git_sync_commit_msg"),
                        &mut self.commit_message,
                        crate::i18n::tr(ui.ctx(), "Enter commit message…", "输入提交信息…"),
                        msg_w,
                        false,
                    );
                    ui.horizontal(|ui| {
                        if crate::ui::chrome::panel_action_icon_button(ui, theme, crate::ui::icons::IconId::Check, crate::i18n::tr(ui.ctx(), "Stage all", "暂存全部"))
                            .clicked() {
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
                });

                ui.add_space(theme.spacing_md());

                // 操作状态显示
                match &self.operation_status {
                    OperationStatus::Idle => {}
                    OperationStatus::Loading => {
                        crate::ui::chrome::busy_row(ui, theme, crate::i18n::tr(ui.ctx(), "Working…", "操作中…"));
                    }
                    OperationStatus::Success(msg) => {
                        crate::ui::icons::icon_label_row(
                            ui,
                            crate::ui::icons::IconId::Check,
                            msg,
                            theme.size_icon_glyph(),
                            6.0,
                            |t| t.color(theme.green_color()),
                        );
                    }
                    OperationStatus::Error(msg) => {
                        crate::ui::icons::icon_label_row(
                            ui,
                            crate::ui::icons::IconId::Cross,
                            msg,
                            theme.size_icon_glyph(),
                            6.0,
                            |t| t.color(theme.red_color()),
                        );
                    }
                }

                // 状态消息
                if !self.status_message.is_empty() {
                    ui.label(
                        egui::RichText::new(&self.status_message)
                            .color(theme.text_tertiary()),
                    );
                }

                // 错误消息
                if !self.error_message.is_empty() {
                    ui.colored_label(theme.red_color(), &self.error_message);
                }
            } else {
                // 未打开仓库
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    crate::ui::icons::icon_label_row(
                        ui,
                        crate::ui::icons::IconId::Package,
                        crate::i18n::tr(ui.ctx(), "No Git repository open", "未打开 Git 仓库"),
                        theme.font_size_empty_state(),
                        8.0,
                        |t| t.size(theme.font_size_empty_state()),
                    );
                    ui.add_space(theme.spacing_list_item_x());
                    ui.label(crate::i18n::tr(ui.ctx(), "Enter a repository path below or clone a new one.", "请输入仓库路径或克隆一个新仓库"));
                    ui.add_space(theme.spacing_list_item_x());
                    if crate::ui::chrome::panel_action_icon_button(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Package,
                        crate::i18n::tr(ui.ctx(), "Clone repository…", "克隆仓库…"),
                    )
                    .clicked() {
                        self.show_clone_dialog = true;
                    }
                });
            }

            // 克隆对话框
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
        });
    }
}

impl Default for GitSyncPanel {
    fn default() -> Self {
        Self::new()
    }
}