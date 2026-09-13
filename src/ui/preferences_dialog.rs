//! 偏好设置弹窗：分块折叠 + 可滚动内容区。

use super::*;
use eframe::egui::RichText;

impl MistTermApp {
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
        crate::ui::chrome::modal_window("preferences_modal", theme, ctx)
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

                        let scroll_h = (ui.available_height() - theme.spacing_sm()).max(120.0);

                        // 与弹窗表面同色直铺，不再套一层 inset 灰底卡片
                        ui.allocate_ui(egui::vec2(ui.available_width(), scroll_h), |ui| {
                            layout_util::set_width_to_available(ui);
                            let inner_h = ui.available_height().max(80.0);
                            egui::ScrollArea::vertical()
                                .id_source("preferences_modal_scroll")
                                .auto_shrink([false; 2])
                                .max_height(inner_h)
                                .show(ui, |ui| {
                                    layout_util::set_width_to_available(ui);
                                    ui.spacing_mut().item_spacing.y = theme.spacing_md();
                                    self.preferences_section_general(
                                        ui,
                                        ctx,
                                        theme,
                                        label_color,
                                        text_low,
                                    );
                                    self.preferences_section_appearance(
                                        ui,
                                        ctx,
                                        theme,
                                        label_color,
                                    );
                                    self.preferences_section_connection(
                                        ui,
                                        ctx,
                                        theme,
                                        label_color,
                                        text_low,
                                    );
                                    self.preferences_section_terminal_logs(
                                        ui,
                                        ctx,
                                        theme,
                                        label_color,
                                        text_low,
                                    );
                                    self.preferences_section_vault(
                                        ui,
                                        ctx,
                                        theme,
                                        label_color,
                                        text_low,
                                    );
                                    self.preferences_section_audit(
                                        ui,
                                        ctx,
                                        theme,
                                        label_color,
                                        text_low,
                                    );
                                    self.preferences_section_team(
                                        ui,
                                        ctx,
                                        theme,
                                        label_color,
                                        text_low,
                                    );
                                    self.preferences_section_sync(
                                        ui,
                                        ctx,
                                        theme,
                                        label_color,
                                        &mut should_close,
                                    );
                                });
                        });
                    });
                });
            });
        self.show_preferences_dialog = open && !should_close;
    }

    fn preferences_collapsing(
        ui: &mut egui::Ui,
        theme: &crate::ui::theme::Theme,
        section_id: &str,
        title: &str,
        label_color: egui::Color32,
        default_open: bool,
        body: impl FnOnce(&mut egui::Ui),
    ) {
        egui::CollapsingHeader::new(
            RichText::new(title)
                .size(theme.font_size_panel_title())
                .strong()
                .color(label_color),
        )
        .id_source(egui::Id::new(("pref_section", section_id)))
        .default_open(default_open)
        .show(ui, |ui| {
            layout_util::set_width_to_available(ui);
            ui.spacing_mut().item_spacing.y = theme.spacing_panel_gap();
            body(ui);
        });
        ui.add_space(theme.spacing_sm());
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
                ui.add_space(theme.spacing_panel_gap());
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
            },
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
            false,
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

                ui.add_space(theme.spacing_md());
                ui.label(
                    RichText::new(crate::i18n::tr(ctx, "Terminal font", "终端字体"))
                        .size(theme.font_size_small())
                        .color(theme.color_form_hint()),
                );
                ui.add_space(theme.spacing_panel_gap());

                let mut preset = self.terminal_font_preset;
                let preset_label =
                    |ctx: &egui::Context, p: crate::platform::TerminalFontPreset| match p {
                        crate::platform::TerminalFontPreset::Default => {
                            crate::i18n::tr(ctx, "System default", "系统默认")
                        }
                        crate::platform::TerminalFontPreset::Consolas => "Consolas",
                        crate::platform::TerminalFontPreset::CascadiaMono => "Cascadia Mono",
                        crate::platform::TerminalFontPreset::JetBrainsMono => "JetBrains Mono",
                    };
                crate::ui::chrome::form_combo(
                    ui,
                    theme,
                    "preferences_terminal_font_preset",
                    preset_label(ctx, preset),
                    layout_util::finite_content_width(ui),
                    |ui| {
                        for p in crate::platform::TerminalFontPreset::ALL {
                            ui.selectable_value(&mut preset, p, preset_label(ctx, p));
                        }
                    },
                );
                if preset != self.terminal_font_preset {
                    self.terminal_font_preset = preset;
                    self.apply_terminal_font_preferences(ctx);
                }

                // 固定行高，避免控件被拉高、标签与输入基线错位
                crate::ui::chrome::form_control_row(ui, theme, |ui| {
                    crate::ui::chrome::form_inline_label(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Font size", "字号"),
                    );
                    let mut size = self.terminal_font_size;
                    if crate::ui::chrome::form_f32_stepper(
                        ui,
                        theme,
                        egui::Id::new("pref_terminal_font_size"),
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
                ui.add_space(theme.spacing_panel_gap());
                ui.label(
                    RichText::new(format!(
                        "{}  Ag gjpqy  终端 ABC 123  #!/bin/bash",
                        crate::i18n::tr(ctx, "Preview:", "预览：")
                    ))
                    .font(egui::FontId::monospace(self.terminal_font_size))
                    .color(theme.text_primary()),
                );
                ui.label(
                    RichText::new(crate::i18n::tr(
                        ctx,
                        "Ctrl+scroll wheel still zooms the active terminal temporarily.",
                        "Ctrl+滚轮仍可在当前终端临时缩放字号。",
                    ))
                    .size(theme.font_size_small())
                    .color(theme.color_form_hint()),
                );
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
                ui.add_space(theme.spacing_panel_gap());
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
                crate::ui::chrome::form_control_row(ui, theme, |ui| {
                    crate::ui::chrome::form_inline_label(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Ping every (s)", "每隔(秒)"),
                    );
                    crate::ui::chrome::form_u32_stepper(
                        ui,
                        theme,
                        egui::Id::new("pref_ka_interval"),
                        &mut self.default_keepalive_interval_secs,
                        5..=300,
                        5,
                    );
                    ui.add_space(theme.spacing_md());
                    crate::ui::chrome::form_inline_label(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Give up after", "连续无应答"),
                    );
                    crate::ui::chrome::form_u8_stepper(
                        ui,
                        theme,
                        egui::Id::new("pref_ka_count"),
                        &mut self.default_keepalive_count_max,
                        1..=20,
                        1,
                    );
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
            },
        );
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
                crate::ui::chrome::form_control_row(ui, theme, |ui| {
                    crate::ui::chrome::form_inline_label(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Retention (days)", "保留天数"),
                    );
                    crate::ui::chrome::form_u32_stepper(
                        ui,
                        theme,
                        egui::Id::new("pref_log_retention"),
                        &mut self.session_log_settings.retention_days,
                        1..=365,
                        1,
                    );
                });
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
                let path = self.session_log_settings.base_dir.display();
                let log_dir_hint = match crate::i18n::language(ctx) {
                    crate::i18n::UiLanguage::En => {
                        format!("Directory: {path} (max {mb} MB per file)")
                    }
                    crate::i18n::UiLanguage::Zh => {
                        format!("目录：{path}(单文件上限 {mb} MB)")
                    }
                };
                ui.label(
                    RichText::new(log_dir_hint)
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
                crate::ui::chrome::form_field_label(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "Address", "地址"),
                );
                let vault_addr = crate::ui::chrome::form_singleline_field(
                    ui,
                    theme,
                    egui::Id::new("pref_vault_addr"),
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
                    egui::Id::new("pref_vault_ns"),
                    &mut self.app_settings.vault.namespace,
                    "",
                    pref_w,
                    false,
                );
                if vault_ns.lost_focus() {
                    let _ = self.app_settings.save();
                }
                crate::ui::chrome::form_field_label(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "Default KV mount", "默认密钥路径"),
                );
                let vault_mount = crate::ui::chrome::form_singleline_field(
                    ui,
                    theme,
                    egui::Id::new("pref_vault_mount"),
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
                crate::ui::chrome::form_field_label(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "Token", "Token"),
                );
                crate::ui::chrome::form_singleline_field(
                    ui,
                    theme,
                    egui::Id::new("pref_vault_token_buf"),
                    &mut self.pref_vault_token_draft,
                    crate::i18n::tr(
                        ctx,
                        "Enter token, then click Save",
                        "输入 Token 后点「保存」",
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
                                "Token 已保存到钥匙串",
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
                let pref_w = layout_util::finite_content_width(ui);
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
                crate::ui::chrome::form_control_row(ui, theme, |ui| {
                    crate::ui::chrome::form_inline_label(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Retention (days)", "保留天数"),
                    );
                    let r = crate::ui::chrome::form_u32_stepper(
                        ui,
                        theme,
                        egui::Id::new("pref_audit_retention"),
                        &mut self.app_settings.audit.retention_days,
                        1..=3650,
                        1,
                    );
                    if r.changed() {
                        let _ = self.app_settings.save();
                    }
                });
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
                // 团队 HTTP 上报由登录后的 configure_team_audit_sink 自动配置
                // (URL / Bearer / team_id)，用户无需在偏好里手动维护。
                crate::ui::chrome::form_checkbox(
                    ui,
                    theme,
                    &mut self.app_settings.audit.syslog.enabled,
                    crate::i18n::tr(ctx, "Also send to a Syslog server", "同时发送到 Syslog 服务器"),
                );
                crate::ui::chrome::form_control_row(ui, theme, |ui| {
                    crate::ui::chrome::form_inline_label(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Host", "主机"),
                    );
                    let host_w = (pref_w * 0.45).max(120.0);
                    crate::ui::chrome::form_inline_singleline(
                        ui,
                        theme,
                        egui::Id::new("pref_audit_syslog_host"),
                        &mut self.app_settings.audit.syslog.host,
                        "127.0.0.1",
                        host_w,
                        false,
                    );
                    crate::ui::chrome::form_inline_label(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Port", "端口"),
                    );
                    crate::ui::chrome::form_u16_stepper(
                        ui,
                        theme,
                        egui::Id::new("pref_audit_syslog_port"),
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
                });
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
            true,
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
                if crate::ui::chrome::panel_action_primary_icon_button(
                    ui,
                    theme,
                    crate::ui::icons::IconId::Cloud,
                    crate::i18n::tr(ctx, "Cloud sync", "云同步"),
                )
                .clicked()
                {
                    *should_close = true;
                    if Self::right_dock_open_allowed(Self::layout_window_width(ctx)) {
                        self.open_right_dock_panel(ActiveRightDock::CloudSync);
                    } else {
                        let w = Self::layout_window_width(ctx);
                        self.notify_warn(Self::narrow_window_right_dock_hint(ctx, w));
                    }
                }
                ui.add_space(theme.spacing_panel_gap());
                ui.label(
                    RichText::new(crate::i18n::tr(
                        ctx,
                        "Use the menu bar for View / Tools / Help.",
                        "其余项请用顶部菜单：视图、工具、帮助。",
                    ))
                    .size(theme.font_size_small())
                    .color(theme.color_form_hint()),
                );
            },
        );
    }
}
