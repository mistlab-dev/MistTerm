//! 云端同步 UI：设置项 + 导出/导入本地包（MVP，无真实账户 API）

use std::fs;
use std::path::{Path, PathBuf};

use eframe::egui;
use rfd::FileDialog;

use crate::core::{
    AuditCategory, AuditEvent, AuditLogger, AuditOutcome, CloudSyncSettings, CredentialVault,
    FragmentManager, FragmentMergeReport, SessionManager, SortBy,
};
use crate::ui::credential_panel::CredentialPanel;
use crate::ui::chrome;
use crate::ui::layout_util::{self, SidePanelProfile};
use crate::ui::theme::{Theme, ThemeManager};

/// 导出/导入所需的本地路径与可变引用（由主窗口注入）
pub struct CloudSyncDeps<'a> {
    pub fragments_path: &'a PathBuf,
    pub sessions_path: &'a PathBuf,
    pub theme_path: &'a PathBuf,
    pub fragment_manager: &'a mut FragmentManager,
    pub theme_manager: &'a mut ThemeManager,
    pub session_manager: &'a mut SessionManager,
    pub credential_panel: &'a mut CredentialPanel,
    pub audit: Option<&'a AuditLogger>,
}

/// 同步面板（右侧栏）
pub struct CloudSyncPanel {
    pub open: bool,
    pub settings: CloudSyncSettings,
    pub message: String,
    /// 从同步包导入片段时是否与现有条目按 id 合并（否则整库替换）
    pub merge_fragments_on_package_import: bool,
    pending_import_dir: Option<PathBuf>,
}

impl CloudSyncPanel {
    pub fn new() -> Self {
        Self {
            open: false,
            settings: CloudSyncSettings::load(),
            message: String::new(),
            merge_fragments_on_package_import: true,
            pending_import_dir: None,
        }
    }

    fn save_settings(&mut self) {
        match self.settings.save() {
            Ok(()) => self.message = "已保存设置".to_string(),
            Err(e) => self.message = format!("保存失败：{}", e),
        }
    }

    fn paint_capability_banner(ui: &mut egui::Ui, theme: &Theme) {
        egui::Frame::none()
            .fill(theme.color_subtle_inset_fill())
            .rounding(theme.radius_list_item())
            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
            .show(ui, |ui| {
                ui.label(
                    chrome::rich_caption(theme, "当前可用")
                        .strong()
                        .color(theme.green_color()),
                );
                ui.label(
                    chrome::rich_caption(
                        theme,
                        "手动导出 / 导入文件夹包：会话、片段、主题、凭证加密文件。",
                    ),
                );
                ui.add_space(4.0);
                ui.label(
                    chrome::rich_caption(theme, "尚未接入")
                        .strong()
                        .color(theme.text_tertiary()),
                );
                ui.label(chrome::rich_caption(theme, "远程账户、定时自动同步、团队与快捷键。"));
            });
    }

    fn paint_sync_status(ui: &mut egui::Ui, theme: &Theme, settings: &CloudSyncSettings) {
        if let Some(ts) = settings.last_sync_unix {
            let t = chrono::DateTime::from_timestamp(ts, 0)
                .map(|x| x.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "—".to_string());
            ui.label(chrome::rich_caption(theme, &format!("最近操作：{t}")));
        } else {
            ui.label(chrome::rich_caption(theme, "最近操作：尚未记录"));
        }
        if !settings.last_error.is_empty() {
            ui.label(
                chrome::rich_caption(theme, &settings.last_error).color(theme.red_color()),
            );
        }
    }

