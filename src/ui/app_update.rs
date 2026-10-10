//! 自动更新的界面部分：后台定时检查、右下角提醒、「软件更新」窗口、偏好设置与「关于」里的入口。
//!
//! 规则（与 docs/release/AUTO_UPDATE.md 一致）：
//! - 默认启动 10 秒后检查一次，之后每 24 小时（加一点随机间隔）检查一次；偏好设置里可关闭。
//! - 默认后台先下载；安装必须用户点按钮；装失败再给「打开下载页」；**绝不自动重启**。
//! - 网络请求和文件操作都在后台线程，界面线程只收消息。

use super::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

use crate::core::updater::apply::{self, ApplyOutcome, ApplyStage};
use crate::core::updater::{
    self, CheckOptions, CheckOutcome, InstallPlan, UpdateError, UpdateInfo, UpdateState,
};

const FIRST_CHECK_DELAY: Duration = Duration::from_secs(10);
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 3600);
const CHECK_JITTER_SECS: u64 = 3600;
/// 自动检查失败后，隔一段时间再试（不弹任何提示）。
const RETRY_AFTER_FAILURE: Duration = Duration::from_secs(6 * 3600);

enum Msg {
    Checked {
        manual: bool,
        result: Result<CheckOutcome, UpdateError>,
    },
    Progress(ApplyStage),
    Applied(Result<ApplyOutcome, UpdateError>),
    Prefetched(Result<(), UpdateError>),
}

#[derive(Debug, Default)]
enum Phase {
    #[default]
    Idle,
    Checking,
    UpToDate(String),
    CheckFailed(UpdateError),
    Available,
    Working(Option<ApplyStage>),
    InstallFailed(UpdateError),
    /// 已装好，重启后生效。
    Installed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Confirm {
    Restart,
    Installer,
}

pub(crate) struct UpdateUi {
    phase: Phase,
    info: Option<Box<UpdateInfo>>,
    rx: Option<mpsc::Receiver<Msg>>,
    cancel: Arc<AtomicBool>,
    show_dialog: bool,
    next_auto_check: Instant,
    last_checked_unix: Option<i64>,
    /// 启动时记下的程序路径（重启时用；Linux 上替换后同一路径就是新版本）。
    startup_exe: Option<PathBuf>,
    /// 有新版本，等没有别的需要确认的提示时再弹出。
    notify_pending: bool,
    prefetched: bool,
    /// 后台下载正在进行（这时 `rx` 被它占着）。
    prefetching: bool,
    /// 后台下载中点了「安装」：下完接着装。
    install_after_prefetch: bool,
    confirm: Option<Confirm>,
    close_requested: bool,
}

impl UpdateUi {
    pub(crate) fn new() -> Self {
        let startup_exe = updater::paths::current_exe();
        let exe_for_cleanup = startup_exe.clone();
        let _ = std::thread::Builder::new()
            .name("mist-update-cleanup".into())
            .spawn(move || apply::cleanup_after_start(exe_for_cleanup.as_deref()));
        Self {
            phase: Phase::Idle,
            info: None,
            rx: None,
            cancel: Arc::new(AtomicBool::new(false)),
            show_dialog: false,
            next_auto_check: Instant::now() + first_check_delay(),
            last_checked_unix: UpdateState::load().last_check_unix,
            startup_exe,
            notify_pending: false,
            prefetched: false,
            prefetching: false,
            install_after_prefetch: false,
            confirm: None,
            close_requested: false,
        }
    }

    pub(crate) fn dialog_open(&self) -> bool {
        self.show_dialog
    }

    fn busy(&self) -> bool {
        self.rx.is_some()
    }

    /// 菜单「检查更新」时后台已有任务在跑。
    fn check_now_while_busy(&mut self) {
        // 后台下载占着通道时不能再检查；刚查到的新版本就是最新结果，窗口直接显示它（含「正在下载」）。
        if !self.prefetching {
            self.phase = Phase::Checking;
        }
    }

    /// 后台下载中点了「安装」：返回 true 表示已记下，下完接着装。
    fn queue_install_during_prefetch(&mut self) -> bool {
        if self.prefetching {
            self.install_after_prefetch = true;
        }
        self.prefetching
    }

    /// 后台下载结束（成功或失败）。返回 true 表示要接着安装。
    fn on_prefetch_finished(&mut self, ok: bool) -> bool {
        self.rx = None;
        self.prefetching = false;
        if ok {
            self.prefetched = true;
        }
        if matches!(self.phase, Phase::Checking) {
            self.phase = if self.info.is_some() {
                Phase::Available
            } else {
                Phase::Idle
            };
        }
        // 下载失败也照样去装：安装会自己重新下载，出错时显示原因和「打开下载页」。
        std::mem::take(&mut self.install_after_prefetch)
    }

