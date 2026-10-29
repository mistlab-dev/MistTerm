//! 规则管理模态（#185）。
//!
//! 主入口：工具 → 规则管理… →「输出触发」。
//! 次入口：偏好 → 扩展模块 → 本行「管理规则…」。
//! 同窗预留给 #186 / #187 的其它规则页；现在只有输出触发这一页。
//! 规则增删改不放进偏好折叠正文。

use eframe::egui;

use crate::modules::output_trigger::{
    check_pattern, ImportReport, OutputTriggerRule, OutputTriggerStore, RuleDraft, RuleError,
    TriggerAction, TriggerSeverity, MAX_RULES,
};
use crate::ui::theme::Theme;

/// 规则管理里的页。新增页时在这里加一项，不要另开 Dock。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuleManagerTab {
    OutputTrigger,
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

    /// 从偏好里打开时关掉偏好，避免两个模态叠在一起。
    pub fn open_from_preferences(&mut self, store: &OutputTriggerStore) {
        self.open_output_trigger(store);
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
        module_enabled: bool,
    ) {
        if !self.open {
            return;
        }
        self.ensure_selection(store);
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
                        crate::i18n::tr(ctx, "Rule manager", "规则管理"),
                        theme.font_size_modal_title(),
                    ) {
                        should_close = true;
                    }
                    ui.horizontal(|ui| {
                        let _ = ui.selectable_label(
                            self.tab == RuleManagerTab::OutputTrigger,
                            crate::i18n::tr(ctx, "Output Trigger", "输出触发"),
                        );
                    });
                    ui.separator();
                    self.paint_output_trigger(ui, theme, ctx, store, module_enabled);
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