    fn package_requires_import_confirm(dir: &Path, settings: &CloudSyncSettings) -> bool {
        let sessions = dir.join("sessions.json");
        let credentials = dir.join("credentials.json");
        (settings.sync_sessions && sessions.exists())
            || (settings.sync_credentials && credentials.exists())
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
        let mut wrote: Vec<String> = Vec::new();

        if self.settings.sync_fragments && deps.fragments_path.exists() {
            match fs::copy(deps.fragments_path, dest.join("fragments.json")) {
                Ok(_) => wrote.push("fragments.json".into()),
                Err(e) => err = Some(format!("fragments.json：{}", e)),
            }
        }

        if err.is_none() && self.settings.sync_sessions && deps.sessions_path.exists() {
            match fs::copy(deps.sessions_path, dest.join("sessions.json")) {
                Ok(_) => wrote.push("sessions.json".into()),
                Err(e) => err = Some(format!("sessions.json：{}", e)),
            }
        }

        if err.is_none() && self.settings.sync_themes && deps.theme_path.exists() {
            match fs::copy(deps.theme_path, dest.join("theme.json")) {
                Ok(_) => wrote.push("theme.json".into()),
                Err(e) => err = Some(format!("theme.json：{}", e)),
            }
        }

        if err.is_none() && self.settings.sync_shortcuts {
            if fs::write(
                dest.join("shortcuts.json"),
                r#"{"note":"快捷键配置占位符"}"#,
            )
            .is_ok()
            {
                wrote.push("shortcuts.json（占位）".into());
            }
        }

        if err.is_none() && self.settings.sync_credentials {
            let cred = CredentialVault::default_path();
            if cred.exists() {
                match fs::copy(&cred, dest.join("credentials.json")) {
                    Ok(_) => wrote.push("credentials.json".into()),
                    Err(e) => err = Some(format!("credentials.json：{}", e)),
                }
            }
        }

        if let Ok(s) = serde_json::to_string_pretty(&self.settings) {
            if fs::write(dest.join("cloud_sync_snapshot.json"), s).is_ok() {
                wrote.push("cloud_sync_snapshot.json".into());
            }
        }

        if let Some(e) = err {
            self.settings.mark_sync_err(e.clone());
            self.message = e;
        } else {
            self.settings.mark_sync_ok();
            if let Some(audit) = deps.audit {
                audit.record(
                    AuditEvent::new(
                        AuditCategory::Config,
                        "cloud_sync.export",
                        AuditOutcome::Success,
                    )
                    .with_detail(serde_json::json!({
                        "dest": dest.display().to_string(),
                        "files": wrote,
                    })),
                );
            }
            let preview = if wrote.is_empty() {
                "（未勾选可导出项或源文件缺失）".to_string()
            } else {
                wrote.join("、")
            };
            self.message = format!(
                "已导出到 {}\n包含：{}",
                dest.display(),
                preview
            );
        }
    }

    fn pick_import_folder(&mut self, deps: &mut CloudSyncDeps<'_>) {
        let Some(dir) = FileDialog::new()
            .set_title("选择同步包目录（含 fragments.json、sessions.json 等）")
            .pick_folder()
        else {
            return;
        };
        if Self::package_requires_import_confirm(&dir, &self.settings) {
            self.pending_import_dir = Some(dir);
            self.message =
                "已选择同步包。若勾选覆盖会话或凭证库，请先确认后继续。".to_string();
        } else {
            self.message = Self::perform_import_package(
                &dir,
                self.merge_fragments_on_package_import,
                &mut self.settings,
                deps,
            );
        }
    }

