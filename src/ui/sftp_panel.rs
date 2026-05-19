//! 远端 SFTP 侧栏：浏览、上传到当前目录、下载到本地、新建目录与删除。
//!
//! 网络与 SFTP 在后台线程执行，通过 `Receiver` 回传结果，避免阻塞 egui。

use crate::ssh::SshSessionId;
use crate::ssh::{SftpClient, SftpEntry, SshManager};
use crate::ui::terminal::TerminalView;
use crate::ui::layout_util::{self, SidePanelProfile};
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
    }

    fn poll_rx(&mut self) {
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
                        self.toast_ok = Some(msg);
                        self.pending_refresh_after_op = true;
                    }
                    Err(e) => self.toast_err = Some(e),
                }
                self.busy = false;
                self.rx = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.busy = false;
                self.rx = None;
                self.toast_err = Some("SFTP 后台任务异常中断".into());
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
        thread::spawn(move || {
            let result = (|| -> Result<Vec<SftpEntry>, String> {
                let session = mgr
                    .get_session(sid)
                    .ok_or_else(|| "SSH 会话不可用".to_string())?;
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
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let ctx = ctx.clone();
        thread::spawn(move || {
            let msg = (|| -> Result<String, String> {
                let session = mgr.get_session(sid).ok_or_else(|| "SSH 会话不可用".to_string())?;
                let client = SftpClient::new(&session)?;
                let n = client.upload(&local, &remote)?;
                Ok(format!(
                    "已上传 {} bytes → {}",
                    n,
                    remote.to_string_lossy()
                ))
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
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let ctx = ctx.clone();
        thread::spawn(move || {
            let msg = (|| -> Result<String, String> {
                let session = mgr.get_session(sid).ok_or_else(|| "SSH 会话不可用".to_string())?;
                let client = SftpClient::new(&session)?;
                let n = client.download(&remote, &local)?;
                Ok(format!(
                    "已下载 {} → {} bytes",
                    remote.to_string_lossy(),
                    n
                ))
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
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let ctx = ctx.clone();
        thread::spawn(move || {
            let msg = (|| -> Result<String, String> {
                let session = mgr.get_session(sid).ok_or_else(|| "SSH 会话不可用".to_string())?;
                let client = SftpClient::new(&session)?;
                client.mkdir(&path)?;
                Ok(format!("已创建目录 {}", path.to_string_lossy()))
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
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let ctx = ctx.clone();
        thread::spawn(move || {
            let msg = (|| -> Result<String, String> {
                let session = mgr.get_session(sid).ok_or_else(|| "SSH 会话不可用".to_string())?;
                let client = SftpClient::new(&session)?;
                client.remove(&path)?;
                Ok(format!("已删除 {}", path.to_string_lossy()))
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
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let ctx = ctx.clone();
        thread::spawn(move || {
            let msg = (|| -> Result<String, String> {
                let session = mgr.get_session(sid).ok_or_else(|| "SSH 会话不可用".to_string())?;
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
                let mut s = format!(
                    "已上传 {} 个文件，合计 {} bytes",
                    ok_n, total_bytes
                );
                if !err_lines.is_empty() {
                    s.push_str("\n部分失败：\n");
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
        terminal: Option<&TerminalView>,
        close_panel: &mut bool,
        right_dock_outer_left: &mut Option<f32>,
    ) {
        let (s_def, s_min, s_max) = layout_util::side_panel_widths(ctx, SidePanelProfile::Standard);
        let panel = egui::SidePanel::right("sftp_browser_panel")
            .default_width(s_def)
            .min_width(s_min)
            .max_width(s_max)
            .resizable(true)
            .frame(crate::ui::chrome::right_dock_panel_frame(theme))
            .show(ctx, |ui| {
                let panel_w = layout_util::dock_panel_content_width(ui, s_min, s_max);
                ui.set_max_width(panel_w);
                self.show_content(ui, ctx, theme, terminal, close_panel);
            });
        layout_util::record_right_dock_panel(&panel.response, right_dock_outer_left);
    }

    fn show_content(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &Theme,
        terminal: Option<&TerminalView>,
        close_panel: &mut bool,
    ) {
        self.poll_rx();

        if crate::ui::chrome::dock_panel_title_close_only(
            ui,
            theme,
            Some(crate::ui::icons::IconId::Folder),
            "SFTP",
            crate::ui::chrome::DockPanelTitleStyle::DockHeading,
            "隐藏侧栏 · 也可用底部 SFTP 切换",
        ) {
            *close_panel = true;
        }
        ui.separator();

        let Some(t) = terminal else {
            ui.label(
                egui::RichText::new("请打开会话并连接后可使用 SFTP。")
                    .color(theme.fg_low_color()),
            );
            return;
        };

        if !t.is_connected() {
            crate::ui::chrome::busy_row(ui, theme, "连接建立中…");
            return;
        }

        let Some((sid, mgr)) = t.sftp_session_for_ops() else {
            ui.label(egui::RichText::new("会话不可用").color(theme.red_color()));
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
            if crate::ui::chrome::chrome_small_button(ui, theme, "关闭提示").clicked() {
                self.toast_ok = None;
            }
            ui.separator();
        }
        if let Some(err) = &self.toast_err {
            ui.label(egui::RichText::new(err).color(theme.red_color()));
            if crate::ui::chrome::chrome_small_button(ui, theme, "关闭").clicked() {
                self.toast_err = None;
            }
            ui.separator();
        }
        if let Some(err) = &self.list_err {
            ui.label(egui::RichText::new(err).color(theme.red_color()));
            ui.separator();
        }

        ui.label(
            egui::RichText::new(format!("本机默认保存目录（ZMODEM）：{}", download_dir_hint))
                .small()
                .color(theme.fg_low_color()),
        );
        ui.add_space(theme.spacing_md());

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("路径").small().color(theme.fg_low_color()),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.path_edit)
                    .desired_width(layout_util::finite_avail_minus(ui, 160.0, 80.0, 1200.0))
                    .hint_text(crate::ui::chrome::hint_rich(
                        theme,
                        "/tmp 或 .",
                        theme.font_size_normal(),
                    )),
            );
            if ui.add_enabled(!self.busy, egui::Button::new("前往")).clicked() {
                let p = PathBuf::from(self.path_edit.trim());
                self.spawn_list(sid, mgr.clone(), p, ctx);
            }
            if ui.add_enabled(!self.busy, egui::Button::new("刷新")).clicked() {
                self.spawn_list(sid, mgr.clone(), self.cwd.clone(), ctx);
            }
        });

        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.busy, egui::Button::new("上层目录"))
                .clicked()
            {
                let parent = self
                    .cwd
                    .parent()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("/"));
                self.spawn_list(sid, mgr.clone(), parent, ctx);
            }
            if ui
                .add_enabled(!self.busy && self.selected.is_some(), egui::Button::new("另存为…"))
                .clicked()
            {
                if let Some(rem) = &self.selected {
                    let name = rem
                        .file_name()
                        .map(|x| x.to_string_lossy().to_string())
                        .unwrap_or_else(|| "remote-file".into());
                    if let Some(e) = self.entries.iter().find(|x| x.path == *rem) {
                        if e.is_dir {
                            self.toast_err = Some("请选文件而非目录".into());
                            return;
                        }
                    }
                    if let Some(save) = FileDialog::new()
                        .set_directory(&download_dir_path)
                        .set_file_name(&name)
                        .save_file()
                    {
                        self.spawn_download(sid, mgr.clone(), rem.clone(), save, ctx);
                    }
                }
            }
            if ui
                .add_enabled(!self.busy && self.selected.is_some(), egui::Button::new("下载到默认目录"))
                .on_hover_text(format!("写入：{}", download_dir_hint))
                .clicked()
            {
                if let Some(rem) = self.selected.clone() {
                    let name = rem
                        .file_name()
                        .map(|x| x.to_string_lossy().to_string())
                        .unwrap_or_else(|| "remote-file".into());
                    if let Some(e) = self.entries.iter().find(|x| x.path == rem) {
                        if e.is_dir {
                            self.toast_err = Some("请选文件而非目录".into());
                            return;
                        }
                    }
                    if let Err(e) = std::fs::create_dir_all(&download_dir_path) {
                        self.toast_err = Some(format!("无法创建本机目录：{}", e));
                        return;
                    }
                    let save = download_dir_path.join(&name);
                    self.spawn_download(sid, mgr.clone(), rem, save, ctx);
                }
            }
        });

        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.busy, egui::Button::new("上传…"))
                .on_hover_text("可多选；文件名保持与本地一致，写入当前远端目录")
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
            if ui
                .add_enabled(!self.busy && self.selected.is_some(), egui::Button::new("删除选中…"))
                .clicked()
            {
                if let Some(p) = self.selected.clone() {
                    self.pending_delete = Some(p);
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("新建目录").small().color(theme.fg_low_color()),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.mkdir_name).hint_text(crate::ui::chrome::hint_rich(
                    theme,
                    "名称",
                    theme.font_size_normal(),
                )),
            );
            if crate::ui::chrome::chrome_small_button(ui, theme, "创建").clicked()
                && !self.mkdir_name.trim().is_empty()
            {
                let p = self.cwd.join(self.mkdir_name.trim());
                self.mkdir_name.clear();
                self.spawn_mkdir(sid, mgr.clone(), p, ctx);
            }
        });

        if let Some(p) = self.pending_delete.clone() {
            ui.group(|ui| {
                ui.label(format!("确认删除？\n{}", p.to_string_lossy()));
                ui.horizontal(|ui| {
                    if ui.button("删除").clicked() {
                        let path = self.pending_delete.take().unwrap();
                        self.spawn_remove(sid, mgr.clone(), path, ctx);
                    }
                    if ui.button("取消").clicked() {
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
                        "释放以上传文件",
                        egui::FontId::proportional(theme.font_size_body()),
                        theme.fg_high_color(),
                    );
                }

                ui.label(
                    egui::RichText::new(format!("{} 项（拖入文件上传，拖出文件下载）", self.entries.len()))
                        .small()
                        .color(theme.fg_low_color()),
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
                        crate::ui::icons::paint_icon(ui, r, icon, theme.fg_medium_color(), px);
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
                            ui.label(format!("拖出后选择位置下载: {}", e.name));
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
                        .set_file_name(&file_name)
                        .save_file()
                    {
                        self.spawn_download(sid, mgr.clone(), remote_path, save, ctx);
                    }
                }
            });

        if self.busy {
            ui.add_space(theme.spacing_panel_gap());
            ui.label(egui::RichText::new("SFTP 处理中…").small().color(theme.fg_low_color()));
        }
    }
}