    fn available_version(&self) -> Option<&str> {
        match self.phase {
            Phase::Available | Phase::Working(_) | Phase::InstallFailed(_) => {
                self.info.as_ref().map(|i| i.manifest.version.as_str())
            }
            _ => None,
        }
    }
}

fn first_check_delay() -> Duration {
    #[cfg(feature = "update-test")]
    if let Some(secs) = std::env::var("MIST_UPDATE_FIRST_CHECK_DELAY_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
    {
        return Duration::from_secs(secs);
    }
    FIRST_CHECK_DELAY
}

fn next_interval() -> Duration {
    use rand::Rng;
    CHECK_INTERVAL + Duration::from_secs(rand::thread_rng().gen_range(0..CHECK_JITTER_SECS))
}

fn is_zh(ctx: &egui::Context) -> bool {
    crate::i18n::language(ctx) == crate::i18n::UiLanguage::Zh
}

fn human_size(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    if bytes as f64 >= MB {
        format!("{:.1} MB", bytes as f64 / MB)
    } else {
        format!("{:.0} KB", (bytes as f64 / 1024.0).max(1.0))
    }
}

/// 更新说明只用到标题和列表，这里按行简单排版（左对齐，不解析链接/代码块）。
fn show_release_notes(ui: &mut egui::Ui, theme: &crate::ui::theme::Theme, notes: &str) {
    for raw in notes.lines() {
        let line = raw.trim();
        if line.is_empty() {
            ui.add_space(theme.spacing_xs());
            continue;
        }
        let heading = line
            .strip_prefix("### ")
            .or_else(|| line.strip_prefix("## "))
            .or_else(|| line.strip_prefix("# "));
        let text = match heading {
            Some(h) => egui::RichText::new(h.trim()).strong().color(theme.text_primary()),
            None => {
                let body = match line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
                    Some(item) => format!("• {}", item.trim()),
                    None => line.to_string(),
                };
                egui::RichText::new(body.replace("**", "").replace('`', ""))
                    .color(theme.text_secondary())
            }
        };
        ui.add(egui::Label::new(text).wrap(true));
    }
}

fn format_local_time(unix: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(unix, 0)
        .single()
        .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

impl MistTermApp {
    /// 每帧调用：收后台消息、到点自动检查、弹出提醒。
    pub(crate) fn update_tick(&mut self, ctx: &egui::Context) {
        self.update_poll(ctx);

        let auto = self.app_settings.update.auto_check && !updater::disabled_by_env();
        if auto {
            let now = Instant::now();
            if now >= self.update_ui.next_auto_check && !self.update_ui.busy() {
                self.update_ui.next_auto_check = now + next_interval();
                if !matches!(self.update_ui.phase, Phase::Working(_) | Phase::Installed(_)) {
                    self.update_start_check(ctx, false);
                }
            }
            let wait = self
                .update_ui
                .next_auto_check
                .saturating_duration_since(Instant::now());
            ctx.request_repaint_after(wait.max(Duration::from_secs(1)));
        }

        if self.update_ui.notify_pending && !self.has_pending_action_toast() {
            self.update_ui.notify_pending = false;
            if let Some(info) = self.update_ui.info.clone() {
                let zh = is_zh(ctx);
                let text = if zh {
                    format!("Mist {} 已发布，你现在用的是 {}。", info.manifest.version, info.current)
                } else {
                    format!(
                        "Mist {} is available. You're using {}.",
                        info.manifest.version, info.current
                    )
                };
                // 右上角的 × 就是「稍后」。
                self.push_action_toast_titled(
                    ToastKind::Info,
                    crate::i18n::tr(ctx, "Update available", "有新版本"),
                    text,
                    ToastAction::OpenUpdateDialog,
                    crate::i18n::tr(ctx, "View", "查看"),
                );
            }
        }
    }

    /// 需要退出（Windows 安装程序已启动 / 用户点了重启）时关闭窗口。
    pub(crate) fn update_handle_close(&mut self, frame: &mut eframe::Frame) {
        if self.update_ui.close_requested {
            self.update_ui.close_requested = false;
            frame.close();
        }
    }

    /// 菜单 / 关于 / 偏好设置里的「检查更新」。
    pub(crate) fn update_check_now(&mut self, ctx: &egui::Context) {
        self.update_ui.show_dialog = true;
        match self.update_ui.phase {
            // 正在下载 / 已装好：直接打开窗口看状态，不重新检查。
            Phase::Working(_) | Phase::Installed(_) => {}
            _ if self.update_ui.busy() => self.update_ui.check_now_while_busy(),
            _ => self.update_start_check(ctx, true),
        }
    }

    pub(crate) fn update_open_dialog(&mut self) {
        self.update_ui.show_dialog = true;
    }

    /// Help 菜单里的文字：有新版本时直接写出版本号。
    pub(crate) fn update_menu_label(&self, ctx: &egui::Context) -> String {
        if let Phase::Installed(v) = &self.update_ui.phase {
            return if is_zh(ctx) {
                format!("重启以使用 {v}…")
            } else {
                format!("Restart to Use {v}…")
            };
        }
        match self.update_ui.available_version() {
            Some(v) if is_zh(ctx) => format!("有新版本 {v}…"),
            Some(v) => format!("Update Available ({v})…"),
            None => crate::i18n::menu::labels(crate::i18n::language(ctx))
                .check_updates
                .to_string(),
        }
    }

    fn update_start_check(&mut self, ctx: &egui::Context, manual: bool) {
        if self.update_ui.busy() {
            return;
        }
        if manual {
            self.update_ui.phase = Phase::Checking;
        }
        let (tx, rx) = mpsc::channel();
        self.update_ui.rx = Some(rx);
        let channel = self.app_settings.update.channel;
        let ctx2 = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("mist-update-check".into())
            .spawn(move || {
                let mut state = UpdateState::load();
                let opts = CheckOptions::for_current_build(channel);
                let result = updater::check_for_update(&opts, &mut state);
                if result.is_ok() {
                    let _ = state.save();
                }
                let _ = tx.send(Msg::Checked { manual, result });
                ctx2.request_repaint();
            });
        if spawned.is_err() {
            self.update_ui.rx = None;
            if manual {
                self.update_ui.phase =
                    Phase::CheckFailed(UpdateError::Network("cannot start thread".into()));
            }
        }
    }

    fn update_start_install(&mut self, ctx: &egui::Context) {
        if self.update_ui.busy() {
            self.update_ui.queue_install_during_prefetch();
            return;
        }
        let Some(info) = self.update_ui.info.clone() else {
            return;
        };
        if !info.plan.is_auto() {
            return;
        }
        let Some(exe) = self.update_ui.startup_exe.clone() else {
            self.update_ui.phase =
                Phase::InstallFailed(UpdateError::Install("cannot locate the running program".into()));
            return;
        };
        self.update_ui.cancel.store(false, Ordering::SeqCst);
        self.update_ui.phase = Phase::Working(None);
        let (tx, rx) = mpsc::channel();
        self.update_ui.rx = Some(rx);
        let cancel = self.update_ui.cancel.clone();
        let ctx2 = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("mist-update-apply".into())
            .spawn(move || {
                let mut last_sent = Instant::now() - Duration::from_secs(1);
                let mut progress = |stage: ApplyStage| {
                    let throttled = matches!(stage, ApplyStage::Downloading { done, total } if done < total)
                        && last_sent.elapsed() < Duration::from_millis(100);
                    if !throttled {
                        last_sent = Instant::now();
                        let _ = tx.send(Msg::Progress(stage));
                        ctx2.request_repaint();
                    }
                };
                let result = apply::apply_update(&info, &exe, true, &mut progress, &cancel);
                let _ = tx.send(Msg::Applied(result));
                ctx2.request_repaint();
            });
        if spawned.is_err() {
            self.update_ui.rx = None;
            self.update_ui.phase =
                Phase::InstallFailed(UpdateError::Install("cannot start thread".into()));
        }
    }

    /// 偏好里打开了「后台提前下载」：只下载并校验，不安装。
    fn update_start_prefetch(&mut self, ctx: &egui::Context) {
        if self.update_ui.busy() || self.update_ui.prefetched {
            return;
        }
        let Some(info) = self.update_ui.info.clone() else {
            return;
        };
        let InstallPlan::Auto { asset, .. } = &info.plan else {
            return;
        };
        let asset = asset.clone();
        let version = info.manifest.version.clone();
        let (tx, rx) = mpsc::channel();
        self.update_ui.rx = Some(rx);
        let cancel = self.update_ui.cancel.clone();
        cancel.store(false, Ordering::SeqCst);
        let ctx2 = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("mist-update-prefetch".into())
            .spawn(move || {
                let result = updater::lock::UpdateLock::acquire().and_then(|_lock| {
                    apply::download_asset(&asset, &version, &mut |_| {}, &cancel).map(|_| ())
                });
                let _ = tx.send(Msg::Prefetched(result));
                ctx2.request_repaint();
            });
        if spawned.is_err() {
            self.update_ui.rx = None;
        } else {
            self.update_ui.prefetching = true;
        }
    }

    fn update_poll(&mut self, ctx: &egui::Context) {
        loop {
            let msg = match self.update_ui.rx.as_ref().map(|rx| rx.try_recv()) {
                Some(Ok(m)) => m,
                Some(Err(mpsc::TryRecvError::Empty)) | None => return,
                Some(Err(mpsc::TryRecvError::Disconnected)) => {
                    self.update_ui.rx = None;
                    if matches!(self.update_ui.phase, Phase::Checking) {
                        self.update_ui.phase = Phase::Idle;
                    }
                    return;
                }
            };
            match msg {
                Msg::Checked { manual, result } => {
                    self.update_ui.rx = None;
                    self.update_on_checked(ctx, manual, result);
                }
                Msg::Progress(stage) => {
                    if matches!(self.update_ui.phase, Phase::Working(_)) {
                        self.update_ui.phase = Phase::Working(Some(stage));
                    }
                }
                Msg::Applied(result) => {
                    self.update_ui.rx = None;
                    self.update_on_applied(ctx, result);
                }
                Msg::Prefetched(result) => {
                    if let Err(e) = &result {
                        log::info!("updater: background download skipped: {e}");
                    }
                    if self.update_ui.on_prefetch_finished(result.is_ok()) {
                        self.update_start_install(ctx);
                    }
                }
            }
        }
    }

    fn update_on_checked(
        &mut self,
        ctx: &egui::Context,
        manual: bool,
        result: Result<CheckOutcome, UpdateError>,
    ) {
        // 用户在检查过程中又点了「检查更新」：按手动检查展示结果。
        let manual = manual || (self.update_ui.show_dialog && matches!(self.update_ui.phase, Phase::Checking));
        let state = UpdateState::load();
        self.update_ui.last_checked_unix = state.last_check_unix;
        if matches!(self.update_ui.phase, Phase::Working(_) | Phase::Installed(_)) {
            return;
        }
        match result {
            Ok(CheckOutcome::UpToDate { current, .. }) => {
                self.update_ui.info = None;
                self.update_ui.phase = if manual {
                    Phase::UpToDate(current.to_string())
                } else {
                    Phase::Idle
                };
            }
            Ok(CheckOutcome::Available(info)) => {
                let version = info.manifest.version.clone();
                let same_as_before = self
                    .update_ui
                    .info
                    .as_ref()
                    .is_some_and(|old| old.manifest.version == version);
                if !same_as_before {
                    self.update_ui.prefetched = false;
                }
                let already_installed = state.installed_pending_restart.as_deref() == Some(version.as_str())
                    && version != updater::APP_VERSION;
                self.update_ui.info = Some(info);
                if already_installed {
                    // 例如在终端里用 `mist update` 装过了：只差重启。
                    self.update_ui.phase = Phase::Installed(version);
                    return;
                }
                self.update_ui.phase = Phase::Available;
                if !manual {
                    let skipped = state.skipped_version.as_deref() == Some(version.as_str());
                    if !skipped && !same_as_before {
                        self.update_ui.notify_pending = true;
                    }
                    if !skipped && self.app_settings.update.auto_download {
                        self.update_start_prefetch(ctx);
                    }
                }
            }
            Err(e) => {
                log::info!("updater: check failed: {e}");
                if manual {
                    self.update_ui.phase = Phase::CheckFailed(e);
                } else {
                    if matches!(self.update_ui.phase, Phase::Checking) {
                        self.update_ui.phase = Phase::Idle;
                    }
                    if !matches!(e, UpdateError::NoTrustedKeys | UpdateError::DisabledByEnv) {
                        self.update_ui.next_auto_check = self
                            .update_ui
                            .next_auto_check
                            .min(Instant::now() + RETRY_AFTER_FAILURE);
                    }
                }
            }
        }
    }

    fn update_on_applied(&mut self, ctx: &egui::Context, result: Result<ApplyOutcome, UpdateError>) {
        match result {
            Ok(ApplyOutcome::Installed { version, .. }) => {
                self.update_ui.phase = Phase::Installed(version.clone());
                if !self.update_ui.show_dialog {
                    let text = if is_zh(ctx) {
                        format!("Mist {version} 已经装好，重启 Mist 后生效。")
                    } else {
                        format!("Mist {version} is installed. Restart Mist to start using it.")
                    };
                    self.push_action_toast_titled(
                        ToastKind::Success,
                        crate::i18n::tr(ctx, "Update installed", "更新已装好"),
                        text,
                        ToastAction::OpenUpdateDialog,
                        crate::i18n::tr(ctx, "View", "查看"),
                    );
                }
            }
            Ok(ApplyOutcome::InstallerStarted { .. }) => {
                // 安装程序会关闭并重新打开 Mist；这里先自己退出。
                self.update_ui.close_requested = true;
                ctx.request_repaint();
            }
            Err(UpdateError::Cancelled) => {
                self.update_ui.phase = Phase::Available;
            }
            Err(e) => {
                log::warn!("updater: install failed: {e}");
                self.update_ui.phase = Phase::InstallFailed(e);
                self.update_ui.show_dialog = true;
            }
        }
    }

    fn update_connected_sessions(&self) -> usize {
        self.tabs
            .iter()
            .map(|t| t.panes.iter().filter(|p| p.terminal.is_connected()).count())
            .sum()
    }

    fn update_restart_now(&mut self, ctx: &egui::Context) {
        let Some(exe) = self.update_ui.startup_exe.clone() else {
            self.notify_error(crate::i18n::tr(
                ctx,
                "Couldn't restart Mist. Please close it and open it again.",
                "无法自动重启，请手动关闭再打开 Mist。",
            ));
            return;
        };
        let mut cmd = std::process::Command::new(&exe);
        match apply::spawn_detached(&mut cmd) {
            Ok(()) => {
                self.update_ui.close_requested = true;
                ctx.request_repaint();
            }
            Err(e) => {
                log::warn!("updater: restart failed: {e}");
                self.notify_error(crate::i18n::tr(
                    ctx,
                    "Couldn't restart Mist. Please close it and open it again.",
                    "无法自动重启，请手动关闭再打开 Mist。",
                ));
            }
        }
    }

    fn update_skip_version(&mut self) {
        if let Some(info) = &self.update_ui.info {
            let mut state = UpdateState::load();
            state.skipped_version = Some(info.manifest.version.clone());
            let _ = state.save();
        }
        self.update_ui.show_dialog = false;
        self.update_ui.phase = Phase::Idle;
    }

    fn open_download_page(&mut self, ctx: &egui::Context, url: &str) {
        if !crate::platform::open_url(url) {
            self.notify_auto(
                crate::i18n::tr(ctx, "Failed to open browser", "无法打开浏览器").to_string(),
            );
        }
    }

    fn update_download_url(&self) -> String {
        match self.update_ui.info.as_ref().map(|i| &i.plan) {
            Some(InstallPlan::Manual { download_url, .. }) => download_url.clone(),
            _ => updater::DOWNLOAD_PAGE_URL.to_string(),
        }
    }

    /// 「软件更新」窗口。
    pub(crate) fn render_update_dialog(
        &mut self,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
    ) {
        if !self.update_ui.show_dialog {
            return;
        }
        let zh = is_zh(ctx);
        let mut open = true;
        let mut should_close = false;
        let title = crate::i18n::tr(ctx, "Software Update", "软件更新");
        let modal_sz = egui::vec2(560.0, 440.0);
        #[derive(PartialEq)]
        enum Act {
            None,
            Retry,
            Install,
            Cancel,
            Skip,
            Later,
            OpenDownload,
            OpenNotes(String),
            Restart,
            ConfirmYes,
            ConfirmNo,
        }
        let mut act = Act::None;
        let confirm = self.update_ui.confirm;
        let connected = self.update_connected_sessions();
        let info = self.update_ui.info.clone();
        let prefetched = self.update_ui.prefetched;
        let prefetching = self.update_ui.prefetching;
        let install_queued = self.update_ui.install_after_prefetch;

        crate::ui::chrome::modal_window("update_modal", theme, ctx)
            .open(&mut open)
            .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
            .default_size(modal_sz)
            .movable(true)
            .resizable(true)
            .show(ctx, |ui| {
                crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                    if crate::ui::chrome::modal_header(ui, theme, title, theme.font_size_modal_title()) {
                        should_close = true;
                    }
                    let body = |ui: &mut egui::Ui, text: &str| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(text)
                                    .size(theme.font_size_panel_title())
                                    .color(theme.text_secondary()),
                            )
                            .wrap(true),
                        );
                    };
                    let hint = |ui: &mut egui::Ui, text: &str| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(text)
                                    .size(theme.font_size_caption())
                                    .color(theme.text_tertiary()),
                            )
                            .wrap(true),
                        );
                    };

                    if let Some(c) = confirm {
                        let text = match c {
                            Confirm::Restart if zh => format!("当前有 {connected} 个已连接的 SSH 会话，重启会断开它们。确定现在重启吗？"),
                            Confirm::Restart => format!("{connected} SSH session(s) are connected. Restarting will disconnect them. Restart now?"),
                            Confirm::Installer if zh => format!("当前有 {connected} 个已连接的 SSH 会话。安装程序会关闭 Mist，这些会话会断开。确定继续吗？"),
                            Confirm::Installer => format!("{connected} SSH session(s) are connected. The installer will close Mist and disconnect them. Continue?"),
                        };
                        body(ui, &text);
                        ui.add_space(theme.spacing_md());
                        crate::ui::chrome::modal_footer_actions(ui, theme, |ui, theme| {
                            let yes = match c {
                                Confirm::Restart => crate::i18n::tr(ctx, "Restart Anyway", "仍然重启"),
                                Confirm::Installer => crate::i18n::tr(ctx, "Continue", "继续"),
                            };
                            if crate::ui::chrome::modal_primary_button(ui, theme, yes).clicked() {
                                act = Act::ConfirmYes;
                            }
                            if crate::ui::chrome::modal_secondary_button(ui, theme, crate::i18n::tr(ctx, "Cancel", "取消")).clicked() {
                                act = Act::ConfirmNo;
                            }
                        });
                        return;
                    }

                    match &self.update_ui.phase {
                        Phase::Idle | Phase::Checking => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                body(ui, crate::i18n::tr(ctx, "Checking for updates…", "正在检查更新…"));
                            });
                        }
                        Phase::UpToDate(v) => {
                            let text = if zh {
                                format!("已是最新版本（{v}）。")
                            } else {
                                format!("You're up to date. Mist {v} is the latest version.")
                            };
                            body(ui, &text);
                            ui.add_space(theme.spacing_md());
                            crate::ui::chrome::modal_footer_actions(ui, theme, |ui, theme| {
                                if crate::ui::chrome::modal_primary_button(ui, theme, crate::i18n::tr(ctx, "OK", "好")).clicked() {
                                    act = Act::Later;
                                }
                            });
                        }
                        Phase::CheckFailed(e) => {
                            body(ui, &e.user_message(zh));
                            ui.add_space(theme.spacing_md());
                            crate::ui::chrome::modal_footer_actions(ui, theme, |ui, theme| {
                                if crate::ui::chrome::modal_primary_button(ui, theme, crate::i18n::tr(ctx, "Try Again", "重试")).clicked() {
                                    act = Act::Retry;
                                }
                                if crate::ui::chrome::modal_secondary_button(ui, theme, crate::i18n::tr(ctx, "Open Download Page", "打开下载页")).clicked() {
                                    act = Act::OpenDownload;
                                }
                                if crate::ui::chrome::modal_secondary_button(ui, theme, crate::i18n::tr(ctx, "Close", "关闭")).clicked() {
                                    act = Act::Later;
                                }
                            });
                        }
                        Phase::Available | Phase::InstallFailed(_) => {
                            let Some(info) = info.as_ref() else {
                                return;
                            };
                            let headline = if zh {
                                format!("Mist {} 已发布，你现在用的是 {}。", info.manifest.version, info.current)
                            } else {
                                format!("Mist {} is available. You're using {}.", info.manifest.version, info.current)
                            };
                            ui.label(
                                egui::RichText::new(headline)
                                    .size(theme.font_size_prominent())
                                    .color(theme.color_body_text_muted()),
                            );
                            if let Some(size) = info.plan.download_size() {
                                let line = if prefetched {
                                    if zh { format!("下载大小：{}（已提前下载好）", human_size(size)) } else { format!("Download size: {} (already downloaded)", human_size(size)) }
                                } else if zh {
                                    format!("下载大小：{}", human_size(size))
                                } else {
                                    format!("Download size: {}", human_size(size))
                                };
                                hint(ui, &line);
                            }
                            if prefetching {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    let text = if install_queued {
                                        crate::i18n::tr(ctx, "Downloading the update… It will install when the download finishes.", "正在下载更新…下载完会接着安装。")
                                    } else {
                                        crate::i18n::tr(ctx, "Downloading the update in the background…", "正在后台下载更新…")
                                    };
                                    body(ui, text);
                                });
                            }
                            ui.add_space(theme.spacing_sm());
                            let notes = if zh { &info.manifest.notes.zh } else { &info.manifest.notes.en };
                            let notes = if notes.trim().is_empty() {
                                if zh { &info.manifest.notes.en } else { &info.manifest.notes.zh }
                            } else {
                                notes
                            };
                            if !notes.trim().is_empty() {
                                ui.label(
                                    egui::RichText::new(crate::i18n::tr(ctx, "What's new", "更新内容"))
                                        .strong()
                                        .color(theme.text_secondary()),
                                );
                                egui::Frame::none()
                                    .stroke(egui::Stroke::new(1.0_f32, theme.color_overlay_fill_subtle()))
                                    .rounding(theme.radius_list_item())
                                    .inner_margin(egui::Margin::same(theme.spacing_sm()))
                                    .show(ui, |ui| {
                                        egui::ScrollArea::vertical()
                                            .max_height(180.0)
                                            .auto_shrink([false, true])
                                            .show(ui, |ui| {
                                                ui.set_width(ui.available_width());
                                                show_release_notes(ui, theme, notes);
                                            });
                                    });
                            }
                            let notes_url = info
                                .manifest
                                .notes_url
                                .clone()
                                .unwrap_or_else(|| format!("{}/tag/v{}", updater::RELEASES_URL, info.manifest.version));
                            if ui
                                .link(crate::i18n::tr(ctx, "Full release notes", "查看完整更新说明"))
                                .clicked()
                            {
                                act = Act::OpenNotes(notes_url);
                            }
                            ui.add_space(theme.spacing_sm());

                            if let Phase::InstallFailed(e) = &self.update_ui.phase {
                                let text = if zh {
                                    format!("更新没有装上：{}当前版本不受影响，可以继续使用。", e.user_message(true))
                                } else {
                                    format!("The update wasn't installed: {} Your current version still works.", e.user_message(false))
                                };
                                ui.add(
                                    egui::Label::new(egui::RichText::new(text).color(theme.red_color()))
                                        .wrap(true),
                                );
                                ui.add_space(theme.spacing_sm());
                            }

                            match &info.plan {
                                InstallPlan::Auto { .. } if info.plan.uses_installer() => {
                                    hint(ui, crate::i18n::tr(
                                        ctx,
                                        "The installer will close Mist (connected SSH sessions will be disconnected), update it, and open it again.",
                                        "安装程序会先关闭 Mist（已连接的 SSH 会话会断开），装好后自动重新打开。",
                                    ));
                                }
                                InstallPlan::Auto { .. } => {
                                    hint(ui, crate::i18n::tr(
                                        ctx,
                                        "Mist downloads the new version, checks that the file is intact, then replaces the program. Sessions and settings are kept. The new version is used after you restart Mist.",
                                        "Mist 会下载新版本，确认文件完好后再替换程序。会话和设置都会保留，重启 Mist 后生效。",
                                    ));
                                }
                                InstallPlan::Manual { reason, .. } => {
                                    body(ui, &reason.user_message(zh));
                                }
                            }
                            ui.add_space(theme.spacing_md());
                            let auto = info.plan.is_auto();
                            let installer = info.plan.uses_installer();
                            let install_failed =
                                matches!(self.update_ui.phase, Phase::InstallFailed(_));
                            let offer_manual =
                                update_dialog_offer_manual_download(auto, install_failed);
                            crate::ui::chrome::modal_footer_actions(ui, theme, |ui, theme| {
                                if auto {
                                    let label = if install_failed {
                                        crate::i18n::tr(ctx, "Try Again", "再试一次")
                                    } else if installer {
                                        crate::i18n::tr(ctx, "Install and Restart", "安装并重启")
                                    } else {
                                        crate::i18n::tr(ctx, "Install Update", "安装更新")
                                    };
                                    if crate::ui::chrome::modal_primary_button(ui, theme, label).clicked() {
                                        act = Act::Install;
                                    }
                                } else if crate::ui::chrome::modal_primary_button(ui, theme, crate::i18n::tr(ctx, "Open Download Page", "打开下载页")).clicked() {
                                    act = Act::OpenDownload;
                                }
                                // Auto 安装失败，或本来就是 Manual：给出/保留手动下载
                                if auto
                                    && offer_manual
                                    && crate::ui::chrome::modal_secondary_button(
                                        ui,
                                        theme,
                                        crate::i18n::tr(ctx, "Open Download Page", "打开下载页"),
                                    )
                                    .clicked()
                                {
                                    act = Act::OpenDownload;
                                }
                                if crate::ui::chrome::modal_secondary_button(ui, theme, crate::i18n::tr(ctx, "Later", "稍后")).clicked() {
                                    act = Act::Later;
                                }
                                if crate::ui::chrome::modal_secondary_button(ui, theme, crate::i18n::tr(ctx, "Skip This Version", "跳过这个版本")).clicked() {
                                    act = Act::Skip;
                                }
                            });
                        }
                        Phase::Working(stage) => {
                            let (text, fraction, cancellable) = match stage {
                                None => (crate::i18n::tr(ctx, "Starting…", "准备中…").to_string(), None, true),
                                Some(ApplyStage::Downloading { done, total }) => {
                                    let frac = if *total > 0 { Some(*done as f32 / *total as f32) } else { None };
                                    let t = if zh {
                                        format!("正在下载… {} / {}", human_size(*done), human_size(*total))
                                    } else {
                                        format!("Downloading… {} / {}", human_size(*done), human_size(*total))
                                    };
                                    (t, frac, true)
                                }
                                Some(ApplyStage::Verifying) => (crate::i18n::tr(ctx, "Checking the file…", "正在检查文件…").to_string(), Some(1.0), false),
                                Some(ApplyStage::Installing) => (crate::i18n::tr(ctx, "Installing…", "正在安装…").to_string(), None, false),
                            };
                            body(ui, &text);
                            ui.add_space(theme.spacing_sm());
                            match fraction {
                                Some(f) => {
                                    ui.add(egui::ProgressBar::new(f.clamp(0.0, 1.0)).show_percentage());
                                }
                                None => {
                                    ui.spinner();
                                }
                            }
                            ui.add_space(theme.spacing_md());
                            if cancellable {
                                crate::ui::chrome::modal_footer_actions(ui, theme, |ui, theme| {
                                    if crate::ui::chrome::modal_secondary_button(ui, theme, crate::i18n::tr(ctx, "Cancel", "取消")).clicked() {
                                        act = Act::Cancel;
                                    }
                                });
                            }
                        }
                        Phase::Installed(v) => {
                            let text = if zh {
                                format!("Mist {v} 已经装好，重启 Mist 后生效。")
                            } else {
                                format!("Mist {v} is installed. Restart Mist to start using it.")
                            };
                            body(ui, &text);
                            hint(ui, crate::i18n::tr(
                                ctx,
                                "No rush: the new version opens the next time you start Mist.",
                                "不急着重启也可以，下次打开 Mist 就是新版本。",
                            ));
                            ui.add_space(theme.spacing_md());
                            crate::ui::chrome::modal_footer_actions(ui, theme, |ui, theme| {
                                if crate::ui::chrome::modal_primary_button(ui, theme, crate::i18n::tr(ctx, "Restart Now", "立即重启")).clicked() {
                                    act = Act::Restart;
                                }
                                if crate::ui::chrome::modal_secondary_button(ui, theme, crate::i18n::tr(ctx, "Later", "稍后")).clicked() {
                                    act = Act::Later;
                                }
                            });
                        }
                    }
                });
            });

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && confirm.is_none() {
            should_close = true;
        }

        match act {
            Act::None => {}
            Act::Retry => {
                if matches!(self.update_ui.phase, Phase::InstallFailed(_)) {
                    self.update_start_install(ctx);
                } else {
                    self.update_start_check(ctx, true);
                }
            }
            Act::Install => {
                if self.update_ui.info.as_ref().is_some_and(|i| i.plan.uses_installer()) && connected > 0 {
                    self.update_ui.confirm = Some(Confirm::Installer);
                } else {
                    self.update_start_install(ctx);
                }
            }
            Act::Cancel => self.update_ui.cancel.store(true, Ordering::SeqCst),
            Act::Skip => self.update_skip_version(),
            Act::Later => should_close = true,
            Act::OpenDownload => {
                let url = self.update_download_url();
                self.open_download_page(ctx, &url);
            }
            Act::OpenNotes(url) => self.open_download_page(ctx, &url),
            Act::Restart => {
                if connected > 0 {
                    self.update_ui.confirm = Some(Confirm::Restart);
                } else {
                    self.update_restart_now(ctx);
                }
            }
            Act::ConfirmYes => {
                let c = self.update_ui.confirm.take();
                match c {
                    Some(Confirm::Restart) => self.update_restart_now(ctx),
                    Some(Confirm::Installer) => self.update_start_install(ctx),
                    None => {}
                }
            }
            Act::ConfirmNo => self.update_ui.confirm = None,
        }

        if !open || should_close {
            self.update_ui.show_dialog = false;
            self.update_ui.confirm = None;
            // 关掉窗口不影响后台下载；结果出来后会再提醒。
            if matches!(
                self.update_ui.phase,
                Phase::UpToDate(_) | Phase::CheckFailed(_) | Phase::Checking
            ) && !self.update_ui.busy()
            {
                self.update_ui.phase = Phase::Idle;
            }
        }
    }

    /// 「关于」窗口里的一行：上次检查时间 + 检查按钮。
    pub(crate) fn update_about_row(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, theme: &crate::ui::theme::Theme) {
        let zh = is_zh(ctx);
        let status = match (&self.update_ui.phase, self.update_ui.last_checked_unix) {
            (Phase::Installed(v), _) => {
                if zh { format!("{v} 已装好，重启后生效") } else { format!("{v} installed, restart to use it") }
            }
            _ if self.update_ui.available_version().is_some() => {
                let v = self.update_ui.available_version().unwrap_or_default();
                if zh { format!("有新版本 {v}") } else { format!("Version {v} is available") }
            }
            (_, Some(t)) => {
                if zh { format!("上次检查更新：{}", format_local_time(t)) } else { format!("Last checked for updates: {}", format_local_time(t)) }
            }
            (_, None) => crate::i18n::tr(ctx, "Not checked for updates yet", "还没有检查过更新").to_string(),
        };
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(status)
                    .size(theme.font_size_caption())
                    .color(theme.color_form_hint()),
            );
            let has_news = matches!(self.update_ui.phase, Phase::Installed(_))
                || self.update_ui.available_version().is_some();
            let link = if has_news {
                crate::i18n::tr(ctx, "View…", "查看…").to_string()
            } else {
                crate::i18n::menu::labels(crate::i18n::language(ctx))
                    .check_updates
                    .to_string()
            };
            if ui.link(link).clicked() {
                self.update_check_now(ctx);
            }
        });
    }

    /// 偏好设置 → 常规 → 更新。
    pub(crate) fn update_preferences_section(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
        text_low: egui::Color32,
    ) {
        crate::ui::chrome::form_field_label(ui, theme, crate::i18n::tr(ctx, "Updates", "更新"));
        let disabled_by_env = updater::disabled_by_env();
        let mut changed = false;
        ui.add_enabled_ui(!disabled_by_env, |ui| {
            let mut auto_check = self.app_settings.update.auto_check;
            if crate::ui::chrome::form_checkbox(
                ui,
                theme,
                &mut auto_check,
                crate::i18n::tr(ctx, "Check for updates automatically (once a day)", "自动检查更新（每天一次）"),
            )
            .changed()
            {
                self.app_settings.update.auto_check = auto_check;
                changed = true;
            }
            let mut auto_download = self.app_settings.update.auto_download;
            if crate::ui::chrome::form_checkbox(
                ui,
                theme,
                &mut auto_download,
                crate::i18n::tr(ctx, "Download new versions in the background", "在后台提前下载新版本"),
            )
            .changed()
            {
                self.app_settings.update.auto_download = auto_download;
                changed = true;
            }
        });
        if changed {
            let _ = self.app_settings.save();
            if self.app_settings.update.auto_check {
                // 刚打开自动检查：稍后检查一次。
                let soon = Instant::now() + FIRST_CHECK_DELAY;
                self.update_ui.next_auto_check = self.update_ui.next_auto_check.min(soon);
            }
        }
        let small = |ui: &mut egui::Ui, text: &str| {
            ui.add(
                egui::Label::new(egui::RichText::new(text).size(theme.font_size_small()).color(text_low))
                    .wrap(true),
            );
        };
        if disabled_by_env {
            small(ui, crate::i18n::tr(
                ctx,
                "Update checks are turned off on this computer (MIST_DISABLE_UPDATE_CHECK).",
                "这台电脑关闭了检查更新（MIST_DISABLE_UPDATE_CHECK）。",
            ));
        }
        small(ui, crate::i18n::tr(
            ctx,
            "Installing always waits for your click, and Mist never restarts on its own. Channel: stable.",
            "安装前都会先问你，Mist 也不会自己重启。更新渠道：稳定版。",
        ));
        small(ui, crate::i18n::tr(
            ctx,
            "Checking sends one request to mistlab.dev and GitHub with only the Mist version and system type, no account or device information. Like any website, those servers can see your IP address.",
            "检查更新只会向 mistlab.dev 和 GitHub 发一次请求，只带 Mist 版本号和系统类型，不带账号或设备信息。和访问任何网站一样，对方能看到你的 IP 地址。",
        ));
        ui.add_space(theme.spacing_sm());
        self.update_about_row(ui, ctx, theme);
    }
}

