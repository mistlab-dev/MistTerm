use super::Theme;
use eframe::egui::{self, Color32};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 曾用其它存储名保存的内置暗色主题（加载配置时迁移到「暗夜」）
const LEGACY_DARK_THEME_STORAGE_NAMES: &[&str] = &["Win11暗色", "现代暗色"];

fn is_legacy_dark_theme_storage_name(name: &str) -> bool {
    LEGACY_DARK_THEME_STORAGE_NAMES.contains(&name)
}

/// 主题存储名是否为已废弃的内置暗色项（偏好/迁移用）
pub fn is_deprecated_dark_theme_storage_name(name: &str) -> bool {
    is_legacy_dark_theme_storage_name(name)
}

/// 主题管理器
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeManager {
    /// 所有可用主题
    themes: Vec<Theme>,
    /// 当前选中的主题索引
    pub current: usize,
    /// 已写入 egui Context 的主题索引；避免每帧 `set_style` 打满 CPU。
    #[serde(skip)]
    applied_ctx_index: Option<usize>,
}

impl ThemeManager {
    /// 创建新的主题管理器（包含所有内置主题）
    pub fn new() -> Self {
        Self {
            themes: vec![
                Theme::dark(),
                Theme::light(),
                Theme::ocean(),
                Theme::forest(),
            ],
            current: 0, // 默认暗夜
            applied_ctx_index: None,
        }
    }

    fn builtin_themes() -> [Theme; 4] {
        [
            Theme::dark(),
            Theme::light(),
            Theme::ocean(),
            Theme::forest(),
        ]
    }

    /// 补全/刷新内置主题；移除已废弃的暗色存储项并统一到「暗夜」
    fn merge_builtin_themes(&mut self) {
        let current_name = self.themes.get(self.current).map(|t| t.name.clone());
        self.themes
            .retain(|t| !is_legacy_dark_theme_storage_name(&t.name));
        for builtin in Self::builtin_themes() {
            if let Some(existing) = self.themes.iter_mut().find(|t| t.name == builtin.name) {
                *existing = builtin;
            } else {
                self.themes.push(builtin);
            }
        }
        if current_name.as_deref() == Some("暗夜")
            || current_name
                .as_deref()
                .is_some_and(is_legacy_dark_theme_storage_name)
        {
            if let Some(i) = self.themes.iter().position(|t| t.name == "暗夜") {
                self.current = i;
            }
        } else if self.current >= self.themes.len() {
            self.current = 0;
        }
    }

    /// 从配置文件加载主题管理器
    pub fn load() -> Self {
        let config_path = Self::config_path();

        if let Ok(content) = std::fs::read_to_string(&config_path) {
            if let Ok(mut manager) = serde_json::from_str::<Self>(&content) {
                manager.merge_builtin_themes();
                return manager;
            }
            log::warn!("Failed to parse theme config; using default theme");
        }

        Self::new()
    }

