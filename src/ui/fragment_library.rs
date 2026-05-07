//! 命令片段库窗口：新建 / 编辑 / 删除（设计文档 §2）

use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui;

use crate::core::{
    FragmentManager, FragmentStats,
    SortBy,
    expand_command_template, list_placeholder_keys,
};
use crate::core::session::SessionConfig;

/// 片段库编辑器状态（不与 `FragmentStats` 强绑定便于表单编辑）
#[derive(Clone, Debug, Default)]
pub struct FragmentLibraryState {
    pub open: bool,
    pub search_query: String,
    pub editing_id: Option<String>,
    pub form_title: String,
    pub form_command: String,
    pub form_category: String,
    pub form_tags: String,
    pub status_msg: String,
}

impl FragmentLibraryState {
    pub fn new() -> Self {
        Self::default()
    }

    fn clear_form(&mut self) {
        self.editing_id = None;
        self.form_title.clear();
        self.form_command.clear();
        self.form_category.clear();
        self.form_tags.clear();
    }

    fn load_from_fragment(&mut self, f: &FragmentStats) {
        self.editing_id = Some(f.id.clone());
        self.form_title = f.title.clone();
        self.form_command = f.command.clone();
        self.form_category = f.category.clone();
        self.form_tags = f.tags.join(", ");
    }

    fn parse_tags(&self) -> Vec<String> {
        self.form_tags
            .split(&[',', '，', ';', '；'][..])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// 显示窗口；`session_hint` 用于变量预览。返回是否写入了 `fragments.json`。
    pub fn show_window(
        &mut self,
        ctx: &egui::Context,
        manager: &mut FragmentManager,
        sort_by: &mut SortBy,
        fragment_cfg_path: &PathBuf,
        session_hint: Option<&SessionConfig>,
        theme: &crate::ui::theme::Theme,
    ) -> bool {
        let mut saved = false;
        if !self.open {
            return false;
        }

        let preview_extras = HashMap::<String, String>::new();

        egui::Window::new("命令片段库")
            .open(&mut self.open)
            .default_size([620.0, 480.0])
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("➕ 新建").clicked() {
                        self.clear_form();
                        self.status_msg = "新建片段".to_string();
                    }
                    ui.separator();
                    ui.label(egui::RichText::new("搜索").color(theme.fg_medium_color()));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.search_query)
                            .desired_width(180.0)
                            .hint_text("标题 / 命令 / 标签"),
                    );
                    ui.separator();
                    let sort_label = match sort_by {
                        SortBy::UsageCount => "排序：次数",
                        SortBy::SuccessRate => "排序：成功率",
                        SortBy::LastUsed => "排序：最近",
                        SortBy::Name => "排序：名称",
                    };
                    if ui.button(sort_label).clicked() {
                        *sort_by = match sort_by {
                            SortBy::UsageCount => SortBy::SuccessRate,
                            SortBy::SuccessRate => SortBy::LastUsed,
                            SortBy::LastUsed => SortBy::Name,
                            SortBy::Name => SortBy::UsageCount,
                        };
                        manager.sort(*sort_by);
                    }
                });
                ui.separator();

                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_min_width(240.0);
                        ui.label(egui::RichText::new("列表").strong());
                        ui.separator();

                        let results: Vec<&FragmentStats> = if self.search_query.is_empty() {
                            manager.get_all().iter().collect()
                        } else {
                            manager.search(&self.search_query)
                        };

                        egui::ScrollArea::vertical()
                            .max_height(320.0)
                            .show(ui, |ui| {
                                for f in results {
                                    let selected = self.editing_id.as_deref() == Some(f.id.as_str());
                                    let label = format!("{} · {}", f.title, f.category);
                                    if ui
                                        .selectable_label(selected, label)
                                        .on_hover_text(&f.command)
                                        .clicked()
                                    {
                                        self.load_from_fragment(f);
                                    }
                                }
                            });
                    });

                    ui.separator();

                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("编辑").strong());
                        ui.label("标题");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.form_title)
                                .desired_width(f32::INFINITY),
                        );
                        ui.label("分类");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.form_category)
                                .desired_width(f32::INFINITY),
                        );
                        ui.label("标签（逗号分隔）");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.form_tags)
                                .hint_text("prod, nginx")
                                .desired_width(f32::INFINITY),
                        );
                        ui.label("命令（支持 `<host>` `<user>` `<port>` `<name>` `<service>` 等）");
                        ui.add(
                            egui::TextEdit::multiline(&mut self.form_command)
                                .desired_width(f32::INFINITY)
                                .desired_rows(4),
                        );

                        let keys = list_placeholder_keys(&self.form_command);
                        if !keys.is_empty() {
                            ui.label(
                                egui::RichText::new(format!(
                                    "占位符：{}",
                                    keys.join(", ")
                                ))
                                .small()
                                .color(theme.fg_low_color()),
                            );
                        }

                        let expanded = expand_command_template(
                            &self.form_command,
                            session_hint,
                            &preview_extras,
                        );
                        ui.collapsing("预览替换后（当前会话上下文）", |ui| {
                            ui.label(
                                egui::RichText::new(expanded.clone())
                                    .monospace()
                                    .color(theme.green_color()),
                            );
                        });

                        ui.horizontal(|ui| {
                            if ui.button("保存").clicked()
                                && !self.form_title.trim().is_empty()
                                && !self.form_category.trim().is_empty()
                            {
                                let tags = self.parse_tags();
                                if let Some(id) = &self.editing_id {
                                    let ok = manager.update_fragment(
                                        id,
                                        self.form_title.trim().to_string(),
                                        self.form_command.clone(),
                                        self.form_category.trim().to_string(),
                                        tags,
                                    );
                                    if ok {
                                        if manager.save(fragment_cfg_path).is_ok() {
                                            self.status_msg = "已保存".to_string();
                                            saved = true;
                                        } else {
                                            self.status_msg = "写入文件失败".to_string();
                                        }
                                    }
                                } else {
                                    manager.add_fragment_with_tags(
                                        self.form_title.trim().to_string(),
                                        self.form_command.clone(),
                                        self.form_category.trim().to_string(),
                                        tags,
                                    );
                                    if manager.save(fragment_cfg_path).is_ok() {
                                        self.status_msg = "已添加片段".to_string();
                                        saved = true;
                                    }
                                }
                            }
                            if ui.button("删除").clicked() {
                                if let Some(id) = self.editing_id.clone() {
                                    if manager.remove_fragment(&id)
                                        && manager.save(fragment_cfg_path).is_ok()
                                    {
                                        self.clear_form();
                                        self.status_msg = "已删除".to_string();
                                        saved = true;
                                    }
                                }
                            }
                        });
                        if !self.status_msg.is_empty() {
                            ui.label(
                                egui::RichText::new(&self.status_msg)
                                    .small()
                                    .color(theme.fg_low_color()),
                            );
                        }
                    });
                });
            });

        saved
    }
}