/// 更新对话框是否额外提供「打开下载页」：Manual 主按钮已是下载页；Auto 仅安装失败时追加。
fn update_dialog_offer_manual_download(auto_plan: bool, install_failed: bool) -> bool {
    auto_plan && install_failed
}

#[cfg(test)]
mod update_dialog_tests {
    use super::*;

    #[test]
    fn auto_plan_adds_manual_download_only_after_install_fails() {
        assert!(!update_dialog_offer_manual_download(true, false));
        assert!(update_dialog_offer_manual_download(true, true));
        // Manual 计划由主按钮打开下载页，不再额外追加
        assert!(!update_dialog_offer_manual_download(false, false));
        assert!(!update_dialog_offer_manual_download(false, true));
    }

    fn test_ui(phase: Phase) -> UpdateUi {
        let json = crate::core::updater::manifest::tests::sample_json("9.9.9", "2026-10-10T00:00:00Z");
        let manifest = serde_json::from_str(&json).unwrap();
        UpdateUi {
            phase,
            info: Some(Box::new(UpdateInfo {
                current: semver::Version::new(1, 2, 5),
                manifest,
                source_url: String::new(),
                plan: InstallPlan::Manual {
                    reason: updater::check::ManualReason::SourceBuild,
                    asset: None,
                    download_url: String::new(),
                },
            })),
            rx: None,
            cancel: Arc::new(AtomicBool::new(false)),
            show_dialog: true,
            next_auto_check: Instant::now(),
            last_checked_unix: None,
            startup_exe: None,
            notify_pending: false,
            prefetched: false,
            prefetching: true,
            install_after_prefetch: false,
            confirm: None,
            close_requested: false,
        }
    }

