//! 云端同步 UI：设置项 + 导出/导入本地包（MVP，无真实账户 API）

use std::fs;
use std::path::PathBuf;

use eframe::egui;
use rfd::FileDialog;

use crate::core::CloudSyncSettings;
use crate::core::{FragmentManager};
use crate::ui::theme::{Theme, ThemeManager};

/// 同步面板（右栏或窗口）
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

    /// 导出勾选的数据到选定目录下的 `mistterm-sync/` 子目录
    pub fn try_export(
        &mut self,
        fragments_path: &PathBuf,
        sessions_path: &PathBuf,
        theme_path: &PathBuf,
    ) {
        let Some(parent) = FileDialog::new().set_title("选择导出目录").pick_folder() else {
            return;
        };

        let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
        let dest = parent.join(format!("mistterm-sync-{}", stamp));
        if let Err(e) = fs::create_dir_all(&dest) {
            self.settings.mark_sync_err(format!("创建目录失败：{}", e));
            self.message = self.settings.last_error.clone();
            return;
        }

        let mut ok = true;

        if self.settings.sync_fragments {
            match fs::copy(fragments_path, dest.join("fragments.json")) {
                Err(e) if e.kind() != std::io::ErrorKind::InvalidInput && !fragments_path.exists() => {}
                Err(e) => {
                    ok = false;
                    self.settings.last_error =
                        format!("复制 fragments.json 失败：{}", e);
                }
                Ok(_) => {}
            }
            if ok && fragments_path.exists() {
                let _ = fs::copy(fragments_path, dest.join("fragments.json"));
            }
        }

        if self.settings.sync_sessions && sessions_path.exists() {
            if let Err(e) = fs::copy(sessions_path, dest.join("sessions.json")) {
                ok = false;
                self.settings.last_error = format!("复制 sessions.json：{}", e);
            }
        }

        if self.settings.sync_themes && theme_path.exists() {
            if let Err(e) = fs::copy(theme_path, dest.join("theme.json")) {
                ok = false;
                self.settings.last_error = format!("复制 theme.json：{}", e);
            }
        }

        if self.settings.sync_shortcuts {
            let p = dest.join("shortcuts.json");
            let _ = fs::write(
                &p,
                r#"{"note":"快捷键配置占位符，后续与设置合并导出"}"#,
            );
        }

        if self.settings.sync_credentials {
            let cred = crate::core::CredentialVault::default_path();
            if cred.exists() {
                if let Err(e) = fs::copy(&cred, dest.join("credentials.json")) {
                    ok = false;
                    self.settings.last_error =
                        format!("复制 credentials.json：{}", e);
                }
            }
        }

        let _ = self.settings.save();
        let _ = serde_json::to_string_pretty(&self.settings)
            .map(|s| fs::write(dest.join("cloud_sync_snapshot.json"), s));

        if ok {
            self.settings.mark_sync_ok();
            self.message = format!(
                "已导出到 {}",
                dest.display()
            );
        } else {
            let err = self.settings.last_error.clone();
            self.settings.mark_sync_err(err.clone());
            self.message = err;
        }
    }

    /// 从用户选择的目录中寻找 `mistterm-sync-*` 或根下的 json 导入
    pub fn try_import(
        &mut self,
        fragment_manager: &mut FragmentManager,
        fragment_path: &PathBuf,
        theme_mgr: &mut ThemeManager,
    ) {
        let Some(dir) = FileDialog::new()
            .set_title("选择同步包目录（含 fragments.json 等）")
            .pick_folder()
        else {
            return;
        };

        let frag_src = dir.join("fragments.json");
        if frag_src.exists() {
            match FragmentManager::load(&frag_src) {
                Ok(other) => {
                    *fragment_manager = other;
                    if let Err(e) = fragment_manager.save(fragment_path) {
                        self.message = format!("写入本地 fragments 失败：{}", e);
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
            if let Ok(bytes) = fs::read_to_string(&theme_src) {
                if let Ok(tm) = serde_json::from_str::<ThemeManager>(&bytes) {
                    *theme_mgr = tm;
                    theme_mgr.save();
                }
            }
        }

        self.settings.last_import_hint();
        self.message = "导入完成（已合并可用项）".to_string();
    }

    pub fn show(&mut self, ctx: &egui::Context, theme: &Theme) {
        if !self.open {
            return;
        }

        egui::SidePanel::right("cloud_sync_panel")
            .default_width(340.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("☁️ 云端同步");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("✕").clicked() {
                            self.open = false;
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
                ui.label(egui::RichText::new("同步频率（分钟，0=仅手动）").color(theme.fg_medium_color()));
                ui.add(egui::DragValue::new(&mut self.settings.frequency_minutes).speed(1.0));

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
                    if ui.button("立即导出包…").on_hover_text("复制到选定目录").clicked() {
                        self.message = "请在弹窗中选择目录…".to_string();
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("从包导入…").clicked() {
                        self.message = "打开目录选择…".to_string();
                    }
                });

                if !self.message.is_empty() {
                    ui.label(
                        egui::RichText::new(&self.message)
                            .small()
                            .color(theme.fg_low_color()),
                    );
                }

                ui.collapsing("说明", |ui| {
                    ui.label("当前为本地同步包（文件夹复制）。连接账户与实时同步将在后续版本提供。");
                });
            });
    }
}

impl CloudSyncSettings {
    fn last_import_hint(&mut self) {
        self.last_sync_unix = Some(chrono::Utc::now().timestamp());
        let _ = self.save();
    }
}
