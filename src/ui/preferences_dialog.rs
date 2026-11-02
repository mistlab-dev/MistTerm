//! 偏好设置弹窗：分块折叠 + 可滚动内容区。

use super::*;
use eframe::egui::RichText;

#[derive(Clone, PartialEq, Eq)]
enum FontPick {
    Preset(crate::platform::TerminalFontPreset),
    Family(String),
}

/// 左栏分类顺序（「同步与数据」并入「团队与同步」，共 10 类）。
const PREFERENCES_NAV: [(&str, &str); 10] = [
    ("General", "常规"),
    ("Appearance", "外观"),
    ("Terminal", "终端"),
    ("Connection", "连接"),
    ("Terminal logs", "终端日志"),
    ("Vault", "密码库"),
    ("Activity audit", "操作审计"),
    ("Team & sync", "团队与同步"),
    ("Monitor alerts", "监控告警"),
    ("Extension modules", "扩展模块"),
];

/// 选中分类下标（egui 持久化，跨帧保存）。
const PREFERENCES_NAV_SELECTED_ID: &str = "preferences_nav_selected";
/// 导航模式标记（temp data）；置位后 `preferences_collapsing` 不再画折叠头。
const PREFERENCES_NAV_MODE_ID: &str = "preferences_nav_mode";

impl MistTermApp {
    /// #282：打开偏好设置并定位「团队与同步」分类（左导航 index 7）。
    /// 供团队成员弹窗「去登录」等入口复用，保持单一入口。
    pub(crate) fn open_preferences_team_sync(&mut self, ctx: &egui::Context) {
        self.open_preferences_dialog();
        let nav_id = egui::Id::unique(PREFERENCES_NAV_SELECTED_ID);
        ctx.data_mut(|d| d.insert_persisted(nav_id, 7usize));
    }

    fn workspace_top_chrome_height(&self, theme: &crate::ui::theme::Theme) -> f32 {
        #[cfg(target_os = "macos")]
        {
            let _ = theme;
            0.0
        }
        #[cfg(not(target_os = "macos"))]
        {
            theme.top_chrome_total_height()
        }
    }