    /// 后台下载中点「检查更新」，下载结束后窗口要回到「有新版本」（带安装按钮），不能一直转圈。
    #[test]
    fn manual_check_during_background_download_does_not_spin_forever() {
        for ok in [true, false] {
            let mut ui = test_ui(Phase::Available);
            ui.check_now_while_busy();
            assert!(!ui.on_prefetch_finished(ok));
            assert!(
                matches!(ui.phase, Phase::Available),
                "download ok={ok}: phase stays {:?}",
                ui.phase
            );
            assert!(!ui.prefetching);
            assert_eq!(ui.prefetched, ok);
        }
        // 即使已经处于「正在检查」（例如旧逻辑留下的），下载结束也要回到「有新版本」。
        let mut ui = test_ui(Phase::Checking);
        ui.on_prefetch_finished(true);
        assert!(matches!(ui.phase, Phase::Available));
    }

    /// 后台下载中点「安装」：先记下（窗口显示正在下载），下载结束后接着安装；下载失败也照样去装（安装会自己重新下载并报错）。
    #[test]
    fn install_click_during_background_download_installs_when_done() {
        for ok in [true, false] {
            let mut ui = test_ui(Phase::Available);
            assert!(ui.queue_install_during_prefetch(), "click must be remembered");
            assert!(ui.install_after_prefetch);
            assert!(ui.on_prefetch_finished(ok), "download ok={ok}: must go on to install");
            assert!(!ui.install_after_prefetch);
        }
        // 没在后台下载时（例如正在检查），点安装不排队。
        let mut ui = test_ui(Phase::Available);
        ui.prefetching = false;
        assert!(!ui.queue_install_during_prefetch());
        assert!(!ui.on_prefetch_finished(true));
    }
}
