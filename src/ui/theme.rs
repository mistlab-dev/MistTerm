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

    /// 顶栏 / 底栏 Panel 底色（与 Tab 条一致，必须不透明）
    pub fn chrome_bar_fill(&self) -> Color32 {
        self.bg_tab_bar_color()
    }

    pub fn frame_chrome_bar(&self) -> egui::Frame {
        egui::Frame::none()
            .fill(self.chrome_bar_fill())
            .inner_margin(self.margin_chrome_bar())
    }

    /// 中央工作区 Panel：须不透明，勿用 TRANSPARENT（浅色主题会露出窗口黑底）
    pub fn frame_central_workspace(&self) -> egui::Frame {
        egui::Frame::none()
            .fill(self.bg_body_color())
            .inner_margin(egui::Margin::ZERO)
            .outer_margin(egui::Margin::ZERO)
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

    /// FUNCTIONAL_SPEC §2.3.2：提示行命令段相对默认前景亮度（设计稿 `.cmd` ≈ 0.9）。
    pub fn terminal_command_dim_factor(&self) -> f32 {
        crate::terminal::style::TERMINAL_COMMAND_DIM_FACTOR
    }

    /// FUNCTIONAL_SPEC §2.3.2：非提示输出行相对默认前景亮度（设计稿 `.out` ≈ 0.4）。
    pub fn terminal_output_dim_factor(&self) -> f32 {
        crate::terminal::style::TERMINAL_OUTPUT_DIM_FACTOR
    }

    /// FUNCTIONAL_SPEC §2.3.4：终端纵向滚动条宽度（px）。
    pub fn terminal_scroll_bar_width(&self) -> f32 {
        crate::terminal::style::TERMINAL_SCROLL_BAR_WIDTH
    }

    /// 终端滚动条轨道底色（设计稿 `rgba(255,255,255,0.06)`，随主题前景派生）。
    pub fn terminal_scroll_bar_track_fill(&self) -> Color32 {
        self.fg_high_alpha(crate::terminal::style::TERMINAL_SCROLL_BAR_TRACK_ALPHA)
    }

    // ══════════════════════════════════════════════════════════════════════════
    // UI 样式令牌 — 调整颜色/间距/尺寸请优先改本节；业务代码勿写裸 RGB/RGBA
    // ══════════════════════════════════════════════════════════════════════════

    // ── 语义色（由主题色派生，随明暗主题变化） ──

    /// 面板 / 弹窗 / 底栏 / 右 dock 统一表面色（= `bg_window`）
    #[inline]
    pub fn color_panel_surface(&self) -> Color32 {
        self.bg_window_color()
    }

    /// ScrollArea、Multiline 的 `extreme_bg_color`（与面板底一致，避免灰条）
    #[inline]
    pub fn color_scroll_extreme_bg(&self) -> Color32 {
        self.bg_window_color()
    }

    /// 侧栏 uppercase 节标题、片段面板小标题（≈ 20% 白）
    #[inline]
    pub fn color_section_title(&self) -> Color32 {
        self.fg_high_a51()
    }

    /// 表单字段标签、弹窗次要标签
    #[inline]
    pub fn color_form_label(&self) -> Color32 {
        self.fg_high_alpha(76)
    }

    /// 表单说明、占位提示
    #[inline]
    pub fn color_form_hint(&self) -> Color32 {
        self.fg_high_alpha(102)
    }

    /// 连接列表前置图标
    #[inline]
    pub fn color_sidebar_icon(&self) -> Color32 {
        self.fg_high_alpha(89)
    }

    /// 在线会话次要状态字
    #[inline]
    pub fn color_status_online_muted(&self) -> Color32 {
        self.fg_high_alpha(77)
    }

    /// 离线会话状态字
    #[inline]
    pub fn color_status_offline_muted(&self) -> Color32 {
        self.fg_high_a64()
    }

    /// 统计/选中徽章淡底、片段筛选高亮底
    #[inline]
    pub fn color_chip_fill(&self) -> Color32 {
        self.fg_high_a64()
    }

    /// 极淡块底（折叠区、表单分组底 ≈ 2% 白）
    #[inline]
    pub fn color_subtle_inset_fill(&self) -> Color32 {
        self.fg_high_alpha(4)
    }

    /// 面板标题行次要工具按钮底（≈ proto `.toolbar-btn.secondary`）
    #[inline]
    pub fn color_panel_toolbar_btn_fill(&self) -> Color32 {
        self.fg_high_alpha(38)
    }

    /// 弹窗描边
    #[inline]
    pub fn color_modal_stroke(&self) -> Color32 {
        self.fg_high_a15()
    }

    /// 片段 team 标签字色
    #[inline]
    pub fn color_fragment_tag_text(&self) -> Color32 {
        self.accent_alpha(115)
    }

    /// 片段 team 标签淡底
    #[inline]
    pub fn color_fragment_tag_fill(&self) -> Color32 {
        self.accent_alpha(48)
    }

    /// 终端内联选区高亮
    #[inline]
    pub fn color_terminal_selection(&self) -> Color32 {
        self.accent_alpha(150)
    }

    /// SFTP 文件行 hover
    #[inline]
    pub fn color_sftp_row_hover(&self) -> Color32 {
        self.accent_alpha(30)
    }

    /// 危险强调字（断开、删除提示等）
    #[inline]
    pub fn color_danger_emphasis(&self) -> Color32 {
        Color32::from_rgb(255, 138, 128)
    }

    /// 监控告警块淡红底
    #[inline]
    pub fn color_alert_box_fill(&self) -> Color32 {
        self.red_color().gamma_multiply(0.12)
    }

    /// 监控告警块描边
    #[inline]
    pub fn color_alert_box_stroke(&self) -> Color32 {
        self.red_color().gamma_multiply(0.45)
    }

    // ── 尺寸：底栏 / 弹窗 / Tab / 列表 ──

    /// 关闭 ×、收起 − 图标字号
    pub fn size_icon_glyph(&self) -> f32 {
        18.0
    }

    /// 底栏快捷按钮行高
    pub fn size_bottom_quick_bar_h(&self) -> f32 {
        44.0
    }

    /// 快捷栏与状态栏缝隙
    pub fn size_bottom_quick_status_gap(&self) -> f32 {
        3.0
    }

    /// 底栏总高（改造后仅单行状态栏）
    pub fn size_bottom_chrome_total_h(&self) -> f32 {
        self.status_bar_height()
    }

    /// 标题栏应用名（≈30% 白）
    #[inline]
    pub fn color_title_bar_app_name(&self) -> Color32 {
        self.fg_high_alpha(76)
    }

    /// 标题栏 / 状态栏次要连接信息（≈20% / 12% 白）
    #[inline]
    pub fn color_title_bar_conn_info(&self) -> Color32 {
        self.fg_high_a51()
    }

    /// 状态栏连接文案（≈12% 白）
    #[inline]
    pub fn color_status_bar_conn(&self) -> Color32 {
        self.fg_high_a31()
    }

    /// 底栏快捷文字按钮高
    pub fn size_bottom_quick_btn_h(&self) -> f32 {
        32.0
    }

    /// 弹窗底栏按钮高（与状态栏行高一致）
    pub fn size_modal_footer_btn_h(&self) -> f32 {
        self.status_bar_height()
    }

    pub fn size_modal_footer_btn_min_w_secondary(&self) -> f32 {
        72.0
    }

    pub fn size_modal_footer_btn_min_w_primary(&self) -> f32 {
        104.0
    }

    pub fn size_tab_min_w(&self) -> f32 {
        146.0
    }

    pub fn size_tab_min_h(&self) -> f32 {
        28.0
    }

    pub fn size_sidebar_filter_chip_h(&self) -> f32 {
        22.0
    }

    /// 侧栏标题行控件统一高度（对齐原型 sidebar-add 18px 量级）
    pub fn size_sidebar_header_control_h(&self) -> f32 {
        20.0
    }

    /// 侧栏标题行：＋ / − 方形按钮边长（原型 18px）
    pub fn size_sidebar_header_icon(&self) -> f32 {
        18.0
    }

    /// 排序下拉宽度（容纳「最近连接」单行）
    pub fn size_sidebar_sort_combo_w(&self) -> f32 {
        96.0
    }

    /// 侧栏区段标题「连接」
    pub fn font_size_sidebar_section(&self) -> f32 {
        self.font_size_panel_title()
    }

    /// 侧栏控件统一字号：搜索框、排序下拉、筛选芯片
    pub fn font_size_sidebar_control(&self) -> f32 {
        self.font_size_search_input()
    }

    /// 侧栏 ＋/− 图标字号
    pub fn font_size_sidebar_icon_glyph(&self) -> f32 {
        12.0
    }

    pub fn spacing_sidebar_search_outer(&self) -> egui::Margin {
        egui::Margin {
            left: self.spacing_panel_title_pad_x(),
            right: self.spacing_panel_title_pad_x(),
            top: 4.0,
            bottom: 6.0,
        }
    }

    pub fn spacing_sidebar_filter_outer(&self) -> egui::Margin {
        egui::Margin {
            left: self.spacing_panel_title_pad_x(),
            right: self.spacing_panel_title_pad_x(),
            top: 0.0,
            bottom: 6.0,
        }
    }

    pub fn size_session_list_row_h(&self) -> f32 {
        36.0
    }

    /// 终端列底部内嵌查找条高度（单行：输入 + 导航 + 命中计数）
    pub fn size_terminal_search_bar_h(&self) -> f32 {
        36.0
    }

    pub fn size_sidebar_search_inset_x(&self) -> f32 {
        2.0
    }

    pub fn size_sidebar_search_inset_y(&self) -> f32 {
        4.0
    }

    pub fn size_fragment_panel_header_btn_h(&self) -> f32 {
        24.0
    }

    pub fn size_bottom_tool_btn_fragment_w(&self) -> f32 {
        108.0
    }

    pub fn size_bottom_tool_btn_default_w(&self) -> f32 {
        88.0
    }

    pub fn size_title_menu_btn_w(&self) -> f32 {
        56.0
    }

    pub fn size_title_menu_btn_h(&self) -> f32 {
        18.0
    }

    pub fn size_fragment_var_field_min_h(&self) -> f32 {
        28.0
    }

    pub fn spacing_modal_content_x(&self) -> f32 {
        16.0
    }

    pub fn spacing_modal_content_y(&self) -> f32 {
        14.0
    }

    pub fn spacing_modal_header_after_title(&self) -> f32 {
        8.0
    }

    pub fn spacing_modal_header_after_sep(&self) -> f32 {
        12.0
    }

    pub fn spacing_tab_bar_inner_y(&self) -> f32 {
        6.0
    }

    pub fn spacing_monitor_alert_inner(&self) -> f32 {
        4.0
    }

    /// 片段变量弹窗正文字号
    pub fn font_size_fragment_dialog_body(&self) -> f32 {
        12.0
    }

    /// 片段变量弹窗说明/标题字号
    pub fn font_size_fragment_dialog_caption(&self) -> f32 {
        11.0
    }

    /// 片段变量等宽预览字号
    pub fn font_size_fragment_dialog_mono(&self) -> f32 {
        12.0
    }

    /// 监控「网络速率」等小节标题（介于 medium 与 tab）
    pub fn font_size_monitor_section(&self) -> f32 {
        13.0
    }

    /// 空状态 / 占位大标题
    pub fn font_size_empty_state(&self) -> f32 {
        18.0
    }

    /// 顶栏菜单项字号
    pub fn font_size_menu_item(&self) -> f32 {
        11.0
    }

    /// 关于页产品名、空状态副标题等
    pub fn font_size_prominent(&self) -> f32 {
        16.0
    }

    // ── 边距 / Frame 工厂（egui Frame，非业务布局） ──

    pub fn margin_sidebar_title(&self) -> egui::Margin {
        egui::Margin::symmetric(self.spacing_panel_title_pad_x(), self.spacing_panel_title_pad_y())
    }

    pub fn margin_sidebar_search_field(&self) -> egui::Margin {
        egui::Margin::symmetric(self.size_sidebar_search_inset_x(), self.size_sidebar_search_inset_y())
    }

    pub fn margin_tab_bar(&self) -> egui::Margin {
        egui::Margin::symmetric(self.spacing_region_pad_x(), self.spacing_tab_bar_inner_y())
    }

    pub fn margin_modal_content(&self) -> egui::Margin {
        egui::Margin::symmetric(self.spacing_modal_content_x(), self.spacing_modal_content_y())
    }

    pub fn margin_monitor_alert_box(&self) -> egui::Margin {
        egui::Margin::symmetric(10.0, 8.0)
    }

    pub fn margin_monitor_metric_row(&self) -> egui::Margin {
        egui::Margin::symmetric(8.0, 4.0)
    }

    pub fn margin_status_chip(&self) -> egui::Margin {
        egui::Margin::symmetric(8.0, 2.0)
    }

    /// 顶栏 / 底栏 Panel 内边距（水平留白 + 垂直居中余量，避免字形被裁切）
    pub fn margin_chrome_bar(&self) -> egui::Margin {
        egui::Margin::symmetric(self.spacing_status_bar_x(), 5.0)
    }

    pub fn margin_title_bar_menu(&self) -> egui::Margin {
        egui::Margin::symmetric(10.0, 7.0)
    }

    /// 居中弹窗外框
    pub fn frame_modal_window(&self) -> egui::Frame {
        egui::Frame::none()
            .fill(self.color_panel_surface())
            .stroke(egui::Stroke::new(1.0, self.color_modal_stroke()))
            .rounding(self.radius_window())
            .inner_margin(egui::Margin::ZERO)
    }

    /// 弹窗内容区内边距
    pub fn frame_modal_content(&self) -> egui::Frame {
        egui::Frame::none().inner_margin(self.margin_modal_content())
    }

    /// 左连接栏 / 右 dock 外框（§7 圆角 6px + 半透明描边）
    pub fn frame_region_panel(&self) -> egui::Frame {
        egui::Frame::none()
            .fill(self.color_panel_surface())
            .stroke(egui::Stroke::new(1.0, self.border_color()))
            .rounding(egui::Rounding::same(self.radius_panel()))
            .inner_margin(self.region_content_margin())
    }

    /// 终端列外框（圆角 6px，与原型 `.terminal-area` 一致）
    pub fn frame_terminal_column(&self) -> egui::Frame {
        egui::Frame::none()
            .fill(self.bg_terminal_color())
            .stroke(egui::Stroke::new(1.0, self.border_color()))
            .rounding(egui::Rounding::same(self.radius_panel()))
            .inner_margin(egui::Margin::ZERO)
    }

    /// 状态徽章（底栏统计等）
    pub fn frame_status_chip(&self) -> egui::Frame {
        egui::Frame::none()
            .fill(self.color_chip_fill())
            .rounding(egui::Rounding::same(self.radius_list_item()))
            .inner_margin(self.margin_status_chip())
    }

    /// 监控告警汇总块
    pub fn frame_monitor_alert(&self) -> egui::Frame {
        egui::Frame::none()
            .fill(self.color_alert_box_fill())
            .stroke(egui::Stroke::new(1.0, self.color_alert_box_stroke()))
            .rounding(self.radius_panel())
            .inner_margin(self.margin_monitor_alert_box())
    }

    pub fn vec2_tab_min_size(&self) -> egui::Vec2 {
        egui::vec2(self.size_tab_min_w(), self.size_tab_min_h())
    }

    pub fn vec2_modal_footer_secondary(&self) -> egui::Vec2 {
        egui::vec2(
            self.size_modal_footer_btn_min_w_secondary(),
            self.size_modal_footer_btn_h(),
        )
    }

    pub fn vec2_modal_footer_primary(&self) -> egui::Vec2 {
        egui::vec2(
            self.size_modal_footer_btn_min_w_primary(),
            self.size_modal_footer_btn_h(),
        )
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
    pub fn font_size_tab_label(&self) -> f32 { 12.0 }       // Tab 标签
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
    /// 主工作区左栏 / 右 dock 外框内容区内边距
    pub fn spacing_region_pad_x(&self) -> f32 { 12.0 }
    pub fn spacing_region_pad_y(&self) -> f32 { 10.0 }
    /// 左栏｜终端｜右栏之间的缝隙（露出 Central 底色）
    pub fn spacing_region_gap(&self) -> f32 { 6.0 }
    /// 主工作区相对 `central_work_rect` 的外圈内边距（对齐原型 `.main { padding: 8px }`）
    pub fn spacing_work_area_pad(&self) -> f32 { self.spacing_body_pad() }

    pub fn region_content_margin(&self) -> egui::Margin {
        egui::Margin::symmetric(self.spacing_region_pad_x(), self.spacing_region_pad_y())
    }

    pub fn terminal_content_margin(&self) -> egui::Margin {
        egui::Margin::symmetric(self.spacing_terminal_pad_x(), self.spacing_terminal_pad_y())
    }
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
    /// 标题栏红绿灯圆点半径（直径 11px，与原型 `.dot` 一致）
    pub fn radius_traffic_light(&self) -> f32 { 5.5 }

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
    pub fn status_bar_height(&self) -> f32 { 36.0 }          // 状态栏（含上下内边距）
    /// 顶栏菜单行（文件 / 视图 / 工具 / 帮助）
    pub fn menu_bar_height(&self) -> f32 { 32.0 }
    /// 顶栏 / 底栏内容区可用高度（Panel 高度减去垂直 margin）
    pub fn chrome_bar_content_height(&self, bar_height: f32) -> f32 {
        (bar_height - self.margin_chrome_bar().sum().y).max(20.0)
    }
    pub fn title_bar_height(&self) -> f32 { 36.0 }
    pub fn top_chrome_total_height(&self) -> f32 {
        self.menu_bar_height()
    }

    // ── 常用 alpha 颜色辅助方法 ──
    /// FG_HIGH alpha 版本
    pub fn fg_high_alpha(&self, alpha: u8) -> Color32 {
        let c = self.fg_high.to_color32();
        let [r, g, b, _] = c.to_array();
        Color32::from_rgba_unmultiplied(r, g, b, alpha)
    }
    pub fn fg_high_a10(&self) -> Color32 { self.fg_high_alpha(10) }   // 状态栏底 / 按钮 idle
    pub fn fg_high_a20_tool(&self) -> Color32 { self.fg_high_alpha(20) } // 工具按钮默认 ≈8%
    pub fn fg_high_a31(&self) -> Color32 { self.fg_high_alpha(31) }   // 占位 / 12% 字
    pub fn fg_high_a64_tool_hover(&self) -> Color32 { self.fg_high_alpha(64) } // 工具按钮 hover ≈25%
    pub fn fg_high_a15(&self) -> Color32 { self.fg_high_alpha(15) }   // 极淡边框
    pub fn fg_high_a20(&self) -> Color32 { self.fg_high_alpha(20) }   // subtle_line
    pub fn fg_high_a46(&self) -> Color32 { self.fg_high_alpha(46) }   // 次选文字
    pub fn fg_high_a51(&self) -> Color32 { self.fg_high_alpha(51) }   // panel title
    pub fn fg_high_a64(&self) -> Color32 { self.fg_high_alpha(64) }   // LOW 约 0.25
    pub fn fg_high_a76(&self) -> Color32 { self.fg_high_alpha(76) }   // 标签文字 0.3
    pub fn fg_high_a100(&self) -> Color32 { self.fg_high_alpha(100) } // placeholder
    pub fn fg_high_a128(&self) -> Color32 { self.fg_high_alpha(128) } // subtle_label
    pub fn fg_high_a179(&self) -> Color32 { self.fg_high_alpha(179) } // 约 0.7 普通文字
    pub fn fg_high_a200(&self) -> Color32 { self.fg_high_alpha(200) } // 选中文字
    pub fn fg_high_a230(&self) -> Color32 { self.fg_high_alpha(230) } // MEDIUM 约 0.9

    /// ACCENT alpha 版本
    pub fn accent_alpha(&self, alpha: u8) -> Color32 {
        let c = self.accent.to_color32();
        let [r, g, b, _] = c.to_array();
        Color32::from_rgba_unmultiplied(r, g, b, alpha)
    }
    pub fn accent_a10(&self) -> Color32 { self.accent_alpha(10) }     // 按钮 hover 背景
    pub fn accent_a13(&self) -> Color32 { self.accent_alpha(13) }     // bg_selected
    pub fn accent_a89(&self) -> Color32 { self.accent_alpha(89) }     // accent_dim
    pub fn accent_a128(&self) -> Color32 { self.accent_alpha(128) }   // 中等高亮
    pub fn accent_a200(&self) -> Color32 { self.accent_alpha(200) }   // 强选中

    /// GREEN alpha 版本
    pub fn green_alpha(&self, alpha: u8) -> Color32 {
        let c = self.green.to_color32();
        let [r, g, b, _] = c.to_array();
        Color32::from_rgba_unmultiplied(r, g, b, alpha)
    }
    pub fn green_a64(&self) -> Color32 { self.green_alpha(64) }       // green_dim
    pub fn green_a200(&self) -> Color32 { self.green_alpha(200) }     // 强绿色

    /// RED alpha 版本
    pub fn red_alpha(&self, alpha: u8) -> Color32 {
        let c = self.red.to_color32();
        let [r, g, b, _] = c.to_array();
        Color32::from_rgba_unmultiplied(r, g, b, alpha)
    }
    pub fn red_a128(&self) -> Color32 { self.red_alpha(128) }         // 半透明红色

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

    /// 侧栏/会话列表等：行悬停底色 rgba(255,255,255,0.03)
    pub fn list_row_hover_bg(&self) -> Color32 {
        self.bg_hover_color()
    }

    /// 侧栏/会话列表等：行选中底色 rgba(102,126,234,0.05)
    pub fn list_row_selected_bg(&self) -> Color32 {
        self.bg_selected_color()
    }

    /// 创建暗夜主题（Dark）- 按设计规范 §0.1 精确配色
    pub fn dark() -> Self {
        Self {
            name: "暗夜".to_string(),
            // === 背景色 ===
            bg_body: Color32Serializable::new(13, 13, 20),            // #0d0d14 — 窗口外背景
            bg_window: Color32Serializable::new(19, 19, 28),          // #13131c — 面板/窗口底色
            bg_terminal: Color32Serializable::new(10, 10, 18),        // #0a0a12 — 终端区域/激活 Tab
            bg_tab_bar: Color32Serializable::new(16, 16, 24), // 顶栏/底栏/Tab 条（勿用极低 alpha，透明会露出窗口黑底）
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
            bg_selected: Color32Serializable::with_alpha(70, 130, 180, 13),
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
            bg_selected: Color32Serializable::with_alpha(90, 170, 100, 13),
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
        style.visuals.widgets.noninteractive.weak_bg_fill = theme.bg_body_color();
        
        // 按钮样式：默认透明底 + 悬停 accent 弱底（避免 border 色块呈「灰按钮」）
        style.visuals.widgets.noninteractive.bg_fill = theme.border_color();
        style.visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
        style.visuals.widgets.inactive.weak_bg_fill = theme.color_subtle_inset_fill();
        style.visuals.widgets.hovered.bg_fill = theme.accent_alpha(38);
        style.visuals.widgets.hovered.weak_bg_fill = theme.accent_alpha(51);
        style.visuals.widgets.active.bg_fill = theme.accent_color();
        
        // 文字颜色
        style.visuals.widgets.noninteractive.fg_stroke.color = theme.fg_low_color();
        style.visuals.widgets.inactive.fg_stroke.color = theme.fg_medium_color();
        style.visuals.widgets.hovered.fg_stroke.color = theme.fg_high_color();
        style.visuals.widgets.active.fg_stroke.color = theme.fg_high_color();
        
        // 选中状态
        style.visuals.selection.bg_fill = theme.accent_color();
        style.visuals.selection.stroke.color = theme.fg_high_color();
        style.visuals.hyperlink_color = theme.accent_color();

        // 间距保持与设计一致
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);

        // §0.2：8 档 TextStyle（9 / 10 / 11 / 12 / 13 + 默认 Body/Heading）
        style.text_styles.insert(
            egui::TextStyle::Name("xs9".into()),
            egui::FontId::proportional(theme.font_size_tag()),
        );
        style.text_styles.insert(
            egui::TextStyle::Name("sm10".into()),
            egui::FontId::proportional(theme.font_size_panel_title()),
        );
        style.text_styles.insert(
            egui::TextStyle::Name("md11".into()),
            egui::FontId::proportional(theme.font_size_status_bar()),
        );
        style.text_styles.insert(
            egui::TextStyle::Name("base12".into()),
            egui::FontId::proportional(theme.font_size_connection_name()),
        );
        style.text_styles.insert(
            egui::TextStyle::Name("lg13".into()),
            egui::FontId::proportional(theme.font_size_title_bar()),
        );
        style.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::proportional(theme.font_size_connection_name()),
        );
        style.text_styles.insert(
            egui::TextStyle::Monospace,
            egui::FontId::monospace(theme.font_size_terminal()),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::proportional(theme.font_size_connection_name()),
        );

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