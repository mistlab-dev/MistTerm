//! 规则管理模态（#185）。
//!
//! 主入口：工具 → 自动化规则… →「输出触发」。
//! 次入口：偏好 → 扩展模块 → 本行「管理规则…」。
//! 同窗预留给 #186 / #187 的其它规则页；现在只有输出触发这一页。
//! 规则增删改不放进偏好折叠正文。

use eframe::egui;

use crate::core::cmd_audit_personal::{CmdAuditPersonalStore, PersonalDraft, PersonalRuleError, MAX_PERSONAL_RULES};
use crate::modules::display_link::{
    check_link_pattern, LinkImportReport, LinkRuleDraft, LinkRuleError, DisplayLinkRule,
    DisplayLinkStore, ACTION_WHITELIST, MAX_RULES,
};
use crate::modules::output_trigger::{
    check_pattern, ImportReport, OutputTriggerRule, OutputTriggerStore, RuleDraft, RuleError,
    TriggerAction, TriggerSeverity, MAX_RULES as OT_MAX_RULES,
};
use crate::ui::theme::Theme;

/// 规则管理里的页。新增页时在这里加一项，不要另开 Dock。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuleManagerTab {
    OutputTrigger,
    DisplayLink,
    CommandChecks,
}

pub struct RuleManagerDialog {
    pub open: bool,
    close_preferences: bool,
    tab: RuleManagerTab,
    selected_id: Option<String>,
    creating: bool,
    pattern: String,
    severity: TriggerSeverity,
    action: TriggerAction,
    priority: u32,
    enabled: bool,
    error: String,
    status: String,
    confirm_reset: bool,
    // --- 可点击链接（#186）独立表单态，与输出触发互不串 ---
    link_selected_id: Option<String>,
    link_creating: bool,
    link_pattern: String,
    link_category: String,
    link_action: String,
    link_priority: u32,
    link_enabled: bool,
    // --- 本地命令检查（#187）独立表单态 ---
    cmd_selected_id: Option<String>,
    cmd_creating: bool,
    cmd_name: String,
    cmd_pattern: String,
    cmd_match_type: String,
    cmd_action: String,
    cmd_description: String,
    cmd_priority: i32,
    cmd_enabled: bool,
    cmd_personal_enabled: bool,
}

impl Default for RuleManagerDialog {
    fn default() -> Self {
        Self {
            open: false,
            close_preferences: false,
            tab: RuleManagerTab::OutputTrigger,
            selected_id: None,
            creating: false,
            pattern: String::new(),
            severity: TriggerSeverity::Info,
            action: TriggerAction::Notify,
            priority: 100,
            enabled: true,
            error: String::new(),
            status: String::new(),
            confirm_reset: false,
            link_selected_id: None,
            link_creating: false,
            link_pattern: String::new(),
            link_category: "path".to_string(),
            link_action: "sftp-open".to_string(),
            link_priority: 100,
            link_enabled: true,
            cmd_selected_id: None,
            cmd_creating: false,
            cmd_name: String::new(),
            cmd_pattern: String::new(),
            cmd_match_type: "regex".to_string(),
            cmd_action: "confirm".to_string(),
            cmd_description: String::new(),
            cmd_priority: 100,
            cmd_enabled: true,
            cmd_personal_enabled: false,
        }
    }
}

impl RuleManagerDialog {
    pub fn open_output_trigger(&mut self, store: &OutputTriggerStore) {
        self.open = true;
        self.tab = RuleManagerTab::OutputTrigger;
        self.error.clear();
        self.confirm_reset = false;
        self.ensure_selection(store);
    }

    /// 次入口（偏好 → 扩展模块 → 管理规则…）或切 Tab 用：落到可点击链接页。
    pub fn open_display_link(&mut self, store: &DisplayLinkStore) {
        self.open = true;
        self.tab = RuleManagerTab::DisplayLink;
        self.error.clear();
        self.confirm_reset = false;
        self.ensure_link_selection(store);
    }

    /// 从偏好里打开时关掉偏好，避免两个模态叠在一起。
    pub fn open_from_preferences(&mut self, store: &OutputTriggerStore) {
        self.open_output_trigger(store);
        self.close_preferences = true;
    }

    /// 偏好 → 扩展模块 → 可点击链接行「管理规则…」。
    pub fn open_display_link_from_preferences(&mut self, store: &DisplayLinkStore) {
        self.open_display_link(store);
        self.close_preferences = true;
    }

    /// 主/次入口落到「本地命令检查」Tab（#187）。
    pub fn open_command_checks(&mut self, personal: &CmdAuditPersonalStore) {
        self.open = true;
        self.tab = RuleManagerTab::CommandChecks;
        self.error.clear();
        self.confirm_reset = false;
        self.cmd_personal_enabled = personal.personal_enabled();
        self.ensure_cmd_selection(personal);
    }

    /// 偏好 → 扩展模块 → 本地命令检查行「管理规则…」。
    pub fn open_command_checks_from_preferences(&mut self, personal: &CmdAuditPersonalStore) {
        self.open_command_checks(personal);
        self.close_preferences = true;
    }

