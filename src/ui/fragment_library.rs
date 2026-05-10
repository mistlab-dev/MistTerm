//! 命令片段库窗口：新建 / 编辑 / 删除（设计文档 §2）

use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui;
use rfd::FileDialog;

use crate::core::{
    FragmentManager, FragmentMergeReport, FragmentStats, FragmentVariable, SortBy,
    expand_command_template, expand_rhai_blocks, list_placeholder_keys, merge_rhai_context,
};
use crate::core::session::SessionConfig;
use crate::ui::layout_util::{
    finite_avail_minus, finite_content_width_inset, fragment_library_window_bounds,
};

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
    /// 变量表单：(name, description, default_value)
    pub form_variables: Vec<(String, String, String)>,
    pub status_msg: String,
    /// 从 JSON 文件导入片段时是否与现有条目按 id 合并
    pub import_merge: bool,
    /// 点击「➕ 新建」后，在下一帧把焦点落在标题框，避免用户以为没有反应
    focus_title_next_frame: bool,
}

impl FragmentLibraryState {
    pub fn new() -> Self {
        Self {
            import_merge: true,
            ..Self::default()
        }
    }

    fn clear_form(&mut self) {
        self.editing_id = None;
        self.form_title.clear();
        self.form_command.clear();
        self.form_category.clear();
        self.form_tags.clear();
        self.form_variables.clear();
        self.focus_title_next_frame = false;
    }