    pub(crate) fn show_preferences_modal(
        &mut self,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
    ) {
        let mut open = self.show_preferences_dialog;
        let mut should_close = false;
        let label_color = theme.color_form_label();
        let text_low = theme.color_form_hint();
        let margin = 16.0;
        let top_inset = self.workspace_top_chrome_height(theme) + margin;
        let bottom_inset = margin;
        let modal_sz = layout_util::modal_pref_size_in_viewport(ctx, top_inset, bottom_inset);
        let modal_pos =
            layout_util::modal_center_pos_clamped(ctx, modal_sz, top_inset, bottom_inset);
        let modal_resp = crate::ui::chrome::modal_window("preferences_modal", theme, ctx)
            .open(&mut open)
            .default_pos(modal_pos)
            .movable(true)
            .resizable(false)
            .fixed_size(modal_sz)
            .show(ctx, |ui| {
                crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                    layout_util::set_width_to_available(ui);
                    ui.vertical(|ui| {
                        Self::modal_header(
                            ui,
                            theme,
                            crate::i18n::tr(ctx, "Preferences", "偏好设置"),
                            &mut should_close,
                        );
                        ui.add_space(theme.spacing_sm());

                        let body_h = (ui.available_height() - theme.spacing_sm()).max(120.0);

                        // #279 导航模式：左栏分类 + 右栏详情。
                        // 用 egui temp data 打标记，让 11 个 section 复用函数在导航模式下只渲染卡片内容。
                        ui.ctx().data_mut(|d| {
                            d.insert_temp(egui::Id::unique(PREFERENCES_NAV_MODE_ID), true)
                        });

                        ui.allocate_ui(egui::vec2(ui.available_width(), body_h), |ui| {
                            layout_util::set_width_to_available(ui);
                            self.preferences_nav_and_detail(
                                ui,
                                ctx,
                                theme,
                                label_color,
                                text_low,
                                &mut should_close,
                            );
                        });
                    });
                });
            });
        if let Some(inner) = &modal_resp {
            crate::ui::chrome::raise_window_response(ctx, &inner.response);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            should_close = true;
        }
        if self.rule_manager_dialog.take_close_preferences() {
            open = false;
        }
        self.show_preferences_dialog = open && !should_close;
    }

    fn preferences_row_divider(ui: &mut egui::Ui, theme: &crate::ui::theme::Theme) {
        crate::ui::chrome::settings_row_divider(ui, theme);
    }

    /// #279 左栏分类 + 右栏详情；右栏独立滚动并铺满可用高度。
    fn preferences_nav_and_detail(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
        label_color: egui::Color32,
        text_low: egui::Color32,
        should_close: &mut bool,
    ) {
        let body_h = ui.available_height().max(120.0);
        let nav_id = egui::Id::unique(PREFERENCES_NAV_SELECTED_ID);
        let mut selected = ui
            .data_mut(|d| d.get_persisted::<usize>(nav_id))
            .unwrap_or(0);
        if selected >= PREFERENCES_NAV.len() {
            selected = 0;
        }

        let nav_w = (ui.available_width() * 0.18).clamp(80.0, 120.0);
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(nav_w, body_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_min_size(egui::vec2(nav_w, body_h));
                    ui.spacing_mut().item_spacing.y = theme.spacing_xs();
                    egui::ScrollArea::vertical()
                        .id_salt("preferences_nav_scroll")
                        .auto_shrink([false; 2])
                        .max_height(body_h)
                        .show(ui, |ui| {
                            layout_util::set_width_to_available(ui);
                            for (idx, (en, zh)) in PREFERENCES_NAV.iter().enumerate() {
                                let label = crate::i18n::tr(ctx, en, zh);
                                if crate::ui::chrome::menu_theme_item(
                                    ui,
                                    theme,
                                    idx == selected,
                                    label,
                                )
                                .clicked()
                                {
                                    selected = idx;
                                }
                            }
                        });
                },
            );
            ui.add_space(theme.spacing_xs());
            ui.separator();
            ui.add_space(theme.spacing_xs());

            let detail_w = ui.available_width().max(80.0);
            ui.allocate_ui_with_layout(
                egui::vec2(detail_w, body_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_min_size(egui::vec2(detail_w, body_h));
                    let (en, zh) = PREFERENCES_NAV[selected];
                    ui.label(
                        RichText::new(crate::i18n::tr(ctx, en, zh))
                            .size(theme.font_size_panel_title())
                            .strong()
                            .color(label_color),
                    );
                    ui.add_space(theme.spacing_sm());
                    let detail_h = ui.available_height().max(80.0);
                    egui::ScrollArea::vertical()
                        .id_salt("preferences_detail_scroll")
                        .auto_shrink([false; 2])
                        .max_height(detail_h)
                        .show(ui, |ui| {
                            let content_w = layout_util::set_width_to_available(ui);
                            ui.set_min_width(content_w);
                            ui.spacing_mut().item_spacing.y = theme.spacing_md();
                            self.preferences_category_body(
                                ui,
                                ctx,
                                theme,
                                selected,
                                label_color,
                                text_low,
                                should_close,
                            );
                        });
                },
            );
        });

        ui.data_mut(|d| d.insert_persisted(nav_id, selected));
    }

    /// 按左栏选中项只渲染该类内容；「团队与同步」含团队登录与同步入口两张卡。
    fn preferences_category_body(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
        selected: usize,
        label_color: egui::Color32,
        text_low: egui::Color32,
        should_close: &mut bool,
    ) {
        match selected {
            0 => self.preferences_section_general(ui, ctx, theme, label_color, text_low),
            1 => self.preferences_section_appearance(ui, ctx, theme, label_color),
            2 => self.preferences_section_terminal(ui, ctx, theme, label_color),
            3 => self.preferences_section_connection(ui, ctx, theme, label_color, text_low),
            4 => self.preferences_section_terminal_logs(ui, ctx, theme, label_color, text_low),
            5 => self.preferences_section_vault(ui, ctx, theme, label_color, text_low),
            6 => self.preferences_section_audit(ui, ctx, theme, label_color, text_low),
            7 => {
                self.preferences_section_team(ui, ctx, theme, label_color, text_low);
                self.preferences_section_sync(ui, ctx, theme, label_color, should_close);
            }
            8 => self.preferences_section_monitor_alert(ui, ctx, theme, label_color, text_low),
            9 => self.preferences_section_modules(ui, ctx, theme, label_color, text_low),
            _ => {}
        }
    }

    /// 分组标题在卡外；卡内抬升底、无描边；子项靠自然间距排列。
    fn preferences_collapsing(
        ui: &mut egui::Ui,
        theme: &crate::ui::theme::Theme,
        section_id: &str,
        title: &str,
        label_color: egui::Color32,
        default_open: bool,
        body: impl FnOnce(&mut egui::Ui),
    ) {
        // #279 导航模式：右栏标题已由左栏选中项给出，这里只渲染卡片内容，不画折叠头。
        let nav_mode = ui.ctx().data(|d| {
            d.get_temp::<bool>(egui::Id::unique(PREFERENCES_NAV_MODE_ID))
                .unwrap_or(false)
        });
        if nav_mode {
            ui.push_id(egui::Id::unique(("pref_section", section_id)), |ui| {
                crate::ui::chrome::settings_elevated_card(ui, theme, body);
            });
            return;
        }
        ui.push_id(egui::Id::unique(("pref_section", section_id)), |ui| {
            egui::CollapsingHeader::new(
                RichText::new(title)
                    .size(theme.font_size_panel_title())
                    .strong()
                    .color(label_color),
            )
            .id_salt(("pref_section", section_id))
            .default_open(default_open)
            .show_unindented(ui, |ui| {
                crate::ui::chrome::settings_elevated_card(ui, theme, body);
            });
        });
    }

    fn preferences_section_general(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
        label_color: egui::Color32,
        text_low: egui::Color32,
    ) {
        Self::preferences_collapsing(
            ui,
            theme,
            "general",
            crate::i18n::tr(ctx, "General", "常规"),
            label_color,
            true,
            |ui| {
                ui.label(
                    RichText::new(crate::i18n::tr(
                        ctx,
                        "Window size, position, and sidebar width/collapse are saved on exit.",
                        "窗口大小与位置、左侧连接栏宽度及折叠状态会在退出时自动保存。",
                    ))
                    .size(theme.font_size_small())
                    .color(text_low),
                );
                Self::preferences_row_divider(ui, theme);
                crate::ui::chrome::form_field_label(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "Language", "界面语言"),
                );
                let mut lang = self.app_settings.ui_language;
                crate::ui::chrome::form_combo(
                    ui,
                    theme,
                    "pref_ui_language",
                    lang.label_in_self(),
                    layout_util::finite_content_width(ui),
                    |ui| {
                        for option in crate::i18n::UiLanguage::ALL {
                            ui.selectable_value(&mut lang, option, option.label_in_self());
                        }
                    },
                );
                if lang != self.app_settings.ui_language {
                    self.app_settings.ui_language = lang;
                    let _ = self.app_settings.save();
                    crate::i18n::set_language(ctx, lang);
                    ctx.request_repaint();
                }
                Self::preferences_row_divider(ui, theme);
                self.preferences_default_local_dir(ui, ctx, theme, text_low);
                Self::preferences_row_divider(ui, theme);
                self.update_preferences_section(ui, ctx, theme, text_low);
            },
        );
    }

    /// 「默认本地目录」：SFTP 本机侧初始目录 + ZMODEM `sz` 接收目录（共用）。
    fn preferences_default_local_dir(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
        text_low: egui::Color32,
    ) {
        crate::ui::chrome::form_field_label(
            ui,
            theme,
            crate::i18n::tr(ctx, "Default local folder", "默认本地目录"),
        );
        let system_default = crate::core::local_dir::system_default();
        let mut draft = self
            .pref_local_dir_draft
            .take()
            .unwrap_or_else(|| self.app_settings.default_local_dir.clone());
        let mut commit: Option<String> = None;
        crate::ui::chrome::form_control_row(ui, theme, |ui| {
            let btn_w = theme.size_control_btn_min_w() * 2.0 + theme.spacing_sm() * 2.0;
            let field_w = (ui.available_width() - btn_w).max(120.0);
            let resp = crate::ui::chrome::form_singleline_field(
                ui,
                theme,
                egui::Id::unique("pref_default_local_dir"),
                &mut draft,
                &system_default.to_string_lossy(),
                field_w,
                false,
            );
            if resp.lost_focus() && draft.trim() != self.app_settings.default_local_dir.trim() {
                commit = Some(draft.clone());
            }
            if crate::ui::chrome::panel_action_button_ex(
                ui,
                theme,
                crate::i18n::tr(ctx, "Browse…", "浏览…"),
                true,
            )
            .clicked()
            {
                let start = crate::core::local_dir::current().dir;
                if let Some(dir) = crate::ui::file_dialog::FileDialog::new()
                    .set_title(crate::i18n::tr(
                        ctx,
                        "Choose default local folder",
                        "选择默认本地目录",
                    ))
                    .set_directory(&start)
                    .pick_folder()
                {
                    draft = dir.to_string_lossy().into_owned();
                    commit = Some(draft.clone());
                }
            }
            if crate::ui::chrome::panel_action_button_ex(
                ui,
                theme,
                crate::i18n::tr(ctx, "Reset", "恢复默认"),
                !self.app_settings.default_local_dir.trim().is_empty(),
            )
            .clicked()
            {
                draft.clear();
                commit = Some(String::new());
            }
        });
        if let Some(raw) = commit {
            match crate::core::local_dir::parse_configured(&raw) {
                Some(p) if !p.is_dir() => {
                    self.notify_warn(match crate::i18n::language(ctx) {
                        crate::i18n::UiLanguage::En => {
                            format!("Folder not found: {}", p.display())
                        }
                        crate::i18n::UiLanguage::Zh => format!("目录不存在：{}", p.display()),
                    });
                    draft = self.app_settings.default_local_dir.clone();
                }
                parsed => {
                    let stored = parsed
                        .map(|p| p.components().collect::<std::path::PathBuf>())
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.app_settings.default_local_dir = stored.clone();
                    let _ = self.app_settings.save();
                    crate::core::local_dir::set_configured(&stored);
                    draft = stored;
                    let shown = crate::core::local_dir::current().dir;
                    self.notify_success(match crate::i18n::language(ctx) {
                        crate::i18n::UiLanguage::En => {
                            format!("Default local folder: {}", shown.display())
                        }
                        crate::i18n::UiLanguage::Zh => {
                            format!("默认本地目录已设为 {}", shown.display())
                        }
                    });
                }
            }
        }
        self.pref_local_dir_draft = Some(draft);
        ui.label(
            RichText::new(crate::i18n::tr(
                ctx,
                "Opened by the SFTP local pane and used for sz (ZMODEM) downloads. Leave empty for your Downloads folder.",
                "SFTP 本机侧默认打开这里，sz（ZMODEM）下载也保存到这里。留空则用系统「下载」目录。",
            ))
            .size(theme.font_size_small())
            .color(text_low),
        );
    }

    fn preferences_section_appearance(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
        label_color: egui::Color32,
    ) {
        Self::preferences_collapsing(
            ui,
            theme,
            "appearance",
            crate::i18n::tr(ctx, "Appearance", "外观"),
            label_color,
            true,
            |ui| {
                let theme_stored: Vec<String> = self
                    .theme_manager
                    .list_themes()
                    .iter()
                    .map(|t| t.name.clone())
                    .collect();
                let current_idx = self.theme_manager.current;
                ui.label(
                    RichText::new(crate::i18n::tr(ctx, "Color theme", "配色主题"))
                        .size(theme.font_size_small())
                        .color(theme.color_form_hint()),
                );
                ui.add_space(theme.spacing_panel_gap());
                for (i, stored) in theme_stored.iter().enumerate() {
                    let selected = i == current_idx;
                    let label = crate::i18n::theme_display_name(ctx, stored).into_owned();
                    if crate::ui::chrome::menu_theme_item(ui, theme, selected, &label).clicked() {
                        self.theme_manager.set_theme_index(i);
                        self.theme_manager.save();
                        ctx.request_repaint();
                    }
                }
            },
        );
    }

    fn preferences_section_terminal(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
        label_color: egui::Color32,
    ) {
        Self::preferences_collapsing(
            ui,
            theme,
            "terminal",
            crate::i18n::tr(ctx, "Terminal", "终端"),
            label_color,
            true,
            |ui| {
                ui.label(
                    RichText::new(crate::i18n::tr(ctx, "Terminal font", "终端字体"))
                        .size(theme.font_size_small())
                        .color(theme.color_form_hint()),
                );
                ui.add_space(theme.spacing_panel_gap());

                let preset_label =
                    |ctx: &egui::Context, p: crate::platform::TerminalFontPreset| match p {
                        crate::platform::TerminalFontPreset::Default => {
                            crate::i18n::tr(ctx, "System default", "系统默认")
                        }
                        crate::platform::TerminalFontPreset::Consolas => "Consolas",
                        crate::platform::TerminalFontPreset::CascadiaMono => "Cascadia Mono",
                        crate::platform::TerminalFontPreset::JetBrainsMono => "JetBrains Mono",
                    };
                let scanned = crate::platform::system_mono_fonts_if_ready();
                let fonts_ready = scanned.is_some();
                let system_fonts = scanned.unwrap_or(&[]);
                let mut pick = if self.terminal_font_family.trim().is_empty() {
                    FontPick::Preset(self.terminal_font_preset)
                } else {
                    FontPick::Family(self.terminal_font_family.clone())
                };
                let selected_text = match &pick {
                    FontPick::Preset(p) => preset_label(ctx, *p).to_string(),
                    FontPick::Family(name) => name.clone(),
                };
                let prev_combo_h = ui.spacing().combo_height;
                ui.spacing_mut().combo_height = 320.0;
                crate::ui::chrome::form_combo(
                    ui,
                    theme,
                    "preferences_terminal_font_preset",
                    selected_text,
                    layout_util::finite_content_width(ui),
                    |ui| {
                        let default_selected = matches!(
                            pick,
                            FontPick::Preset(crate::platform::TerminalFontPreset::Default)
                        );
                        if ui
                            .selectable_label(
                                default_selected,
                                preset_label(ctx, crate::platform::TerminalFontPreset::Default),
                            )
                            .clicked()
                        {
                            pick = FontPick::Preset(crate::platform::TerminalFontPreset::Default);
                        }
                        let named_presets = [
                            crate::platform::TerminalFontPreset::Consolas,
                            crate::platform::TerminalFontPreset::CascadiaMono,
                            crate::platform::TerminalFontPreset::JetBrainsMono,
                        ];
                        for p in named_presets {
                            let family = crate::platform::terminal_preset_family_name(p);
                            let selected_now = match &pick {
                                FontPick::Preset(cur) => *cur == p,
                                FontPick::Family(name) => family.is_some_and(|preset_name| {
                                    crate::platform::font_family_eq(name, preset_name)
                                }),
                            };
                            if !crate::platform::terminal_preset_installed(p) && !selected_now {
                                continue;
                            }
                            if ui
                                .selectable_label(selected_now, preset_label(ctx, p))
                                .clicked()
                            {
                                pick = FontPick::Preset(p);
                            }
                        }
                        let extras: Vec<_> = system_fonts
                            .iter()
                            .filter(|font| {
                                !named_presets.iter().any(|p| {
                                    crate::platform::terminal_preset_family_name(*p).is_some_and(
                                        |name| crate::platform::font_family_eq(&font.family, name),
                                    )
                                })
                            })
                            .collect();
                        if !extras.is_empty() {
                            ui.separator();
                        }
                        for font in extras {
                            let selected = matches!(
                                &pick,
                                FontPick::Family(name)
                                    if crate::platform::font_family_eq(name, &font.family)
                            );
                            if ui
                                .selectable_label(selected, font.family.as_str())
                                .clicked()
                            {
                                pick = FontPick::Family(font.family.clone());
                            }
                        }
                        if let FontPick::Family(name) = &pick {
                            let listed = system_fonts
                                .iter()
                                .any(|font| crate::platform::font_family_eq(&font.family, name))
                                || named_presets.iter().any(|p| {
                                    crate::platform::terminal_preset_family_name(*p).is_some_and(
                                        |preset_name| {
                                            crate::platform::font_family_eq(name, preset_name)
                                        },
                                    )
                                });
                            if !fonts_ready {
                                ui.separator();
                                let _ = ui.selectable_label(true, name.as_str());
                            } else if !listed {
                                ui.separator();
                                let missing = format!(
                                    "{name} ({})",
                                    crate::i18n::tr(ctx, "not installed", "未安装")
                                );
                                let _ = ui.selectable_label(true, missing);
                            }
                        }
                    },
                );
                ui.spacing_mut().combo_height = prev_combo_h;
                let (next_preset, next_family) = match &pick {
                    FontPick::Preset(p) => (*p, String::new()),
                    FontPick::Family(name) => {
                        (crate::platform::TerminalFontPreset::Default, name.clone())
                    }
                };
                if next_preset != self.terminal_font_preset
                    || next_family != self.terminal_font_family
                {
                    self.terminal_font_preset = next_preset;
                    self.terminal_font_family = next_family;
                    self.apply_terminal_font_preferences(ctx);
                }
                ui.label(
                    RichText::new(crate::i18n::tr(
                        ctx,
                        "Lists monospace fonts installed on this computer. Presets that are not installed are hidden. Newly installed fonts appear after a restart.",
                        "列出本机已安装的等宽字体；没安装的预设不显示。新安装的字体需重启后出现。",
                    ))
                    .size(theme.font_size_small())
                    .color(theme.color_form_hint()),
                );
                if !fonts_ready {
                    ui.label(
                        RichText::new(crate::i18n::tr(
                            ctx,
                            "Reading installed fonts…",
                            "正在读取本机字体…",
                        ))
                        .size(theme.font_size_small())
                        .color(theme.color_form_hint()),
                    );
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                }

                Self::preferences_row_divider(ui, theme);
                crate::ui::chrome::form_control_row(ui, theme, |ui| {
                    crate::ui::chrome::form_inline_label(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Font size (px)", "字号 (px)"),
                    );
                    let mut size = self.terminal_font_size;
                    if crate::ui::chrome::form_f32_stepper(
                        ui,
                        theme,
                        egui::Id::unique("pref_terminal_font_size"),
                        &mut size,
                        crate::platform::TERMINAL_FONT_SIZE_MIN
                            ..=crate::platform::TERMINAL_FONT_SIZE_MAX,
                        1.0,
                        " px",
                    )
                    .changed()
                    {
                        self.terminal_font_size =
                            crate::platform::clamp_terminal_font_size(size);
                        self.apply_terminal_font_size_to_all_terminals();
                        ctx.request_repaint();
                    }
                });
                // #279：预览区独立成块，说明文字与预览分离，不再与「预览：」前缀混排。
                ui.add_space(theme.spacing_sm());
                ui.label(
                    RichText::new(crate::i18n::tr(ctx, "Preview", "字体预览"))
                        .size(theme.font_size_small())
                        .color(theme.color_form_hint()),
                );
                ui.add_space(theme.spacing_xs());
                egui::Frame::NONE
                    .fill(theme.color_form_input_fill())
                    .corner_radius(theme.radius_search_input())
                    .inner_margin(egui::Margin::symmetric(theme.spacing_search_input_x() as i8, theme.spacing_search_input_y() as i8))
                    .show(ui, |ui| {
                        layout_util::set_width_to_available(ui);
                        ui.label(
                            RichText::new("Ag gjpqy  终端 ABC 123  #!/bin/bash")
                                .font(egui::FontId::monospace(self.terminal_font_size))
                                .color(theme.text_primary()),
                        );
                    });
                ui.label(
                    RichText::new(crate::i18n::tr(
                        ctx,
                        "Ctrl+scroll wheel still zooms the active terminal temporarily.",
                        "Ctrl+滚轮仍可在当前终端临时缩放字号。",
                    ))
                    .size(theme.font_size_small())
                    .color(theme.color_form_hint()),
                );
                Self::preferences_row_divider(ui, theme);
                let mut remember_paste = self.app_settings.remember_paste_history;
                if crate::ui::chrome::form_checkbox(
                    ui,
                    theme,
                    &mut remember_paste,
                    crate::i18n::tr(
                        ctx,
                        "Remember recent pastes (last 20, stored on this computer)",
                        "记住最近的粘贴（最多 20 条，只存在这台电脑上）",
                    ),
                )
                .on_hover_text(crate::i18n::tr(
                    ctx,
                    "Off: new pastes are not saved. Paste history can still show what was saved before. Private keys, common tokens, and password prompts are skipped on a best-effort basis; the filter is not complete.",
                    "关闭后，新的粘贴不再写入。面板里仍能看到之前保存的内容。私钥、常见令牌和密码/口令提示会尽量不记住，这是尽力过滤，并不完备。",
                ))
                .changed()
                {
                    self.app_settings.remember_paste_history = remember_paste;
                    let _ = self.app_settings.save();
                }
            },
        );
    }

    fn preferences_section_connection(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
        label_color: egui::Color32,
        text_low: egui::Color32,
    ) {
        Self::preferences_collapsing(
            ui,
            theme,
            "connection",
            crate::i18n::tr(ctx, "Connection", "连接"),
            label_color,
            false,
            |ui| {
                let mut ar = self.auto_reconnect_enabled;
                if crate::ui::chrome::form_checkbox(
                    ui,
                    theme,
                    &mut ar,
                    crate::i18n::tr(
                        ctx,
                        "Reconnect automatically after disconnect (up to 5 tries, longer waits each time)",
                        "断线后自动重连（最多 5 次，间隔逐渐加长）",
                    ),
                )
                .on_hover_text(crate::i18n::tr(
                    ctx,
                    "Off by default. Only reconnects after unexpected drops — not when you click Disconnect.",
                    "默认关闭。仅在意外断线时重连；你手动点「断开」不会重连。",
                ))
                .changed()
                {
                    self.auto_reconnect_enabled = ar;
                }
                Self::preferences_row_divider(ui, theme);
                // #201：窗口/标签回到前台时自动重连（默认关）。
                let mut fg = self.app_settings.reconnect_on_foreground;
                if crate::ui::chrome::form_checkbox(
                    ui,
                    theme,
                    &mut fg,
                    crate::i18n::tr(
                        ctx,
                        "Reconnect when the window or tab becomes active again",
                        "回到窗口/标签时自动重连",
                    ),
                )
                .on_hover_text(crate::i18n::tr(
                    ctx,
                    "Off by default. Only for unexpected drops (timeout / network) — not when you click Disconnect. Output and scrollback are kept.",
                    "默认关闭。仅对意外断线（超时/网络）生效；你手动点「断开」不会重连。终端输出与滚动缓冲保留。",
                ))
                .changed()
                {
                    self.app_settings.reconnect_on_foreground = fg;
                    let _ = self.app_settings.save();
                }
                Self::preferences_row_divider(ui, theme);
                let mut ka = self.default_keepalive_enabled;
                if crate::ui::chrome::form_checkbox(
                    ui,
                    theme,
                    &mut ka,
                    crate::i18n::tr(
                        ctx,
                        "Keep connections alive when idle (for new sessions)",
                        "空闲时保持连接（新建连接默认开启）",
                    ),
                )
                .changed()
                {
                    self.default_keepalive_enabled = ka;
                }
                // 窄弹窗里两个步进器挤在一行会折标签；间隔与连续无应答各占一行（#130）。
                let every = crate::i18n::tr(ctx, "Ping every (s)", "每隔(秒)");
                let give_up = crate::i18n::tr(ctx, "Give up after (times)", "连续无应答(次)");
                let label_w = crate::ui::chrome::form_label_advance(ui, theme, every)
                    .max(crate::ui::chrome::form_label_advance(ui, theme, give_up));
                crate::ui::chrome::form_labeled_control_row(ui, theme, every, label_w, |ui| {
                    crate::ui::chrome::form_u32_stepper(
                        ui,
                        theme,
                        egui::Id::unique("pref_ka_interval"),
                        &mut self.default_keepalive_interval_secs,
                        5..=300,
                        5,
                    )
                });
                crate::ui::chrome::form_labeled_control_row(ui, theme, give_up, label_w, |ui| {
                    crate::ui::chrome::form_u8_stepper(
                        ui,
                        theme,
                        egui::Id::unique("pref_ka_count"),
                        &mut self.default_keepalive_count_max,
                        1..=20,
                        1,
                    )
                });
                ui.label(
                    RichText::new(crate::i18n::tr(
                        ctx,
                        "Sends a small ping so idle sessions are not dropped. After several missed replies the connection may be treated as lost.",
                        "定时发一次轻量探测，减少空闲被服务器踢下线。连续多次无应答时，可能判定连接已断开。",
                    ))
                    .size(theme.font_size_small())
                    .color(text_low),
                );
                if crate::platform::url_handler::supported() {
                    Self::preferences_row_divider(ui, theme);
                    self.preferences_ssh_url_row(ui, ctx, theme, text_low);
                }
            },
        );
    }

    /// 「浏览器里的 ssh:// 链接」：显示当前由谁打开，可一键改成 MistTerm。
    fn preferences_ssh_url_row(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
        text_low: egui::Color32,
    ) {
        use crate::platform::url_handler::{self, HandlerStatus};
        if self.ssh_url_handler_status.is_none() {
            self.ssh_url_handler_status = Some(url_handler::status());
        }
        let status = self.ssh_url_handler_status.clone().unwrap_or(HandlerStatus::Unsupported);
        let zh = crate::i18n::language(ctx) == crate::i18n::UiLanguage::Zh;
        crate::ui::chrome::form_control_row(ui, theme, |ui| {
            crate::ui::chrome::form_inline_label(
                ui,
                theme,
                crate::i18n::tr(ctx, "ssh:// links in the browser", "浏览器里的 ssh:// 链接"),
            );
            let label = if status == HandlerStatus::ThisApp {
                crate::i18n::tr(ctx, "Opened by MistTerm", "已用 MistTerm 打开")
            } else {
                crate::i18n::tr(ctx, "Open with MistTerm", "用 MistTerm 打开")
            };
            if crate::ui::chrome::panel_action_button_ex(ui, theme, label, status != HandlerStatus::ThisApp)
                .clicked()
            {
                match url_handler::register() {
                    Ok(()) => {
                        self.ssh_url_handler_status = Some(url_handler::status());
                        self.audit_logger.record(AuditEvent::new(
                            AuditCategory::Config,
                            "settings.ssh_url_handler",
                            AuditOutcome::Success,
                        ));
                        self.notify_auto(
                            crate::i18n::tr(
                                ctx,
                                "ssh:// links will now open in MistTerm",
                                "以后点 ssh:// 链接会用 MistTerm 打开",
                            )
                            .to_string(),
                        );
                    }
                    Err(e) => self.notify_warn(format!(
                        "{}{e}",
                        crate::i18n::tr(ctx, "Could not set it: ", "没设置成功：")
                    )),
                }
            }
        });
        let hint = match &status {
            HandlerStatus::ThisApp => None,
            HandlerStatus::Other(who) => Some(if zh {
                format!("现在由别的程序打开（{who}）。点按钮改成 MistTerm。")
            } else {
                format!("Currently opened by another program ({who}).")
            }),
            HandlerStatus::None => Some(
                crate::i18n::tr(
                    ctx,
                    "No program opens ssh:// links yet.",
                    "现在没有程序打开 ssh:// 链接。",
                )
                .to_string(),
            ),
            HandlerStatus::Unsupported => None,
        };
        if let Some(h) = hint {
            ui.label(RichText::new(h).size(theme.font_size_small()).color(text_low));
        }
    }

    fn preferences_section_terminal_logs(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
        label_color: egui::Color32,
        text_low: egui::Color32,
    ) {
        Self::preferences_collapsing(
            ui,
            theme,
            "terminal_logs",
            crate::i18n::tr(ctx, "Terminal logs", "终端日志"),
            label_color,
            false,
            |ui| {
                let mut log_on = self.session_log_enabled;
                if crate::ui::chrome::form_checkbox(
                    ui,
                    theme,
                    &mut log_on,
                    crate::i18n::tr(
                        ctx,
                        "Save terminal output locally",
                        "自动保存终端输出到本地",
                    ),
                )
                .changed()
                {
                    self.session_log_enabled = log_on;
                    self.session_log_settings.enabled = log_on;
                }
                Self::preferences_row_divider(ui, theme);
                crate::ui::chrome::form_control_row(ui, theme, |ui| {
                    crate::ui::chrome::form_inline_label(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Retention (days)", "保留天数"),
                    );
                    crate::ui::chrome::form_u32_stepper(
                        ui,
                        theme,
                        egui::Id::unique("pref_log_retention"),
                        &mut self.session_log_settings.retention_days,
                        1..=365,
                        1,
                    );
                });
                Self::preferences_row_divider(ui, theme);
                let mut ansi = self.session_log_settings.include_ansi;
                if crate::ui::chrome::form_checkbox(
                    ui,
                    theme,
                    &mut ansi,
                    crate::i18n::tr(ctx, "Include ANSI colors in logs", "日志保留终端颜色代码"),
                )
                .changed()
                {
                    self.session_log_settings.include_ansi = ansi;
                }
                let mb = self.session_log_settings.max_file_bytes / (1024 * 1024);
                let full_path = self.session_log_settings.base_dir.display().to_string();
                let shown_path = ellipsize_middle(&full_path, 42);
                let dir_line = match crate::i18n::language(ctx) {
                    crate::i18n::UiLanguage::En => format!("Directory: {shown_path}"),
                    crate::i18n::UiLanguage::Zh => format!("目录：{shown_path}"),
                };
                let cap_line = match crate::i18n::language(ctx) {
                    crate::i18n::UiLanguage::En => format!("Max {mb} MB per file"),
                    crate::i18n::UiLanguage::Zh => format!("单文件上限 {mb} MB"),
                };
                ui.label(
                    RichText::new(dir_line)
                        .size(theme.font_size_small())
                        .color(text_low),
                )
                .on_hover_text(full_path);
                ui.label(
                    RichText::new(cap_line)
                        .size(theme.font_size_small())
                        .color(text_low),
                );
            },
        );
    }

    fn preferences_section_vault(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
        label_color: egui::Color32,
        text_low: egui::Color32,
    ) {
        Self::preferences_collapsing(
            ui,
            theme,
            "vault",
            "HashiCorp Vault",
            label_color,
            false,
            |ui| {
                let pref_w = layout_util::finite_content_width(ui);
                // #279：术语补白话说明，避免只出现产品名。
                ui.label(
                    RichText::new(crate::i18n::tr(
                        ctx,
                        "Vault (HashiCorp Vault) is a shared password server. Fill in its address and a token, and MistTerm can fetch login credentials from it instead of storing them here.",
                        "密码库（HashiCorp Vault）是一台集中存放密码的服务器。填好地址和令牌后，MistTerm 可以从那里取登录凭据，不必在本机重复保存。",
                    ))
                    .size(theme.font_size_small())
                    .color(text_low),
                );
                ui.add_space(theme.spacing_sm());
                if crate::ui::chrome::form_checkbox(
                    ui,
                    theme,
                    &mut self.app_settings.vault.enabled,
                    crate::i18n::tr(ctx, "Enable Vault integration", "启用密码库（HashiCorp Vault）"),
                )
                .changed()
                {
                    self.app_settings.vault.team_auto_apply = false;
                    self.app_settings.vault.managed_by_team_id = None;
                    let _ = self.app_settings.save();
                }
                if self.app_settings.vault.managed_by_team_id.is_some()
                    && self.app_settings.vault.team_auto_apply
                {
                    let name = self.team_service.current_team_name();
                    let vault_hint = match crate::i18n::language(ctx) {
                        crate::i18n::UiLanguage::En => format!(
                            "Vault settings from team «{name}» (read-only until you change them)"
                        ),
                        crate::i18n::UiLanguage::Zh => {
                            format!("Vault 由团队「{name}」自动配置(修改任意项后将不再自动覆盖)")
                        }
                    };
                    ui.label(
                        RichText::new(vault_hint)
                            .size(theme.font_size_sidebar_control())
                            .color(text_low),
                    );
                }
                Self::preferences_row_divider(ui, theme);
                crate::ui::chrome::form_field_label(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "Address", "地址"),
                );
                let vault_addr = crate::ui::chrome::form_singleline_field(
                    ui,
                    theme,
                    egui::Id::unique("pref_vault_addr"),
                    &mut self.app_settings.vault.address,
                    "https://vault.example.com:8200",
                    pref_w,
                    false,
                );
                if vault_addr.lost_focus() {
                    self.app_settings.vault.team_auto_apply = false;
                    self.app_settings.vault.managed_by_team_id = None;
                    let _ = self.app_settings.save();
                }
                crate::ui::chrome::form_field_label(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "Namespace", "命名空间（可选）"),
                );
                let vault_ns = crate::ui::chrome::form_singleline_field(
                    ui,
                    theme,
                    egui::Id::unique("pref_vault_ns"),
                    &mut self.app_settings.vault.namespace,
                    "",
                    pref_w,
                    false,
                );
                if vault_ns.lost_focus() {
                    let _ = self.app_settings.save();
                }
                Self::preferences_row_divider(ui, theme);
                crate::ui::chrome::form_field_label(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "Default KV mount", "默认密钥路径"),
                );
                let vault_mount = crate::ui::chrome::form_singleline_field(
                    ui,
                    theme,
                    egui::Id::unique("pref_vault_mount"),
                    &mut self.app_settings.vault.default_mount,
                    "secret",
                    pref_w,
                    false,
                );
                if vault_mount.lost_focus() {
                    let _ = self.app_settings.save();
                }
                let mut auth = self.app_settings.vault.auth;
                let auth_label = match auth {
                    crate::core::VaultAuthSettings::None => {
                        crate::i18n::tr(ctx, "Not configured", "未配置").to_owned()
                    }
                    crate::core::VaultAuthSettings::Token => {
                        crate::i18n::tr(ctx, "Token (Keychain)", "令牌（存系统钥匙串）").to_owned()
                    }
                    crate::core::VaultAuthSettings::AppRole => {
                        crate::i18n::tr(ctx, "AppRole (Keychain)", "AppRole（存系统钥匙串）")
                            .to_owned()
                    }
                };
                crate::ui::chrome::form_combo(ui, theme, "pref_vault_auth", auth_label, pref_w, |ui| {
                    for v in [
                        crate::core::VaultAuthSettings::None,
                        crate::core::VaultAuthSettings::Token,
                        crate::core::VaultAuthSettings::AppRole,
                    ] {
                        let label = match v {
                            crate::core::VaultAuthSettings::None => {
                                crate::i18n::tr(ctx, "Not configured", "未配置")
                            }
                            crate::core::VaultAuthSettings::Token => {
                                crate::i18n::tr(ctx, "Token (Keychain)", "令牌（存系统钥匙串）")
                            }
                            crate::core::VaultAuthSettings::AppRole => crate::i18n::tr(
                                ctx,
                                "AppRole (Keychain)",
                                "AppRole（存系统钥匙串）",
                            ),
                        };
                        if ui.selectable_label(auth == v, label).clicked() {
                            auth = v;
                        }
                    }
                });
                if auth != self.app_settings.vault.auth {
                    self.app_settings.vault.auth = auth;
                    self.app_settings.vault.team_auto_apply = false;
                    self.app_settings.vault.managed_by_team_id = None;
                    let _ = self.app_settings.save();
                }
                Self::preferences_row_divider(ui, theme);
                crate::ui::chrome::form_field_label(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "Token", "令牌"),
                );
                crate::ui::chrome::form_singleline_field(
                    ui,
                    theme,
                    egui::Id::unique("pref_vault_token_buf"),
                    &mut self.pref_vault_token_draft,
                    crate::i18n::tr(
                        ctx,
                        "Enter token, then click Save",
                        "输入令牌后点「保存」",
                    ),
                    pref_w,
                    true,
                );
                ui.horizontal(|ui| {
                    if crate::ui::chrome::panel_action_primary_icon_button(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Key,
                        crate::i18n::tr(ctx, "Save", "保存"),
                    )
                    .clicked()
                        && !self.pref_vault_token_draft.is_empty()
                    {
                        self.app_settings.vault.auth = crate::core::VaultAuthSettings::Token;
                        let _ = crate::core::HashiCorpVaultClient::save_token_to_keyring(
                            &self.pref_vault_token_draft,
                        );
                        let _ = self.app_settings.save();
                        self.pref_vault_token_draft.clear();
                        self.notify_success(
                            crate::i18n::tr(
                                ctx,
                                "Vault token saved to keychain",
                                "令牌已保存到钥匙串",
                            )
                            .to_string(),
                        );
                    }
                    if crate::ui::chrome::panel_action_icon_button(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Plug,
                        crate::i18n::tr(ctx, "Test", "测试"),
                    )
                    .clicked()
                    {
                        match crate::core::HashiCorpVaultClient::new(
                            self.app_settings.vault.clone(),
                        ) {
                            Ok(c) => match c.test_connection() {
                                Ok(()) => {
                                    self.notify_success(
                                        crate::i18n::tr(
                                            ctx,
                                            "Connected to HashiCorp Vault",
                                            "已连接到 HashiCorp Vault",
                                        )
                                        .to_string(),
                                    );
                                }
                                Err(e) => {
                                    self.notify_error(format!(
                                        "{} {}",
                                        crate::i18n::tr(
                                            ctx,
                                            "HashiCorp Vault:",
                                            "HashiCorp Vault：",
                                        ),
                                        e
                                    ));
                                }
                            },
                            Err(e) => {
                                self.notify_error(format!(
                                    "{} {}",
                                    crate::i18n::tr(ctx, "HashiCorp Vault:", "HashiCorp Vault："),
                                    e
                                ));
                            }
                        }
                    }
                });
                let _ = text_low;
            },
        );
    }

    fn preferences_section_audit(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
        label_color: egui::Color32,
        text_low: egui::Color32,
    ) {
        Self::preferences_collapsing(
            ui,
            theme,
            "audit",
            crate::i18n::tr(ctx, "Activity audit", "操作审计"),
            label_color,
            false,
            |ui| {
                // #279：术语补白话说明。
                ui.label(
                    RichText::new(crate::i18n::tr(
                        ctx,
                        "Audit log: keeps a local record of what was run and when, so you can trace things afterwards. It only records events — it does not block your actions.",
                        "操作审计：把执行过什么、什么时候执行的记录下来，便于事后追查。它只记录事件，不会拦截你的操作。",
                    ))
                    .size(theme.font_size_small())
                    .color(text_low),
                );
                ui.add_space(theme.spacing_sm());
                if crate::ui::chrome::form_checkbox(
                    ui,
                    theme,
                    &mut self.app_settings.audit.enabled,
                    crate::i18n::tr(ctx, "Enable audit log", "启用审计日志"),
                )
                .changed()
                {
                    self.audit_logger
                        .update_settings(self.app_settings.audit.clone());
                    let _ = self.app_settings.save();
                }
                Self::preferences_row_divider(ui, theme);
                crate::ui::chrome::form_control_row(ui, theme, |ui| {
                    crate::ui::chrome::form_inline_label(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Retention (days)", "保留天数"),
                    );
                    let r = crate::ui::chrome::form_u32_stepper(
                        ui,
                        theme,
                        egui::Id::unique("pref_audit_retention"),
                        &mut self.app_settings.audit.retention_days,
                        1..=3650,
                        1,
                    );
                    if r.changed() {
                        let _ = self.app_settings.save();
                    }
                });
                Self::preferences_row_divider(ui, theme);
                crate::ui::chrome::form_checkbox(
                    ui,
                    theme,
                    &mut self.app_settings.audit.log_command_preview,
                    crate::i18n::tr(
                        ctx,
                        "Log a short summary of commands run",
                        "记录执行过的命令摘要",
                    ),
                );
                Self::preferences_row_divider(ui, theme);
                if crate::ui::chrome::form_checkbox(
                    ui,
                    theme,
                    &mut self.app_settings.audit.interactive_pty_capture,
                    crate::i18n::tr(
                        ctx,
                        "Check and record commands typed in the terminal",
                        "检查并记录终端里手输的命令",
                    ),
                )
                .changed()
                {
                    let _ = self.app_settings.save();
                }
                ui.label(
                    RichText::new(crate::i18n::tr(
                        ctx,
                        "When team audit is on, Enter uses the same local block/confirm/allow check as snippets and records one local audit entry. Off: Enter is sent immediately. Without a team login, typing is unchanged.",
                        "团队审计生效时，回车与片段一样先做本地拦截/确认/放行，并记一条本地审计。关闭后回车立即发出。未登录团队时，打字行为不变。",
                    ))
                    .size(theme.font_size_small())
                    .color(text_low),
                );
                // 团队 HTTP 上报由登录后的 configure_team_audit_sink 自动配置
                // (URL / Bearer / team_id)，用户无需在偏好里手动维护。
                Self::preferences_row_divider(ui, theme);
                crate::ui::chrome::form_checkbox(
                    ui,
                    theme,
                    &mut self.app_settings.audit.syslog.enabled,
                    crate::i18n::tr(ctx, "Also send to a Syslog server", "同时发送到 Syslog 服务器"),
                );
                let host_label = crate::i18n::tr(ctx, "Host", "主机");
                let port_label = crate::i18n::tr(ctx, "Port", "端口");
                let syslog_label_w = crate::ui::chrome::form_label_advance(ui, theme, host_label)
                    .max(crate::ui::chrome::form_label_advance(
                        ui,
                        theme,
                        port_label,
                    ));
                crate::ui::chrome::form_labeled_control_row(
                    ui,
                    theme,
                    host_label,
                    syslog_label_w,
                    |ui| {
                        let host_w = (ui.available_width() - 4.0).max(120.0);
                        crate::ui::chrome::form_inline_singleline(
                            ui,
                            theme,
                            egui::Id::unique("pref_audit_syslog_host"),
                            &mut self.app_settings.audit.syslog.host,
                            "127.0.0.1",
                            host_w,
                            false,
                        )
                    },
                );
                crate::ui::chrome::form_labeled_control_row(
                    ui,
                    theme,
                    port_label,
                    syslog_label_w,
                    |ui| {
                        crate::ui::chrome::form_u16_stepper(
                            ui,
                            theme,
                            egui::Id::unique("pref_audit_syslog_port"),
                            &mut self.app_settings.audit.syslog.port,
                            1..=65535,
                            1,
                        );
                        crate::ui::chrome::form_inline_checkbox(
                            ui,
                            theme,
                            "pref_audit_syslog_tcp",
                            &mut self.app_settings.audit.syslog.use_tcp,
                            "TCP",
                        );
                    },
                );
                ui.horizontal(|ui| {
                    if crate::ui::chrome::panel_action_primary_icon_button(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Check,
                        crate::i18n::tr(ctx, "Save", "保存"),
                    )
                    .clicked()
                    {
                        self.audit_logger
                            .update_settings(self.app_settings.audit.clone());
                        match self.app_settings.save() {
                            Ok(()) => {
                                self.audit_logger.record(crate::core::AuditEvent::new(
                                    crate::core::AuditCategory::Config,
                                    "config.audit.updated",
                                    crate::core::AuditOutcome::Success,
                                ));
                                self.notify_success(
                                    crate::i18n::tr(
                                        ctx,
                                        "Saved Vault & audit settings",
                                        "已保存 Vault 与审计设置",
                                    )
                                    .to_string(),
                                );
                            }
                            Err(e) => {
                                self.notify_error(format!(
                                    "{} {}",
                                    crate::i18n::tr(ctx, "Save failed:", "保存失败："),
                                    e
                                ));
                            }
                        }
                    }
                });
                ui.label(
                    RichText::new(format!(
                        "{}{}",
                        crate::i18n::tr(ctx, "Audit directory: ", "审计目录："),
                        self.app_settings.audit.file_dir.display(),
                    ))
                    .size(theme.font_size_small())
                    .color(text_low),
                );
            },
        );
    }

    fn preferences_section_team(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
        label_color: egui::Color32,
        text_low: egui::Color32,
    ) {
        Self::preferences_collapsing(
            ui,
            theme,
            "team",
            crate::i18n::tr(ctx, "Team platform", "团队平台"),
            label_color,
            false,
            |ui| {
                let pref_w = layout_util::finite_content_width(ui);
                ui.label(
                    RichText::new(crate::i18n::tr(
                        ctx,
                        "Connect to Mist team server (mistlab.dev).",
                        "对接 Mist 团队服务端(mistlab.dev)。",
                    ))
                    .size(theme.font_size_small())
                    .color(text_low),
                );
                ui.add_space(theme.spacing_sm());
                let action = crate::ui::team_ui::paint_team_controls(
                    ui,
                    ctx,
                    theme,
                    &mut self.team_service,
                    &mut self.team_login_form,
                    Some(&self.audit_logger),
                    pref_w,
                    "preferences_team",
                );
                if matches!(action, crate::ui::team_ui::TeamUiAction::LoggedOut) {
                    let _ = self.app_settings.save();
                }
                if matches!(action, crate::ui::team_ui::TeamUiAction::TeamChanged) {
                    self.apply_team_vault_from_sync();
                    self.apply_cmd_audit_cache_for_current_team();
                    self.team_service.spawn_config_sync();
                    self.team_service.spawn_cmd_audit_sync();
                }
                if matches!(action, crate::ui::team_ui::TeamUiAction::OpenMembers) {
                    self.team_members_dialog.open(&mut self.team_service);
                }
            },
        );
    }

    fn preferences_section_sync(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
        label_color: egui::Color32,
        should_close: &mut bool,
    ) {
        Self::preferences_collapsing(
            ui,
            theme,
            "sync",
            crate::i18n::tr(ctx, "Sync & data", "同步与数据"),
            label_color,
            false,
            |ui| {
                // 同步已并入「团队与同步」：卡片内加小节标题，再给出登录状态。
                ui.label(
                    RichText::new(crate::i18n::tr(ctx, "Sync & data", "同步与数据"))
                        .strong()
                        .size(theme.font_size_small())
                        .color(label_color),
                );
                let status = if self.team_service.is_logged_in() {
                    match &self.team_service.state.user {
                        Some(user) => format!(
                            "{} {}",
                            crate::i18n::tr(ctx, "Signed in:", "已登录："),
                            user.email
                        ),
                        None => crate::i18n::tr(ctx, "Signed in", "已登录").to_string(),
                    }
                } else {
                    crate::i18n::tr(ctx, "Not signed in", "未登录").to_string()
                };
                ui.label(
                    RichText::new(format!(
                        "{}{}",
                        crate::i18n::tr(ctx, "Team sign-in status: ", "团队登录状态："),
                        status
                    ))
                    .size(theme.font_size_small())
                    .color(theme.color_form_hint()),
                );
                ui.add_space(theme.spacing_sm());
                if crate::ui::chrome::panel_action_primary_icon_button(
                    ui,
                    theme,
                    crate::ui::icons::IconId::Cloud,
                    crate::i18n::tr(ctx, "Open sync panel", "打开同步面板"),
                )
                .clicked()
                {
                    *should_close = true;
                    self.open_cloud_sync_panel(ctx);
                }
                Self::preferences_row_divider(ui, theme);
                ui.label(
                    RichText::new(crate::i18n::tr(
                        ctx,
                        "Host list, credential sync, encryption password, conflicts, and export/import are in Cloud Sync. This page only opens that panel.",
                        "主机列表、凭证同步、加密密码、冲突和导出/导入都在云端同步里。这里只负责打开那个面板。",
                    ))
                    .size(theme.font_size_small())
                    .color(theme.color_form_hint()),
                );
            },
        );
    }

    /// 扩展模块 / 内置能力开关（#118 / #215 / #266）。
    /// 为已编进主程序的官方内置模块提供按模块启用/禁用开关；
    /// 主说明不暴露模块 id / 环境变量名；区块底部次要文案提示 env 覆盖并可跳文档。

    fn preferences_section_monitor_alert(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
        label_color: egui::Color32,
        text_low: egui::Color32,
    ) {
        Self::preferences_collapsing(
            ui,
            theme,
            "monitor_alert",
            crate::i18n::tr(ctx, "Monitor alerts", "监控告警"),
            label_color,
            false,
            |ui| {
                ui.label(
                    egui::RichText::new(crate::i18n::tr(
                        ctx,
                        "Thresholds and how you are notified live together below. Default: system notification.",
                        "阈值与推送方式都在这里设置（默认：系统通知）。",
                    ))
                    .size(theme.font_size_small())
                    .color(text_low),
                );
                ui.add_space(theme.spacing_sm());

                // 阈值：监控面板的会话级字段（当前会话有效），与推送方式同属本分类。
                crate::ui::chrome::form_field_label(
                    ui,
                    theme,
                    crate::i18n::tr(
                        ctx,
                        "Alert thresholds (this session, %)",
                        "告警阈值（仅当前会话，%）",
                    ),
                );
                let (cpu, mem, disk) = self.monitor_panel.alert_thresholds_mut();
                let cpu_label = crate::i18n::tr(ctx, "CPU", "CPU");
                let mem_label = crate::i18n::tr(ctx, "Memory", "内存");
                let disk_label = crate::i18n::tr(ctx, "Disk", "磁盘");
                let th_label_w = crate::ui::chrome::form_label_advance(ui, theme, cpu_label)
                    .max(crate::ui::chrome::form_label_advance(ui, theme, mem_label))
                    .max(crate::ui::chrome::form_label_advance(ui, theme, disk_label));
                for (idx, (label, value)) in [
                    (cpu_label, cpu),
                    (mem_label, mem),
                    (disk_label, disk),
                ]
                .into_iter()
                .enumerate()
                {
                    crate::ui::chrome::form_labeled_control_row(
                        ui,
                        theme,
                        label,
                        th_label_w,
                        |ui| {
                            crate::ui::chrome::form_f32_stepper(
                                ui,
                                theme,
                                egui::Id::unique(("pref_monitor_alert_th", idx)),
                                value,
                                50.0..=100.0,
                                1.0,
                                "%",
                            )
                        },
                    );
                }
                ui.label(
                    egui::RichText::new(crate::i18n::tr(
                        ctx,
                        "Range 50%–100%. These values apply to the current session and can also be changed in the monitor panel.",
                        "范围 50%–100%。阈值仅对当前会话生效，也可在监控面板里调整。",
                    ))
                    .size(theme.font_size_small())
                    .color(text_low),
                );

                Self::preferences_row_divider(ui, theme);
                let lang = crate::i18n::language(ctx);
                let mut mode = self.app_settings.monitor_alert.notify_mode;
                for option in crate::core::AlertNotifyMode::ALL {
                    let label = match lang {
                        crate::i18n::UiLanguage::Zh => option.as_label_zh(),
                        crate::i18n::UiLanguage::En => option.as_label_en(),
                    };
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut mode, option, label);
                        // 降级说明放在对应选项旁，而不是单独一段。
                        if option == crate::core::AlertNotifyMode::SystemNotify {
                            ui.label(
                                egui::RichText::new(crate::i18n::tr(
                                    ctx,
                                    "Falls back to an in-app alert if the OS denies permission.",
                                    "系统拒绝权限时自动降级为应用内告警。",
                                ))
                                .size(theme.font_size_small())
                                .color(text_low),
                            );
                        }
                    });
                }
                if mode != self.app_settings.monitor_alert.notify_mode {
                    self.app_settings.monitor_alert.notify_mode = mode;
                    // 用户改回系统通知时，允许再次解释降级原因。
                    if mode == crate::core::AlertNotifyMode::SystemNotify {
                        self.app_settings.monitor_alert.permission_denied_explained = false;
                    }
                    let _ = self.app_settings.save();
                }

                Self::preferences_row_divider(ui, theme);
                // 复用已有推送入口（系统通知 / 声音 / 应用内提示），不新增依赖。
                if crate::ui::chrome::panel_action_button(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "Test alert", "测试告警"),
                )
                .clicked()
                {
                    let title = crate::i18n::tr(ctx, "Monitor alert (test)", "监控告警（测试）");
                    let body = crate::i18n::tr(
                        ctx,
                        "This is a test alert sent from Preferences.",
                        "这是从偏好设置发出的测试告警。",
                    );
                    match crate::core::dispatch_for_mode(mode) {
                        crate::core::AlertDispatch::TrySystemNotify => {
                            match crate::platform::desktop_notify::send_desktop_notification(
                                title, body, true,
                            ) {
                                crate::platform::desktop_notify::DesktopNotifyResult::Sent => {
                                    self.notify_auto(format!("{title}：{body}"))
                                }
                                crate::platform::desktop_notify::DesktopNotifyResult::DeniedOrUnavailable => {
                                    self.notify_warn(format!(
                                        "{title}：{body}（{}）",
                                        crate::i18n::tr(
                                            ctx,
                                            "system notification unavailable, shown in-app",
                                            "系统通知不可用，已改为应用内提示",
                                        )
                                    ))
                                }
                            }
                        }
                        crate::core::AlertDispatch::PlaySound => {
                            crate::platform::desktop_notify::play_alert_sound();
                            self.notify_auto(format!("{title}：{body}"));
                        }
                        crate::core::AlertDispatch::PanelOnly => {
                            self.notify_auto(format!("{title}：{body}"));
                        }
                    }
                }
            },
        );
    }

    fn preferences_section_modules(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
        label_color: egui::Color32,
        text_low: egui::Color32,
    ) {
        Self::preferences_collapsing(
            ui,
            theme,
            "modules",
            crate::i18n::tr(ctx, "Extension Modules", "扩展模块 / 内置能力"),
            label_color,
            false,
            |ui| {
                // #279：上下结构 —— 上方只列模块开关，选中后在下方统一编辑规则。
                ui.label(
                    egui::RichText::new(crate::i18n::tr(
                        ctx,
                        "Built-in helpers such as output reminders and clickable links. Turn one off and its rules stay saved but stop running.",
                        "扩展模块是一些内置小功能（如终端输出提醒、把日志里的路径变成可点链接）。关掉后规则仍保留，但运行时不再生效。",
                    ))
                    .size(theme.font_size_small())
                    .color(text_low),
                );
                ui.add_space(theme.spacing_sm());

                let modules = self.module_registry.snapshot();
                if modules.is_empty() {
                    ui.label(
                        egui::RichText::new(crate::i18n::tr(
                            ctx,
                            "No modular capabilities registered currently.",
                            "当前尚无已注册的模块化能力。",
                        ))
                        .color(text_low),
                    );
                    return;
                }

                let lang = crate::i18n::language(ctx);
                let mut dirty = false;

                let sel_id = egui::Id::unique("preferences_module_selected");
                let stored_id = ui
                    .data_mut(|d| d.get_persisted::<String>(sel_id))
                    .unwrap_or_default();
                let mut selected_id = if modules.iter().any(|m| m.id == stored_id) {
                    stored_id
                } else {
                    modules
                        .first()
                        .map(|m| m.id.clone())
                        .unwrap_or_default()
                };

                for m in &modules {
                    let is_env_locked = self.module_registry.is_env_disabled(&m.id);
                    let mut enabled = m.enabled;
                    let display_name = if lang == crate::i18n::UiLanguage::Zh {
                        m.name_zh.clone()
                    } else {
                        m.name_en.clone()
                    };
                    let subtitle = module_subtitle(ctx, &m.id);
                    let is_selected = m.id == selected_id;

                    ui.horizontal(|ui| {
                        if is_env_locked {
                            let mut dummy = false;
                            ui.add_enabled(false, egui::Checkbox::new(&mut dummy, ""));
                        } else if ui.checkbox(&mut enabled, "").changed() {
                            if enabled {
                                self.app_settings.disabled_modules.retain(|id| id != &m.id);
                            } else if !self.app_settings.disabled_modules.contains(&m.id) {
                                self.app_settings.disabled_modules.push(m.id.clone());
                            }
                            dirty = true;
                        }

                        ui.vertical(|ui| {
                            let name_color = if is_selected {
                                theme.accent_color()
                            } else if is_env_locked {
                                text_low
                            } else {
                                label_color
                            };
                            let name = egui::RichText::new(display_name).color(name_color);
                            if ui.selectable_label(is_selected, name).clicked() {
                                selected_id = m.id.clone();
                            }
                            ui.label(
                                egui::RichText::new(subtitle)
                                    .size(theme.font_size_small())
                                    .color(text_low),
                            );
                            if is_env_locked {
                                ui.label(
                                    egui::RichText::new(crate::i18n::tr(
                                        ctx,
                                        "[Disabled by advanced override]",
                                        "[已由高级覆盖强制禁用]",
                                    ))
                                    .size(theme.font_size_small())
                                    .color(theme.accent_color()),
                                );
                            }
                        });
                    });
                    ui.add_space(theme.spacing_xs());
                }
                ui.data_mut(|d| d.insert_persisted(sel_id, selected_id.clone()));

                // 统一编辑区：不再在每个模块旁放「管理规则」按钮。
                Self::preferences_row_divider(ui, theme);
                if let Some(m) = modules.iter().find(|m| m.id == selected_id) {
                    let display_name = if lang == crate::i18n::UiLanguage::Zh {
                        m.name_zh.clone()
                    } else {
                        m.name_en.clone()
                    };
                    ui.label(egui::RichText::new(display_name).color(label_color));
                    ui.label(
                        egui::RichText::new(module_subtitle(ctx, &m.id))
                            .size(theme.font_size_small())
                            .color(text_low),
                    );
                    ui.add_space(theme.spacing_md());
                    if m.id == crate::modules::output_trigger::MODULE_ID {
                        if crate::ui::chrome::panel_action_button_with_icon_ex(
                            ui,
                            theme,
                            crate::ui::icons::IconId::Settings,
                            crate::i18n::tr(ctx, "Edit rules…", "编辑规则…"),
                            true,
                        )
                        .clicked()
                        {
                            self.rule_manager_dialog
                                .open_from_preferences(&self.output_trigger_rules);
                        }
                    } else if m.id == crate::modules::display_link::MODULE_ID {
                        if crate::ui::chrome::panel_action_button_with_icon_ex(
                            ui,
                            theme,
                            crate::ui::icons::IconId::Settings,
                            crate::i18n::tr(ctx, "Edit rules…", "编辑规则…"),
                            true,
                        )
                        .clicked()
                        {
                            self.rule_manager_dialog
                                .open_display_link_from_preferences(&self.display_link_rules);
                        }
                    } else if m.id == crate::modules::cmd_audit_local::MODULE_ID {
                        if crate::ui::chrome::panel_action_button_with_icon_ex(
                            ui,
                            theme,
                            crate::ui::icons::IconId::Settings,
                            crate::i18n::tr(ctx, "Edit rules…", "编辑规则…"),
                            true,
                        )
                        .clicked()
                        {
                            self.rule_manager_dialog
                                .open_command_checks_from_preferences(
                                    &self.cmd_audit_personal_rules,
                                );
                        }
                    } else {
                        ui.label(
                            egui::RichText::new(crate::i18n::tr(
                                ctx,
                                "This module has no editable rules — the on/off switch is all it needs.",
                                "这个模块没有可编辑规则，开关即可生效。",
                            ))
                            .size(theme.font_size_small())
                            .color(text_low),
                        );
                    }
                }

                // 保留底部环境变量/文档提示（不是折叠项）。
                ui.add_space(theme.spacing_sm());
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(crate::i18n::tr(
                            ctx,
                            "An environment variable can also override the UI switches.",
                            "也可用环境变量覆盖界面开关。",
                        ))
                        .size(theme.font_size_small())
                        .color(text_low),
                    );
                    if crate::ui::chrome::panel_caption_link(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "See docs", "见文档"),
                    )
                    .clicked()
                    {
                        let _ = crate::platform::open_url(crate::platform::DOCS_INDEX_URL);
                    }
                });

                if dirty {
                    self.module_registry.sync_effective_disabled(&self.app_settings.disabled_modules);
                    let _ = self.app_settings.save();
                }
            },
        );
    }

}