    pub fn take_close_preferences(&mut self) -> bool {
        let close = self.close_preferences;
        self.close_preferences = false;
        close
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        store: &OutputTriggerStore,
        link_store: &DisplayLinkStore,
        cmd_personal: &CmdAuditPersonalStore,
        cmd_engine: &std::sync::Arc<std::sync::Mutex<crate::core::cmd_audit::CmdAuditEngine>>,
        module_enabled: bool,
        link_module_enabled: bool,
        cmd_module_enabled: bool,
    ) {
        if !self.open {
            return;
        }
        match self.tab {
            RuleManagerTab::OutputTrigger => self.ensure_selection(store),
            RuleManagerTab::DisplayLink => self.ensure_link_selection(link_store),
            RuleManagerTab::CommandChecks => self.ensure_cmd_selection(cmd_personal),
        }
        let mut keep_open = self.open;
        let mut should_close = false;
        let modal_sz = egui::vec2(860.0, 620.0);
        crate::ui::chrome::modal_window("rule_manager_dialog", theme, ctx)
            .open(&mut keep_open)
            .default_size(modal_sz)
            .min_width(640.0)
            .movable(true)
            .resizable(true)
            .show(ctx, |ui| {
                crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                    if crate::ui::chrome::modal_header(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Automation Rules", "自动化规则"),
                        theme.font_size_modal_title(),
                    ) {
                        should_close = true;
                    }
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(
                                self.tab == RuleManagerTab::OutputTrigger,
                                crate::i18n::tr(ctx, "Output Trigger", "输出触发"),
                            )
                            .clicked()
                        {
                            self.tab = RuleManagerTab::OutputTrigger;
                            self.ensure_selection(store);
                        }
                        if ui
                            .selectable_label(
                                self.tab == RuleManagerTab::DisplayLink,
                                crate::i18n::tr(ctx, "Clickable links", "可点击链接"),
                            )
                            .clicked()
                        {
                            self.tab = RuleManagerTab::DisplayLink;
                            self.ensure_link_selection(link_store);
                        }
                        if ui
                            .selectable_label(
                                self.tab == RuleManagerTab::CommandChecks,
                                crate::i18n::tr(ctx, "Command checks", "本地命令检查"),
                            )
                            .clicked()
                        {
                            self.tab = RuleManagerTab::CommandChecks;
                            self.ensure_cmd_selection(cmd_personal);
                        }
                    });
                    ui.separator();
                    match self.tab {
                        RuleManagerTab::OutputTrigger => {
                            self.paint_output_trigger(ui, theme, ctx, store, module_enabled);
                        }
                        RuleManagerTab::DisplayLink => {
                            self.paint_display_link(ui, theme, ctx, link_store, link_module_enabled);
                        }
                        RuleManagerTab::CommandChecks => {
                            self.paint_command_checks(ui, theme, ctx, cmd_personal, cmd_engine, cmd_module_enabled);
                        }
                    }
                });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            should_close = true;
        }
        self.open = keep_open && !should_close;
    }

    fn ensure_selection(&mut self, store: &OutputTriggerStore) {
        if self.creating {
            return;
        }
        let rules = store.rules();
        let selected_ok = self
            .selected_id
            .as_ref()
            .is_some_and(|id| rules.iter().any(|rule| &rule.id == id));
        if selected_ok {
            return;
        }
        if let Some(first) = rules.first() {
            self.load_rule(first);
        } else {
            self.clear_form();
        }
    }

    fn paint_output_trigger(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        ctx: &egui::Context,
        store: &OutputTriggerStore,
        module_enabled: bool,
    ) {
        if !module_enabled {
            ui.label(
                egui::RichText::new(crate::i18n::tr(
                    ctx,
                    "Output Trigger is off (Preferences, or MISTTERM_DISABLE_MODULES). You can still edit rules. They will not fire until the module is on.",
                    "输出触发模块已关闭 (偏好设置, 或环境变量 MISTTERM_DISABLE_MODULES). 规则仍可编辑, 模块打开后才会触发.",
                ))
                .size(theme.font_size_caption())
                .color(theme.accent_color()),
            );
            ui.add_space(theme.spacing_xs());
        }
        let source = if store.customized() {
            crate::i18n::tr(
                ctx,
                "Using your saved rules. Changes apply on the next output line. No restart.",
                "正在使用已保存的规则. 改完后下一行输出立即生效, 不用重启.",
            )
        } else {
            crate::i18n::tr(
                ctx,
                "Built-in rules are in effect until you edit them. Changes apply on the next output line. No restart.",
                "尚未自定义, 正在使用内置规则. 改完后下一行输出立即生效, 不用重启.",
            )
        };
        ui.label(
            egui::RichText::new(source)
                .size(theme.font_size_caption())
                .color(theme.text_tertiary()),
        );
        ui.add_space(theme.spacing_sm());
        let content_w = crate::ui::layout_util::finite_content_width(ui);
        ui.set_min_width(content_w);
        ui.horizontal(|ui| {
            self.paint_list(ui, theme, ctx, store, content_w * 0.38);
            ui.separator();
            self.paint_editor(ui, theme, ctx, store, content_w * 0.58);
        });
        if !self.error.is_empty() {
            ui.add_space(theme.spacing_sm());
            ui.label(
                egui::RichText::new(&self.error)
                    .size(theme.font_size_caption())
                    .color(theme.color_danger_emphasis()),
            );
        } else if !self.status.is_empty() {
            ui.add_space(theme.spacing_sm());
            ui.label(
                egui::RichText::new(&self.status)
                    .size(theme.font_size_caption())
                    .color(theme.text_secondary()),
            );
        }
        ui.add_space(theme.spacing_sm());
        self.paint_footer(ui, theme, ctx, store);
    }

    fn paint_list(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        ctx: &egui::Context,
        store: &OutputTriggerStore,
        width: f32,
    ) {
        ui.vertical(|ui| {
            ui.set_width(width.max(200.0));
            ui.horizontal(|ui| {
                if crate::ui::chrome::panel_header_new_button_with_label(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "New rule", "新建规则"),
                )
                .clicked()
                {
                    self.clear_form();
                    self.confirm_reset = false;
                }
                ui.label(
                    egui::RichText::new(format!("{}/{}", store.rules().len(), MAX_RULES))
                        .size(theme.font_size_caption())
                        .color(theme.text_tertiary()),
                );
            });
            ui.add_space(theme.spacing_xs());
            let mut rules = store.rules();
            rules.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.id.cmp(&b.id)));
            let mut toggles: Vec<(String, bool)> = Vec::new();
            let mut pick: Option<String> = None;
            egui::ScrollArea::vertical()
                .id_source("output_trigger_rule_list")
                .max_height(340.0)
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    if rules.is_empty() {
                        ui.label(
                            egui::RichText::new(crate::i18n::tr(
                                ctx,
                                "No rules. Built-in rules return when you restore defaults.",
                                "没有规则. 点「恢复内置规则」可以回到默认.",
                            ))
                            .size(theme.font_size_caption())
                            .color(theme.text_tertiary()),
                        );
                        return;
                    }
                    for rule in &rules {
                        ui.horizontal(|ui| {
                            let mut on = rule.enabled;
                            if ui
                                .checkbox(&mut on, "")
                                .on_hover_text(crate::i18n::tr(
                                    ctx,
                                    "Enable or disable this rule",
                                    "启用或停用这条规则",
                                ))
                                .changed()
                            {
                                toggles.push((rule.id.clone(), on));
                            }
                            let selected = self.selected_id.as_deref() == Some(rule.id.as_str())
                                && !self.creating;
                            let label = rule_list_label(ctx, rule);
                            let mut text = egui::RichText::new(label);
                            if !rule.enabled || rule.pattern_error.is_some() {
                                text = text.color(theme.text_tertiary());
                            }
                            if ui.selectable_label(selected, text).clicked() {
                                pick = Some(rule.id.clone());
                            }
                        });
                        if rule.pattern_error.is_some() {
                            ui.label(
                                egui::RichText::new(crate::i18n::tr(
                                    ctx,
                                    "Invalid regular expression. This rule will not match.",
                                    "正则无效, 这条规则不会命中.",
                                ))
                                .size(theme.font_size_caption())
                                .color(theme.color_danger_emphasis()),
                            );
                        }
                    }
                });
            for (id, enabled) in toggles {
                self.confirm_reset = false;
                if let Err(err) = store.set_enabled(&id, enabled) {
                    self.set_error(explain_rule_error(ctx, &err));
                } else if let Some(err) = store.last_save_error() {
                    self.set_error(save_failed(ctx, &err));
                } else {
                    self.set_status(crate::i18n::tr(
                        ctx,
                        "Saved. Takes effect on the next output line.",
                        "已保存, 下一行输出立即生效.",
                    ));
                    if self.selected_id.as_deref() == Some(id.as_str()) {
                        self.enabled = enabled;
                    }
                }
            }
            if let Some(id) = pick {
                if let Some(rule) = store.rules().into_iter().find(|rule| rule.id == id) {
                    self.load_rule(&rule);
                    self.confirm_reset = false;
                }
            }
        });
    }

    fn paint_editor(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        ctx: &egui::Context,
        store: &OutputTriggerStore,
        width: f32,
    ) {
        ui.vertical(|ui| {
            let width = width.max(280.0);
            ui.set_width(width);
            let id_line = if self.creating || self.selected_id.is_none() {
                crate::i18n::tr(
                    ctx,
                    "A new id is assigned when you save.",
                    "保存时会生成规则 ID.",
                )
                .to_string()
            } else {
                format!(
                    "{} {}",
                    crate::i18n::tr(ctx, "Rule id", "规则 ID"),
                    self.selected_id.as_deref().unwrap_or("")
                )
            };
            ui.label(
                egui::RichText::new(id_line)
                    .size(theme.font_size_caption())
                    .color(theme.text_tertiary()),
            );
            ui.add_space(theme.spacing_xs());
            let mut enabled = self.enabled;
            if ui
                .checkbox(&mut enabled, crate::i18n::tr(ctx, "Enabled", "启用"))
                .changed()
            {
                self.enabled = enabled;
                self.confirm_reset = false;
            }
            ui.add_space(theme.spacing_sm());
            crate::ui::chrome::form_field_label(
                ui,
                theme,
                crate::i18n::tr(ctx, "Pattern (regular expression)", "匹配模式 (正则)"),
            );
            crate::ui::chrome::form_multiline_field_with_hint(
                ui,
                theme,
                egui::Id::new("output_trigger_pattern"),
                &mut self.pattern,
                crate::i18n::tr(ctx, "HELLO_MIST", "HELLO_MIST"),
                width,
                4,
                false,
            );
            if let Some(message) = live_pattern_error(ctx, &self.pattern) {
                ui.label(
                    egui::RichText::new(message)
                        .size(theme.font_size_caption())
                        .color(theme.color_danger_emphasis()),
                );
            }
            ui.add_space(theme.spacing_sm());
            let combo_w = (width * 0.48).clamp(140.0, 240.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    crate::ui::chrome::form_field_label(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Severity", "严重级别"),
                    );
                    let selected = severity_label(ctx, self.severity);
                    crate::ui::chrome::form_combo(
                        ui,
                        theme,
                        "output_trigger_severity",
                        selected,
                        combo_w,
                        |ui| {
                            for severity in [
                                TriggerSeverity::Error,
                                TriggerSeverity::Warn,
                                TriggerSeverity::Success,
                                TriggerSeverity::Info,
                            ] {
                                ui.selectable_value(
                                    &mut self.severity,
                                    severity,
                                    severity_label(ctx, severity),
                                );
                            }
                        },
                    );
                });
                ui.vertical(|ui| {
                    crate::ui::chrome::form_field_label(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Notify action", "通知动作"),
                    );
                    let selected = action_label(ctx, self.action);
                    crate::ui::chrome::form_combo(
                        ui,
                        theme,
                        "output_trigger_action",
                        selected,
                        combo_w,
                        |ui| {
                            ui.selectable_value(
                                &mut self.action,
                                TriggerAction::Notify,
                                action_label(ctx, TriggerAction::Notify),
                            );
                        },
                    );
                });
            });
            ui.add_space(theme.spacing_sm());
            crate::ui::chrome::form_control_row(ui, theme, |ui| {
                crate::ui::chrome::form_inline_label(
                    ui,
                    theme,
                    crate::i18n::tr(
                        ctx,
                        "Priority (lower matches first)",
                        "优先级 (越小越先匹配)",
                    ),
                );
                crate::ui::chrome::form_u32_stepper(
                    ui,
                    theme,
                    egui::Id::new("output_trigger_priority"),
                    &mut self.priority,
                    0..=255,
                    1,
                );
            });
            ui.add_space(theme.spacing_sm());
            ui.horizontal(|ui| {
                if crate::ui::chrome::panel_action_primary_button(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "Save", "保存"),
                )
                .clicked()
                {
                    self.save_editor(ctx, store);
                }
                if self.selected_id.is_some() && !self.creating {
                    if crate::ui::chrome::panel_action_button(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Delete", "删除"),
                    )
                    .clicked()
                    {
                        self.delete_selected(ctx, store);
                    }
                }
            });
        });
    }

    fn paint_footer(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        ctx: &egui::Context,
        store: &OutputTriggerStore,
    ) {
        let reset_label = if self.confirm_reset {
            crate::i18n::tr(
                ctx,
                "Click again to restore built-in rules",
                "再点一次, 确认恢复内置规则",
            )
        } else {
            crate::i18n::tr(ctx, "Restore built-in rules", "恢复内置规则")
        };
        if crate::ui::chrome::panel_action_button(ui, theme, reset_label).clicked() {
            if self.confirm_reset {
                match store.reset_to_builtin() {
                    Ok(()) => {
                        self.creating = false;
                        self.selected_id = None;
                        self.ensure_selection(store);
                        self.confirm_reset = false;
                        self.set_status(crate::i18n::tr(
                            ctx,
                            "Built-in rules restored. Takes effect on the next output line.",
                            "已恢复内置规则, 下一行输出立即生效.",
                        ));
                    }
                    Err(err) => self.set_error(save_failed(ctx, &err)),
                }
            } else {
                self.confirm_reset = true;
                self.status.clear();
                self.error.clear();
            }
        }
        ui.add_space(theme.spacing_sm());
        egui::CollapsingHeader::new(crate::i18n::tr(
            ctx,
            "How a toast shows up",
            "怎样看到 Toast",
        ))
        .id_source("output_trigger_toast_help")
        .default_open(false)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(crate::i18n::tr(
                    ctx,
                    "Save a rule whose pattern is HELLO_MIST, severity Info, action Notify. In a connected terminal run: sleep 2; echo HELLO_MIST. Switch to another tab before it prints. The toast title is Output notice. The same rule will not toast again for 5 seconds. The tab you are looking at does not toast (same as the built-in rules).",
                    "保存一条模式为 HELLO_MIST, 严重级别为「提示」, 动作为「通知」的规则. 在已连接的终端执行 sleep 2; echo HELLO_MIST, 打印前先切到另一个标签. Toast 标题是「输出提示」. 同一条规则 5 秒内不会再弹. 你正在看的标签不弹 Toast (和内置规则一样).",
                ))
                .size(theme.font_size_caption())
                .color(theme.text_tertiary()),
            );
        });
        egui::CollapsingHeader::new(crate::i18n::tr(
            ctx,
            "Advanced: import / export JSON",
            "高级: 导入 / 导出 JSON",
        ))
        .id_source("output_trigger_import_export")
        .default_open(false)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(crate::i18n::tr(
                    ctx,
                    "Import replaces every current rule. Invalid patterns are kept but do not match. A broken file leaves the current rules untouched.",
                    "导入会替换当前全部规则. 无效正则会留下但不会命中. 文件损坏时不会改掉现有规则.",
                ))
                .size(theme.font_size_caption())
                .color(theme.text_tertiary()),
            );
            ui.add_space(theme.spacing_xs());
            ui.horizontal(|ui| {
                if crate::ui::chrome::panel_action_button(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "Export JSON…", "导出 JSON…"),
                )
                .clicked()
                {
                    self.export_json(ctx, store);
                }
                if crate::ui::chrome::panel_action_button(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "Import JSON…", "导入 JSON…"),
                )
                .clicked()
                {
                    self.import_json(ctx, store);
                }
            });
        });
    }

    fn save_editor(&mut self, ctx: &egui::Context, store: &OutputTriggerStore) {
        self.confirm_reset = false;
        if let Err(err) = check_pattern(&self.pattern) {
            self.set_error(explain_rule_error(ctx, &err));
            return;
        }
        let draft = RuleDraft {
            id: if self.creating {
                None
            } else {
                self.selected_id.clone()
            },
            pattern: self.pattern.clone(),
            severity: self.severity,
            action: self.action,
            enabled: self.enabled,
            priority: self.priority.min(255) as u8,
        };
        match store.upsert(draft) {
            Ok(rule) => {
                if let Some(err) = store.last_save_error() {
                    self.load_rule(&rule);
                    self.set_error(save_failed(ctx, &err));
                    return;
                }
                self.load_rule(&rule);
                self.set_status(crate::i18n::tr(
                    ctx,
                    "Saved. Takes effect on the next output line.",
                    "已保存, 下一行输出立即生效.",
                ));
            }
            Err(err) => self.set_error(explain_rule_error(ctx, &err)),
        }
    }

    fn delete_selected(&mut self, ctx: &egui::Context, store: &OutputTriggerStore) {
        self.confirm_reset = false;
        let Some(id) = self.selected_id.clone() else {
            return;
        };
        if !store.delete(&id) {
            self.set_error(explain_rule_error(ctx, &RuleError::NotFound));
            return;
        }
        if let Some(err) = store.last_save_error() {
            self.set_error(save_failed(ctx, &err));
        } else {
            self.set_status(crate::i18n::tr(
                ctx,
                "Deleted. Takes effect on the next output line.",
                "已删除, 下一行输出立即生效.",
            ));
        }
        self.creating = false;
        self.selected_id = None;
        self.ensure_selection(store);
    }

    fn export_json(&mut self, ctx: &egui::Context, store: &OutputTriggerStore) {
        self.confirm_reset = false;
        let Some(dest) = crate::ui::file_dialog::FileDialog::new()
            .set_title(crate::i18n::tr(
                ctx,
                "Export output trigger rules",
                "导出输出触发规则",
            ))
            .add_filter("JSON", &["json"])
            .set_file_name("mistterm-output-trigger-rules.json")
            .save_file()
        else {
            return;
        };
        match std::fs::write(&dest, store.export_json()) {
            Ok(()) => self.set_status(match crate::i18n::language(ctx) {
                crate::i18n::UiLanguage::Zh => format!("已导出 {}", dest.display()),
                crate::i18n::UiLanguage::En => format!("Exported {}", dest.display()),
            }),
            Err(err) => self.set_error(format!(
                "{}{err}",
                crate::i18n::tr(ctx, "Export failed: ", "导出失败: ")
            )),
        }
    }

    fn import_json(&mut self, ctx: &egui::Context, store: &OutputTriggerStore) {
        self.confirm_reset = false;
        let Some(src) = crate::ui::file_dialog::FileDialog::new()
            .set_title(crate::i18n::tr(
                ctx,
                "Import output trigger rules",
                "导入输出触发规则",
            ))
            .add_filter("JSON", &["json"])
            .pick_file()
        else {
            return;
        };
        let text = match std::fs::read_to_string(&src) {
            Ok(text) => text,
            Err(err) => {
                self.set_error(format!(
                    "{}{err}",
                    crate::i18n::tr(ctx, "Could not read file: ", "无法读取文件: ")
                ));
                return;
            }
        };
        match store.import_json(&text) {
            Ok(report) => {
                self.creating = false;
                self.selected_id = None;
                self.ensure_selection(store);
                if let Some(err) = store.last_save_error() {
                    self.set_error(save_failed(ctx, &err));
                } else {
                    self.set_status(import_status(ctx, &report));
                }
            }
            Err(err) => self.set_error(explain_rule_error(ctx, &err)),
        }
    }

    fn load_rule(&mut self, rule: &OutputTriggerRule) {
        self.creating = false;
        self.selected_id = Some(rule.id.clone());
        self.pattern = rule.pattern.clone();
        self.severity = rule.severity;
        self.action = rule.action;
        self.priority = u32::from(rule.priority);
        self.enabled = rule.enabled;
    }

    fn clear_form(&mut self) {
        self.creating = true;
        self.selected_id = None;
        self.pattern.clear();
        self.severity = TriggerSeverity::Info;
        self.action = TriggerAction::Notify;
        self.priority = 100;
        self.enabled = true;
        self.error.clear();
        self.status.clear();
    }

    fn set_error(&mut self, message: impl Into<String>) {
        self.error = message.into();
        self.status.clear();
    }

    fn set_status(&mut self, message: impl Into<String>) {
        self.status = message.into();
        self.error.clear();
    }

    // --- 可点击链接（#186）：复制 #185 布局节奏，字段独立不串台 ---

    fn ensure_cmd_selection(&mut self, personal: &CmdAuditPersonalStore) {
        if self.cmd_creating {
            return;
        }
        self.cmd_personal_enabled = personal.personal_enabled();
        let rules = personal.rules();
        let selected_ok = self
            .cmd_selected_id
            .as_ref()
            .is_some_and(|id| rules.iter().any(|r| &r.id == id));
        if selected_ok {
            return;
        }
        if let Some(first) = rules.first() {
            self.load_cmd_rule(first);
        } else {
            self.clear_cmd_form();
        }
    }

    fn load_cmd_rule(&mut self, rule: &crate::core::cmd_audit::CmdAuditRule) {
        self.cmd_creating = false;
        self.cmd_selected_id = Some(rule.id.clone());
        self.cmd_name = rule.name.clone();
        self.cmd_pattern = rule.pattern.clone();
        self.cmd_match_type = rule.match_type.clone();
        self.cmd_action = rule.action.clone();
        self.cmd_description = rule.description.clone();
        self.cmd_priority = rule.priority;
        self.cmd_enabled = rule.enabled;
    }

    fn clear_cmd_form(&mut self) {
        self.cmd_creating = true;
        self.cmd_selected_id = None;
        self.cmd_name.clear();
        self.cmd_pattern.clear();
        self.cmd_match_type = "regex".to_string();
        self.cmd_action = "confirm".to_string();
        self.cmd_description.clear();
        self.cmd_priority = 100;
        self.cmd_enabled = true;
    }

    fn paint_command_checks(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        ctx: &egui::Context,
        personal: &CmdAuditPersonalStore,
        engine: &std::sync::Arc<std::sync::Mutex<crate::core::cmd_audit::CmdAuditEngine>>,
        module_enabled: bool,
    ) {
        if !module_enabled {
            ui.label(
                egui::RichText::new(crate::i18n::tr(
                    ctx,
                    "Command checks is off (Preferences, or MISTTERM_DISABLE_MODULES). You can still edit rules. They will not apply until the module is on.",
                    "本地命令检查模块已关闭 (偏好设置, 或环境变量 MISTTERM_DISABLE_MODULES). 规则仍可编辑, 模块打开后才会生效.",
                ))
                .size(theme.font_size_caption())
                .color(theme.accent_color()),
            );
            ui.add_space(theme.spacing_xs());
        }

        // 方案 §7 三段式之一：个人规则（可 CRUD）——主交付。
        ui.heading(egui::RichText::new(crate::i18n::tr(
            ctx,
            "Personal rules",
            "个人规则",
        ))
        .size(theme.font_size_section_title()));
        let enable_label = if self.cmd_personal_enabled {
            crate::i18n::tr(
                ctx,
                "Personal checks are ON: they run even without a team policy.",
                "个人检查已开启: 没有团队策略时也会生效.",
            )
        } else {
            crate::i18n::tr(
                ctx,
                "Personal checks are OFF: they only run when a team policy is active.",
                "个人检查已关闭: 仅在团队策略激活时生效.",
            )
        };
        ui.label(
            egui::RichText::new(enable_label)
                .size(theme.font_size_caption())
                .color(theme.text_tertiary()),
        );
        ui.add_space(theme.spacing_xs());
        let content_w = crate::ui::layout_util::finite_content_width(ui);
        ui.set_min_width(content_w);
        ui.horizontal(|ui| {
            self.paint_cmd_list(ui, theme, ctx, personal, engine, content_w * 0.34);
            ui.separator();
            self.paint_cmd_editor(ui, theme, ctx, personal, engine, content_w * 0.62);
        });
        if !self.error.is_empty() {
            ui.add_space(theme.spacing_sm());
            ui.label(
                egui::RichText::new(&self.error)
                    .size(theme.font_size_caption())
                    .color(theme.color_danger_emphasis()),
            );
        } else if !self.status.is_empty() {
            ui.add_space(theme.spacing_sm());
            ui.label(
                egui::RichText::new(&self.status)
                    .size(theme.font_size_caption())
                    .color(theme.text_secondary()),
            );
        }
        ui.add_space(theme.spacing_sm());

        // 方案 §7 三段式之二：内置规则（只读 + 本地启停）。
        ui.separator();
        ui.heading(egui::RichText::new(crate::i18n::tr(
            ctx,
            "Built-in rules (read-only)",
            "内置规则 (只读)",
        ))
        .size(theme.font_size_section_title()));
        self.paint_cmd_builtin(ui, theme, ctx, personal, engine);

        // 方案 §7 三段式之三：团队规则（只读说明 + 生效条数）。
        ui.separator();
        ui.heading(egui::RichText::new(crate::i18n::tr(
            ctx,
            "Team rules (read-only)",
            "团队规则 (只读)",
        ))
        .size(theme.font_size_section_title()));
        let team = match engine.lock() {
            Ok(e) => {
                if e.team_policy_active() {
                    let count = e.team_rule_count();
                    format!(
                        "{}", 
                        crate::i18n::tr(
                            ctx,
                            "Team policy is active. Team rules are managed on the server; the desktop cannot change them.",
                            "团队策略已激活. 团队规则在服务端管理, 桌面端不能修改.",
                        )
                    )
                } else {
                    crate::i18n::tr(
                        ctx,
                        "No team policy. Team rules are managed on the server; the desktop cannot change them.",
                        "没有团队策略. 团队规则在服务端管理, 桌面端不能修改.",
                    )
                    .to_string()
                }
            }
            Err(_) => crate::i18n::tr(
                ctx,
                "Team rule status is unavailable.",
                "团队规则状态不可用.",
            )
            .to_string(),
        };
        ui.label(
            egui::RichText::new(team)
                .size(theme.font_size_caption())
                .color(theme.text_tertiary()),
        );
        ui.add_space(theme.spacing_sm());
        self.paint_cmd_footer(ui, theme, ctx, personal, engine);
    }

    fn paint_cmd_list(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        ctx: &egui::Context,
        personal: &CmdAuditPersonalStore,
        engine: &std::sync::Arc<std::sync::Mutex<crate::core::cmd_audit::CmdAuditEngine>>,
        width: f32,
    ) {
        ui.vertical(|ui| {
            ui.set_width(width.max(200.0));
            ui.horizontal(|ui| {
                if crate::ui::chrome::panel_header_new_button_with_label(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "New rule", "新建规则"),
                )
                .clicked()
                {
                    self.clear_cmd_form();
                    self.confirm_reset = false;
                }
                ui.label(
                    egui::RichText::new(format!(
                        "{}/{}",
                        personal.rules().len(),
                        MAX_PERSONAL_RULES
                    ))
                    .size(theme.font_size_caption())
                    .color(theme.text_tertiary()),
                );
            });
            ui.add_space(theme.spacing_xs());
            let mut rules = personal.rules();
            rules.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.id.cmp(&b.id)));
            let mut toggles: Vec<(String, bool)> = Vec::new();
            let mut pick: Option<String> = None;
            egui::ScrollArea::vertical()
                .id_source("cmd_personal_rule_list")
                .max_height(300.0)
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    if rules.is_empty() {
                        ui.label(
                            egui::RichText::new(crate::i18n::tr(
                                ctx,
                                "No personal rules yet.",
                                "还没有个人规则.",
                            ))
                            .size(theme.font_size_caption())
                            .color(theme.text_tertiary()),
                        );
                        return;
                    }
                    for rule in &rules {
                        ui.horizontal(|ui| {
                            let mut on = rule.enabled;
                            if ui.checkbox(&mut on, "").changed() {
                                toggles.push((rule.id.clone(), on));
                            }
                            let selected = self.cmd_selected_id.as_deref()
                                == Some(rule.id.as_str())
                                && !self.cmd_creating;
                            let mut text =
                                egui::RichText::new(cmd_rule_list_label(ctx, rule));
                            if !rule.enabled || rule.pattern.is_empty() {
                                text = text.color(theme.text_tertiary());
                            }
                            if ui.selectable_label(selected, text).clicked() {
                                pick = Some(rule.id.clone());
                            }
                        });
                    }
                });
            for (id, enabled) in toggles {
                self.confirm_reset = false;
                if let Some(mut rule) = personal.rules().into_iter().find(|r| r.id == id) {
                    rule.enabled = enabled;
                    let draft = PersonalDraft {
                        id: Some(rule.id.clone()),
                        name: rule.name.clone(),
                        pattern: rule.pattern.clone(),
                        match_type: rule.match_type.clone(),
                        action: rule.action.clone(),
                        scope: rule.scope.clone(),
                        description: rule.description.clone(),
                        priority: rule.priority,
                        enabled: rule.enabled,
                    };
                    if let Err(err) = personal.upsert(draft, engine) {
                        self.set_error(err.explain(crate::i18n::language(ctx)
                            == crate::i18n::UiLanguage::Zh));
                    } else if let Some(err) = personal.last_save_error() {
                        self.set_error(save_failed(ctx, &err));
                    }
                }
            }
            if let Some(id) = pick {
                if let Some(rule) = personal.rules().into_iter().find(|r| r.id == id) {
                    self.load_cmd_rule(&rule);
                    self.confirm_reset = false;
                }
            }
        });
    }

    fn paint_cmd_editor(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        ctx: &egui::Context,
        personal: &CmdAuditPersonalStore,
        engine: &std::sync::Arc<std::sync::Mutex<crate::core::cmd_audit::CmdAuditEngine>>,
        width: f32,
    ) {
        ui.vertical(|ui| {
            let width = width.max(280.0);
            ui.set_width(width);
            let id_line = if self.cmd_creating || self.cmd_selected_id.is_none() {
                crate::i18n::tr(
                    ctx,
                    "A new id is assigned when you save.",
                    "保存时会生成规则 ID.",
                )
                .to_string()
            } else {
                format!(
                    "{} {}",
                    crate::i18n::tr(ctx, "Rule id", "规则 ID"),
                    self.cmd_selected_id.as_deref().unwrap_or("")
                )
            };
            ui.label(
                egui::RichText::new(id_line)
                    .size(theme.font_size_caption())
                    .color(theme.text_tertiary()),
            );
            ui.add_space(theme.spacing_xs());
            crate::ui::chrome::form_field_label(
                ui,
                theme,
                crate::i18n::tr(ctx, "Rule name (shown in prompts)", "规则名 (提示里显示)"),
            );
            crate::ui::chrome::form_singleline_field(
                ui,
                theme,
                egui::Id::new("cmd_rule_name"),
                &mut self.cmd_name,
                crate::i18n::tr(ctx, "e.g. Guard prod db", "例如: 保护生产库"),
                width,
                false,
            );
            ui.add_space(theme.spacing_sm());
            crate::ui::chrome::form_field_label(
                ui,
                theme,
                crate::i18n::tr(ctx, "Pattern", "匹配模式"),
            );
            crate::ui::chrome::form_multiline_field_with_hint(
                ui,
                theme,
                egui::Id::new("cmd_rule_pattern"),
                &mut self.cmd_pattern,
                "rm\\s+-rf\\s+/",
                width,
                3,
                false,
            );
            if let Some(message) = live_cmd_pattern_error(ctx, &self.cmd_pattern, &self.cmd_match_type) {
                ui.label(
                    egui::RichText::new(message)
                        .size(theme.font_size_caption())
                        .color(theme.color_danger_emphasis()),
                );
            }
            ui.add_space(theme.spacing_sm());
            let combo_w = (width * 0.3).clamp(110.0, 190.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    crate::ui::chrome::form_field_label(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Match type", "匹配方式"),
                    );
                    let selected = cmd_match_type_label(ctx, &self.cmd_match_type);
                    crate::ui::chrome::form_combo(
                        ui,
                        theme,
                        "cmd_rule_match_type",
                        selected,
                        combo_w,
                        |ui| {
                            for mt in ["regex", "prefix", "contains", "exact"] {
                                ui.selectable_value(
                                    &mut self.cmd_match_type,
                                    mt.to_string(),
                                    cmd_match_type_label(ctx, mt),
                                );
                            }
                        },
                    );
                });
                ui.vertical(|ui| {
                    crate::ui::chrome::form_field_label(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Action", "动作"),
                    );
                    let selected = cmd_action_label(ctx, &self.cmd_action);
                    crate::ui::chrome::form_combo(
                        ui,
                        theme,
                        "cmd_rule_action",
                        selected,
                        combo_w,
                        |ui| {
                            for action in ["block", "confirm", "alert", "allow"] {
                                ui.selectable_value(
                                    &mut self.cmd_action,
                                    action.to_string(),
                                    cmd_action_label(ctx, action),
                                );
                            }
                        },
                    );
                });
                ui.vertical(|ui| {
                    let mut enabled = self.cmd_enabled;
                    crate::ui::chrome::form_field_label(ui, theme, "");
                    if ui
                        .checkbox(&mut enabled, crate::i18n::tr(ctx, "Enabled", "启用"))
                        .changed()
                    {
                        self.cmd_enabled = enabled;
                        self.confirm_reset = false;
                    }
                });
            });
            ui.add_space(theme.spacing_sm());
            crate::ui::chrome::form_control_row(ui, theme, |ui| {
                crate::ui::chrome::form_inline_label(
                    ui,
                    theme,
                    crate::i18n::tr(
                        ctx,
                        "Priority (lower matches first)",
                        "优先级 (越小越先匹配)",
                    ),
                );
                let mut prio = self.cmd_priority.max(0).min(255) as u32;
                crate::ui::chrome::form_u32_stepper(
                    ui,
                    theme,
                    egui::Id::new("cmd_rule_priority"),
                    &mut prio,
                    0..=255,
                    1,
                );
                self.cmd_priority = prio as i32;
            });
            ui.add_space(theme.spacing_sm());
            ui.horizontal(|ui| {
                if crate::ui::chrome::panel_action_primary_button(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "Save", "保存"),
                )
                .clicked()
                {
                    self.save_cmd_editor(ctx, personal, engine);
                }
                if self.cmd_selected_id.is_some() && !self.cmd_creating {
                    if crate::ui::chrome::panel_action_button(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Delete", "删除"),
                    )
                    .clicked()
                    {
                        self.delete_cmd_selected(ctx, personal, engine);
                    }
                }
            });
        });
    }

    fn paint_cmd_builtin(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        ctx: &egui::Context,
        personal: &CmdAuditPersonalStore,
        engine: &std::sync::Arc<std::sync::Mutex<crate::core::cmd_audit::CmdAuditEngine>>,
    ) {
        let disabled_ids = personal.disabled_builtin_ids();
        let builtin = match engine.lock() {
            Ok(e) => e.builtin_overview(),
            Err(_) => return,
        };
        ui.label(
            egui::RichText::new(crate::i18n::tr(
                ctx,
                "Bundled with the app. You can disable individual rules on this machine; the patterns themselves cannot be edited here.",
                "随应用内置. 可以在本机停用某条规则; 规则正文不能在这里修改.",
            ))
            .size(theme.font_size_caption())
            .color(theme.text_tertiary()),
        );
        ui.add_space(theme.spacing_xs());
        egui::ScrollArea::vertical()
            .id_source("cmd_builtin_list")
            .max_height(180.0)
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                for b in &builtin {
                    ui.horizontal(|ui| {
                        let disabled = disabled_ids.iter().any(|id| id == &b.id);
                        let mut off = disabled;
                        if ui
                            .checkbox(&mut off, "")
                            .on_hover_text(crate::i18n::tr(
                                ctx,
                                "Uncheck to disable this built-in rule locally",
                                "取消勾选可在本机停用这条内置规则",
                            ))
                            .changed()
                        {
                            personal.set_builtin_disabled(&b.id, off, engine);
                            self.set_status(crate::i18n::tr(
                                ctx,
                                "Saved. Takes effect on the next command.",
                                "已保存, 下一条命令即生效.",
                            ));
                        }
                        let mut text = egui::RichText::new(format!("{}  {}", b.level, b.message));
                        if disabled {
                            text = text.color(theme.text_tertiary());
                        }
                        ui.label(text);
                    });
                }
            });
    }

    fn paint_cmd_footer(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        ctx: &egui::Context,
        personal: &CmdAuditPersonalStore,
        engine: &std::sync::Arc<std::sync::Mutex<crate::core::cmd_audit::CmdAuditEngine>>,
    ) {
        ui.horizontal(|ui| {
            let mut on = self.cmd_personal_enabled;
            if ui
                .checkbox(
                    &mut on,
                    crate::i18n::tr(
                        ctx,
                        "Run personal checks even without a team policy",
                        "没有团队策略时也运行个人检查",
                    ),
                )
                .changed()
            {
                self.cmd_personal_enabled = on;
                personal.set_personal_enabled(on, engine);
                self.set_status(crate::i18n::tr(
                    ctx,
                    "Saved. Takes effect on the next command.",
                    "已保存, 下一条命令即生效.",
                ));
            }
        });
        ui.add_space(theme.spacing_sm());
        let reset_label = if self.confirm_reset {
            crate::i18n::tr(
                ctx,
                "Click again to clear personal rules and re-enable all built-in rules",
                "再点一次, 确认清空个人规则并恢复全部内置规则",
            )
        } else {
            crate::i18n::tr(
                ctx,
                "Reset to built-in",
                "恢复内置",
            )
        };
        if crate::ui::chrome::panel_action_button(ui, theme, reset_label).clicked() {
            if self.confirm_reset {
                match personal.reset_to_builtin(engine) {
                    Ok(()) => {
                        self.cmd_creating = false;
                        self.cmd_selected_id = None;
                        self.cmd_personal_enabled = false;
                        self.ensure_cmd_selection(personal);
                        self.confirm_reset = false;
                        self.set_status(crate::i18n::tr(
                            ctx,
                            "Personal rules cleared. Takes effect on the next command.",
                            "已清空个人规则, 下一条命令即生效.",
                        ));
                    }
                    Err(err) => self.set_error(save_failed(ctx, &err)),
                }
            } else {
                self.confirm_reset = true;
                self.status.clear();
                self.error.clear();
            }
        }
        egui::CollapsingHeader::new(crate::i18n::tr(
            ctx,
            "Advanced: import / export JSON",
            "高级: 导入 / 导出 JSON",
        ))
        .id_source("cmd_personal_import_export")
        .default_open(false)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(crate::i18n::tr(
                    ctx,
                    "Import replaces every current personal rule. A broken file leaves the current rules untouched.",
                    "导入会替换当前全部个人规则. 文件损坏时不会改掉现有规则.",
                ))
                .size(theme.font_size_caption())
                .color(theme.text_tertiary()),
            );
            ui.add_space(theme.spacing_xs());
            ui.horizontal(|ui| {
                if crate::ui::chrome::panel_action_button(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "Export JSON…", "导出 JSON…"),
                )
                .clicked()
                {
                    self.export_cmd_json(ctx, personal);
                }
                if crate::ui::chrome::panel_action_button(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "Import JSON…", "导入 JSON…"),
                )
                .clicked()
                {
                    self.import_cmd_json(ctx, personal, engine);
                }
            });
        });
    }

    fn save_cmd_editor(
        &mut self,
        ctx: &egui::Context,
        personal: &CmdAuditPersonalStore,
        engine: &std::sync::Arc<std::sync::Mutex<crate::core::cmd_audit::CmdAuditEngine>>,
    ) {
        self.confirm_reset = false;
        let draft = PersonalDraft {
            id: if self.cmd_creating {
                None
            } else {
                self.cmd_selected_id.clone()
            },
            name: self.cmd_name.clone(),
            pattern: self.cmd_pattern.clone(),
            match_type: self.cmd_match_type.clone(),
            action: self.cmd_action.clone(),
            scope: "command".to_string(),
            description: self.cmd_description.clone(),
            priority: self.cmd_priority,
            enabled: self.cmd_enabled,
        };
        match personal.upsert(draft, engine) {
            Ok(rule) => {
                if let Some(err) = personal.last_save_error() {
                    self.load_cmd_rule(&rule);
                    self.set_error(save_failed(ctx, &err));
                    return;
                }
                self.load_cmd_rule(&rule);
                self.set_status(crate::i18n::tr(
                    ctx,
                    "Saved. Takes effect on the next command.",
                    "已保存, 下一条命令即生效.",
                ));
            }
            Err(err) => self.set_error(err.explain(
                crate::i18n::language(ctx) == crate::i18n::UiLanguage::Zh,
            )),
        }
    }

    fn delete_cmd_selected(
        &mut self,
        ctx: &egui::Context,
        personal: &CmdAuditPersonalStore,
        engine: &std::sync::Arc<std::sync::Mutex<crate::core::cmd_audit::CmdAuditEngine>>,
    ) {
        self.confirm_reset = false;
        let Some(id) = self.cmd_selected_id.clone() else {
            return;
        };
        if !personal.delete(&id, engine) {
            self.set_error(PersonalRuleError::NotFound.explain(
                crate::i18n::language(ctx) == crate::i18n::UiLanguage::Zh,
            ));
            return;
        }
        if let Some(err) = personal.last_save_error() {
            self.set_error(save_failed(ctx, &err));
        } else {
            self.set_status(crate::i18n::tr(
                ctx,
                "Deleted. Takes effect on the next command.",
                "已删除, 下一条命令即生效.",
            ));
        }
        self.cmd_creating = false;
        self.cmd_selected_id = None;
        self.ensure_cmd_selection(personal);
    }

    fn export_cmd_json(&mut self, ctx: &egui::Context, personal: &CmdAuditPersonalStore) {
        self.confirm_reset = false;
        let Some(dest) = crate::ui::file_dialog::FileDialog::new()
            .set_title(crate::i18n::tr(
                ctx,
                "Export personal command check rules",
                "导出个人命令检查规则",
            ))
            .add_filter("JSON", &["json"])
            .set_file_name("mistterm-cmd-audit-personal-rules.json")
            .save_file()
        else {
            return;
        };
        match std::fs::write(&dest, personal.export_json()) {
            Ok(()) => self.set_status(match crate::i18n::language(ctx) {
                crate::i18n::UiLanguage::Zh => format!("已导出 {}", dest.display()),
                crate::i18n::UiLanguage::En => format!("Exported {}", dest.display()),
            }),
            Err(err) => self.set_error(format!(
                "{}{err}",
                crate::i18n::tr(ctx, "Export failed: ", "导出失败: ")
            )),
        }
    }

    fn import_cmd_json(
        &mut self,
        ctx: &egui::Context,
        personal: &CmdAuditPersonalStore,
        engine: &std::sync::Arc<std::sync::Mutex<crate::core::cmd_audit::CmdAuditEngine>>,
    ) {
        self.confirm_reset = false;
        let Some(src) = crate::ui::file_dialog::FileDialog::new()
            .set_title(crate::i18n::tr(
                ctx,
                "Import personal command check rules",
                "导入个人命令检查规则",
            ))
            .add_filter("JSON", &["json"])
            .pick_file()
        else {
            return;
        };
        let text = match std::fs::read_to_string(&src) {
            Ok(text) => text,
            Err(err) => {
                self.set_error(format!(
                    "{}{err}",
                    crate::i18n::tr(ctx, "Could not read file: ", "无法读取文件: ")
                ));
                return;
            }
        };
        match personal.import_json(&text, engine) {
            Ok(count) => {
                self.cmd_personal_enabled = personal.personal_enabled();
                self.cmd_creating = false;
                self.cmd_selected_id = None;
                self.ensure_cmd_selection(personal);
                self.set_status(match crate::i18n::language(ctx) {
                    crate::i18n::UiLanguage::Zh => {
                        format!("已导入 {count} 条, 下一条命令即生效.")
                    }
                    crate::i18n::UiLanguage::En => format!(
                        "Imported {count} rules. Takes effect on the next command."
                    ),
                });
            }
            Err(err) => self.set_error(err.explain(
                crate::i18n::language(ctx) == crate::i18n::UiLanguage::Zh,
            )),
        }
    }

    fn ensure_link_selection(&mut self, store: &DisplayLinkStore) {
        if self.link_creating {
            return;
        }
        let rules = store.rules();
        let selected_ok = self
            .link_selected_id
            .as_ref()
            .is_some_and(|id| rules.iter().any(|rule| &rule.id == id));
        if selected_ok {
            return;
        }
        if let Some(first) = rules.first() {
            self.load_link_rule(first);
        } else {
            self.clear_link_form();
        }
    }

    fn load_link_rule(&mut self, rule: &DisplayLinkRule) {
        self.link_creating = false;
        self.link_selected_id = Some(rule.id.clone());
        self.link_pattern = rule.pattern.clone();
        self.link_category = if rule.category.is_empty() {
            "path".to_string()
        } else {
            rule.category.clone()
        };
        self.link_action = rule.action.clone();
        self.link_priority = u32::from(rule.priority);
        self.link_enabled = rule.enabled;
    }

    fn clear_link_form(&mut self) {
        self.link_creating = true;
        self.link_selected_id = None;
        self.link_pattern.clear();
        self.link_category = "path".to_string();
        self.link_action = "sftp-open".to_string();
        self.link_priority = 100;
        self.link_enabled = true;
    }

    fn paint_display_link(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        ctx: &egui::Context,
        store: &DisplayLinkStore,
        module_enabled: bool,
    ) {
        if !module_enabled {
            ui.label(
                egui::RichText::new(crate::i18n::tr(
                    ctx,
                    "Clickable links is off (Preferences, or MISTTERM_DISABLE_MODULES). You can still edit rules. They will not apply until the module is on.",
                    "可点击链接模块已关闭 (偏好设置, 或环境变量 MISTTERM_DISABLE_MODULES). 规则仍可编辑, 模块打开后才会生效.",
                ))
                .size(theme.font_size_caption())
                .color(theme.accent_color()),
            );
            ui.add_space(theme.spacing_xs());
        }
        let source = if store.customized() {
            crate::i18n::tr(
                ctx,
                "Using your saved rules. Changes apply to the next redrawn screen. No restart.",
                "正在使用已保存的规则. 改完后终端重绘即生效, 不用重启.",
            )
        } else {
            crate::i18n::tr(
                ctx,
                "Built-in rules are in effect until you edit them. Changes apply to the next redrawn screen. No restart.",
                "尚未自定义, 正在使用内置规则. 改完后终端重绘即生效, 不用重启.",
            )
        };
        ui.label(
            egui::RichText::new(source)
                .size(theme.font_size_caption())
                .color(theme.text_tertiary()),
        );
        ui.add_space(theme.spacing_sm());
        let content_w = crate::ui::layout_util::finite_content_width(ui);
        ui.set_min_width(content_w);
        ui.horizontal(|ui| {
            self.paint_link_list(ui, theme, ctx, store, content_w * 0.38);
            ui.separator();
            self.paint_link_editor(ui, theme, ctx, store, content_w * 0.58);
        });
        if !self.error.is_empty() {
            ui.add_space(theme.spacing_sm());
            ui.label(
                egui::RichText::new(&self.error)
                    .size(theme.font_size_caption())
                    .color(theme.color_danger_emphasis()),
            );
        } else if !self.status.is_empty() {
            ui.add_space(theme.spacing_sm());
            ui.label(
                egui::RichText::new(&self.status)
                    .size(theme.font_size_caption())
                    .color(theme.text_secondary()),
            );
        }
        ui.add_space(theme.spacing_sm());
        self.paint_link_footer(ui, theme, ctx, store);
    }

    fn paint_link_list(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        ctx: &egui::Context,
        store: &DisplayLinkStore,
        width: f32,
    ) {
        ui.vertical(|ui| {
            ui.set_width(width.max(200.0));
            ui.horizontal(|ui| {
                if crate::ui::chrome::panel_header_new_button_with_label(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "New rule", "新建规则"),
                )
                .clicked()
                {
                    self.clear_link_form();
                    self.confirm_reset = false;
                }
                ui.label(
                    egui::RichText::new(format!("{}/{}", store.rules().len(), MAX_RULES))
                        .size(theme.font_size_caption())
                        .color(theme.text_tertiary()),
                );
            });
            ui.add_space(theme.spacing_xs());
            let mut rules = store.rules();
            rules.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.id.cmp(&b.id)));
            let mut toggles: Vec<(String, bool)> = Vec::new();
            let mut pick: Option<String> = None;
            egui::ScrollArea::vertical()
                .id_source("display_link_rule_list")
                .max_height(340.0)
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    if rules.is_empty() {
                        ui.label(
                            egui::RichText::new(crate::i18n::tr(
                                ctx,
                                "No rules. Built-in rules return when you restore defaults.",
                                "没有规则. 点「恢复内置规则」可以回到默认.",
                            ))
                            .size(theme.font_size_caption())
                            .color(theme.text_tertiary()),
                        );
                        return;
                    }
                    for rule in &rules {
                        ui.horizontal(|ui| {
                            let mut on = rule.enabled;
                            if ui
                                .checkbox(&mut on, "")
                                .on_hover_text(crate::i18n::tr(
                                    ctx,
                                    "Enable or disable this rule",
                                    "启用或停用这条规则",
                                ))
                                .changed()
                            {
                                toggles.push((rule.id.clone(), on));
                            }
                            let selected = self.link_selected_id.as_deref()
                                == Some(rule.id.as_str())
                                && !self.link_creating;
                            let mut text =
                                egui::RichText::new(link_rule_list_label(ctx, rule));
                            if !rule.enabled || rule.pattern_error.is_some() {
                                text = text.color(theme.text_tertiary());
                            }
                            if ui.selectable_label(selected, text).clicked() {
                                pick = Some(rule.id.clone());
                            }
                        });
                        if rule.pattern_error.is_some() {
                            ui.label(
                                egui::RichText::new(crate::i18n::tr(
                                    ctx,
                                    "Invalid regular expression. This rule will not match.",
                                    "正则无效, 这条规则不会命中.",
                                ))
                                .size(theme.font_size_caption())
                                .color(theme.color_danger_emphasis()),
                            );
                        }
                    }
                });
            for (id, enabled) in toggles {
                self.confirm_reset = false;
                if let Err(err) = store.set_enabled(&id, enabled) {
                    self.set_error(explain_link_rule_error(ctx, &err));
                } else if let Some(err) = store.last_save_error() {
                    self.set_error(save_failed(ctx, &err));
                } else {
                    self.set_status(crate::i18n::tr(
                        ctx,
                        "Saved. Takes effect on the next redrawn screen.",
                        "已保存, 终端重绘即生效.",
                    ));
                    if self.link_selected_id.as_deref() == Some(id.as_str()) {
                        self.link_enabled = enabled;
                    }
                }
            }
            if let Some(id) = pick {
                if let Some(rule) = store.rules().into_iter().find(|rule| rule.id == id) {
                    self.load_link_rule(&rule);
                    self.confirm_reset = false;
                }
            }
        });
    }

    fn paint_link_editor(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        ctx: &egui::Context,
        store: &DisplayLinkStore,
        width: f32,
    ) {
        ui.vertical(|ui| {
            let width = width.max(280.0);
            ui.set_width(width);
            let id_line = if self.link_creating || self.link_selected_id.is_none() {
                crate::i18n::tr(
                    ctx,
                    "A new id is assigned when you save.",
                    "保存时会生成规则 ID.",
                )
                .to_string()
            } else {
                format!(
                    "{} {}",
                    crate::i18n::tr(ctx, "Rule id", "规则 ID"),
                    self.link_selected_id.as_deref().unwrap_or("")
                )
            };
            ui.label(
                egui::RichText::new(id_line)
                    .size(theme.font_size_caption())
                    .color(theme.text_tertiary()),
            );
            ui.add_space(theme.spacing_xs());
            let mut enabled = self.link_enabled;
            if ui
                .checkbox(&mut enabled, crate::i18n::tr(ctx, "Enabled", "启用"))
                .changed()
            {
                self.link_enabled = enabled;
                self.confirm_reset = false;
            }
            ui.add_space(theme.spacing_sm());
            crate::ui::chrome::form_field_label(
                ui,
                theme,
                crate::i18n::tr(ctx, "Pattern (regular expression)", "匹配模式 (正则)"),
            );
            crate::ui::chrome::form_multiline_field_with_hint(
                ui,
                theme,
                egui::Id::new("display_link_pattern"),
                &mut self.link_pattern,
                "https?://\\S+",
                width,
                4,
                false,
            );
            if let Some(message) = live_link_pattern_error(ctx, &self.link_pattern) {
                ui.label(
                    egui::RichText::new(message)
                        .size(theme.font_size_caption())
                        .color(theme.color_danger_emphasis()),
                );
            }
            ui.add_space(theme.spacing_sm());
            let combo_w = (width * 0.48).clamp(140.0, 240.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    crate::ui::chrome::form_field_label(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Category", "类别"),
                    );
                    let selected = link_category_label(ctx, &self.link_category);
                    crate::ui::chrome::form_combo(
                        ui,
                        theme,
                        "display_link_category",
                        selected,
                        combo_w,
                        |ui| {
                            for category in ["url", "errno", "ip", "path"] {
                                ui.selectable_value(
                                    &mut self.link_category,
                                    category.to_string(),
                                    link_category_label(ctx, category),
                                );
                            }
                        },
                    );
                });
                ui.vertical(|ui| {
                    crate::ui::chrome::form_field_label(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Click action", "点击动作"),
                    );
                    let selected = link_action_label(ctx, &self.link_action);
                    crate::ui::chrome::form_combo(
                        ui,
                        theme,
                        "display_link_action",
                        selected,
                        combo_w,
                        |ui| {
                            for action in ACTION_WHITELIST {
                                ui.selectable_value(
                                    &mut self.link_action,
                                    action.to_string(),
                                    link_action_label(ctx, action),
                                );
                            }
                        },
                    );
                });
            });
            ui.add_space(theme.spacing_sm());
            crate::ui::chrome::form_control_row(ui, theme, |ui| {
                crate::ui::chrome::form_inline_label(
                    ui,
                    theme,
                    crate::i18n::tr(
                        ctx,
                        "Priority (lower matches first)",
                        "优先级 (越小越先匹配)",
                    ),
                );
                crate::ui::chrome::form_u32_stepper(
                    ui,
                    theme,
                    egui::Id::new("display_link_priority"),
                    &mut self.link_priority,
                    0..=255,
                    1,
                );
            });
            ui.add_space(theme.spacing_sm());
            ui.horizontal(|ui| {
                if crate::ui::chrome::panel_action_primary_button(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "Save", "保存"),
                )
                .clicked()
                {
                    self.save_link_editor(ctx, store);
                }
                if self.link_selected_id.is_some() && !self.link_creating {
                    if crate::ui::chrome::panel_action_button(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Delete", "删除"),
                    )
                    .clicked()
                    {
                        self.delete_link_selected(ctx, store);
                    }
                }
            });
        });
    }

    fn paint_link_footer(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        ctx: &egui::Context,
        store: &DisplayLinkStore,
    ) {
        let reset_label = if self.confirm_reset {
            crate::i18n::tr(
                ctx,
                "Click again to restore built-in rules",
                "再点一次, 确认恢复内置规则",
            )
        } else {
            crate::i18n::tr(ctx, "Restore built-in rules", "恢复内置规则")
        };
        if crate::ui::chrome::panel_action_button(ui, theme, reset_label).clicked() {
            if self.confirm_reset {
                match store.reset_to_builtin() {
                    Ok(()) => {
                        self.link_creating = false;
                        self.link_selected_id = None;
                        self.ensure_link_selection(store);
                        self.confirm_reset = false;
                        self.set_status(crate::i18n::tr(
                            ctx,
                            "Built-in rules restored. Takes effect on the next redrawn screen.",
                            "已恢复内置规则, 终端重绘即生效.",
                        ));
                    }
                    Err(err) => self.set_error(save_failed(ctx, &err)),
                }
            } else {
                self.confirm_reset = true;
                self.status.clear();
                self.error.clear();
            }
        }
        ui.add_space(theme.spacing_sm());
        egui::CollapsingHeader::new(crate::i18n::tr(
            ctx,
            "How a link shows up",
            "怎样看到链接",
        ))
        .id_source("display_link_help")
        .default_open(false)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(crate::i18n::tr(
                    ctx,
                    "Built-in rules already highlight paths, error codes, IPs, and URLs in terminal output. Hover or right-click a hit to use the click action. Path hits inside system binary directories (/usr/bin etc.) are never linked. A line shows at most 16 links.",
                    "内置规则已经在终端输出里识别路径/错误码/IP/URL. 悬停或右键命中片段即可使用对应动作. 系统二进制目录 (/usr/bin 等) 下的路径不画链接. 单行最多 16 个链接.",
                ))
                .size(theme.font_size_caption())
                .color(theme.text_tertiary()),
            );
        });
        egui::CollapsingHeader::new(crate::i18n::tr(
            ctx,
            "Advanced: import / export JSON",
            "高级: 导入 / 导出 JSON",
        ))
        .id_source("display_link_import_export")
        .default_open(false)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(crate::i18n::tr(
                    ctx,
                    "Import replaces every current rule. Invalid patterns are kept but do not match. A broken file leaves the current rules untouched.",
                    "导入会替换当前全部规则. 无效正则会留下但不会命中. 文件损坏时不会改掉现有规则.",
                ))
                .size(theme.font_size_caption())
                .color(theme.text_tertiary()),
            );
            ui.add_space(theme.spacing_xs());
            ui.horizontal(|ui| {
                if crate::ui::chrome::panel_action_button(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "Export JSON…", "导出 JSON…"),
                )
                .clicked()
                {
                    self.export_link_json(ctx, store);
                }
                if crate::ui::chrome::panel_action_button(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "Import JSON…", "导入 JSON…"),
                )
                .clicked()
                {
                    self.import_link_json(ctx, store);
                }
            });
        });
    }

    fn save_link_editor(&mut self, ctx: &egui::Context, store: &DisplayLinkStore) {
        self.confirm_reset = false;
        if let Err(err) = check_link_pattern(&self.link_pattern) {
            self.set_error(explain_link_rule_error(ctx, &err));
            return;
        }
        let draft = LinkRuleDraft {
            id: if self.link_creating {
                None
            } else {
                self.link_selected_id.clone()
            },
            pattern: self.link_pattern.clone(),
            category: self.link_category.clone(),
            action: self.link_action.clone(),
            enabled: self.link_enabled,
            priority: self.link_priority.min(255) as u8,
        };
        match store.upsert(draft) {
            Ok(rule) => {
                if let Some(err) = store.last_save_error() {
                    self.load_link_rule(&rule);
                    self.set_error(save_failed(ctx, &err));
                    return;
                }
                self.load_link_rule(&rule);
                self.set_status(crate::i18n::tr(
                    ctx,
                    "Saved. Takes effect on the next redrawn screen.",
                    "已保存, 终端重绘即生效.",
                ));
            }
            Err(err) => self.set_error(explain_link_rule_error(ctx, &err)),
        }
    }

    fn delete_link_selected(&mut self, ctx: &egui::Context, store: &DisplayLinkStore) {
        self.confirm_reset = false;
        let Some(id) = self.link_selected_id.clone() else {
            return;
        };
        if !store.delete(&id) {
            self.set_error(explain_link_rule_error(ctx, &LinkRuleError::NotFound));
            return;
        }
        if let Some(err) = store.last_save_error() {
            self.set_error(save_failed(ctx, &err));
        } else {
            self.set_status(crate::i18n::tr(
                ctx,
                "Deleted. Takes effect on the next redrawn screen.",
                "已删除, 终端重绘即生效.",
            ));
        }
        self.link_creating = false;
        self.link_selected_id = None;
        self.ensure_link_selection(store);
    }

    fn export_link_json(&mut self, ctx: &egui::Context, store: &DisplayLinkStore) {
        self.confirm_reset = false;
        let Some(dest) = crate::ui::file_dialog::FileDialog::new()
            .set_title(crate::i18n::tr(
                ctx,
                "Export clickable link rules",
                "导出可点击链接规则",
            ))
            .add_filter("JSON", &["json"])
            .set_file_name("mistterm-display-link-rules.json")
            .save_file()
        else {
            return;
        };
        match std::fs::write(&dest, store.export_json()) {
            Ok(()) => self.set_status(match crate::i18n::language(ctx) {
                crate::i18n::UiLanguage::Zh => format!("已导出 {}", dest.display()),
                crate::i18n::UiLanguage::En => format!("Exported {}", dest.display()),
            }),
            Err(err) => self.set_error(format!(
                "{}{err}",
                crate::i18n::tr(ctx, "Export failed: ", "导出失败: ")
            )),
        }
    }

    fn import_link_json(&mut self, ctx: &egui::Context, store: &DisplayLinkStore) {
        self.confirm_reset = false;
        let Some(src) = crate::ui::file_dialog::FileDialog::new()
            .set_title(crate::i18n::tr(
                ctx,
                "Import clickable link rules",
                "导入可点击链接规则",
            ))
            .add_filter("JSON", &["json"])
            .pick_file()
        else {
            return;
        };
        let text = match std::fs::read_to_string(&src) {
            Ok(text) => text,
            Err(err) => {
                self.set_error(format!(
                    "{}{err}",
                    crate::i18n::tr(ctx, "Could not read file: ", "无法读取文件: ")
                ));
                return;
            }
        };
        match store.import_json(&text) {
            Ok(report) => {
                self.link_creating = false;
                self.link_selected_id = None;
                self.ensure_link_selection(store);
                if let Some(err) = store.last_save_error() {
                    self.set_error(save_failed(ctx, &err));
                } else {
                    self.set_status(link_import_status(ctx, &report));
                }
            }
            Err(err) => self.set_error(explain_link_rule_error(ctx, &err)),
        }
    }
}

