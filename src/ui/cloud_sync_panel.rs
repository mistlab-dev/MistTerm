//! 云端同步 UI：设置项 + 导出/导入本地包（MVP，无真实账户 API）

use std::fs;
use std::path::{Path, PathBuf};

use eframe::egui;
use rfd::FileDialog;

use crate::core::{
    AppSettings, AuditCategory, AuditEvent, AuditLogger, AuditOutcome, CloudSyncSettings,
    CredentialVault, FragmentManager, FragmentMergeReport, SessionManager, SortBy, TeamService,
};
use crate::ui::team_ui::{paint_team_controls, TeamLoginForm, TeamUiAction};
use crate::i18n::{self, UiLanguage};
use crate::ui::credential_panel::CredentialPanel;
use crate::ui::chrome;
use crate::ui::layout_util;
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
    /// 本帧 `SidePanel` 槽位（Central 之后 Foreground 重绘用）。
    last_panel_slot_rect: Option<egui::Rect>,
}

impl CloudSyncPanel {
    pub fn new() -> Self {
        Self {
            open: false,
            settings: CloudSyncSettings::load(),
            message: String::new(),
            merge_fragments_on_package_import: true,
            pending_import_dir: None,
            last_panel_slot_rect: None,
        }
    }

    fn save_settings(&mut self, lang: UiLanguage) {
        let loc = i18n::Locale::from(lang);
        match self.settings.save() {
            Ok(()) => self.message = loc.tr("Settings saved", "已保存设置").to_string(),
            Err(e) => {
                self.message = format!(
                    "{}{}",
                    loc.tr("Save failed: ", "保存失败："),
                    e
                )
            }
        }
    }

    fn paint_capability_banner(ui: &mut egui::Ui, theme: &Theme, lang: UiLanguage) {
        let loc = i18n::Locale::from(lang);
        egui::Frame::none()
            .fill(theme.color_subtle_inset_fill())
            .rounding(theme.radius_list_item())
            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
            .show(ui, |ui| {
                ui.label(
                    chrome::rich_caption(
                        theme,
                        loc.tr(
                            "Ready",
                            "当前可用",
                        ),
                    )
                        .strong()
                        .color(theme.green_color()),
                );
                ui.label(
                    chrome::rich_caption(
                        theme,
                        loc.tr(
                            "Export / import a folder pack: sessions, fragments, themes, encrypted credentials.",
                            "手动导出 / 导入文件夹包：会话、片段、主题、凭证加密文件。",
                        ),
                    ),
                );
                ui.add_space(4.0);
                ui.label(
                    chrome::rich_caption(
                        theme,
                        loc.tr("Not connected yet", "尚未接入"),
                    )
                        .strong()
                        .color(theme.text_tertiary()),
                );
                ui.label(chrome::rich_caption(
                    theme,
                    loc.tr(
                        "Remote account, scheduled sync, teams, and shortcuts.",
                        "远程账户、定时自动同步、团队与快捷键。",
                    ),
                ));
            });
    }

