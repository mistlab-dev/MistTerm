//! 远端 SFTP 侧栏：浏览、上传到当前目录、下载到本地、新建目录与删除。
//!
//! 网络与 SFTP 在后台线程执行，通过 `Receiver` 回传结果，避免阻塞 egui。

use crate::core::{AuditCategory, AuditEvent, AuditLogger, AuditOutcome};
use crate::ssh::SshSessionId;
use crate::ssh::{SftpClient, SftpEntry, SshManager};
use crate::i18n::UiLanguage;
use crate::ui::terminal::TerminalView;
use crate::ui::layout_util;
use crate::ui::theme::Theme;
use eframe::egui;
use rfd::FileDialog;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;

enum SftpJobResult {
    Listed {
        dir: PathBuf,
        result: Result<Vec<SftpEntry>, String>,
    },
    Msg(Result<String, String>),
}

pub struct SftpPanel {
    cwd: PathBuf,
    entries: Vec<SftpEntry>,
    path_edit: String,
    selected: Option<PathBuf>,
    list_err: Option<String>,
    toast_ok: Option<String>,
    toast_err: Option<String>,
    busy: bool,
    rx: Option<Receiver<SftpJobResult>>,
    mkdir_name: String,
    pending_delete: Option<PathBuf>,
    pending_refresh_after_op: bool,
    /// 面板打开后与切换标签时为 true，触发一次列表加载
    pending_auto_list: bool,
    /// 后台操作成功后待写入审计
    pending_audit: Option<(&'static str, String)>,
    /// 右 dock 槽位（用于 Central 之后前景重绘）
    last_panel_slot_rect: Option<egui::Rect>,
}

impl Default for SftpPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl SftpPanel {
    pub fn new() -> Self {
        Self {
            cwd: PathBuf::from("."),
            entries: Vec::new(),
            path_edit: ".".to_string(),
            selected: None,
            list_err: None,
            toast_ok: None,
            toast_err: None,
            busy: false,
            rx: None,
            mkdir_name: String::new(),
            pending_delete: None,
            pending_refresh_after_op: false,
            pending_auto_list: false,
            pending_audit: None,
            last_panel_slot_rect: None,
        }
    }

    pub fn request_list_on_open(&mut self) {
        self.pending_auto_list = true;
    }

    pub fn reset(&mut self) {
        self.cwd = PathBuf::from(".");
        self.entries.clear();
        self.path_edit = ".".to_string();
        self.selected = None;
        self.list_err = None;
        self.toast_ok = None;
        self.toast_err = None;
        self.busy = false;
        self.rx = None;
        self.mkdir_name.clear();
        self.pending_delete = None;
        self.pending_refresh_after_op = false;
        self.pending_auto_list = false;
        self.pending_audit = None;
        self.last_panel_slot_rect = None;
    }

    fn poll_rx(&mut self, audit: &AuditLogger, lang: UiLanguage) {
        let Some(rx) = &self.rx else {
            return;
        };
        match rx.try_recv() {
            Ok(SftpJobResult::Listed { dir, result }) => {
                match result {
                    Ok(entries) => {
                        self.entries = entries;
                        self.cwd = dir;
                        self.path_edit = self.cwd.to_string_lossy().to_string();
                        self.list_err = None;
                    }
                    Err(e) => {
                        self.list_err = Some(e);
                    }
                }
                self.busy = false;
                self.rx = None;
            }
            Ok(SftpJobResult::Msg(result)) => {
                match result {
                    Ok(msg) => {
                        if let Some((action, resource)) = self.pending_audit.take() {
                            audit.record(
                                AuditEvent::new(
                                    AuditCategory::Session,
                                    action,
                                    AuditOutcome::Success,
                                )
                                .with_resource(resource),
                            );
                        }
                        self.toast_ok = Some(msg);
                        self.pending_refresh_after_op = true;
                    }
                    Err(e) => {
                        if let Some((action, resource)) = self.pending_audit.take() {
                            audit.record(
                                AuditEvent::new(
                                    AuditCategory::Session,
                                    action,
                                    AuditOutcome::Failure,
                                )
                                .with_resource(resource)
                                .with_detail(serde_json::json!({ "error": e })),
                            );
                        }
                        self.toast_err = Some(e);
                    }
                }
                self.busy = false;
                self.rx = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.busy = false;
                self.rx = None;
                self.toast_err = Some(
                    crate::i18n::Locale::from(lang)
                        .tr(
                            "SFTP background worker stopped unexpectedly",
                            "SFTP 后台任务异常中断",
                        )
                        .to_string(),
                );
            }
        }
    }