fn rule_list_label(ctx: &egui::Context, rule: &OutputTriggerRule) -> String {
    let pattern = truncate_chars(&rule.pattern, 42);
    format!(
        "{:>3}  {}  {}",
        rule.priority,
        severity_label(ctx, rule.severity),
        pattern
    )
}

fn cmd_rule_list_label(ctx: &egui::Context, rule: &crate::core::cmd_audit::CmdAuditRule) -> String {
    let pattern = truncate_chars(&rule.pattern, 36);
    let name = if rule.name.trim().is_empty() {
        String::new()
    } else {
        format!("{} · ", truncate_chars(rule.name.trim(), 20))
    };
    format!(
        "{:>3}  {}{} {}",
        rule.priority,
        name,
        cmd_action_label(ctx, &rule.action),
        pattern
    )
}

fn cmd_match_type_label(ctx: &egui::Context, mt: &str) -> String {
    match mt {
        "prefix" => crate::i18n::tr(ctx, "Prefix", "前缀").to_string(),
        "contains" => crate::i18n::tr(ctx, "Contains", "包含").to_string(),
        "exact" => crate::i18n::tr(ctx, "Exact", "完全匹配").to_string(),
        _ => crate::i18n::tr(ctx, "Regex", "正则").to_string(),
    }
}

