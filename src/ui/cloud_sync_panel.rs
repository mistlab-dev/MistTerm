//! 云端同步 UI：设置项 + 导出/导入本地包（MVP，无真实账户 API）

use std::fs;
use std::path::PathBuf;

use eframe::egui;
use rfd::FileDialog;

use crate::core::{CloudSyncSettings, FragmentManager};
use crate::ui::theme::{Theme, ThemeManager};

/// 导出/导入所需的本地路径与可变引用（由主窗口注入）
pub struct CloudSyncDeps<'a> {
    pub fragments_path: &'a PathBuf,
    pub sessions_path: &'a PathBuf,
    pub theme_path: &'a PathBuf,
    pub fragment_manager: &'a mut FragmentManager,
    pub theme_manager: &'a mut ThemeManager,
}

/// 同步面板（右侧栏）
pub struct CloudSyncPanel {
    pub open: bool,
    pub settings: CloudSyncSettings,
    pub message: String,
}

impl CloudSyncPanel {
    pub fn new() -> Self {
        Self {
            open: false,
            settings: CloudSyncSettings::load(),
            message: String::new(),
        }
    }

    fn save_settings(&mut self) {
        match self.settings.save() {
            Ok(()) => self.message = "已保存设置".to_string(),
            Err(e) => self.message = format!("保存失败：{}", e),
        }
    }

    fn run_export(&mut self, deps: &CloudSyncDeps<'_>) {
        let Some(parent) = FileDialog::new().set_title("选择导出目录").pick_folder() else {
            return;
        };

        let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
        let dest = parent.join(format!("mistterm-sync-{}", stamp));
        if let Err(e) = fs::create_dir_all(&dest) {
            self.settings.mark_sync_err(format!("创建目录失败：{}", e));
            self.message.clone_from(&self.settings.last_error);
            return;
        }

        let mut err: Option<String> = None;

        if self.settings.sync_fragments && deps.fragments_path.exists() {
            if let Err(e) = fs::copy(deps.fragments_path, dest.join("fragments.json")) {
                err = Some(format!("fragments.json：{}", e));
            }
        }

        if err.is_none() && self.settings.sync_sessions && deps.sessions_path.exists() {
            if let Err(e) = fs::copy(deps.sessions_path, dest.join("sessions.json")) {
                err = Some(format!("sessions.json：{}", e));
            }
        }

        if err.is_none() && self.settings.sync_themes && deps.theme_path.exists() {
            if let Err(e) = fs::copy(deps.theme_path, dest.join("theme.json")) {
                err = Some(format!("theme.json：{}", e));
            }
        }

        if self.settings.sync_shortcuts {
            let _ = fs::write(
                dest.join("shortcuts.json"),
                r#"{"note":"快捷键配置占位符"}"#,
            );
        }

        if err.is_none() && self.settings.sync_credentials {
            let cred = crate::core::CredentialVault::default_path();
            if cred.exists() {
                if let Err(e) = fs::copy(&cred, dest.join("credentials.json")) {
                    err = Some(format!("credentials.json：{}", e));
                }
            }
        }

        if let Ok(s) = serde_json::to_string_pretty(&self.settings) {
            let _ = fs::write(dest.join("cloud_sync_snapshot.json"), s);
        }

        if let Some(e) = err {
            self.settings.mark_sync_err(e.clone());
            self.message = e;
        } else {
            self.settings.mark_sync_ok();
            self.message = format!("已导出到 {}", dest.display());
        }
    }

    fn run_import(&mut self, deps: &mut CloudSyncDeps<'_>) {
        let Some(dir) = FileDialog::new()
            .set_title("选择同步包目录（含 fragments.json 等）")
            .pick_folder()
        else {
            return;
        };

        let mut any = false;
        let frag_src = dir.join("fragments.json");
        if frag_src.exists() {
            match FragmentManager::load(&frag_src) {
                Ok(other) => {
                    *deps.fragment_manager = other;
                    if deps.fragment_manager.save(deps.fragments_path).is_ok() {
                        any = true;
                    } else {
                        self.message =
                            format!("写入 {} 失败", deps.fragments_path.display());
                        return;
                    }
                }
                Err(e) => {
                    self.message = format!("读取 fragments 失败：{}", e);
                    return;
                }
            }
        }

        let theme_src = dir.join("theme.json");
        if theme_src.exists() {
            if let Ok(txt) = fs::read_to_string(&theme_src) {
                if let Ok(tm) = serde_json::from_str::<ThemeManager>(&txt) {
                    *deps.theme_manager = tm;
                    deps.theme_manager.save();
                    any = true;
                }
            }
        }

        if any {
            self.settings.record_manual_import_ok();
            self.message = "导入完成".to_string();
        } else {
            self.message = "所选目录未发现可识别的同步文件".to_string();
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, theme: &Theme, deps: &mut CloudSyncDeps<'_>) {
        if !self.open {
            return;
        }

        let mut close_me = false;
        egui::SidePanel::right("cloud_sync_panel")
            .default_width(340.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("☁️ 云端同步");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("✕").clicked() {
                            close_me = true;
                        }
                    });
                });
                ui.separator();

                ui.label(egui::RichText::new("账号（展示）").color(theme.fg_medium_color()));
                ui.add(
                    egui::TextEdit::singleline(&mut self.settings.account_hint)
                        .hint_text("未登录 — 后续对接账户")
                        .desired_width(f32::INFINITY),
                );

                ui.label(egui::RichText::new("同步内容").strong());
                ui.checkbox(&mut self.settings.sync_sessions, "会话配置");
                ui.checkbox(&mut self.settings.sync_fragments, "命令片段");
                ui.checkbox(&mut self.settings.sync_themes, "主题配置");
                ui.checkbox(&mut self.settings.sync_shortcuts, "快捷键（占位）");
                ui.checkbox(&mut self.settings.sync_credentials, "凭证库（加密文件）");
                ui.checkbox(&mut self.settings.sync_team_config, "团队配置（占位）");

                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("同步频率（分钟，0=仅手动）")
                        .color(theme.fg_medium_color()),
                );
                ui.add(
                    egui::DragValue::new(&mut self.settings.frequency_minutes).speed(1.0),
                );

                if let Some(ts) = self.settings.last_sync_unix {
                    let t = chrono::DateTime::from_timestamp(ts, 0)
                        .map(|x| x.format("%Y-%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| "—".to_string());
                    ui.label(
                        egui::RichText::new(format!("最近同步：{}", t))
                            .small()
                            .color(theme.fg_low_color()),
                    );
                } else {
                    ui.label(
                        egui::RichText::new("最近同步：尚未记录")
                            .small()
                            .color(theme.fg_low_color()),
                    );
                }

                if !self.settings.last_error.is_empty() {
                    ui.label(
                        egui::RichText::new(&self.settings.last_error)
                            .small()
                            .color(theme.red_color()),
                    );
                }

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("保存设置").clicked() {
                        self.save_settings();
                    }
                    if ui.button("立即导出包…").clicked() {
                        self.run_export(deps);
                    }
                });
                if ui.button("从包导入…").clicked() {
                    self.run_import(deps);
                }

                if !self.message.is_empty() {
                    ui.label(
                        egui::RichText::new(&self.message)
                            .small()
                            .color(theme.fg_low_color()),
                    );
                }

                ui.collapsing("说明", |ui| {
                    ui.label(
                        "当前为本地文件夹同步包。远端账户、实时增量与端到端密钥将在后续迭代接入。",
                    );
                });
            });

        if close_me {
            self.open = false;
        }
    }
}