    fn spawn_list(&mut self, sid: SshSessionId, mgr: SshManager, dir: PathBuf, ctx: &egui::Context) {
        if self.busy {
            return;
        }
        self.busy = true;
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let ctx = ctx.clone();
        let lang = crate::i18n::language(&ctx);
        thread::spawn(move || {
            let loc = crate::i18n::Locale::from(lang);
            let result = (|| -> Result<Vec<SftpEntry>, String> {
                let session = mgr
                    .get_session(sid)
                    .ok_or_else(|| {
                        loc.tr("SSH session unavailable", "SSH 会话不可用")
                            .to_string()
                    })?;
                let client = SftpClient::new(&session)?;
                client.list_dir(&dir)
            })();
            let _ = tx.send(SftpJobResult::Listed { dir, result });
            ctx.request_repaint();
        });
    }

    fn spawn_upload(
        &mut self,
        sid: SshSessionId,
        mgr: SshManager,
        remote: PathBuf,
        local: PathBuf,
        ctx: &egui::Context,
    ) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.pending_audit = Some((
            "sftp.upload",
            remote.to_string_lossy().into_owned(),
        ));
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let ctx = ctx.clone();
        let lang = crate::i18n::language(&ctx);
        thread::spawn(move || {
            let loc = crate::i18n::Locale::from(lang);
            let msg = (|| -> Result<String, String> {
                let session = mgr.get_session(sid).ok_or_else(|| {
                    loc.tr("SSH session unavailable", "SSH 会话不可用")
                        .to_string()
                })?;
                let client = SftpClient::new(&session)?;
                let n = client.upload(&local, &remote)?;
                Ok(match lang {
                    UiLanguage::En => format!(
                        "Uploaded {} bytes → {}",
                        n,
                        remote.to_string_lossy()
                    ),
                    UiLanguage::Zh => format!(
                        "已上传 {} bytes → {}",
                        n,
                        remote.to_string_lossy()
                    ),
                })
            })();
            let _ = tx.send(SftpJobResult::Msg(msg));
            ctx.request_repaint();
        });
    }