fn cmd_action_label(ctx: &egui::Context, action: &str) -> String {
    match action {
        "block" => crate::i18n::tr(ctx, "Block", "拦截").to_string(),
        "confirm" => crate::i18n::tr(ctx, "Confirm", "需确认").to_string(),
        "alert" => crate::i18n::tr(ctx, "Alert", "告警放行").to_string(),
        "allow" => crate::i18n::tr(ctx, "Allow", "直接放行").to_string(),
        other => other.to_string(),
    }
}

fn live_cmd_pattern_error(ctx: &egui::Context, pattern: &str, match_type: &str) -> Option<String> {
    if pattern.trim().is_empty() {
        return None;
    }
    let zh = crate::i18n::language(ctx) == crate::i18n::UiLanguage::Zh;
    match crate::core::cmd_audit_personal::check_pattern_for_ui(pattern, match_type) {
        Ok(()) => None,
        Err(err) => Some(err.explain(zh)),
    }
}

fn link_rule_list_label(ctx: &egui::Context, rule: &DisplayLinkRule) -> String {
    let pattern = truncate_chars(&rule.pattern, 42);
    format!(
        "{:>3}  {} {}  {}",
        rule.priority,
        link_category_label(ctx, &rule.category),
        link_action_label(ctx, &rule.action),
        pattern
    )
}

