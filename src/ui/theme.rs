//! 主题系统
//!
//! 提供多主题切换和自定义功能，包括暗夜、晨曦、海洋、森林四种内置主题。

use eframe::egui::{self, Color32};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 主题颜色配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    /// 主题名称（用于显示和配置保存）
    pub name: String,
    /// 窗口背景色（主面板）
    pub bg_body: Color32Serializable,
    /// 面板底色（侧边栏等）
    pub bg_window: Color32Serializable,
    /// 终端区域背景色
    pub bg_terminal: Color32Serializable,
    /// 标签栏背景色
    pub bg_tab_bar: Color32Serializable,
    /// 高亮文字颜色
    pub fg_high: Color32Serializable,
    /// 普通文字颜色
    pub fg_medium: Color32Serializable,
    /// 暗淡文字颜色
    pub fg_low: Color32Serializable,
    /// 主色调（按钮、状态栏等）
    pub accent: Color32Serializable,
    /// 主色调暗（悬停状态等）
    pub accent_dim: Color32Serializable,
    /// 边框颜色
    pub border: Color32Serializable,
    /// 成功色（在线状态等）
    pub green: Color32Serializable,
    /// 错误色（离线状态等）
    pub red: Color32Serializable,
    /// 警告/琥珀色（中等负载、中间档位进度等）
    #[serde(default = "default_theme_amber")]
    pub amber: Color32Serializable,
}

fn default_theme_amber() -> Color32Serializable {
    Color32Serializable::new(255, 200, 50)
}