    fn load_from_fragment(&mut self, f: &FragmentStats) {
        self.focus_title_next_frame = false;
        self.editing_id = Some(f.id.clone());
        self.form_title = f.title.clone();
        self.form_command = f.command.clone();
        self.form_category = f.category.clone();
        self.form_tags = f.tags.join(", ");
        self.form_variables = f.variables.iter().map(|v| {
            (v.name.clone(), v.description.clone(), v.default_value.clone().unwrap_or_default())
        }).collect();
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

        let mut win_open = self.open;
        let (lib_def, lib_min) = fragment_library_window_bounds(ctx);
        egui::Window::new("命令片段库")
            .id(egui::Id::new("mistterm_fragment_library_window"))
            .open(&mut win_open)
            // 与「新建会话」等弹窗一致：屏幕居中；勿同时使用 default_pos（egui 会冲突导致窗口偏一侧、右侧留空）
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .default_size(lib_def)
            .min_width(lib_min[0])
            .min_height(lib_min[1])
            .resizable(true)
            .show(ctx, |ui| {
                let sw = ctx.screen_rect().width().max(360.0);
                let fill_w = finite_content_width_inset(
                    ui,
                    16.0,
                    (sw * 0.52).clamp(420.0, 800.0),
                    (sw * 0.92).min(1200.0),
                );
                ui.set_min_width(fill_w);
                ui.horizontal(|ui| {
                    if ui.button("➕ 新建").clicked() {
                        self.clear_form();
                        self.focus_title_next_frame = true;
                        self.status_msg =
                            "新建片段：请在右侧填写标题、分类与命令，再点「保存」".to_string();
                    }
                    ui.separator();
                    ui.label(egui::RichText::new("搜索").color(theme.fg_medium_color()));
                    let search_w = finite_avail_minus(ui, 10.0, 140.0, 280.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.search_query)
                            .desired_width(search_w)
                            .hint_text("标题 / 命令 / 标签"),
                    );
                    ui.separator();
                    if ui
                        .button("导出 JSON…")
                        .on_hover_text("将当前片段库保存为用户选择的任意路径")
                        .clicked()
                    {
                        let stem = format!(
                            "mistterm-fragments-{}",
                            chrono::Utc::now().format("%Y%m%d-%H%M%S")
                        );
                        if let Some(dest) = FileDialog::new()
                            .set_title("导出片段 JSON")
                            .add_filter("JSON", &["json"])
                            .set_file_name(format!("{}.json", stem))
                            .save_file()
                        {
                            match manager.save(&dest) {
                                Ok(()) => {
                                    self.status_msg =
                                        format!("已导出 {}", dest.display());
                                    saved = true;
                                }
                                Err(e) => self.status_msg = format!("导出失败：{}", e),
                            }
                        }
                    }
                    if ui
                        .button("导入 JSON…")
                        .on_hover_text("从文件合并或替换当前库；合并时相同 id 的条目保留本地")
                        .clicked()
                    {
                        if let Some(src) =
                            FileDialog::new().add_filter("JSON", &["json"]).pick_file()
                        {
                            let src_label = src.display().to_string();
                            let path = PathBuf::from(&src);
                            match FragmentManager::import_from_json_path(
                                &path,
                                self.import_merge,
                                manager,
                            ) {
                                Ok(FragmentMergeReport {
                                    added,
                                    skipped_duplicate_id,
                                }) => {
                                    if manager.save(fragment_cfg_path).is_ok() {
                                        if self.import_merge {
                                            self.status_msg = format!(
                                                "已从 {} 合并：新增 {}，跳过 {}",
                                                src_label,
                                                added,
                                                skipped_duplicate_id
                                            );
                                        } else {
                                            self.status_msg = format!(
                                                "已从 {} 替换为 {} 条",
                                                src_label,
                                                added
                                            );
                                        }
                                        saved = true;
                                    } else {
                                        self.status_msg = "写入配置目录失败".to_string();
                                    }
                                }
                                Err(e) => {
                                    self.status_msg = format!("导入失败：{}", e);
                                }
                            }
                        }
                    }
                    ui.separator();
                    ui.checkbox(&mut self.import_merge, "合并导入");
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

                // 主内容区占满窗口剩余高度；左列固定宽度，右列用 row_w 显式占满，避免出现窗体右侧大块空白。
                let body_h = ui.available_height().max(180.0);
                let list_scroll_h = (body_h - 52.0).max(100.0);
                let row_w = finite_content_width_inset(ui, 16.0, 680.0, 1150.0);
                let left_col = (row_w * 0.355).clamp(200.0, 340.0);
                ui.horizontal(|ui| {
                    ui.set_min_width(row_w);
                    ui.set_min_height(body_h);
                    ui.vertical(|ui| {
                        ui.set_width(left_col);
                        ui.label(egui::RichText::new("列表").strong());
                        ui.separator();

                        let results: Vec<&FragmentStats> = if self.search_query.is_empty() {
                            manager.get_all().iter().collect()
                        } else {
                            manager.search(&self.search_query)
                        };

                        egui::ScrollArea::vertical()
                            .id_source("mistterm_fragment_lib_list_scroll")
                            .max_height(list_scroll_h)
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
                        ui.set_min_width((row_w - left_col - 12.0).max(200.0));
                        ui.label(egui::RichText::new("编辑").strong());
                        if self.editing_id.is_none() {
                            let c = theme.accent_dim_color();
                            let [r, g, b, _] = c.to_array();
                            egui::Frame::none()
                                .fill(egui::Color32::from_rgba_unmultiplied(r, g, b, 48))
                                .inner_margin(egui::Margin::same(8.0))
                                .rounding(4.0)
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(
                                            "新片段：在下面填写标题、分类与命令，完成后点「保存」。",
                                        )
                                        .size(13.0)
                                        .color(theme.fg_high_color()),
                                    );
                                });
                            ui.add_space(4.0);
                        }

                        // 表单在列内纵向滚动；高度用布局分配后的剩余空间（优于固定像素）。
                        let scroll_h = ui.available_height().max(120.0);
                        egui::ScrollArea::vertical()
                            .id_source("mistterm_fragment_lib_form_scroll")
                            .max_height(scroll_h)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let edit_w = finite_content_width_inset(ui, 12.0, 360.0, 840.0);

                                ui.label("标题");
                                let title_edit = egui::TextEdit::singleline(&mut self.form_title)
                                    .id(egui::Id::new("fragment_library_form_title"))
                                    .hint_text("例如：查看磁盘占用")
                                    .desired_width(edit_w);
                                let title_resp = ui.add(title_edit);
                                if self.focus_title_next_frame {
                                    title_resp.request_focus();
                                    self.focus_title_next_frame = false;
                                }
                                ui.label("分类");
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.form_category)
                                        .hint_text("例如：运维 · 主机")
                                        .desired_width(edit_w),
                                );
                                ui.label("标签（逗号分隔）");
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.form_tags)
                                        .hint_text("prod, nginx")
                                        .desired_width(edit_w),
                                );
                                ui.label(
                                    "命令（`<host>` 等会话占位符；`{{ md5(a) }}` 等见 Rhai 表达式，变量名须为合法标识符）",
                                );
                                ui.add(
                                    egui::TextEdit::multiline(&mut self.form_command)
                                        .hint_text("例如：df -h 或 systemctl status <service>")
                                        .desired_width(edit_w)
                                        .desired_rows(5),
                                );

                                ui.label(egui::RichText::new("变量定义").strong());
                                let mut var_to_remove = None;
                                for (idx, (name, desc, default)) in
                                    self.form_variables.iter_mut().enumerate()
                                {
                                    ui.push_id(idx, |ui| {
                                    ui.group(|ui| {
                                        ui.set_width(edit_w + 8.0);
                                        ui.label("名称");
                                        ui.add(
                                            egui::TextEdit::singleline(name).desired_width(edit_w),
                                        );
                                        ui.label("描述");
                                        ui.add(
                                            egui::TextEdit::singleline(desc).desired_width(edit_w),
                                        );
                                        ui.horizontal(|ui| {
                                            ui.label("默认");
                                            ui.add(
                                                egui::TextEdit::singleline(default)
                                                    .desired_width((edit_w - 56.0).max(72.0)),
                                            );
                                            if ui.button("🗑️").clicked() {
                                                var_to_remove = Some(idx);
                                            }
                                        });
                                    });
                                    });
                                }
                                if let Some(idx) = var_to_remove {
                                    self.form_variables.remove(idx);
                                }
                                if ui.button("➕ 添加变量").clicked() {
                                    self.form_variables.push((String::new(), String::new(), String::new()));
                                }

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

                                let mut preview_map = HashMap::new();
                                for (name, _, def) in &self.form_variables {
                                    let n = name.trim();
                                    if n.is_empty() || def.is_empty() {
                                        continue;
                                    }
                                    preview_map.insert(n.to_string(), def.clone());
                                }
                                let ctx = merge_rhai_context(session_hint, &preview_map);
                                let after_rhai = match expand_rhai_blocks(&self.form_command, &ctx) {
                                    Ok(s) => s,
                                    Err(_) => self.form_command.clone(),
                                };
                                let expanded = expand_command_template(
                                    &after_rhai,
                                    session_hint,
                                    &preview_map,
                                );
                                egui::CollapsingHeader::new("预览替换后（当前会话上下文）")
                                    .id_source("mistterm_fragment_lib_preview_expand")
                                    .show(ui, |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(expanded)
                                                .monospace()
                                                .color(theme.green_color()),
                                        )
                                        .wrap(true),
                                    );
                                });

                                ui.horizontal(|ui| {
                                    if ui.button("保存").clicked()
                                        && !self.form_title.trim().is_empty()
                                        && !self.form_category.trim().is_empty()
                                    {
                                        let tags = self.parse_tags();
                                        let variables: Vec<FragmentVariable> = self.form_variables
                                            .iter()
                                            .filter(|(name, _, _)| !name.trim().is_empty())
                                            .map(|(name, desc, default)| {
                                                FragmentVariable {
                                                    name: name.trim().to_string(),
                                                    description: desc.clone(),
                                                    default_value: if default.is_empty() {
                                                        None
                                                    } else {
                                                        Some(default.clone())
                                                    },
                                                }
                                            })
                                            .collect();

                                        if let Some(id) = &self.editing_id {
                                            let ok = manager.update_fragment_with_vars(
                                                id,
                                                self.form_title.trim().to_string(),
                                                self.form_command.clone(),
                                                self.form_category.trim().to_string(),
                                                tags,
                                                variables,
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
                                            manager.add_fragment_with_all(
                                                self.form_title.trim().to_string(),
                                                self.form_command.clone(),
                                                self.form_category.trim().to_string(),
                                                tags,
                                                variables,
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
                                    let accent_hint = self.status_msg.contains("新建片段：");
                                    ui.label(
                                        egui::RichText::new(&self.status_msg)
                                            .size(if accent_hint { 13.0 } else { 12.0 })
                                            .color(if accent_hint {
                                                theme.fg_medium_color()
                                            } else {
                                                theme.fg_low_color()
                                            }),
                                    );
                                }
                            });
                    });
                });
            });

        self.open = win_open;

        saved
    }
}