fn link_category_label(ctx: &egui::Context, category: &str) -> String {
    match category {
        "url" => crate::i18n::tr(ctx, "URL", "URL").to_string(),
        "errno" => crate::i18n::tr(ctx, "Error code", "错误码").to_string(),
        "ip" => crate::i18n::tr(ctx, "IP", "IP").to_string(),
        "path" => crate::i18n::tr(ctx, "Path", "路径").to_string(),
        other => other.to_string(),
    }
}

fn link_action_label(ctx: &egui::Context, action: &str) -> String {
    match action {
        "sftp-open" => {
            crate::i18n::tr(ctx, "Open in SFTP", "在 SFTP 打开").to_string()
        }
        "runbook-recall" => {
            crate::i18n::tr(ctx, "Recall runbook", "召回 Runbook").to_string()
        }
        "copy-or-connect" => {
            crate::i18n::tr(ctx, "Copy / connect", "复制 / 连接").to_string()
        }
        "browser-open" => {
            crate::i18n::tr(ctx, "Open in browser", "在浏览器打开").to_string()
        }
        other => other.to_string(),
    }
}

fn live_link_pattern_error(ctx: &egui::Context, pattern: &str) -> Option<String> {
    if pattern.trim().is_empty() {
        return None;
    }
    match check_link_pattern(pattern) {
        Ok(()) => None,
        Err(LinkRuleError::EmptyPattern) => None,
        Err(err) => Some(explain_link_rule_error(ctx, &err)),
    }
}

