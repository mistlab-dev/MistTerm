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
    /// 窗口外背景色
    pub bg_body: Color32Serializable,
    /// 面板底色（侧边栏等）
    pub bg_window: Color32Serializable,
    /// 终端区域/激活 Tab 背景色
    pub bg_terminal: Color32Serializable,
    /// Tab 栏背景色
    pub bg_tab_bar: Color32Serializable,
    /// 悬停背景色
    pub bg_hover: Color32Serializable,
    /// 选中背景色
    pub bg_selected: Color32Serializable,
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
    /// 边框分隔线色
    pub border_divider: Color32Serializable,
    /// 成功色（在线状态等）
    pub green: Color32Serializable,
    /// 绿色暗色（在线状态 dim）
    pub green_dim: Color32Serializable,
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

    pub fn bg_hover_color(&self) -> Color32 {
        self.bg_hover.to_color32()
    }

    pub fn bg_selected_color(&self) -> Color32 {
        self.bg_selected.to_color32()
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

    pub fn border_divider_color(&self) -> Color32 {
        self.border_divider.to_color32()
    }

    pub fn green_color(&self) -> Color32 {
        self.green.to_color32()
    }

    pub fn green_dim_color(&self) -> Color32 {
        self.green_dim.to_color32()
    }

    pub fn red_color(&self) -> Color32 {
        self.red.to_color32()
    }

    pub fn amber_color(&self) -> Color32 {
        self.amber.to_color32()
    }

    // ── 字体大小（按设计规范 §0.2 映射） ──
    pub fn font_size_title_bar(&self) -> f32 { 13.0 }      // 标题栏
    pub fn font_size_title_bar_info(&self) -> f32 { 11.0 }  // 标题栏信息
    pub fn font_size_panel_title(&self) -> f32 { 10.0 }     // 面板标题（uppercase）
    pub fn font_size_connection_name(&self) -> f32 { 12.0 } // 连接条目名称
    pub fn font_size_connection_meta(&self) -> f32 { 10.0 } // 连接元信息
    pub fn font_size_terminal(&self) -> f32 { 13.0 }        // 终端输出
    pub fn font_size_fragment_title(&self) -> f32 { 12.0 }  // 片段标题
    pub fn font_size_fragment_cmd(&self) -> f32 { 10.0 }    // 片段命令原文
    pub fn font_size_fragment_stats(&self) -> f32 { 10.0 }  // 片段统计
    pub fn font_size_tab_label(&self) -> f32 { 11.0 }       // Tab 标签
    pub fn font_size_search_input(&self) -> f32 { 11.0 }    // 搜索框
    pub fn font_size_status_bar(&self) -> f32 { 11.0 }      // 状态栏
    pub fn font_size_status_bar_stats(&self) -> f32 { 10.0 } // 状态栏统计
    pub fn font_size_restore_btn(&self) -> f32 { 10.0 }     // 复原按钮
    pub fn font_size_tool_btn(&self) -> f32 { 12.0 }        // 工具按钮
    pub fn font_size_tag(&self) -> f32 { 9.0 }               // 标签（team/personal）
    pub fn font_size_category_label(&self) -> f32 { 10.0 }  // 分类标签

    // ── 间距系统（按设计规范 §8） ──
    pub fn spacing_panel_gap(&self) -> f32 { 6.0 }           // 面板间 gap
    pub fn spacing_panel_title_pad_x(&self) -> f32 { 10.0 }  // 面板标题左右 padding
    pub fn spacing_panel_title_pad_y(&self) -> f32 { 9.0 }   // 面板标题上下 padding
    pub fn spacing_panel_content_x(&self) -> f32 { 4.0 }     // 面板内容左右 padding
    pub fn spacing_panel_content_y(&self) -> f32 { 4.0 }     // 面板内容上下 padding
    pub fn spacing_search_area_x(&self) -> f32 { 8.0 }       // 搜索框区域左右 padding
    pub fn spacing_search_area_y(&self) -> f32 { 6.0 }       // 搜索框区域上下 padding
    pub fn spacing_search_input_x(&self) -> f32 { 8.0 }      // 搜索框输入左右 padding
    pub fn spacing_search_input_y(&self) -> f32 { 5.0 }      // 搜索框输入上下 padding
    pub fn spacing_list_item_x(&self) -> f32 { 10.0 }        // 列表条目左右 padding
    pub fn spacing_list_item_y(&self) -> f32 { 8.0 }         // 列表条目上下 padding
    pub fn spacing_list_item_gap(&self) -> f32 { 1.0 }       // 列表条目间距
    pub fn spacing_tab_x(&self) -> f32 { 14.0 }              // Tab 左右 padding
    pub fn spacing_tab_y(&self) -> f32 { 7.0 }               // Tab 上下 padding
    pub fn spacing_tab_dot_text(&self) -> f32 { 6.0 }        // Tab 圆点与文字间距
    pub fn spacing_terminal_pad_x(&self) -> f32 { 16.0 }     // 终端滚动区左右 padding
    pub fn spacing_terminal_pad_y(&self) -> f32 { 10.0 }     // 终端滚动区上下 padding
    pub fn spacing_card_x(&self) -> f32 { 8.0 }              // 卡片左右 padding
    pub fn spacing_card_y(&self) -> f32 { 7.0 }              // 卡片上下 padding
    pub fn spacing_status_bar_x(&self) -> f32 { 14.0 }       // 状态栏左右 padding
    pub fn spacing_status_bar_y(&self) -> f32 { 4.0 }        // 状态栏上下 padding
    pub fn spacing_status_left_gap(&self) -> f32 { 8.0 }     // 状态栏左侧 gap
    pub fn spacing_status_right_gap(&self) -> f32 { 4.0 }    // 状态栏右侧 gap
    pub fn spacing_tool_btn_gap(&self) -> f32 { 3.0 }        // 工具按钮间距
    pub fn spacing_title_bar_x(&self) -> f32 { 16.0 }        // 标题栏左右 padding
    pub fn spacing_title_bar_y(&self) -> f32 { 10.0 }        // 标题栏上下 padding
    pub fn spacing_body_pad(&self) -> f32 { 8.0 }            // 主区域 body padding

    // ── 圆角系统（按设计规范 §7） ──
    pub fn radius_window(&self) -> f32 { 10.0 }              // 窗口
    pub fn radius_panel(&self) -> f32 { 6.0 }                // 面板
    pub fn radius_list_item(&self) -> f32 { 4.0 }            // 连接条目
    pub fn radius_card(&self) -> f32 { 4.0 }                 // 片段卡片
    pub fn radius_search_input(&self) -> f32 { 4.0 }         // 搜索框
    pub fn radius_status_btn(&self) -> f32 { 3.0 }           // 状态栏按钮
    pub fn radius_tag(&self) -> f32 { 3.0 }                  // 标签（team/personal）
    pub fn radius_category(&self) -> f32 { 3.0 }             // 分类标签
    pub fn radius_restore_btn(&self) -> f32 { 3.0 }          // 复原按钮
    pub fn radius_traffic_light(&self) -> f32 { 50.0 }       // 红绿灯圆点（圆形）

    // ── 向后兼容的旧方法名（逐渐迁移到新命名） ──
    pub fn font_size_small(&self) -> f32 { self.font_size_connection_meta() }   // 辅助文字
    pub fn font_size_normal(&self) -> f32 { self.font_size_connection_name() }  // 常规文字
    pub fn font_size_medium(&self) -> f32 { self.font_size_terminal() }         // 标签/终端文字
    pub fn font_size_large(&self) -> f32 { self.font_size_title_bar() }        // 标题文字
    pub fn font_size_xl(&self) -> f32 { 15.0 }                                   // 大标题

    pub fn spacing_xs(&self) -> f32 { 2.0 }
    pub fn spacing_sm(&self) -> f32 { self.spacing_search_input_y() }
    pub fn spacing_md(&self) -> f32 { self.spacing_body_pad() }
    pub fn spacing_lg(&self) -> f32 { 16.0 }

    // ── 组件尺寸 ──
    pub fn progress_bar_height(&self) -> f32 { 8.0 }         // 进度条高度
    pub fn panel_title_height(&self) -> f32 { 28.0 }         // 面板标题栏高度
    pub fn status_bar_height(&self) -> f32 { 28.0 }          // 状态栏高度
    pub fn title_bar_height(&self) -> f32 { 36.0 }           // 标题栏高度

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

    /// 创建暗夜主题（Dark）- 按设计规范 §0.1 精确配色
    pub fn dark() -> Self {
        Self {
            name: "暗夜".to_string(),
            // === 背景色 ===
            bg_body: Color32Serializable::new(13, 13, 20),            // #0d0d14 — 窗口外背景
            bg_window: Color32Serializable::new(19, 19, 28),          // #13131c — 面板/窗口底色
            bg_terminal: Color32Serializable::new(10, 10, 18),        // #0a0a12 — 终端区域/激活 Tab
            bg_tab_bar: Color32Serializable::with_alpha(5, 5, 5, 5), // rgba(255,255,255,0.02) — Tab 栏背景
            bg_hover: Color32Serializable::with_alpha(8, 8, 8, 8),   // rgba(255,255,255,0.03) — 悬停背景
            bg_selected: Color32Serializable::with_alpha(5, 6, 12, 13), // rgba(102,126,234,0.05) — 选中背景
            // === 文字 ===
            fg_high: Color32Serializable::with_alpha(229, 229, 229, 230), // rgba(255,255,255,0.9)
            fg_medium: Color32Serializable::with_alpha(128, 128, 128, 128), // rgba(255,255,255,0.5)
            fg_low: Color32Serializable::with_alpha(64, 64, 64, 64),       // rgba(255,255,255,0.25)
            // === 主色调 ===
            accent: Color32Serializable::new(102, 126, 234),          // #667eea
            accent_dim: Color32Serializable::with_alpha(36, 44, 82, 89), // rgba(102,126,234,0.35)
            // === 边框 ===
            border: Color32Serializable::with_alpha(15, 15, 15, 15),       // rgba(255,255,255,0.06)
            border_divider: Color32Serializable::with_alpha(8, 8, 8, 8),   // rgba(255,255,255,0.03) — 分隔线
            // === 状态色 ===
            green: Color32Serializable::new(76, 175, 80),             // #4CAF50 — 成功/连接
            green_dim: Color32Serializable::with_alpha(19, 44, 20, 64), // rgba(76,175,80,0.25)
            red: Color32Serializable::new(244, 67, 54),               // #f44336
            amber: Color32Serializable::new(255, 200, 50),
        }
    }

    /// 创建晨曦主题（Light）- 浅色背景，柔和舒适
    pub fn light() -> Self {
        Self {
            name: "晨曦".to_string(),
            // === 背景色 ===
            bg_body: Color32Serializable::new(240, 240, 240),        // #f0f0f0
            bg_window: Color32Serializable::new(245, 245, 245),      // #f5f5f5
            bg_terminal: Color32Serializable::new(252, 252, 252),    // #fcfcfc
            bg_tab_bar: Color32Serializable::new(245, 245, 245),     // #f5f5f5
            bg_hover: Color32Serializable::with_alpha(230, 230, 230, 230), // rgba(0,0,0,0.03)
            bg_selected: Color32Serializable::with_alpha(235, 240, 255, 255), // rgba(102,126,234,0.05)
            // === 文字 ===
            fg_high: Color32Serializable::new(51, 51, 51),           // #333
            fg_medium: Color32Serializable::new(102, 102, 102),      // #666
            fg_low: Color32Serializable::new(153, 153, 153),         // #999
            // === 主色调 ===
            accent: Color32Serializable::new(102, 126, 234),         // #667eea
            accent_dim: Color32Serializable::new(224, 240, 254),     // #e8f0fe
            // === 边框 ===
            border: Color32Serializable::new(224, 224, 224),         // #e0e0e0
            border_divider: Color32Serializable::new(235, 235, 235),   // #ebebeb
            // === 状态色 ===
            green: Color32Serializable::new(76, 175, 80),            // #4CAF50
            green_dim: Color32Serializable::with_alpha(76, 175, 80, 64),
            red: Color32Serializable::new(244, 67, 54),              // #f44336
            amber: Color32Serializable::new(245, 124, 0),
        }
    }

    /// 创建海洋主题（Ocean）- 蓝调背景，专业冷静
    pub fn ocean() -> Self {
        Self {
            name: "海洋".to_string(),
            // === 背景色 ===
            bg_body: Color32Serializable::new(35, 55, 75),           // #23374b
            bg_window: Color32Serializable::new(28, 45, 62),         // #1c2d3e
            bg_terminal: Color32Serializable::new(22, 36, 50),       // #162432
            bg_tab_bar: Color32Serializable::new(35, 55, 75),        // #23374b
            bg_hover: Color32Serializable::with_alpha(255, 255, 255, 8), // rgba(255,255,255,0.03)
            bg_selected: Color32Serializable::with_alpha(102, 126, 234, 13), // rgba(102,126,234,0.05)
            // === 文字 ===
            fg_high: Color32Serializable::new(230, 240, 250),        // #e6f0fa
            fg_medium: Color32Serializable::new(180, 200, 220),      // #b4c8dc
            fg_low: Color32Serializable::new(140, 160, 180),         // #8ca0b4
            // === 主色调 ===
            accent: Color32Serializable::new(70, 130, 180),          // steel blue
            accent_dim: Color32Serializable::new(50, 90, 130),       // dim steel blue
            // === 边框 ===
            border: Color32Serializable::new(60, 90, 120),           // #3c5a78
            border_divider: Color32Serializable::with_alpha(255, 255, 255, 8), // rgba(255,255,255,0.03)
            // === 状态色 ===
            green: Color32Serializable::new(80, 200, 120),           // teal green
            green_dim: Color32Serializable::with_alpha(80, 200, 120, 64),
            red: Color32Serializable::new(220, 80, 80),              // coral red
            amber: Color32Serializable::new(255, 200, 100),
        }
    }

    /// 创建森林主题（Forest）- 绿色调背景，自然清新
    pub fn forest() -> Self {
        Self {
            name: "森林".to_string(),
            // === 背景色 ===
            bg_body: Color32Serializable::new(40, 60, 50),           // #283c32
            bg_window: Color32Serializable::new(32, 50, 42),         // #20322a
            bg_terminal: Color32Serializable::new(26, 42, 35),       // #1a2a23
            bg_tab_bar: Color32Serializable::new(40, 60, 50),        // #283c32
            bg_hover: Color32Serializable::with_alpha(255, 255, 255, 8), // rgba(255,255,255,0.03)
            bg_selected: Color32Serializable::with_alpha(102, 126, 234, 13), // rgba(102,126,234,0.05)
            // === 文字 ===
            fg_high: Color32Serializable::new(230, 245, 235),        // #e6f5eb
            fg_medium: Color32Serializable::new(180, 210, 190),      // #b4d2be
            fg_low: Color32Serializable::new(140, 170, 150),         // #8caa96
            // === 主色调 ===
            accent: Color32Serializable::new(90, 170, 100),          // forest green
            accent_dim: Color32Serializable::new(70, 130, 80),       // dim forest green
            // === 边框 ===
            border: Color32Serializable::new(60, 90, 70),            // #3c5a46
            border_divider: Color32Serializable::with_alpha(255, 255, 255, 8), // rgba(255,255,255,0.03)
            // === 状态色 ===
            green: Color32Serializable::new(100, 200, 120),          // bright forest green
            green_dim: Color32Serializable::with_alpha(100, 200, 120, 64),
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
    pub fn config_path() -> PathBuf {
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