    fn paint_sync_status(ui: &mut egui::Ui, theme: &Theme, settings: &CloudSyncSettings, lang: UiLanguage) {
        let loc = i18n::Locale::from(lang);
        if let Some(ts) = settings.last_sync_unix {
            let t = chrono::DateTime::from_timestamp(ts, 0)
                .map(|x| x.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "—".to_string());
            ui.label(chrome::rich_caption(
                theme,
                &format!(
                    "{}{}",
                    loc.tr("Last action: ", "最近操作："),
                    t
                ),
            ));
        } else {
            ui.label(chrome::rich_caption(
                theme,
                loc.tr("Last action: none yet", "最近操作：尚未记录"),
            ));
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

    fn run_export(&mut self, deps: &CloudSyncDeps<'_>, lang: UiLanguage) {
        let loc = i18n::Locale::from(lang);
        let Some(parent) = FileDialog::new()
            .set_title(loc.tr(
                "Choose export folder",
                "选择导出目录",
            ))
            .pick_folder()
        else {
            return;
        };

        let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
        let dest = parent.join(format!("mistterm-sync-{}", stamp));
        if let Err(e) = fs::create_dir_all(&dest) {
            self.settings.mark_sync_err(format!(
                "{}{}",
                loc.tr("Create folder failed: ", "创建目录失败："),
                e
            ));
            self.message.clone_from(&self.settings.last_error);
            return;
        }

        let mut err: Option<String> = None;
        let mut wrote: Vec<String> = Vec::new();

        if self.settings.sync_fragments && deps.fragments_path.exists() {
            match fs::copy(deps.fragments_path, dest.join("fragments.json")) {
                Ok(_) => wrote.push("fragments.json".into()),
                Err(e) => {
                    err = Some(match lang {
                        UiLanguage::En => format!("fragments.json: {e}"),
                        UiLanguage::Zh => format!("fragments.json：{e}"),
                    });
                }
            }
        }

        if err.is_none() && self.settings.sync_sessions && deps.sessions_path.exists() {
            match fs::copy(deps.sessions_path, dest.join("sessions.json")) {
                Ok(_) => wrote.push("sessions.json".into()),
                Err(e) => {
                    err = Some(match lang {
                        UiLanguage::En => format!("sessions.json: {e}"),
                        UiLanguage::Zh => format!("sessions.json：{e}"),
                    });
                }
            }
        }

        if err.is_none() && self.settings.sync_themes && deps.theme_path.exists() {
            match fs::copy(deps.theme_path, dest.join("theme.json")) {
                Ok(_) => wrote.push("theme.json".into()),
                Err(e) => {
                    err = Some(match lang {
                        UiLanguage::En => format!("theme.json: {e}"),
                        UiLanguage::Zh => format!("theme.json：{e}"),
                    });
                }
            }
        }

        if err.is_none() && self.settings.sync_shortcuts {
            if fs::write(
                dest.join("shortcuts.json"),
                r#"{"note":"shortcuts placeholder"}"#,
            )
            .is_ok()
            {
                wrote.push(
                    format!(
                        "shortcuts.json{}",
                        loc.tr(" (placeholder)", "（占位）")
                    ),
                );
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
                loc.tr(
                    "(Nothing selected to export or sources missing)",
                    "（未勾选可导出项或源文件缺失）",
                )
                .to_string()
            } else {
                wrote.join(match lang {
                    UiLanguage::En => ", ",
                    UiLanguage::Zh => "、",
                })
            };
            self.message = match lang {
                UiLanguage::En => format!(
                    "Exported to {}\nIncluded: {}",
                    dest.display(),
                    preview
                ),
                UiLanguage::Zh => format!(
                    "已导出到 {}\n包含：{}",
                    dest.display(),
                    preview
                ),
            };
        }
    }

    fn pick_import_folder(&mut self, deps: &mut CloudSyncDeps<'_>, lang: UiLanguage) {
        let loc = i18n::Locale::from(lang);
        let Some(dir) = FileDialog::new()
            .set_title(loc.tr(
                "Choose sync pack folder (fragments.json, sessions.json, …)",
                "选择同步包目录（含 fragments.json、sessions.json 等）",
            ))
            .pick_folder()
        else {
            return;
        };
        if Self::package_requires_import_confirm(&dir, &self.settings) {
            self.pending_import_dir = Some(dir);
            self.message = loc
                .tr(
                    "Sync pack chosen. If sessions or credentials will be overwritten, confirm below.",
                    "已选择同步包。若勾选覆盖会话或凭证库，请先确认后继续。",
                )
                .to_string();
        } else {
            self.message = Self::perform_import_package(
                &dir,
                self.merge_fragments_on_package_import,
                &mut self.settings,
                deps,
                lang,
            );
        }
    }

    fn perform_import_package(
        dir: &Path,
        merge_fragments: bool,
        settings: &mut CloudSyncSettings,
        deps: &mut CloudSyncDeps<'_>,
        lang: UiLanguage,
    ) -> String {
        let loc = i18n::Locale::from(lang);
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
                            parts.push(match lang {
                                UiLanguage::En => format!(
                                    "Fragments: added {added}, skipped duplicate id {skipped_duplicate_id}"
                                ),
                                UiLanguage::Zh => format!(
                                    "命令片段：新增 {added}，跳过已有 id {skipped_duplicate_id}"
                                ),
                            });
                        } else {
                            parts.push(match lang {
                                UiLanguage::En => format!("Fragments: replaced with {added} entries"),
                                UiLanguage::Zh => format!("命令片段：已替换为 {added} 条"),
                            });
                        }
                    } else {
                        return match lang {
                            UiLanguage::En => format!("Write {} failed", deps.fragments_path.display()),
                            UiLanguage::Zh => format!("写入 {} 失败", deps.fragments_path.display()),
                        };
                    }
                }
                Err(e) => {
                    return format!(
                        "{}{}",
                        loc.tr("Failed to read fragments: ", "读取 fragments 失败："),
                        e
                    );
                }
            }
        }

        let sessions_src = dir.join("sessions.json");
        if settings.sync_sessions && sessions_src.exists() {
            match deps
                .session_manager
                .import_sessions_from_file_path(&sessions_src)
            {
                Ok(()) => {
                    parts.push(
                        loc.tr("Sessions: restored from pack", "会话：已从包还原")
                            .to_string(),
                    );
                }
                Err(e) => {
                    return format!(
                        "{}{}",
                        loc.tr("Import sessions failed: ", "导入 sessions 失败："),
                        e
                    );
                }
            }
        }

        let theme_src = dir.join("theme.json");
        if settings.sync_themes && theme_src.exists() {
            if let Ok(txt) = fs::read_to_string(&theme_src) {
                if let Ok(tm) = serde_json::from_str::<ThemeManager>(&txt) {
                    *deps.theme_manager = tm;
                    deps.theme_manager.save();
                    parts.push(
                        loc.tr("Theme: restored from pack", "主题：已从包还原")
                            .to_string(),
                    );
                }
            }
        }

        let credentials_src = dir.join("credentials.json");
        if settings.sync_credentials && credentials_src.exists() {
            match CredentialVault::restore_from_file_into_default_location(&credentials_src) {
                Ok(()) => {
                    deps.credential_panel.reload_after_external_file_replace();
                    parts.push(
                        loc.tr(
                            "Credentials: overwritten; sidebar cache refreshed",
                            "凭证库：已从包覆盖并刷新侧栏缓存",
                        )
                        .to_string(),
                    );
                }
                Err(e) => {
                    return format!(
                        "{}{}",
                        loc.tr("Import credentials failed: ", "导入 credentials 失败："),
                        e
                    );
                }
            }
        }

        if parts.is_empty() {
            let _ = settings.save();
            return loc
                .tr(
                    "Nothing importable here (enable items and ensure files exist)",
                    "所选目录无可导入项（请勾选同步项且包内需含对应文件）",
                )
                .to_string();
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

    /// 仅占右栏布局槽（正文在 Central 之后 [`show_foreground_panel`] 绘制）。
    pub fn show_side_panel(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        right_dock_outer_left: &mut Option<f32>,
        dock_col_w: f32,
    ) {
        if !self.open {
            self.last_panel_slot_rect = None;
            return;
        }

        let (def_w, min_w, max_w) =
            crate::ui::layout_util::right_dock_resize_bounds(dock_col_w);
        let panel = egui::SidePanel::right("cloud_sync_panel")
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

    /// Central 之后绘制云端同步正文（避免被 CentralPanel 盖住）。
    pub fn show_foreground_panel(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        deps: &mut CloudSyncDeps<'_>,
        close_panel: &mut bool,
        team_service: Option<&mut TeamService>,
        team_form: Option<&mut TeamLoginForm>,
        app_settings: Option<&mut AppSettings>,
    ) {
        if !self.open {
            return;
        }

        let screen = ctx.screen_rect();
        let dock_inset = theme.spacing_right_dock_screen_inset();
        let Some(slot) = layout_util::right_dock_foreground_slot(
            self.last_panel_slot_rect,
            ctx,
            "cloud_sync_panel",
            layout_util::SidePanelProfile::Standard,
            None,
            dock_inset,
        ) else {
            return;
        };
        let geom = chrome::prepare_right_dock_foreground_geom(slot, screen, theme);
        let layer_id = chrome::right_dock_foreground_layer_id("mistterm_cloud_sync_fg");
        chrome::paint_right_dock_foreground_shell(ctx, layer_id, geom.paint, theme);
        chrome::show_right_dock_foreground_body(
            "mistterm_cloud_sync_fg",
            ctx,
            &geom,
            layout_util::SidePanelProfile::Standard,
            |ui, _body_w| {
                let lang = i18n::language(ctx);
                let loc = i18n::Locale::from(lang);
                let panel_w = ui.available_width();
                ui.set_max_width(panel_w);
                let mut header_closed = false;
                theme.frame_right_dock_header_band().show(ui, |ui| {
                    header_closed = chrome::dock_panel_title_close_only(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Cloud,
                        i18n::tr(ctx, "Cloud Sync", "云端同步"),
                        i18n::tr(ctx, "Close Cloud Sync", "关闭云端同步"),
                    );
                });
                if header_closed {
                    *close_panel = true;
                }
                chrome::right_dock_header_divider(ui, theme);
                ui.label(chrome::rich_caption(
                    theme,
                    i18n::tr(
                        ctx,
                        "Local sync pack: back up / restore config between folders (not online cloud storage)",
                        "本地同步包：在文件夹间备份/恢复配置（非在线云盘）",
                    ),
                ));
                ui.add_space(theme.spacing_sm());

                let cloud_scroll_h = layout_util::scroll_area_fill_height(ui, 140.0);
                egui::ScrollArea::vertical()
                    .max_height(cloud_scroll_h)
                    .show(ui, |ui| {
                        ui.set_width(ui.max_rect().width());
                        Self::paint_capability_banner(ui, theme, lang);

                        ui.add_space(theme.spacing_panel_gap());
                        if let (Some(service), Some(form), Some(settings)) =
                            (team_service, team_form, app_settings)
                        {
                            chrome::form_field_label(
                                ui,
                                theme,
                                i18n::tr(ctx, "Team account", "团队账户"),
                            );
                            let pref_w = ui.available_width();
                            let action = paint_team_controls(
                                ui,
                                ctx,
                                theme,
                                service,
                                form,
                                deps.audit,
                                pref_w,
                                "cloud_sync_team",
                            );
                            if matches!(action, TeamUiAction::LoggedOut) {
                                let _ = settings.save();
                            }
                            ui.add_space(theme.spacing_panel_gap());
                        }

                        ui.add_space(theme.spacing_panel_gap());
                        chrome::form_field_label(ui, theme, i18n::tr(ctx, "Pack contents", "包内包含项"));
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            if chrome::chrome_small_icon_button(ui, theme, crate::ui::icons::IconId::Check)
                                .on_hover_text(i18n::tr(ctx, "Select all", "全选"))
                                .clicked() {
                                self.settings.sync_sessions = true;
                                self.settings.sync_fragments = true;
                                self.settings.sync_themes = true;
                                self.settings.sync_credentials = true;
                            }
                            if chrome::chrome_small_icon_button(ui, theme, crate::ui::icons::IconId::Server)
                                .on_hover_text(i18n::tr(ctx, "Core only", "仅核心"))
                                .clicked() {
                                self.settings.sync_sessions = true;
                                self.settings.sync_fragments = true;
                                self.settings.sync_themes = true;
                                self.settings.sync_credentials = false;
                                self.settings.sync_shortcuts = false;
                            }
                            if chrome::chrome_small_icon_button(ui, theme, crate::ui::icons::IconId::Trash)
                                .on_hover_text(i18n::tr(ctx, "Clear", "清空"))
                                .clicked() {
                                self.settings.sync_sessions = false;
                                self.settings.sync_fragments = false;
                                self.settings.sync_themes = false;
                                self.settings.sync_credentials = false;
                                self.settings.sync_shortcuts = false;
                            }
                        });
                        ui.add_space(4.0);
                        ui.columns(2, |cols| {
                            chrome::form_checkbox(
                                &mut cols[0],
                                theme,
                                &mut self.settings.sync_sessions,
                                loc.tr("Sessions", "会话"),
                            );
                            chrome::form_checkbox(
                                &mut cols[1],
                                theme,
                                &mut self.settings.sync_fragments,
                                loc.tr("Fragments", "命令片段"),
                            );
                            chrome::form_checkbox(
                                &mut cols[0],
                                theme,
                                &mut self.settings.sync_themes,
                                loc.tr("Themes", "主题"),
                            );
                            chrome::form_checkbox(
                                &mut cols[1],
                                theme,
                                &mut self.settings.sync_credentials,
                                loc.tr("Credentials", "凭证库"),
                            );
                        });
                        ui.label(chrome::rich_caption(
                            theme,
                            loc.tr(
                                "Credentials are encrypted copies; same device key required on another machine.",
                                "凭证为加密文件副本，换机需相同设备密钥。",
                            ),
                        )
                        .weak());
                        ui.add_enabled_ui(false, |ui| {
                            let mut off = false;
                            chrome::form_checkbox(ui, theme, &mut off, loc.tr("Shortcuts", "快捷键"));
                            chrome::form_checkbox(ui, theme, &mut off, loc.tr("Team config", "团队配置"));
                        });
                        ui.label(chrome::rich_caption(
                            theme,
                            loc.tr(
                                "Shortcuts / team: not implemented; checkboxes have no effect.",
                                "快捷键 / 团队：尚未实现，勾选无效。",
                            ),
                        )
                        .weak());
                        ui.label(chrome::rich_caption(
                            theme,
                            loc.tr(
                                "SSH passwords are not included in the pack; enter them locally after import.",
                                "SSH 密码不会写入包内，导入后请在各设备本地填写。",
                            ),
                        )
                        .weak());

                        ui.add_space(theme.spacing_panel_gap());
                        chrome::form_field_label(
                            ui,
                            theme,
                            loc.tr("Auto-sync interval", "自动同步间隔"),
                        );
                        ui.horizontal(|ui| {
                            chrome::form_drag_value_field(
                                ui,
                                theme,
                                egui::Id::new("cloud_sync_freq_min"),
                                |ui| {
                                    ui.add(
                                        egui::DragValue::new(&mut self.settings.frequency_minutes)
                                            .speed(1.0)
                                            .prefix(loc.tr("Every ", "每 ")),
                                    )
                                },
                            );
                            ui.label(chrome::rich_caption(
                                theme,
                                loc.tr("minutes (0 = manual only)", "分钟（0 = 仅手动）"),
                            ));
                        });
                        ui.label(chrome::rich_caption(
                            theme,
                            loc.tr(
                                "This version does not auto-export on a schedule; use the buttons below.",
                                "当前版本不会按间隔自动导出，请用下方按钮手动操作。",
                            ),
                        )
                        .weak());

                        ui.add_space(theme.spacing_panel_gap());
                        chrome::form_field_label(
                            ui,
                            theme,
                            loc.tr("Fragment import", "导入片段"),
                        );
                        chrome::form_checkbox(
                            ui,
                            theme,
                            &mut self.merge_fragments_on_package_import,
                            loc.tr("Merge with existing library (skip duplicate ids)", "与现有库合并（按 id 跳过重复）"),
                        );
                        ui.label(chrome::rich_caption(
                            theme,
                            loc.tr(
                                "When off, the whole library is replaced. Session/credential import may overwrite local files.",
                                "关闭上项则整库替换。会话/凭证导入可能覆盖本机文件。",
                            ),
                        )
                        .weak());

                        ui.add_space(theme.spacing_panel_gap());
                        Self::paint_sync_status(ui, theme, &self.settings, lang);

                        ui.add_space(theme.spacing_panel_gap());
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 6.0;
                            if chrome::panel_action_icon_button(
                                ui,
                                theme,
                                crate::ui::icons::IconId::Check,
                                loc.tr("Save selection & interval", "保存勾选与间隔"),
                            )
                            .clicked() {
                                self.save_settings(lang);
                            }
                            if chrome::panel_action_primary_icon_button(
                                ui,
                                theme,
                                crate::ui::icons::IconId::Upload,
                                loc.tr("Export sync pack…", "导出同步包…"),
                            )
                            .on_hover_text(loc.tr(
                                "Create mistterm-sync-timestamp folder and copy selected files",
                                "新建 mistterm-sync-时间戳 目录并复制勾选文件",
                            ))
                            .clicked()
                            {
                                self.run_export(deps, lang);
                            }
                            if chrome::panel_action_icon_button(
                                ui,
                                theme,
                                crate::ui::icons::IconId::Package,
                                loc.tr("Import from sync pack…", "从同步包导入…"),
                            )
                            .clicked() {
                                self.pick_import_folder(deps, lang);
                            }
                        });
                    });

                if let Some(dir) = self.pending_import_dir.clone() {
                    ui.add_space(theme.spacing_panel_gap());
                    ui.group(|ui| {
                        crate::ui::icons::icon_label_row(
                            ui,
                            crate::ui::icons::IconId::Warning,
                            loc.tr("Confirm import", "导入确认"),
                            theme.font_size_body(),
                            6.0,
                            |t| t.strong().color(theme.red_color()),
                        );
                        ui.label(
                            egui::RichText::new(format!(
                                "{}{}",
                                loc.tr("Pack path: ", "包路径： "),
                                dir.display()
                            ))
                            .small(),
                        );
                        if self.settings.sync_sessions && dir.join("sessions.json").exists() {
                            ui.label(loc.tr(
                                "• Will replace the current session list with sessions.json from the pack.",
                                "• 将用包内 sessions.json 替换当前会话列表。",
                            ));
                        }
                        if self.settings.sync_credentials && dir.join("credentials.json").exists() {
                            ui.label(loc.tr(
                                "• Will overwrite local encrypted credentials (same device key required).",
                                "• 将用包内凭证库覆盖本机加密文件（需同源设备密钥才解密）。",
                            ));
                        }
                        ui.horizontal(|ui| {
                            if chrome::panel_action_icon_button(
                                ui,
                                theme,
                                crate::ui::icons::IconId::Cross,
                                loc.tr("Cancel", "取消"),
                            )
                            .clicked() {
                                self.pending_import_dir = None;
                                self.message =
                                    loc.tr("Import cancelled", "已取消导入").to_string();
                            }
                            if chrome::panel_action_primary_icon_button(
                                ui,
                                theme,
                                crate::ui::icons::IconId::Check,
                                loc.tr("Confirm import", "确认导入"),
                            )
                                .clicked() {
                                let msg = Self::perform_import_package(
                                    &dir,
                                    self.merge_fragments_on_package_import,
                                    &mut self.settings,
                                    deps,
                                    lang,
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
            },
        );

        if *close_panel {
            self.pending_import_dir = None;
            self.open = false;
        }
    }
}