/// 路径中间省略，悬停仍给出全文。`max_chars` 含省略号。
fn ellipsize_middle(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if max_chars == 0 || count <= max_chars {
        return text.to_string();
    }
    if max_chars == 1 {
        return "…".to_string();
    }
    let keep = max_chars - 1;
    let head = keep / 2;
    let tail = keep - head;
    let h: String = text.chars().take(head).collect();
    let t: String = text
        .chars()
        .rev()
        .take(tail)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{h}…{t}")
}

#[cfg(test)]
mod tests {
    use super::ellipsize_middle;

    #[test]
    fn ellipsize_middle_keeps_both_ends() {
        assert_eq!(ellipsize_middle("short", 10), "short");
        let out = ellipsize_middle("/home/user/.local/share/mistterm/logs", 16);
        assert!(out.starts_with("/home"));
        assert!(out.ends_with("logs") || out.ends_with("/logs") || out.contains('…'));
        assert!(out.chars().count() <= 16);
        assert!(out.contains('…'));
    }
}


fn module_subtitle(ctx: &egui::Context, module_id: &str) -> &'static str {
    match module_id {
        crate::modules::cmd_audit_local::MODULE_ID => crate::i18n::tr(
            ctx,
            "Confirm or block risky commands before they run",
            "执行危险命令前先确认或拦截",
        ),
        crate::modules::output_trigger::MODULE_ID => crate::i18n::tr(
            ctx,
            "Output trigger rules (alert when terminal output matches)",
            "输出触发规则（终端输出命中时提醒）",
        ),
        crate::modules::display_link::MODULE_ID => crate::i18n::tr(
            ctx,
            "Turn paths/URLs in output into links",
            "把日志里的路径/URL 变成可点链接",
        ),
        crate::modules::session_import::MODULE_ID => crate::i18n::tr(
            ctx,
            "Import sessions/hosts from other terminals",
            "从其它终端导入会话/主机",
        ),
        _ => "",
    }
}