fn explain_link_rule_error(ctx: &egui::Context, err: &LinkRuleError) -> String {
    match err {
        LinkRuleError::EmptyPattern => crate::i18n::tr(
            ctx,
            "Enter a regular expression. Nothing was saved.",
            "请填写正则表达式. 没有保存.",
        )
        .to_string(),
        LinkRuleError::PatternTooLong => crate::i18n::tr(
            ctx,
            "That pattern is too long. Nothing was saved.",
            "这条正则太长. 没有保存.",
        )
        .to_string(),
        LinkRuleError::InvalidRegex(message) => format!(
            "{} {message}",
            crate::i18n::tr(
                ctx,
                "Invalid regular expression. Nothing was saved:",
                "正则表达式无效, 没有保存:",
            )
        ),
        LinkRuleError::NotFound => {
            crate::i18n::tr(ctx, "That rule no longer exists.", "这条规则已经不在了.").to_string()
        }
        LinkRuleError::TooMany => {
            crate::i18n::tr(ctx, "The rule list is full (64).", "规则已满 (最多 64 条).")
                .to_string()
        }
        LinkRuleError::EmptyId => {
            crate::i18n::tr(ctx, "Rule id is empty.", "规则 ID 为空.").to_string()
        }
        LinkRuleError::IdTooLong => {
            crate::i18n::tr(ctx, "Rule id is too long.", "规则 ID 太长.").to_string()
        }
        LinkRuleError::BadAction => crate::i18n::tr(
            ctx,
            "Unknown click action. Use sftp-open, runbook-recall, copy-or-connect, or browser-open.",
            "无法识别的点击动作. 请用 sftp-open, runbook-recall, copy-or-connect 或 browser-open.",
        )
        .to_string(),
        LinkRuleError::EmptyFile => crate::i18n::tr(
            ctx,
            "That file has no rules. Current rules were not changed.",
            "文件里没有规则. 当前规则没有改动.",
        )
        .to_string(),
        LinkRuleError::InvalidFile(message) => format!(
            "{} {message}",
            crate::i18n::tr(
                ctx,
                "Could not import that file. Current rules were not changed:",
                "无法导入这个文件, 当前规则没有改动:",
            )
        ),
    }
}