    fn spawn_download(
        &mut self,
        sid: SshSessionId,
        mgr: SshManager,
        remote: PathBuf,
        local: PathBuf,
        ctx: &egui::Context,
    ) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.pending_audit = Some((
            "sftp.download",
            remote.to_string_lossy().into_owned(),
        ));
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let ctx = ctx.clone();
        let lang = crate::i18n::language(&ctx);
        thread::spawn(move || {
            let loc = crate::i18n::Locale::from(lang);
            let msg = (|| -> Result<String, String> {
                let session = mgr.get_session(sid).ok_or_else(|| {
                    loc.tr("SSH session unavailable", "SSH 会话不可用")
                        .to_string()
                })?;
                let client = SftpClient::new(&session)?;
                let n = client.download(&remote, &local)?;
                Ok(match lang {
                    UiLanguage::En => format!(
                        "Downloaded {} → {} bytes",
                        remote.to_string_lossy(),
                        n
                    ),
                    UiLanguage::Zh => format!(
                        "已下载 {} → {} bytes",
                        remote.to_string_lossy(),
                        n
                    ),
                })
            })();
            let _ = tx.send(SftpJobResult::Msg(msg));
            ctx.request_repaint();
        });
    }

    fn spawn_mkdir(&mut self, sid: SshSessionId, mgr: SshManager, path: PathBuf, ctx: &egui::Context) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.pending_audit = Some(("sftp.mkdir", path.to_string_lossy().into_owned()));
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let ctx = ctx.clone();
        let lang = crate::i18n::language(&ctx);
        thread::spawn(move || {
            let loc = crate::i18n::Locale::from(lang);
            let msg = (|| -> Result<String, String> {
                let session = mgr.get_session(sid).ok_or_else(|| {
                    loc.tr("SSH session unavailable", "SSH 会话不可用")
                        .to_string()
                })?;
                let client = SftpClient::new(&session)?;
                client.mkdir(&path)?;
                Ok(match lang {
                    UiLanguage::En => format!("Created directory {}", path.to_string_lossy()),
                    UiLanguage::Zh => format!("已创建目录 {}", path.to_string_lossy()),
                })
            })();
            let _ = tx.send(SftpJobResult::Msg(msg));
            ctx.request_repaint();
        });
    }

    fn spawn_remove(&mut self, sid: SshSessionId, mgr: SshManager, path: PathBuf, ctx: &egui::Context) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.pending_audit = Some(("sftp.delete", path.to_string_lossy().into_owned()));
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let ctx = ctx.clone();
        let lang = crate::i18n::language(&ctx);
        thread::spawn(move || {
            let loc = crate::i18n::Locale::from(lang);
            let msg = (|| -> Result<String, String> {
                let session = mgr.get_session(sid).ok_or_else(|| {
                    loc.tr("SSH session unavailable", "SSH 会话不可用")
                        .to_string()
                })?;
                let client = SftpClient::new(&session)?;
                client.remove(&path)?;
                Ok(match lang {
                    UiLanguage::En => format!("Deleted {}", path.to_string_lossy()),
                    UiLanguage::Zh => format!("已删除 {}", path.to_string_lossy()),
                })
            })();
            let _ = tx.send(SftpJobResult::Msg(msg));
            ctx.request_repaint();
        });
    }

    fn spawn_upload_many(
        &mut self,
        sid: SshSessionId,
        mgr: SshManager,
        cwd: PathBuf,
        locals: Vec<PathBuf>,
        ctx: &egui::Context,
    ) {
        if self.busy || locals.is_empty() {
            return;
        }
        self.busy = true;
        self.pending_audit = Some(("sftp.upload_batch", cwd.to_string_lossy().into_owned()));
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let ctx = ctx.clone();
        let lang = crate::i18n::language(&ctx);
        thread::spawn(move || {
            let loc = crate::i18n::Locale::from(lang);
            let msg = (|| -> Result<String, String> {
                let session = mgr.get_session(sid).ok_or_else(|| {
                    loc.tr("SSH session unavailable", "SSH 会话不可用")
                        .to_string()
                })?;
                let client = SftpClient::new(&session)?;
                let mut ok_n = 0usize;
                let mut total_bytes = 0u64;
                let mut err_lines = Vec::new();
                for local in locals {
                    let fname = local
                        .file_name()
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from("upload.bin"));
                    let remote_path = cwd.join(&fname);
                    match client.upload(&local, &remote_path) {
                        Ok(n) => {
                            ok_n += 1;
                            total_bytes += n;
                        }
                        Err(e) => err_lines.push(format!("{}: {}", local.display(), e)),
                    }
                }
                if ok_n == 0 && !err_lines.is_empty() {
                    return Err(err_lines.join("\n"));
                }
                let mut s = match lang {
                    UiLanguage::En => format!(
                        "Uploaded {} file(s), {} bytes total",
                        ok_n, total_bytes
                    ),
                    UiLanguage::Zh => format!(
                        "已上传 {} 个文件，合计 {} bytes",
                        ok_n, total_bytes
                    ),
                };
                if !err_lines.is_empty() {
                    s.push_str(match lang {
                        UiLanguage::En => "\nSome uploads failed:\n",
                        UiLanguage::Zh => "\n部分失败：\n",
                    });
                    s.push_str(&err_lines.join("\n"));
                }
                Ok(s)
            })();
            let _ = tx.send(SftpJobResult::Msg(msg));
            ctx.request_repaint();
        });
    }

    /// 右侧 SFTP 侧栏入口（`close_panel` 置为 true 时由宿主隐藏侧栏）
    pub fn show_side_panel(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        right_dock_outer_left: &mut Option<f32>,
        dock_col_w: f32,
    ) {
        let (def_w, min_w, max_w) = layout_util::right_dock_resize_bounds(dock_col_w);
        let panel = egui::SidePanel::right("sftp_browser_panel")
            .default_width(def_w)
            .min_width(min_w)
            .max_width(max_w)
            .resizable(true)
            .frame(crate::ui::chrome::right_dock_placeholder_frame(theme))
            .show(ctx, |ui| {
                crate::ui::chrome::paint_right_dock_left_gap(ui, theme);
                self.last_panel_slot_rect = Some(ui.max_rect());
                let h = ui.available_height().max(1.0);
                let w = ui.available_width().max(1.0);
                ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
            });
        if let Some(slot) = self.last_panel_slot_rect {
            layout_util::record_right_dock_panel_rect(&slot, right_dock_outer_left);
        } else {
            layout_util::record_right_dock_panel(&panel.response, right_dock_outer_left);
        }
        let _ = theme;
    }

    /// Central 之后绘制 SFTP 前景正文（与 AI/监控一致，避免列壳层风格不一致）。
    pub fn show_foreground_panel(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        terminal: Option<&TerminalView>,
        audit: &AuditLogger,
        close_panel: &mut bool,
    ) {
        let screen = ctx.screen_rect();
        let dock_inset = theme.spacing_right_dock_screen_inset();
        let Some(slot) = layout_util::right_dock_foreground_slot(
            self.last_panel_slot_rect,
            ctx,
            "sftp_browser_panel",
            layout_util::SidePanelProfile::Standard,
            None,
            dock_inset,
        ) else {
            return;
        };
        let geom = crate::ui::chrome::prepare_right_dock_foreground_geom(slot, screen, theme);
        let layer_id = crate::ui::chrome::right_dock_foreground_layer_id("mistterm_sftp_fg");
        crate::ui::chrome::paint_right_dock_foreground_shell(ctx, layer_id, geom.paint, theme);
        crate::ui::chrome::show_right_dock_foreground_body(
            "mistterm_sftp_fg",
            ctx,
            &geom,
            layout_util::SidePanelProfile::Standard,
            |ui, _body_w| {
                self.show_content(ui, ctx, theme, terminal, audit, close_panel);
            },
        );
    }

    fn show_content(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &Theme,
        terminal: Option<&TerminalView>,
        audit: &AuditLogger,
        close_panel: &mut bool,
    ) {
        self.poll_rx(audit, crate::i18n::language(ctx));

        let mut header_closed = false;
        let prev_gap_y = ui.spacing().item_spacing.y;
        ui.spacing_mut().item_spacing.y = 0.0;
        theme.frame_right_dock_header_band().show(ui, |ui| {
            header_closed = crate::ui::chrome::dock_panel_title_close_only(
                ui,
                theme,
                crate::ui::icons::IconId::Folder,
                "SFTP",
                crate::i18n::tr(ctx, "Hide sidebar · or use bottom SFTP toggle", "隐藏侧栏 · 也可用底部 SFTP 切换"),
            );
        });
        if header_closed {
            *close_panel = true;
        }
        crate::ui::chrome::right_dock_header_divider(ui, theme);
        ui.spacing_mut().item_spacing.y = prev_gap_y;
        ui.add_space(theme.spacing_xs());

        let Some(t) = terminal else {
            ui.label(
                egui::RichText::new(crate::i18n::tr(
                    ctx,
                    "Connect a session before using SFTP.",
                    "请打开会话并连接后可使用 SFTP。",
                ))
                    .color(theme.text_tertiary()),
            );
            return;
        };

        if !t.is_connected() {
            crate::ui::chrome::busy_row(ui, theme, crate::i18n::tr(ctx, "Connecting…", "连接建立中…"));
            return;
        }

        let Some((sid, mgr)) = t.sftp_session_for_ops() else {
            ui.label(egui::RichText::new(crate::i18n::tr(ctx, "Session unavailable", "会话不可用")).color(theme.red_color()));
            return;
        };

        // 可变操作成功后自动刷新；否则处理「打开面板时首次加载」
        if self.pending_refresh_after_op && !self.busy && self.rx.is_none() {
            self.pending_refresh_after_op = false;
            self.spawn_list(sid, mgr.clone(), self.cwd.clone(), ctx);
        } else if self.pending_auto_list && !self.busy && self.rx.is_none() {
            self.pending_auto_list = false;
            self.spawn_list(sid, mgr.clone(), self.cwd.clone(), ctx);
        }

        let download_dir_hint = t.download_dir().to_string();
        let download_dir_path = PathBuf::from(&download_dir_hint);

        if let Some(ok) = &self.toast_ok {
            ui.label(egui::RichText::new(ok).color(theme.green_color()));
            if crate::ui::chrome::chrome_small_icon_button(ui, theme, crate::ui::icons::IconId::Close)
                .on_hover_text(crate::i18n::tr(ui.ctx(), "Dismiss", "关闭提示"))
                .clicked() {
                self.toast_ok = None;
            }
            ui.separator();
        }
        if let Some(err) = &self.toast_err {
            ui.label(egui::RichText::new(err).color(theme.red_color()));
            if crate::ui::chrome::chrome_small_icon_button(ui, theme, crate::ui::icons::IconId::Close)
                .on_hover_text(crate::i18n::tr(ui.ctx(), "Close", "关闭"))
                .clicked() {
                self.toast_err = None;
            }
            ui.separator();
        }
        if let Some(err) = &self.list_err {
            ui.label(egui::RichText::new(err).color(theme.red_color()));
            ui.separator();
        }

        ui.label(
            egui::RichText::new(format!(
                "{} {}",
                crate::i18n::tr(ctx, "Local download folder (ZMODEM):", "本机默认保存目录（ZMODEM）："),
                download_dir_hint
            ))
                .small()
                .color(theme.text_tertiary()),
        );
        ui.add_space(theme.spacing_md());

        ui.horizontal(|ui| {
            crate::ui::chrome::form_field_label(ui, theme, crate::i18n::tr(ui.ctx(), "Path", "路径"));
            let path_w = layout_util::finite_avail_minus(ui, 200.0, 120.0, 480.0);
            crate::ui::chrome::form_singleline_field(
                ui,
                theme,
                egui::Id::new("sftp_path_edit"),
                &mut self.path_edit,
                crate::i18n::tr(ui.ctx(), "/tmp or .", "/tmp 或 ."),
                path_w,
                false,
            );
            if crate::ui::chrome::panel_action_icon_button_ex(
                ui,
                theme,
                crate::ui::icons::IconId::Search,
                crate::i18n::tr(ui.ctx(), "Go", "前往"),
                !self.busy,
            )
            .clicked() {
                let p = PathBuf::from(self.path_edit.trim());
                self.spawn_list(sid, mgr.clone(), p, ctx);
            }
            if crate::ui::chrome::panel_action_icon_button_ex(
                ui,
                theme,
                crate::ui::icons::IconId::Refresh,
                crate::i18n::tr(ui.ctx(), "Refresh", "刷新"),
                !self.busy,
            )
            .clicked() {
                self.spawn_list(sid, mgr.clone(), self.cwd.clone(), ctx);
            }
        });

        ui.horizontal(|ui| {
            if crate::ui::chrome::panel_action_icon_button_ex(
                ui,
                theme,
                crate::ui::icons::IconId::ChevronLeft,
                crate::i18n::tr(ui.ctx(), "Parent directory", "上层目录"),
                !self.busy,
            )
            .clicked()
            {
                let parent = self
                    .cwd
                    .parent()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("/"));
                self.spawn_list(sid, mgr.clone(), parent, ctx);
            }
            if crate::ui::chrome::panel_action_icon_button_ex(
                ui,
                theme,
                crate::ui::icons::IconId::File,
                crate::i18n::tr(ui.ctx(), "Save As…", "另存为…"),
                !self.busy && self.selected.is_some(),
            )
            .clicked()
            {
                if let Some(rem) = &self.selected {
                    let name = rem
                        .file_name()
                        .map(|x| x.to_string_lossy().to_string())
                        .unwrap_or_else(|| "remote-file".into());
                    if let Some(e) = self.entries.iter().find(|x| x.path == *rem) {
                        if e.is_dir {
                            self.toast_err = Some(crate::i18n::tr(ctx, "Select a file, not a folder", "请选文件而非目录").to_string());
                            return;
                        }
                    }
                    if let Some(save) = FileDialog::new()
                        .set_title(crate::i18n::tr(ctx, "Save remote file as…", "另存远端文件"))
                        .set_directory(&download_dir_path)
                        .set_file_name(&name)
                        .save_file()
                    {
                        self.spawn_download(sid, mgr.clone(), rem.clone(), save, ctx);
                    }
                }
            }
            if crate::ui::chrome::panel_action_icon_button_ex(
                ui,
                theme,
                crate::ui::icons::IconId::Package,
                crate::i18n::tr(ui.ctx(), "Download to default folder", "下载到默认目录"),
                !self.busy && self.selected.is_some(),
            )
            .on_hover_text(format!(
                "{} {}",
                crate::i18n::tr(ctx, "Write to:", "写入："),
                download_dir_hint
            ))
            .clicked()
            {
                if let Some(rem) = self.selected.clone() {
                    let name = rem
                        .file_name()
                        .map(|x| x.to_string_lossy().to_string())
                        .unwrap_or_else(|| "remote-file".into());
                    if let Some(e) = self.entries.iter().find(|x| x.path == rem) {
                        if e.is_dir {
                            self.toast_err = Some(crate::i18n::tr(ctx, "Select a file, not a folder", "请选文件而非目录").to_string());
                            return;
                        }
                    }
                    if let Err(e) = std::fs::create_dir_all(&download_dir_path) {
                        self.toast_err = Some(format!(
                            "{} {}",
                            crate::i18n::tr(ctx, "Could not create local folder:", "无法创建本机目录："),
                            e
                        ));
                        return;
                    }
                    let save = download_dir_path.join(&name);
                    self.spawn_download(sid, mgr.clone(), rem, save, ctx);
                }
            }
        });

        ui.horizontal(|ui| {
            if crate::ui::chrome::panel_action_icon_button_ex(
                ui,
                theme,
                crate::ui::icons::IconId::Upload,
                crate::i18n::tr(ui.ctx(), "Upload… (multi-select; keep local filenames)", "上传…（可多选；文件名保持与本地一致）"),
                !self.busy,
            )
            .clicked()
            {
                if let Some(files) = FileDialog::new().pick_files() {
                    if files.is_empty() {
                        return;
                    }
                    if files.len() == 1 {
                        let picked = files.into_iter().next().expect("len checked");
                        let fname = picked
                            .file_name()
                            .map(PathBuf::from)
                            .unwrap_or_else(|| PathBuf::from("upload.bin"));
                        let remote_path = self.cwd.join(fname);
                        self.spawn_upload(sid, mgr.clone(), remote_path, picked, ctx);
                    } else {
                        self.spawn_upload_many(sid, mgr.clone(), self.cwd.clone(), files, ctx);
                    }
                }
            }
            if crate::ui::chrome::panel_action_icon_button_ex(
                ui,
                theme,
                crate::ui::icons::IconId::Trash,
                crate::i18n::tr(ui.ctx(), "Delete selected…", "删除选中…"),
                !self.busy && self.selected.is_some(),
            )
            .clicked()
            {
                if let Some(p) = self.selected.clone() {
                    self.pending_delete = Some(p);
                }
            }
        });

        ui.horizontal(|ui| {
            crate::ui::chrome::form_field_label(ui, theme, crate::i18n::tr(ui.ctx(), "New folder", "新建目录"));
            let mkdir_w = layout_util::finite_avail_minus(ui, 120.0, 80.0, 200.0);
            crate::ui::chrome::form_singleline_field(
                ui,
                theme,
                egui::Id::new("sftp_mkdir_name"),
                &mut self.mkdir_name,
                crate::i18n::tr(ui.ctx(), "Name", "名称"),
                mkdir_w,
                false,
            );
            if crate::ui::chrome::panel_action_icon_button_ex(
                ui,
                theme,
                crate::ui::icons::IconId::Plus,
                crate::i18n::tr(ui.ctx(), "Create folder", "创建目录"),
                !self.mkdir_name.trim().is_empty(),
            )
            .clicked()
                && !self.mkdir_name.trim().is_empty()
            {
                let p = self.cwd.join(self.mkdir_name.trim());
                self.mkdir_name.clear();
                self.spawn_mkdir(sid, mgr.clone(), p, ctx);
            }
        });

        if let Some(p) = self.pending_delete.clone() {
            ui.group(|ui| {
                ui.label(format!(
                    "{}\n{}",
                    crate::i18n::tr(ui.ctx(), "Delete this?", "确认删除？"),
                    p.to_string_lossy()
                ));
                ui.horizontal(|ui| {
                    if crate::ui::chrome::panel_action_primary_icon_button(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Trash,
                        crate::i18n::tr(ui.ctx(), "Confirm delete", "确认删除"),
                    )
                    .clicked() {
                        let path = self.pending_delete.take().unwrap();
                        self.spawn_remove(sid, mgr.clone(), path, ctx);
                    }
                    if crate::ui::chrome::panel_action_icon_button(ui, theme, crate::ui::icons::IconId::Cross, crate::i18n::tr(ui.ctx(), "Cancel", "取消"))
                        .clicked() {
                        self.pending_delete = None;
                    }
                });
            });
        }

        ui.separator();

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // === 拖拽检测：检测文件拖入 SFTP 面板（上传） ===
                let has_dropped = !ui.ctx().input(|i| i.raw.dropped_files.is_empty());
                let is_hovering = ui.ctx().input(|i| i.pointer.hover_pos().map_or(false, |p| ui.clip_rect().contains(p)));
                
                if has_dropped && is_hovering {
                    let files: Vec<PathBuf> = ui.ctx().input(|i| {
                        i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect()
                    });
                    if !files.is_empty() {
                        // 上传拖入的文件到当前目录
                        self.spawn_upload_many(sid, mgr.clone(), self.cwd.clone(), files, ctx);
                    }
                }

                // 拖拽提示
                if ui.ctx().input(|i| i.raw.dropped_files.is_empty() && !i.raw.hovered_files.is_empty()) && is_hovering {
                    ui.painter().rect_filled(
                        ui.clip_rect(),
                        0.0,
                        theme.color_sftp_row_hover(),
                    );
                    let center = ui.clip_rect().center();
                    ui.painter().text(
                        center,
                        egui::Align2::CENTER_CENTER,
                        crate::i18n::tr(ui.ctx(), "Release to upload", "释放以上传文件"),
                        egui::FontId::proportional(theme.font_size_body()),
                        theme.text_primary(),
                    );
                }

                ui.label(
                    egui::RichText::new(format!(
                        "{} {}",
                        self.entries.len(),
                        crate::i18n::tr(
                            ctx,
                            "items (drag in to upload, drag out to download)",
                            "项（拖入文件上传，拖出文件下载）",
                        ),
                    ))
                    .small()
                    .color(theme.text_tertiary()),
                );

                let mut enter_dir: Option<PathBuf> = None;
                let mut download_path: Option<(PathBuf, String)> = None;

                for e in self.entries.iter() {
                    let is_sel = self.selected.as_ref() == Some(&e.path);
                    let response = ui.horizontal(|ui| {
                        let icon = if e.is_dir {
                            crate::ui::icons::IconId::Folder
                        } else {
                            crate::ui::icons::IconId::File
                        };
                        let px = theme.font_size_body();
                        let (r, _) =
                            ui.allocate_exact_size(egui::vec2(px, px), egui::Sense::hover());
                        crate::ui::icons::paint_icon(ui, r, icon, theme.text_secondary(), px);
                        ui.add(egui::SelectableLabel::new(
                            is_sel,
                            format!("{} · {}", &e.name, e.size_human()),
                        ))
                    }).inner;
                    let response =
                        response.on_hover_text(e.path.to_string_lossy());
                    if response.clicked() {
                        self.selected = Some(e.path.clone());
                    }
                    if response.double_clicked() && e.is_dir {
                        enter_dir = Some(e.path.clone());
                    }

                    // === 拖拽文件下载 ===
                    if !e.is_dir && response.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                        egui::popup::show_tooltip(ui.ctx(), ui.id().with("drag_tip"), |ui| {
                            ui.label(format!(
                                "{} {}",
                                crate::i18n::tr(ui.ctx(), "Choose save location:", "拖出后选择位置下载："),
                                e.name
                            ));
                        });
                    }
                    if !e.is_dir && response.drag_released() {
                        // 拖拽释放时弹出保存对话框
                        let file_name = e.path.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("download")
                            .to_string();
                        download_path = Some((e.path.clone(), file_name));
                    }
                }

                if let Some(d) = enter_dir {
                    self.spawn_list(sid, mgr.clone(), d, ctx);
                }

                // 处理下载（在循环外，避免 borrow checker 问题）
                if let Some((remote_path, file_name)) = download_path {
                    if let Some(save) = FileDialog::new()
                        .set_title(crate::i18n::tr(ctx, "Save downloaded file…", "保存下载的文件"))
                        .set_file_name(&file_name)
                        .save_file()
                    {
                        self.spawn_download(sid, mgr.clone(), remote_path, save, ctx);
                    }
                }
            });

        if self.busy {
            ui.add_space(theme.spacing_panel_gap());
            ui.label(egui::RichText::new(crate::i18n::tr(ui.ctx(), "SFTP busy…", "SFTP 处理中…")).small().color(theme.text_tertiary()));
        }
    }
}