/// 用于 serde 序列化的 Color32 包装
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Color32Serializable {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color32Serializable {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub fn with_alpha(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub fn to_color32(&self) -> Color32 {
        Color32::from_rgba_unmultiplied(self.r, self.g, self.b, self.a)
    }

    pub fn from_color32(color: Color32) -> Self {
        let [r, g, b, a] = color.to_array();
        Self { r, g, b, a }
    }
}

impl Theme {
    /// 转换为 egui::Color32
    pub fn bg_body_color(&self) -> Color32 {
        self.bg_body.to_color32()
    }

    pub fn bg_window_color(&self) -> Color32 {
        self.bg_window.to_color32()
    }

    pub fn bg_terminal_color(&self) -> Color32 {
        self.bg_terminal.to_color32()
    }

    pub fn bg_tab_bar_color(&self) -> Color32 {
        self.bg_tab_bar.to_color32()
    }

    pub fn fg_high_color(&self) -> Color32 {
        self.fg_high.to_color32()
    }

    pub fn fg_medium_color(&self) -> Color32 {
        self.fg_medium.to_color32()
    }

    pub fn fg_low_color(&self) -> Color32 {
        self.fg_low.to_color32()
    }

    pub fn accent_color(&self) -> Color32 {
        self.accent.to_color32()
    }

    pub fn accent_dim_color(&self) -> Color32 {
        self.accent_dim.to_color32()
    }

    pub fn border_color(&self) -> Color32 {
        self.border.to_color32()
    }

    pub fn green_color(&self) -> Color32 {
        self.green.to_color32()
    }

    pub fn red_color(&self) -> Color32 {
        self.red.to_color32()
    }

    pub fn amber_color(&self) -> Color32 {
        self.amber.to_color32()
    }

    /// 图表网格线等极淡分隔（随前景色变化，适配明暗主题）
    pub fn subtle_line_color(&self) -> Color32 {
        let c = self.fg_high.to_color32();
        let [r, g, b, _] = c.to_array();
        Color32::from_rgba_unmultiplied(r, g, b, 20)
    }

    /// 图表坐标刻度等次要标注
    pub fn subtle_label_color(&self) -> Color32 {
        let c = self.fg_high.to_color32();
        let [r, g, b, _] = c.to_array();
        Color32::from_rgba_unmultiplied(r, g, b, 60)
    }

    fn mix_rgb(a: Color32, b: Color32, t: f32) -> Color32 {
        let [ar, ag, ab, _] = a.to_array();
        let [br, bg, bb, _] = b.to_array();
        let lerp =
            |x: u8, y: u8| -> u8 { (x as f32 * (1.0 - t) + y as f32 * t).round() as u8 };
        Color32::from_rgb(lerp(ar, br), lerp(ag, bg), lerp(ab, bb))
    }

    /// 侧栏/会话列表等：行悬停底色（介于面板底色与边框色之间）
    pub fn list_row_hover_bg(&self) -> Color32 {
        Self::mix_rgb(self.bg_window_color(), self.border_color(), 0.42)
    }

    /// 侧栏/会话列表等：行选中底色
    pub fn list_row_selected_bg(&self) -> Color32 {
        Self::mix_rgb(self.bg_window_color(), self.border_color(), 0.62)
    }

    /// 创建暗夜主题（Dark）- 深色背景高对比度
    pub fn dark() -> Self {
        Self {
            name: "暗夜".to_string(),
            bg_body: Color32Serializable::new(45, 45, 45),           // #2d2d2d
            bg_window: Color32Serializable::new(37, 37, 38),         // #252526
            bg_terminal: Color32Serializable::new(30, 30, 30),       // #1e1e1e
            bg_tab_bar: Color32Serializable::new(45, 45, 45),        // #2d2d2d
            fg_high: Color32Serializable::new(255, 255, 255),        // #fff
            fg_medium: Color32Serializable::new(212, 212, 212),      // #d4d4d4
            fg_low: Color32Serializable::new(153, 153, 153),         // #999
            accent: Color32Serializable::new(102, 126, 234),         // #667eea
            accent_dim: Color32Serializable::new(76, 76, 76),        // #4c4c4c
            border: Color32Serializable::new(60, 60, 60),            // #3c3c3c
            green: Color32Serializable::new(76, 175, 80),            // #4CAF50
            red: Color32Serializable::new(244, 67, 54),              // #f44336
            amber: Color32Serializable::new(255, 200, 50),
        }
    }

    /// 创建晨曦主题（Light）- 浅色背景，柔和舒适
    pub fn light() -> Self {
        Self {
            name: "晨曦".to_string(),
            bg_body: Color32Serializable::new(240, 240, 240),        // #f0f0f0
            bg_window: Color32Serializable::new(245, 245, 245),      // #f5f5f5
            bg_terminal: Color32Serializable::new(252, 252, 252),    // #fcfcfc
            bg_tab_bar: Color32Serializable::new(245, 245, 245),     // #f5f5f5
            fg_high: Color32Serializable::new(51, 51, 51),           // #333
            fg_medium: Color32Serializable::new(102, 102, 102),      // #666
            fg_low: Color32Serializable::new(153, 153, 153),         // #999
            accent: Color32Serializable::new(102, 126, 234),         // #667eea
            accent_dim: Color32Serializable::new(224, 240, 254),     // #e8f0fe
            border: Color32Serializable::new(224, 224, 224),         // #e0e0e0
            green: Color32Serializable::new(76, 175, 80),            // #4CAF50
            red: Color32Serializable::new(244, 67, 54),              // #f44336
            amber: Color32Serializable::new(245, 124, 0),
        }
    }

    /// 创建海洋主题（Ocean）- 蓝调背景，专业冷静
    pub fn ocean() -> Self {
        Self {
            name: "海洋".to_string(),
            bg_body: Color32Serializable::new(35, 55, 75),           // #23374b
            bg_window: Color32Serializable::new(28, 45, 62),         // #1c2d3e
            bg_terminal: Color32Serializable::new(22, 36, 50),       // #162432
            bg_tab_bar: Color32Serializable::new(35, 55, 75),        // #23374b
            fg_high: Color32Serializable::new(230, 240, 250),        // #e6f0fa
            fg_medium: Color32Serializable::new(180, 200, 220),      // #b4c8dc
            fg_low: Color32Serializable::new(140, 160, 180),         // #8ca0b4
            accent: Color32Serializable::new(70, 130, 180),          // steel blue
            accent_dim: Color32Serializable::new(50, 90, 130),       // dim steel blue
            border: Color32Serializable::new(60, 90, 120),           // #3c5a78
            green: Color32Serializable::new(80, 200, 120),           // teal green
            red: Color32Serializable::new(220, 80, 80),              // coral red
            amber: Color32Serializable::new(255, 200, 100),
        }
    }

    /// 创建森林主题（Forest）- 绿色调背景，自然清新
    pub fn forest() -> Self {
        Self {
            name: "森林".to_string(),
            bg_body: Color32Serializable::new(40, 60, 50),           // #283c32
            bg_window: Color32Serializable::new(32, 50, 42),         // #20322a
            bg_terminal: Color32Serializable::new(26, 42, 35),       // #1a2a23
            bg_tab_bar: Color32Serializable::new(40, 60, 50),        // #283c32
            fg_high: Color32Serializable::new(230, 245, 235),        // #e6f5eb
            fg_medium: Color32Serializable::new(180, 210, 190),      // #b4d2be
            fg_low: Color32Serializable::new(140, 170, 150),         // #8caa96
            accent: Color32Serializable::new(90, 170, 100),          // forest green
            accent_dim: Color32Serializable::new(70, 130, 80),       // dim forest green
            border: Color32Serializable::new(60, 90, 70),            // #3c5a46
            green: Color32Serializable::new(100, 200, 120),          // bright forest green
            red: Color32Serializable::new(200, 90, 90),              // muted red
            amber: Color32Serializable::new(220, 180, 60),
        }
    }
}

/// 主题管理器
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeManager {
    /// 所有可用主题
    themes: Vec<Theme>,
    /// 当前选中的主题索引
    pub current: usize,
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
            current: 0, // 默认暗夜主题
        }
    }

    /// 从配置文件加载主题管理器
    pub fn load() -> Self {
        let config_path = Self::config_path();
        
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            if let Ok(manager) = serde_json::from_str(&content) {
                return manager;
            }
            log::warn!("主题配置文件解析失败，使用默认主题");
        }
        
        Self::new()
    }

    /// 保存主题配置到文件
    pub fn save(&self) {
        let config_path = Self::config_path();
        
        if let Some(parent) = config_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                log::error!("创建主题配置目录失败: {}", e);
                return;
            }
        }
        
        if let Ok(content) = serde_json::to_string_pretty(self) {
            if let Err(e) = std::fs::write(&config_path, content) {
                log::error!("保存主题配置失败: {}", e);
            } else {
                log::info!("主题配置已保存到 {}", config_path.display());
            }
        }
    }

    /// 获取配置文件路径
    fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("mistterm")
            .join("theme.json")
    }

    /// 应用主题到 egui Context
    pub fn apply_theme(&self, ctx: &egui::Context) {
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
        style.visuals.panel_fill = theme.bg_window_color();
        style.visuals.faint_bg_color = theme.bg_tab_bar_color();
        style.visuals.extreme_bg_color = theme.bg_terminal_color();
        style.visuals.window_fill = theme.bg_window_color();
        
        // 按钮样式
        style.visuals.widgets.noninteractive.bg_fill = theme.border_color();
        style.visuals.widgets.inactive.bg_fill = theme.border_color();
        style.visuals.widgets.hovered.bg_fill = theme.accent_dim_color();
        style.visuals.widgets.active.bg_fill = theme.accent_color();
        
        // 文字颜色
        style.visuals.widgets.noninteractive.fg_stroke.color = theme.fg_low_color();
        style.visuals.widgets.inactive.fg_stroke.color = theme.fg_medium_color();
        style.visuals.widgets.hovered.fg_stroke.color = theme.fg_high_color();
        style.visuals.widgets.active.fg_stroke.color = theme.fg_high_color();
        
        // 选中状态
        style.visuals.selection.bg_fill = theme.accent_color();
        style.visuals.selection.stroke.color = theme.fg_high_color();
        
        // 间距保持与设计一致
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        
        ctx.set_style(style);
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
                self.current = i;
                return true;
            }
        }
        false
    }

    /// 切换到指定主题（按索引）
    pub fn set_theme_index(&mut self, index: usize) -> bool {
        if index < self.themes.len() {
            self.current = index;
            true
        } else {
            false
        }
    }

    /// 循环切换主题
    pub fn cycle_theme(&mut self) {
        self.current = (self.current + 1) % self.themes.len();
    }
}

impl Default for ThemeManager {
    fn default() -> Self {
        Self::new()
    }
}