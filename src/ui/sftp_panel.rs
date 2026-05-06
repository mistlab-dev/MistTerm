//! 远端 SFTP 侧栏：浏览、上传到当前目录、下载到本地、新建目录与删除。
//!
//! 网络与 SFTP 在后台线程执行，通过 `Receiver` 回传结果，避免阻塞 egui。

use crate::ssh::manager::SshSessionId;
use crate::ssh::{SftpClient, SftpEntry, SshManager};
use crate::ui::terminal::TerminalView;
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

    /// 右侧 SFTP 侧栏入口
    pub fn show_side_panel(&mut self, ctx: &egui::Context, theme: &Theme, terminal: Option<&TerminalView>) {
        egui::SidePanel::right("sftp_browser_panel")
            .default_width(360.0)
            .resizable(true)
            .show(ctx, |ui| {
                self.show_content(ui, ctx, theme, terminal);
            });
    }

    fn show_content(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, theme: &Theme, terminal: Option<&TerminalView>) {
        self.poll_rx();

        ui.horizontal(|ui| {
            ui.heading(egui::RichText::new("📂 SFTP").color(theme.fg_high_color()));
        });
        ui.separator();

        let Some(t) = terminal else {
            ui.label(
                egui::RichText::new("请打开会话并连接后可使用 SFTP。")
                    .color(theme.fg_low_color()),
            );
            return;
        };

        if !t.is_connected() {
            ui.label(egui::RichText::new("连接建立中…").color(theme.fg_low_color()));
            return;
        }

        let Some((sid, mgr)) = t.sftp_session_for_ops() else {
            ui.label(egui::RichText::new("会话不可用").color(theme.red_color()));
            return;
        };

        // 可变操作成功后自动刷新列表
        if self.pending_refresh_after_op && !self.busy && self.rx.is_none() {
            self.pending_refresh_after_op = false;
            self.spawn_list(sid, mgr.clone(), self.cwd.clone(), ctx);
        }

        let download_dir_hint = t.download_dir().to_string();

        if let Some(ok) = &self.toast_ok {
            ui.label(egui::RichText::new(ok).color(theme.green_color()));
            if ui.small_button("关闭提示").clicked() {
                self.toast_ok = None;
            }
            ui.separator();
        }
        if let Some(err) = &self.toast_err {
            ui.label(egui::RichText::new(err).color(theme.red_color()));
            if ui.small_button("关闭").clicked() {
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
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("路径").small().color(theme.fg_low_color()),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.path_edit)
                    .desired_width(ui.available_width() - 148.0)
                    .hint_text("/tmp 或 ."),
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
                .add_enabled(!self.busy && self.selected.is_some(), egui::Button::new("下载选中"))
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
                    if let Some(save) =
                        FileDialog::new().set_file_name(&name).save_file()
                    {
                        self.spawn_download(sid, mgr.clone(), rem.clone(), save.clone(), ctx);
                    }
                }
            }
            if ui.add_enabled(!self.busy, egui::Button::new("上传…")).clicked() {
                if let Some(picked) = FileDialog::new().pick_file() {
                    let fname = picked
                        .file_name()
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from("upload.bin"));
                    let remote_path = self.cwd.join(fname);
                    self.spawn_upload(sid, mgr.clone(), remote_path, picked, ctx);
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
            ui.add(egui::TextEdit::singleline(&mut self.mkdir_name).hint_text("名称"));
            if ui.small_button("创建").clicked() && !self.mkdir_name.trim().is_empty() {
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
                ui.label(
                    egui::RichText::new(format!("{} 项（双击目录进入）", self.entries.len()))
                        .small()
                        .color(theme.fg_low_color()),
                );

                let mut enter_dir: Option<PathBuf> = None;

                for e in self.entries.iter() {
                    let is_sel = self.selected.as_ref() == Some(&e.path);
                    let line = format!(
                        "{}  {} · {}",
                        if e.is_dir { "📁" } else { "📄" },
                        &e.name,
                        e.size_human()
                    );

                    let response = ui.add(egui::SelectableLabel::new(is_sel, line));
                    let response =
                        response.on_hover_text(e.path.to_string_lossy());
                    if response.clicked() {
                        self.selected = Some(e.path.clone());
                    }
                    if response.double_clicked() && e.is_dir {
                        enter_dir = Some(e.path.clone());
                    }
                }

                if let Some(d) = enter_dir {
                    self.spawn_list(sid, mgr.clone(), d, ctx);
                }
            });

        if self.busy {
            ui.add_space(6.0);
            ui.label(egui::RichText::new("SFTP 处理中…").small().color(theme.fg_low_color()));
        }
    }
}