    /// 保存主题配置到文件
    pub fn save(&self) {
        let config_path = Self::config_path();

        if let Some(parent) = config_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                log::error!("Failed to create theme config directory: {}", e);
                return;
            }
        }

        if let Ok(content) = serde_json::to_string_pretty(self) {
            if let Err(e) = std::fs::write(&config_path, content) {
                log::error!("Failed to save theme config: {}", e);
            } else {
                log::info!("Theme config saved to {}", config_path.display());
            }
        }
    }

    /// 获取配置文件路径
    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("mistterm")
            .join("theme.json")
    }

    /// 应用主题到 egui Context（同索引重复调用为 no-op）。
    pub fn apply_theme(&mut self, ctx: &egui::Context) {
        if self.applied_ctx_index == Some(self.current) {
            return;
        }
        let theme = self.current_theme();
        let mut style = (*ctx.style()).clone();

        // 根据主题背景亮度判断是否为深色模式
        let is_dark = theme.bg_body.r < 128;
        style.visuals = if is_dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };

        // 应用自定义颜色
        style.visuals.panel_fill = theme.surface_panel();
        style.visuals.faint_bg_color = theme.surface_elevated();
        // TextEdit / 裸输入框底色（勿用终端色，否则侧栏表单与面板融在一起）
        style.visuals.extreme_bg_color = theme.color_text_input_fill();
        style.visuals.window_fill = theme.surface_panel();
        style.visuals.window_stroke = theme.panel_stroke();
        style.visuals.widgets.noninteractive.weak_bg_fill = theme.surface_body();

        // 按钮 / ComboBox：暗夜浅灰实底；其它主题透明底 + 悬停 accent 弱底（裸 checkbox 请用 form_checkbox）
        style.visuals.widgets.noninteractive.bg_fill = theme.color_subtle_inset_fill();
        style.visuals.widgets.noninteractive.bg_stroke = theme.divider_stroke();
        if theme.uses_modern_palette() {
            let rounding = egui::Rounding::same(theme.radius_list_item());
            style.visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
            style.visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
            style.visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
            style.visuals.widgets.inactive.rounding = rounding;
            style.visuals.widgets.hovered.bg_fill = theme.color_widget_hover_fill();
            style.visuals.widgets.hovered.weak_bg_fill = theme.color_widget_hover_fill();
            style.visuals.widgets.active.bg_fill = theme.color_widget_active_fill();
            style.visuals.widgets.open.bg_fill = Color32::TRANSPARENT;
            style.visuals.widgets.open.weak_bg_fill = Color32::TRANSPARENT;
            style.visuals.widgets.open.bg_stroke = egui::Stroke::NONE;
            style.visuals.widgets.open.rounding = rounding;
        } else {
            style.visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
            style.visuals.widgets.inactive.weak_bg_fill = theme.color_subtle_inset_fill();
            style.visuals.widgets.inactive.bg_stroke =
                egui::Stroke::new(1.0, theme.color_checkbox_off_stroke_color());
            style.visuals.widgets.hovered.bg_fill = theme.color_widget_hover_fill();
            style.visuals.widgets.hovered.weak_bg_fill = theme.color_widget_hover_fill();
            style.visuals.widgets.active.bg_fill = theme.color_widget_active_fill();
        }

        // 文字颜色（语义 token；占位符仍建议 RichText + color_form_hint）
        style.visuals.override_text_color = Some(theme.text_primary());
        let widget_label = if is_dark {
            theme.text_primary()
        } else {
            theme.fg_medium_color()
        };
        let widget_label_secondary = if is_dark {
            theme.text_secondary()
        } else {
            theme.fg_medium_color()
        };
        style.visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, widget_label);
        style.visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, widget_label_secondary);
        style.visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, theme.text_primary());
        style.visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, theme.text_primary());

        // 文本拖选（勿用 accent 纯色底，避免与 accent 色 RichText 冲突）
        style.visuals.selection.bg_fill = theme.color_text_selection_bg();
        style.visuals.selection.stroke.color = theme.color_text_selection_fg();
        style.visuals.hyperlink_color = theme.accent_color();

        // 间距保持与设计一致
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);

        // §0.2：TextStyle 档位与语义字号一致
        style.text_styles.insert(
            egui::TextStyle::Name("xs9".into()),
            egui::FontId::proportional(theme.font_size_tag()),
        );
        style.text_styles.insert(
            egui::TextStyle::Name("sm10".into()),
            egui::FontId::proportional(theme.font_size_caption()),
        );
        style.text_styles.insert(
            egui::TextStyle::Name("md11".into()),
            egui::FontId::proportional(theme.font_size_ui_control()),
        );
        style.text_styles.insert(
            egui::TextStyle::Name("base12".into()),
            egui::FontId::proportional(theme.font_size_body()),
        );
        style.text_styles.insert(
            egui::TextStyle::Name("lg13".into()),
            egui::FontId::proportional(theme.font_size_terminal()),
        );
        style.text_styles.insert(
            egui::TextStyle::Name("xl15".into()),
            egui::FontId::proportional(theme.font_size_dock_title()),
        );
        style.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::proportional(theme.font_size_body()),
        );
        style.text_styles.insert(
            egui::TextStyle::Monospace,
            egui::FontId::monospace(theme.font_size_terminal()),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::proportional(theme.font_size_body()),
        );
        style.text_styles.insert(
            egui::TextStyle::Heading,
            egui::FontId::proportional(theme.font_size_section_title()),
        );
        style.text_styles.insert(
            egui::TextStyle::Small,
            egui::FontId::proportional(theme.font_size_caption()),
        );

        ctx.set_style(style);
        self.applied_ctx_index = Some(self.current);
        // 勿在此 request_repaint：每帧 apply 时再立即重绘会形成无限帧循环。
        // 切换主题时由偏好/菜单等调用方自行 request_repaint。
    }

    fn invalidate_applied_style(&mut self) {
        self.applied_ctx_index = None;
    }

    /// 获取当前主题
    pub fn current_theme(&self) -> &Theme {
        &self.themes[self.current]
    }

    /// 获取当前主题名称
    pub fn current_theme_name(&self) -> &str {
        &self.current_theme().name
    }

    /// 根据名称获取主题
    pub fn get_theme(&self, name: &str) -> Option<&Theme> {
        self.themes.iter().find(|t| t.name == name)
    }

    /// 获取所有主题列表
    pub fn list_themes(&self) -> &[Theme] {
        &self.themes
    }

    /// 切换到指定主题（按名称）
    pub fn set_theme(&mut self, name: &str) -> bool {
        for (i, theme) in self.themes.iter().enumerate() {
            if theme.name == name {
                if self.current != i {
                    self.current = i;
                    self.invalidate_applied_style();
                }
                return true;
            }
        }
        false
    }

    /// 切换到指定主题（按索引）
    pub fn set_theme_index(&mut self, index: usize) -> bool {
        if index < self.themes.len() {
            if self.current != index {
                self.current = index;
                self.invalidate_applied_style();
            }
            true
        } else {
            false
        }
    }

    /// 循环切换主题
    pub fn cycle_theme(&mut self) {
        self.current = (self.current + 1) % self.themes.len();
        self.invalidate_applied_style();
    }
}

impl Default for ThemeManager {
    fn default() -> Self {
        Self::new()
    }
}