fn link_import_status(ctx: &egui::Context, report: &LinkImportReport) -> String {
    let mut parts = vec![match crate::i18n::language(ctx) {
        crate::i18n::UiLanguage::Zh => {
            format!("已导入 {} 条, 终端重绘即生效.", report.imported)
        }
        crate::i18n::UiLanguage::En => format!(
            "Imported {} rules. Takes effect on the next redrawn screen.",
            report.imported
        ),
    }];
    if !report.invalid_pattern_ids.is_empty() {
        parts.push(match crate::i18n::language(ctx) {
            crate::i18n::UiLanguage::Zh => format!(
                "这些规则正则无效, 已保留但不会命中: {}",
                report.invalid_pattern_ids.join(", ")
            ),
            crate::i18n::UiLanguage::En => format!(
                "These rules have an invalid pattern and will not match: {}",
                report.invalid_pattern_ids.join(", ")
            ),
        });
    }
    if !report.skipped.is_empty() {
        let shown: Vec<&str> = report.skipped.iter().take(3).map(String::as_str).collect();
        parts.push(match crate::i18n::language(ctx) {
            crate::i18n::UiLanguage::Zh => format!("已跳过: {}", shown.join("; ")),
            crate::i18n::UiLanguage::En => format!("Skipped: {}", shown.join("; ")),
        });
    }
    parts.join(" ")
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.replace('\n', " ");
    }
    let head: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{}…", head.replace('\n', " "))
}