    fn perform_import_package(
        dir: &Path,
        merge_fragments: bool,
        settings: &mut CloudSyncSettings,
        deps: &mut CloudSyncDeps<'_>,
    ) -> String {
        let mut parts = Vec::<String>::new();

        let frag_src = dir.join("fragments.json");
        if settings.sync_fragments && frag_src.exists() {
            match FragmentManager::import_from_json_path(
                &frag_src,
                merge_fragments,
                deps.fragment_manager,
            ) {
                Ok(FragmentMergeReport {
                    added,
                    skipped_duplicate_id,
                }) => {
                    if merge_fragments {
                        deps.fragment_manager.sort(SortBy::UsageCount);
                    }
                    if deps.fragment_manager.save(deps.fragments_path).is_ok() {
                        if merge_fragments {
                            parts.push(format!(
                                "命令片段：新增 {}，跳过已有 id {}",
                                added, skipped_duplicate_id
                            ));
                        } else {
                            parts.push(format!("命令片段：已替换为 {} 条", added));
                        }
                    } else {
                        return format!("写入 {} 失败", deps.fragments_path.display());
                    }
                }
                Err(e) => return format!("读取 fragments 失败：{}", e),
            }
        }

        let sessions_src = dir.join("sessions.json");
        if settings.sync_sessions && sessions_src.exists() {
            match deps
                .session_manager
                .import_sessions_from_file_path(&sessions_src)
            {
                Ok(()) => parts.push("会话：已从包还原".into()),
                Err(e) => return format!("导入 sessions 失败：{}", e),
            }
        }

        let theme_src = dir.join("theme.json");
        if settings.sync_themes && theme_src.exists() {
            if let Ok(txt) = fs::read_to_string(&theme_src) {
                if let Ok(tm) = serde_json::from_str::<ThemeManager>(&txt) {
                    *deps.theme_manager = tm;
                    deps.theme_manager.save();
                    parts.push("主题：已从包还原".into());
                }
            }
        }

        let credentials_src = dir.join("credentials.json");
        if settings.sync_credentials && credentials_src.exists() {
            match CredentialVault::restore_from_file_into_default_location(&credentials_src) {
                Ok(()) => {
                    deps.credential_panel.reload_after_external_file_replace();
                    parts.push("凭证库：已从包覆盖并刷新侧栏缓存".into());
                }
                Err(e) => return format!("导入 credentials 失败：{}", e),
            }
        }

        if parts.is_empty() {
            let _ = settings.save();
            return "所选目录无可导入项（请勾选同步项且包内需含对应文件）".to_string();
        }

        settings.record_manual_import_ok();
        if let Some(audit) = deps.audit {
            audit.record(
                AuditEvent::new(
                    AuditCategory::Config,
                    "cloud_sync.import",
                    AuditOutcome::Success,
                )
                .with_detail(serde_json::json!({
                    "dir": dir.display().to_string(),
                    "summary": parts,
                })),
            );
        }
        parts.join(" · ")
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        deps: &mut CloudSyncDeps<'_>,
        right_dock_outer_left: &mut Option<f32>,
    ) {
        if !self.open {
            return;
        }

        let mut close_me = false;
        let (cl_def, cl_min, cl_max) = layout_util::side_panel_widths(ctx, SidePanelProfile::Standard);
        let panel = egui::SidePanel::right("cloud_sync_panel")
            .default_width(cl_def)
            .min_width(cl_min)
            .max_width(cl_max)
            .resizable(true)
            .frame(crate::ui::chrome::right_dock_panel_frame(theme))
            .show(ctx, |ui| {
                let panel_w = layout_util::dock_panel_content_width(ui, cl_min, cl_max);
                ui.set_max_width(panel_w);
                let mut header_closed = false;
                theme.frame_panel_header_band().show(ui, |ui| {
                    header_closed = chrome::dock_panel_title_close_only(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Cloud,
                        "云端同步",
                        "关闭云端同步",
                    );
                });
                if header_closed {
                    close_me = true;
                }
                chrome::panel_header_divider(ui, theme);
                ui.label(
                    chrome::rich_caption(
                        theme,
                        "本地同步包：在文件夹间备份/恢复配置（非在线云盘）",
                    ),
                );
                ui.add_space(theme.spacing_sm());

                let cloud_scroll_h = layout_util::scroll_area_fill_height(ui, 140.0);
                egui::ScrollArea::vertical()
                    .max_height(cloud_scroll_h)
                    .show(ui, |ui| {
                        ui.set_width(ui.max_rect().width());
                        Self::paint_capability_banner(ui, theme);

                        ui.add_space(theme.spacing_panel_gap());
                        chrome::form_field_label(ui, theme, "远程账户");
                        ui.add_enabled_ui(false, |ui| {
                            ui.label(
                                chrome::rich_body(theme, "未登录（后续版本对接）")
                                    .weak(),
                            );
                        });

                        ui.add_space(theme.spacing_panel_gap());
                        chrome::form_field_label(ui, theme, "包内包含项");
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            if chrome::chrome_small_button(ui, theme, "全选").clicked() {
                                self.settings.sync_sessions = true;
                                self.settings.sync_fragments = true;
                                self.settings.sync_themes = true;
                                self.settings.sync_credentials = true;
                            }
                            if chrome::chrome_small_button(ui, theme, "仅核心").clicked() {
                                self.settings.sync_sessions = true;
                                self.settings.sync_fragments = true;
                                self.settings.sync_themes = true;
                                self.settings.sync_credentials = false;
                                self.settings.sync_shortcuts = false;
                            }
                            if chrome::chrome_small_button(ui, theme, "清空").clicked() {
                                self.settings.sync_sessions = false;
                                self.settings.sync_fragments = false;
                                self.settings.sync_themes = false;
                                self.settings.sync_credentials = false;
                                self.settings.sync_shortcuts = false;
                            }
                        });
                        ui.add_space(4.0);
                        ui.columns(2, |cols| {
                            cols[0].checkbox(&mut self.settings.sync_sessions, "会话");
                            cols[1].checkbox(&mut self.settings.sync_fragments, "命令片段");
                            cols[0].checkbox(&mut self.settings.sync_themes, "主题");
                            cols[1].checkbox(&mut self.settings.sync_credentials, "凭证库");
                        });
                        ui.label(
                            chrome::rich_caption(theme, "凭证为加密文件副本，换机需相同设备密钥。")
                                .weak(),
                        );
                        ui.add_enabled_ui(false, |ui| {
                            let mut off = false;
                            ui.checkbox(&mut off, "快捷键");
                            ui.checkbox(&mut off, "团队配置");
                        });
                        ui.label(
                            chrome::rich_caption(theme, "快捷键 / 团队：尚未实现，勾选无效。")
                                .weak(),
                        );
                        ui.label(
                            chrome::rich_caption(
                                theme,
                                "SSH 密码不会写入包内，导入后请在各设备本地填写。",
                            )
                            .weak(),
                        );

                        ui.add_space(theme.spacing_panel_gap());
                        chrome::form_field_label(ui, theme, "自动同步间隔");
                        ui.horizontal(|ui| {
                            chrome::form_drag_value_field(
                                ui,
                                theme,
                                egui::Id::new("cloud_sync_freq_min"),
                                |ui| {
                                    ui.add(
                                        egui::DragValue::new(&mut self.settings.frequency_minutes)
                                            .speed(1.0)
                                            .prefix("每 "),
                                    )
                                },
                            );
                            ui.label(chrome::rich_caption(theme, "分钟（0 = 仅手动）"));
                        });
                        ui.label(
                            chrome::rich_caption(
                                theme,
                                "当前版本不会按间隔自动导出，请用下方按钮手动操作。",
                            )
                            .weak(),
                        );

                        ui.add_space(theme.spacing_panel_gap());
                        chrome::form_field_label(ui, theme, "导入片段");
                        ui.checkbox(
                            &mut self.merge_fragments_on_package_import,
                            "与现有库合并（按 id 跳过重复）",
                        );
                        ui.label(
                            chrome::rich_caption(
                                theme,
                                "关闭上项则整库替换。会话/凭证导入可能覆盖本机文件。",
                            )
                            .weak(),
                        );

                        ui.add_space(theme.spacing_panel_gap());
                        Self::paint_sync_status(ui, theme, &self.settings);

                        ui.add_space(theme.spacing_panel_gap());
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 6.0;
                            if chrome::panel_action_button(ui, theme, "保存勾选与间隔").clicked() {
                                self.save_settings();
                            }
                            if chrome::panel_action_primary_button(ui, theme, "导出同步包…")
                                .on_hover_text("新建 mistterm-sync-时间戳 目录并复制勾选文件")
                                .clicked()
                            {
                                self.run_export(deps);
                            }
                            if chrome::panel_action_button(ui, theme, "从同步包导入…").clicked() {
                                self.pick_import_folder(deps);
                            }
                        });
                    });

                if let Some(dir) = self.pending_import_dir.clone() {
                    ui.add_space(theme.spacing_panel_gap());
                    ui.group(|ui| {
                        crate::ui::icons::icon_label_row(
                            ui,
                            crate::ui::icons::IconId::Warning,
                            "导入确认",
                            theme.font_size_body(),
                            6.0,
                            |t| t.strong().color(theme.red_color()),
                        );
                        ui.label(
                            egui::RichText::new(format!(
                                "包路径： {}",
                                dir.display()
                            ))
                            .small(),
                        );
                        if self.settings.sync_sessions && dir.join("sessions.json").exists() {
                            ui.label("• 将用包内 sessions.json 替换当前会话列表。");
                        }
                        if self.settings.sync_credentials && dir.join("credentials.json").exists() {
                            ui.label("• 将用包内凭证库覆盖本机加密文件（需同源设备密钥才解密）。");
                        }
                        ui.horizontal(|ui| {
                            if chrome::panel_action_button(ui, theme, "取消").clicked() {
                                self.pending_import_dir = None;
                                self.message = "已取消导入".to_string();
                            }
                            if chrome::panel_action_primary_button(ui, theme, "确认导入").clicked() {
                                let msg = Self::perform_import_package(
                                    &dir,
                                    self.merge_fragments_on_package_import,
                                    &mut self.settings,
                                    deps,
                                );
                                self.message = msg;
                                self.pending_import_dir = None;
                            }
                        });
                    });
                }

                if !self.message.is_empty() {
                    ui.add_space(theme.spacing_sm());
                    egui::Frame::none()
                        .fill(theme.color_subtle_inset_fill())
                        .rounding(theme.radius_list_item())
                        .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                        .show(ui, |ui| {
                            ui.label(
                                chrome::rich_caption(theme, &self.message)
                                    .color(theme.text_secondary()),
                            );
                        });
                }
            });
        layout_util::record_right_dock_panel(&panel.response, right_dock_outer_left);

        if close_me {
            self.pending_import_dir = None;
            self.open = false;
        }
    }
}