fn severity_label(ctx: &egui::Context, severity: TriggerSeverity) -> &'static str {
    match severity {
        TriggerSeverity::Error => crate::i18n::tr(ctx, "Error", "错误"),
        TriggerSeverity::Warn => crate::i18n::tr(ctx, "Warning", "警告"),
        TriggerSeverity::Success => crate::i18n::tr(ctx, "Success", "成功"),
        TriggerSeverity::Info => crate::i18n::tr(ctx, "Info", "提示"),
    }
}

fn action_label(ctx: &egui::Context, action: TriggerAction) -> &'static str {
    match action {
        TriggerAction::Notify => crate::i18n::tr(ctx, "Notify (toast)", "通知 (Toast)"),
    }
}

fn live_pattern_error(ctx: &egui::Context, pattern: &str) -> Option<String> {
    if pattern.trim().is_empty() {
        return None;
    }
    match check_pattern(pattern) {
        Ok(()) => None,
        Err(RuleError::EmptyPattern) => None,
        Err(err) => Some(explain_rule_error(ctx, &err)),
    }
}

fn explain_rule_error(ctx: &egui::Context, err: &RuleError) -> String {
    match err {
        RuleError::EmptyPattern => crate::i18n::tr(
            ctx,
            "Enter a regular expression. Nothing was saved.",
            "请填写正则表达式. 没有保存.",
        )
        .to_string(),
        RuleError::PatternTooLong => crate::i18n::tr(
            ctx,
            "That pattern is too long. Nothing was saved.",
            "这条正则太长. 没有保存.",
        )
        .to_string(),
        RuleError::InvalidRegex(message) => format!(
            "{} {message}",
            crate::i18n::tr(
                ctx,
                "Invalid regular expression. Nothing was saved:",
                "正则表达式无效, 没有保存:",
            )
        ),
        RuleError::NotFound => {
            crate::i18n::tr(ctx, "That rule no longer exists.", "这条规则已经不在了.").to_string()
        }
        RuleError::TooMany => {
            crate::i18n::tr(ctx, "The rule list is full (64).", "规则已满 (最多 64 条).")
                .to_string()
        }
        RuleError::EmptyId => {
            crate::i18n::tr(ctx, "Rule id is empty.", "规则 ID 为空.").to_string()
        }
        RuleError::IdTooLong => {
            crate::i18n::tr(ctx, "Rule id is too long.", "规则 ID 太长.").to_string()
        }
        RuleError::BadSeverity => crate::i18n::tr(
            ctx,
            "Unknown severity. Use error, warn, success, or info.",
            "无法识别的严重级别. 请用 error, warn, success 或 info.",
        )
        .to_string(),
        RuleError::BadAction => crate::i18n::tr(
            ctx,
            "Unknown notify action. Use notify.",
            "无法识别的通知动作. 请用 notify.",
        )
        .to_string(),
        RuleError::EmptyFile => crate::i18n::tr(
            ctx,
            "That file has no rules. Current rules were not changed.",
            "文件里没有规则. 当前规则没有改动.",
        )
        .to_string(),
        RuleError::InvalidFile(message) => format!(
            "{} {message}",
            crate::i18n::tr(
                ctx,
                "Could not import that file. Current rules were not changed:",
                "无法导入这个文件, 当前规则没有改动:",
            )
        ),
    }
}

fn save_failed(ctx: &egui::Context, err: &str) -> String {
    format!(
        "{} {err}",
        crate::i18n::tr(
            ctx,
            "The change is active in this session, but it could not be written to the config file:",
            "这次运行里已经生效, 但没能写入配置文件:",
        )
    )
}

fn import_status(ctx: &egui::Context, report: &ImportReport) -> String {
    let mut parts = vec![match crate::i18n::language(ctx) {
        crate::i18n::UiLanguage::Zh => {
            format!("已导入 {} 条, 下一行输出立即生效.", report.imported)
        }
        crate::i18n::UiLanguage::En => format!(
            "Imported {} rules. Takes effect on the next output line.",
            report.imported
        ),
    }];
    if !report.invalid_pattern_ids.is_empty() {
        parts.push(match crate::i18n::language(ctx) {
            crate::i18n::UiLanguage::Zh => format!(
                "这些规则正则无效, 已保留但不会命中: {}",
                report.invalid_pattern_ids.join(", ")
            ),
            crate::i18n::UiLanguage::En => format!(
                "These rules have an invalid pattern and will not match: {}",
                report.invalid_pattern_ids.join(", ")
            ),
        });
    }
    if !report.skipped.is_empty() {
        let shown: Vec<&str> = report.skipped.iter().take(3).map(String::as_str).collect();
        parts.push(match crate::i18n::language(ctx) {
            crate::i18n::UiLanguage::Zh => format!("已跳过: {}", shown.join("; ")),
            crate::i18n::UiLanguage::En => format!("Skipped: {}", shown.join("; ")),
        });
    }
    parts.join(" ")
}
