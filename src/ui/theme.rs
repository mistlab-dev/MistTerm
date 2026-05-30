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

    /// 高亮前景。暗夜主题的 `fg_high` 存的是 alpha 档位，须用 [`fg_high_alpha`] 解析为可读实色。
    pub fn fg_high_color(&self) -> Color32 {
        if self.is_light_theme() || self.uses_solid_fg_palette() {
            self.fg_high.to_color32()
        } else {
            self.fg_high_alpha(self.fg_high.a)
        }
    }

    pub fn fg_medium_color(&self) -> Color32 {
        if self.is_light_theme() || self.uses_solid_fg_palette() {
            self.fg_medium.to_color32()
        } else {
            self.fg_high_alpha(self.fg_medium.a)
        }
    }

    pub fn fg_low_color(&self) -> Color32 {
        if self.is_light_theme() || self.uses_solid_fg_palette() {
            self.fg_low.to_color32()
        } else {
            self.fg_high_alpha(self.fg_low.a)
        }
    }

    /// 浅色主题（晨曦等）：语义色须用实色档位，勿用 `fg_high_alpha`（在浅底上几乎不可见）。
    pub fn is_light_theme(&self) -> bool {
        self.bg_body.r >= 128
    }

    /// 前景为实色档位（晨曦、海洋、森林）；暗夜 `fg_*` 的 RGB 承载 alpha 曲线，须用 `fg_high_alpha` 派生。
    pub fn uses_solid_fg_palette(&self) -> bool {
        self.fg_high.a >= 250
    }

    /// 次要正文（标签、侧栏图标、Tab ×/+ 等）— 与 [`fg_medium_color`] 同一档位
    #[inline]
    fn muted_secondary_text(&self) -> Color32 {
        self.fg_medium_color()
    }

    /// 更弱文字（占位 hint、离线状态等）— 与 [`fg_low_color`] 同一档位
    #[inline]
    fn muted_tertiary_text(&self) -> Color32 {
        self.fg_low_color()
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

    // ── Theme Token v2：表面 / 描边 / 文字（业务优先调这些方法，勿用 fg_high_alpha 画边框）──

    /// 窗口外背景（= `bg_body`）
    #[inline]
    pub fn surface_body(&self) -> Color32 {
        self.bg_body_color()
    }

    /// 侧栏 / 右 dock / 弹窗面板底（= `bg_window`）
    #[inline]
    pub fn surface_panel(&self) -> Color32 {
        self.bg_window_color()
    }

    /// 顶栏 / 底栏 / Tab 条（= `bg_tab_bar`）
    #[inline]
    pub fn surface_elevated(&self) -> Color32 {
        self.bg_tab_bar_color()
    }

    /// 终端区 / 激活 Tab（= `bg_terminal`）
    #[inline]
    pub fn surface_terminal(&self) -> Color32 {
        self.bg_terminal_color()
    }

    /// 正文 / 主标签
    #[inline]
    pub fn text_primary(&self) -> Color32 {
        self.fg_high_color()
    }

    /// 次要正文（节标题、图标、Tab 控件）
    #[inline]
    pub fn text_secondary(&self) -> Color32 {
        self.muted_secondary_text()
    }

    /// 占位 / hint / 离线状态
    #[inline]
    pub fn text_tertiary(&self) -> Color32 {
        self.muted_tertiary_text()
    }

    /// 面板外框线宽（逻辑 px）
    pub fn stroke_width_panel(&self) -> f32 {
        1.0
    }

    #[inline]
    pub fn panel_stroke_color(&self) -> Color32 {
        self.border_color()
    }

    #[inline]
    pub fn divider_stroke_color(&self) -> Color32 {
        self.border_divider_color()
    }

    /// 输入框 / 搜索框描边色
    #[inline]
    pub fn stroke_input_color(&self) -> Color32 {
        self.border_color()
    }

    /// 聚焦环描边色
    #[inline]
    pub fn stroke_focus_color(&self) -> Color32 {
        self.accent_alpha(51)
    }

    pub fn panel_stroke(&self) -> egui::Stroke {
        egui::Stroke::new(self.stroke_width_panel(), self.panel_stroke_color())
    }

    pub fn divider_stroke(&self) -> egui::Stroke {
        egui::Stroke::new(self.stroke_width_panel(), self.divider_stroke_color())
    }

    pub fn stroke_input(&self) -> egui::Stroke {
        egui::Stroke::new(self.stroke_width_panel(), self.stroke_input_color())
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
        if self.is_light_theme() {
            0.62
        } else {
            crate::terminal::style::TERMINAL_OUTPUT_DIM_FACTOR
        }
    }

    /// FUNCTIONAL_SPEC §2.3.4：终端纵向滚动条宽度（px）。
    pub fn terminal_scroll_bar_width(&self) -> f32 {
        crate::terminal::style::TERMINAL_SCROLL_BAR_WIDTH
    }

    /// 终端滚动条轨道底色（设计稿 `rgba(255,255,255,0.06)`，随主题前景派生）。
    pub fn terminal_scroll_bar_track_fill(&self) -> Color32 {
        if self.is_light_theme() {
            Color32::from_rgba_unmultiplied(0, 0, 0, 15)
        } else if self.uses_solid_fg_palette() {
            Color32::from_rgba_unmultiplied(255, 255, 255, 15)
        } else {
            self.fg_high_alpha(crate::terminal::style::TERMINAL_SCROLL_BAR_TRACK_ALPHA)
        }
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

    /// 侧栏 / 右 dock / 弹窗等面板标题字色（与 [`color_panel_header_title`] 一致，保留别名）
    #[inline]
    pub fn color_section_title(&self) -> Color32 {
        self.color_panel_header_title()
    }

    /// 面板标题行文字（连接、新建会话、系统监控、命令片段等统一）
    #[inline]
    pub fn color_panel_header_title(&self) -> Color32 {
        self.text_primary()
    }

    /// 表单字段标签、弹窗次要标签（须亮于 hint、暗于输入正文）
    #[inline]
    pub fn color_form_label(&self) -> Color32 {
        if self.uses_solid_fg_palette() {
            self.text_secondary()
        } else {
            // 暗夜：勿用 fg_medium(50%)，否则与占位符灰度贴太近
            self.fg_high_alpha(200)
        }
    }

    /// 表单说明、占位提示（输入框 hint，须明显弱于 [`color_text_input_text`]）
    #[inline]
    pub fn color_form_hint(&self) -> Color32 {
        if self.uses_solid_fg_palette() {
            self.fg_low_color()
        } else {
            // 暗夜：弱于正文，仍满足 hint 在输入框底上的可读对比
            self.fg_high_alpha(88)
        }
    }

    /// 居中弹窗主标题（与 [`color_panel_header_title`] 一致）
    #[inline]
    pub fn color_modal_title(&self) -> Color32 {
        self.color_panel_header_title()
    }

    /// 连接列表前置图标
    #[inline]
    pub fn color_sidebar_icon(&self) -> Color32 {
        self.muted_secondary_text()
    }

    /// 在线会话次要状态字
    #[inline]
    pub fn color_status_online_muted(&self) -> Color32 {
        self.muted_tertiary_text()
    }

    /// 离线会话状态字
    #[inline]
    pub fn color_status_offline_muted(&self) -> Color32 {
        self.text_tertiary()
    }

    /// 统计/选中徽章淡底、片段筛选高亮底
    #[inline]
    pub fn color_chip_fill(&self) -> Color32 {
        if self.is_light_theme() {
            self.accent_alpha(76)
        } else if self.uses_solid_fg_palette() {
            self.accent_alpha(38)
        } else {
            self.fg_high_a64()
        }
    }

    /// 极淡块底（折叠区、表单分组底）
    #[inline]
    pub fn color_subtle_inset_fill(&self) -> Color32 {
        if self.is_light_theme() {
            Color32::from_rgba_unmultiplied(0, 0, 0, 12)
        } else if self.uses_solid_fg_palette() {
            Color32::from_rgba_unmultiplied(255, 255, 255, 10)
        } else {
            self.fg_high_alpha(8)
        }
    }

    /// 未勾选复选框填充（与输入框底一致，须在面板底上可见；勿用全透明）
    #[inline]
    pub fn color_checkbox_off_fill(&self) -> Color32 {
        self.color_text_input_fill()
    }

    /// 未勾选复选框描边色（比分割线略强，避免「只有悬停才看得见方框」）
    #[inline]
    pub fn color_checkbox_off_stroke_color(&self) -> Color32 {
        if self.is_light_theme() {
            self.stroke_input_color()
        } else if self.uses_solid_fg_palette() {
            Color32::from_rgba_unmultiplied(255, 255, 255, 90)
        } else {
            self.fg_high_alpha(72)
        }
    }

    /// 未勾选复选框悬停底
    #[inline]
    pub fn color_checkbox_hover_fill(&self) -> Color32 {
        self.accent_alpha(28)
    }

    /// 勾选标记颜色（叠在 accent 底上）
    #[inline]
    pub fn color_checkbox_checkmark(&self) -> Color32 {
        if self.is_light_theme() {
            Color32::WHITE
        } else {
            Color32::from_rgb(248, 250, 255)
        }
    }

    /// 复选框圆角
    #[inline]
    pub fn radius_checkbox(&self) -> f32 {
        3.0
    }

    /// Slider 滑轨底色（egui 用 `widgets.inactive.bg_fill` 绘制轨道）
    #[inline]
    pub fn color_slider_rail_fill(&self) -> Color32 {
        if self.is_light_theme() {
            Color32::from_rgba_unmultiplied(0, 0, 0, 52)
        } else if self.uses_solid_fg_palette() {
            Color32::from_rgba_unmultiplied(255, 255, 255, 28)
        } else {
            self.fg_high_alpha(22)
        }
    }

    /// 底栏状态徽章底（贴 chrome 条；勿用 [`color_chip_fill`] 的 accent 淡紫）
    #[inline]
    pub fn color_status_chip_fill(&self) -> Color32 {
        if self.is_light_theme() {
            Color32::from_rgba_unmultiplied(0, 0, 0, 10)
        } else if self.uses_solid_fg_palette() {
            Color32::from_rgba_unmultiplied(255, 255, 255, 8)
        } else {
            self.fg_high_alpha(10)
        }
    }

    /// 面板标题条底（侧栏 / 右 dock / 居中弹窗共用）
    #[inline]
    pub fn color_panel_header_band_fill(&self) -> Color32 {
        // 使用不透明底色，避免标题带下方正文文字透出。
        self.bg_tab_bar_color()
    }

    #[inline]
    pub fn color_modal_title_band_fill(&self) -> Color32 {
        self.color_panel_header_band_fill()
    }

    /// 标题条下分隔线（须强于 `color_tab_inactive_stroke`，暗夜勿用 7% 白边）
    #[inline]
    pub fn color_panel_header_divider(&self) -> Color32 {
        if self.uses_solid_fg_palette() {
            self.divider_stroke_color()
        } else {
            Color32::from_rgb(72, 72, 92)
        }
    }

    #[inline]
    pub fn color_modal_header_divider(&self) -> Color32 {
        self.color_panel_header_divider()
    }

    /// 单行/多行输入框底色（相对面板略提亮，勿过亮以免描边显得粗）
    #[inline]
    pub fn color_text_input_fill(&self) -> Color32 {
        if self.is_light_theme() {
            let w = self.bg_window_color();
            Color32::from_rgb(
                w.r().saturating_sub(6),
                w.g().saturating_sub(6),
                w.b().saturating_sub(6),
            )
        } else if self.uses_solid_fg_palette() {
            let w = self.bg_window_color();
            Color32::from_rgb(
                w.r().saturating_add(10),
                w.g().saturating_add(10),
                w.b().saturating_add(10),
            )
        } else {
            // 暗夜：实色阶梯 #151520 → #222236，比 fg_high_alpha 叠层更可辨
            Color32::from_rgb(34, 34, 52)
        }
    }

    /// 输入框描边（§10.2：1px）
    #[inline]
    pub fn color_text_input_stroke(&self) -> Color32 {
        if self.uses_solid_fg_palette() {
            self.stroke_input_color()
        } else {
            Color32::from_rgb(70, 70, 88)
        }
    }

    /// 输入框正文色（须明显亮于 [`color_form_hint`] / egui 占位符）
    #[inline]
    pub fn color_text_input_text(&self) -> Color32 {
        if self.uses_solid_fg_palette() {
            self.text_primary()
        } else {
            // 暗夜：满强度白，避免与 gray_out(hint) 的占位符融在一起
            Color32::from_rgba_unmultiplied(255, 255, 255, 255)
        }
    }

    /// 输入框 Frame（圆角、内边距；外框 1px，内层 TextEdit 须 `frame(false)` 避免双边）
    pub fn frame_form_text_input(&self, focused: bool) -> egui::Frame {
        let stroke = if focused {
            egui::Stroke::new(self.stroke_width_panel(), self.stroke_focus_color())
        } else {
            egui::Stroke::new(self.stroke_width_panel(), self.color_text_input_stroke())
        };
        egui::Frame::none()
            .fill(self.color_text_input_fill())
            .stroke(stroke)
            .rounding(self.radius_search_input())
            .inner_margin(egui::Margin::symmetric(
                self.spacing_search_input_x(),
                self.spacing_search_input_y(),
            ))
    }

    /// 面板标题行次要工具按钮底（≈ proto `.toolbar-btn.secondary`）
    #[inline]
    pub fn color_panel_toolbar_btn_fill(&self) -> Color32 {
        if self.is_light_theme() {
            Color32::from_rgba_unmultiplied(0, 0, 0, 20)
        } else if self.uses_solid_fg_palette() {
            Color32::from_rgba_unmultiplied(255, 255, 255, 18)
        } else {
            self.fg_high_alpha(38)
        }
    }

    /// 弹窗描边
    #[inline]
    pub fn color_modal_stroke(&self) -> Color32 {
        self.panel_stroke_color()
    }

    /// 弹窗主按钮底
    #[inline]
    pub fn color_modal_primary_fill(&self) -> Color32 {
        self.accent_color()
    }

    /// 弹窗主按钮悬停底（须明显亮于 [`color_modal_primary_fill`]）
    #[inline]
    pub fn color_modal_primary_fill_hover(&self) -> Color32 {
        let c = self.accent_color();
        Color32::from_rgb(
            c.r().saturating_add(36),
            c.g().saturating_add(36),
            c.b().saturating_add(36),
        )
    }

    /// 弹窗主按钮字（与 accent 底高对比）
    #[inline]
    pub fn color_modal_primary_text(&self) -> Color32 {
        let a = self.accent_color();
        let lum = 0.299 * f32::from(a.r()) + 0.587 * f32::from(a.g()) + 0.114 * f32::from(a.b());
        if lum > 136.0 {
            Color32::from_rgb(22, 28, 24)
        } else {
            Color32::WHITE
        }
    }

    /// 弹窗次按钮字
    #[inline]
    pub fn color_modal_secondary_text(&self) -> Color32 {
        self.text_secondary()
    }

    /// 侧栏筛选芯片：未选中文字
    #[inline]
    pub fn color_filter_chip_inactive_text(&self) -> Color32 {
        self.muted_secondary_text()
    }

    /// 侧栏筛选芯片：选中文字
    #[inline]
    pub fn color_filter_chip_active_text(&self) -> Color32 {
        if self.is_light_theme() || self.uses_solid_fg_palette() {
            self.accent_color()
        } else {
            self.accent_a128()
        }
    }

    /// 侧栏筛选芯片：选中底
    #[inline]
    pub fn color_filter_chip_active_fill(&self) -> Color32 {
        self.accent_alpha(51)
    }

    /// 侧栏标题区图标按钮
    #[inline]
    pub fn color_sidebar_header_icon(&self) -> Color32 {
        self.muted_secondary_text()
    }

    /// 片段 team 标签字色
    #[inline]
    pub fn color_fragment_tag_text(&self) -> Color32 {
        if self.is_light_theme() || self.uses_solid_fg_palette() {
            self.accent_color()
        } else {
            self.accent_alpha(115)
        }
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

    /// 终端块状光标（闪烁时绘制整格实心块）
    #[inline]
    pub fn color_terminal_cursor_block(&self) -> Color32 {
        if self.is_light_theme() {
            self.accent_color()
        } else {
            self.accent_a128()
        }
    }

    /// UI 文本拖选高亮底（勿用饱和 accent，否则 accent 色字会融进选区）
    #[inline]
    pub fn color_text_selection_bg(&self) -> Color32 {
        if self.is_light_theme() {
            self.accent_alpha(89)
        } else {
            Color32::from_rgba_unmultiplied(255, 255, 255, 72)
        }
    }

    /// UI 文本拖选字色（无内嵌颜色的标签会采用）
    #[inline]
    pub fn color_text_selection_fg(&self) -> Color32 {
        if self.is_light_theme() {
            self.fg_high_color()
        } else {
            Color32::WHITE
        }
    }

    /// 激活 Tab 底色（与终端区一致，整块标签可见）
    #[inline]
    pub fn color_tab_active_fill(&self) -> Color32 {
        self.bg_terminal_color()
    }

    /// 未激活 Tab 默认底（标签形态，略强于透明）
    #[inline]
    pub fn color_tab_inactive_fill(&self) -> Color32 {
        if self.is_light_theme() {
            Color32::from_rgba_unmultiplied(0, 0, 0, 32)
        } else if self.uses_solid_fg_palette() {
            Color32::from_rgba_unmultiplied(255, 255, 255, 14)
        } else {
            self.fg_high_alpha(12)
        }
    }

    /// 未激活 Tab 悬停底
    #[inline]
    pub fn color_tab_inactive_hover_fill(&self) -> Color32 {
        if self.is_light_theme() {
            Color32::from_rgba_unmultiplied(0, 0, 0, 48)
        } else if self.uses_solid_fg_palette() {
            Color32::from_rgba_unmultiplied(255, 255, 255, 22)
        } else {
            self.fg_high_alpha(20)
        }
    }

    /// 未激活 Tab 描边（勾勒标签轮廓）
    #[inline]
    pub fn color_tab_inactive_stroke(&self) -> Color32 {
        if self.is_light_theme() {
            Color32::from_rgba_unmultiplied(0, 0, 0, 64)
        } else if self.uses_solid_fg_palette() {
            Color32::from_rgba_unmultiplied(255, 255, 255, 28)
        } else {
            self.fg_high_alpha(18)
        }
    }

    /// Tab 离线状态圆点
    #[inline]
    pub fn color_tab_offline_dot(&self) -> Color32 {
        self.text_tertiary()
    }

    /// 正文级次要字（弹窗说明、面板工具按钮）
    #[inline]
    pub fn color_body_text_muted(&self) -> Color32 {
        self.text_secondary()
    }

    /// 更弱说明/标签（版本号、底栏统计、状态 chip）
    #[inline]
    pub fn color_caption_text(&self) -> Color32 {
        self.text_secondary()
    }

    /// 极淡嵌底（chip、kbd、关于页信息块）
    #[inline]
    pub fn color_overlay_fill_subtle(&self) -> Color32 {
        if self.is_light_theme() {
            Color32::from_rgba_unmultiplied(0, 0, 0, 10)
        } else if self.uses_solid_fg_palette() {
            Color32::from_rgba_unmultiplied(255, 255, 255, 12)
        } else {
            self.fg_high_a10()
        }
    }

    /// 状态栏工具图标默认字色（刻意弱于 caption，仅装饰性图标）
    #[inline]
    pub fn color_toolbar_glyph_idle(&self) -> Color32 {
        if self.is_light_theme() || self.uses_solid_fg_palette() {
            self.fg_low_color()
        } else {
            self.fg_high_alpha(31)
        }
    }

    /// 状态栏工具图标悬停字色
    #[inline]
    pub fn color_toolbar_glyph_hover(&self) -> Color32 {
        self.text_secondary()
    }

    /// Tab 外描边（与标签栏分隔）
    #[inline]
    pub fn color_tab_stroke(&self) -> Color32 {
        self.border_divider_color()
    }

    /// Tab 栏图标按钮字色（× / ＋）
    #[inline]
    pub fn color_tab_bar_icon(&self) -> Color32 {
        self.muted_secondary_text()
    }

    /// Tab 栏图标按钮悬停字色
    #[inline]
    pub fn color_tab_bar_icon_hover(&self) -> Color32 {
        self.fg_high_color()
    }

    /// Tab 栏图标按钮悬停底（亦用于弹窗 ×、侧栏图标等 `icon_hit_button`）
    #[inline]
    pub fn color_tab_bar_icon_btn_hover_fill(&self) -> Color32 {
        if self.is_light_theme() {
            Color32::from_rgba_unmultiplied(0, 0, 0, 18)
        } else if self.uses_solid_fg_palette() {
            Color32::from_rgba_unmultiplied(255, 255, 255, 20)
        } else {
            Color32::from_rgba_unmultiplied(255, 255, 255, 18)
        }
    }

    /// SFTP 文件列表区底色（表头 + 滚动列表，比面板略深）
    #[inline]
    pub fn color_file_list_bg(&self) -> Color32 {
        if self.is_light_theme() {
            Color32::from_rgb(236, 238, 244)
        } else {
            self.surface_terminal()
        }
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

    /// 标题栏 / 工具条内嵌图标边长（×、排序、新建等，egui 逻辑点；图集 HiDPI 由纹理格负责）
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
        self.text_secondary()
    }

    /// 标题栏 / 状态栏次要连接信息（≈20% / 12% 白）
    #[inline]
    pub fn color_title_bar_conn_info(&self) -> Color32 {
        self.muted_tertiary_text()
    }

    /// 状态栏连接文案（≈12% 白）
    #[inline]
    pub fn color_status_bar_conn(&self) -> Color32 {
        self.text_tertiary()
    }

    /// 底栏快捷文字按钮高
    pub fn size_bottom_quick_btn_h(&self) -> f32 {
        32.0
    }

    /// 搜索框 / 表单单行输入统一字号（13px）
    pub fn font_size_control_input(&self) -> f32 {
        self.font_size_body()
    }

    /// 次要 / 工具按钮统一字号
    pub fn font_size_control_btn(&self) -> f32 {
        self.font_size_body()
    }

    /// 搜索框、表单单行、标题栏工具、弹窗底栏按钮统一高度
    pub fn size_control_btn_h(&self) -> f32 {
        28.0
    }

    /// 次要按钮最小宽度（取消、刷新等）
    pub fn size_control_btn_min_w(&self) -> f32 {
        56.0
    }

    /// 主按钮最小宽度（保存并连接等）
    pub fn size_control_btn_min_w_primary(&self) -> f32 {
        96.0
    }

    /// 弹窗底栏按钮高（与 [`size_control_btn_h`] 一致）
    pub fn size_modal_footer_btn_h(&self) -> f32 {
        self.size_control_btn_h()
    }

    pub fn size_modal_footer_btn_min_w_secondary(&self) -> f32 {
        self.size_control_btn_min_w()
    }

    pub fn size_modal_footer_btn_min_w_primary(&self) -> f32 {
        self.size_control_btn_min_w_primary()
    }

    pub fn size_tab_min_w(&self) -> f32 {
        160.0
    }

    /// Tab 栏 × / ＋ 可点区域边长
    pub fn size_tab_bar_icon_btn(&self) -> f32 {
        24.0
    }

    /// Tab 行统一高度（内边距 + 图标区，保证 × / ＋ 垂直对齐）
    pub fn size_tab_bar_row_h(&self) -> f32 {
        self.spacing_tab_y() * 2.0 + self.size_tab_bar_icon_btn()
    }

    pub fn size_tab_min_h(&self) -> f32 {
        self.size_tab_bar_row_h()
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
        self.font_size_section_title()
    }

    /// 侧栏控件统一字号：搜索框、排序下拉
    pub fn font_size_sidebar_control(&self) -> f32 {
        self.font_size_control_input()
    }

    /// 侧栏 ＋/− 图标字号
    pub fn font_size_sidebar_icon_glyph(&self) -> f32 {
        13.0
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

    /// 右 dock / 片段面板标题行：工具按钮与关闭 × 统一高度（与 Tab 栏 × 同尺寸）
    pub fn size_panel_header_control_h(&self) -> f32 {
        self.size_tab_bar_icon_btn()
    }

    /// 面板 / dock 标题行总高（与终端 Tab 条 [`size_tab_bar_row_h`] 一致）
    pub fn size_panel_header_row_h(&self) -> f32 {
        self.size_tab_bar_row_h()
    }

    /// 标题行上下内边距（dock 标题行由 [`dock_header_horizontal`] 固定总高，此处为 0）
    pub fn spacing_panel_header_pad_y(&self) -> f32 {
        0.0
    }

    #[inline]
    pub fn size_fragment_panel_header_btn_h(&self) -> f32 {
        self.size_panel_header_control_h()
    }

    /// 标题行工具按钮字号（排序 / 新建等统一）
    pub fn font_size_panel_header_control(&self) -> f32 {
        self.font_size_control_btn()
    }

    /// 标题行工具按钮水平内边距
    pub fn spacing_panel_header_btn_pad_x(&self) -> f32 {
        self.spacing_search_input_x()
    }

    /// 标题行工具按钮最小宽度（短标签兜底）
    pub fn size_panel_header_btn_min_w(&self) -> f32 {
        self.font_size_panel_header_control() * 2.0 + self.spacing_panel_header_btn_pad_x() * 2.0
    }

    /// 片段面板筛选芯片行高（与标题行控件对齐）
    pub fn size_panel_filter_chip_h(&self) -> f32 {
        self.size_sidebar_filter_chip_h()
    }

    /// 片段面板分类筛选芯片固定宽度上限：dock 拉宽时按钮不应跟着撑开。
    pub fn size_panel_filter_chip_max_w(&self) -> f32 { 72.0 }

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
        self.font_size_body()
    }

    /// 片段变量弹窗说明/字段标签
    pub fn font_size_fragment_dialog_caption(&self) -> f32 {
        self.font_size_form_label()
    }

    /// 片段变量等宽预览字号
    pub fn font_size_fragment_dialog_mono(&self) -> f32 {
        self.font_size_body()
    }

    /// 监控「网络速率」等小节标题
    pub fn font_size_monitor_section(&self) -> f32 {
        self.font_size_body()
    }

    /// 空状态 / 占位大标题
    pub fn font_size_empty_state(&self) -> f32 {
        19.0
    }

    /// 关于页产品名、空状态副标题等
    pub fn font_size_prominent(&self) -> f32 {
        17.0
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
        egui::Margin::symmetric(8.0, 3.0)
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
            .stroke(self.panel_stroke())
            .rounding(self.radius_window())
            .inner_margin(egui::Margin::ZERO)
    }

    /// 弹窗内容区内边距
    pub fn frame_modal_content(&self) -> egui::Frame {
        egui::Frame::none().inner_margin(self.margin_modal_content())
    }

    /// 左连接栏 / 右 dock 外框（§7 圆角 6px + 半透明描边）
    pub fn frame_region_panel(&self) -> egui::Frame {
        self.frame_region_panel_rounding(egui::Rounding::same(self.radius_panel()))
    }

    /// 贴底栏的侧栏/面板：顶角与底角均不圆（与终端 Tab 条顶缘齐平）
    pub fn frame_region_panel_flush_bottom(&self) -> egui::Frame {
        self.frame_region_panel_rounding(egui::Rounding::ZERO)
    }

    fn frame_region_panel_rounding(&self, rounding: egui::Rounding) -> egui::Frame {
        egui::Frame::none()
            .fill(self.color_panel_surface())
            .stroke(self.panel_stroke())
            .rounding(rounding)
            .inner_margin(self.region_content_margin())
    }

    /// 终端列外框：顶部与 Tab 条平齐（无上圆角），底部圆角；外框描边由 [`crate::ui::chrome::paint_rect_border_ltr`] 单独绘制。
    pub fn frame_terminal_column(&self) -> egui::Frame {
        egui::Frame::none()
            .fill(self.bg_terminal_color())
            .rounding(egui::Rounding {
                nw: 0.0,
                ne: 0.0,
                sw: self.radius_panel(),
                se: self.radius_panel(),
            })
            .inner_margin(egui::Margin::ZERO)
    }

    /// 状态徽章（底栏连接、通知、片段统计等）
    pub fn frame_status_chip(&self) -> egui::Frame {
        let border = self.divider_stroke_color();
        egui::Frame::none()
            .fill(self.color_status_chip_fill())
            .stroke(egui::Stroke::new(
                1.0,
                Color32::from_rgba_unmultiplied(
                    border.r(),
                    border.g(),
                    border.b(),
                    border.a() / 2,
                ),
            ))
            .rounding(egui::Rounding::ZERO)
            .inner_margin(self.margin_status_chip())
    }

    /// 信息标签（连接元信息、分组名、弹窗标题条等，比 status_chip 略显眼）
    pub fn frame_label_tag(&self) -> egui::Frame {
        egui::Frame::none()
            .fill(self.color_chip_fill())
            .stroke(egui::Stroke::new(1.0, self.color_tab_inactive_stroke()))
            .rounding(egui::Rounding::same(self.radius_category()))
            .inner_margin(self.margin_status_chip())
    }

    /// 面板标题行底带（侧栏 / 右 dock / 弹窗共用内边距与底色）
    pub fn frame_panel_header_band(&self) -> egui::Frame {
        egui::Frame::none()
            .fill(self.color_panel_header_band_fill())
            // 标题带只负责底色；分隔线由 `panel_header_divider` 单独绘制，
            // 避免与外层 dock 圆角描边在左上角叠加出白色接缝。
            .stroke(egui::Stroke::NONE)
            .rounding(egui::Rounding::ZERO)
            .inner_margin(egui::Margin {
                left: self.spacing_panel_title_pad_x(),
                right: self.spacing_panel_title_pad_x(),
                top: 0.0,
                bottom: 0.0,
            })
    }

    /// 弹窗标题行底带（仅顶部圆角，与底部分隔线齐平）
    pub fn frame_modal_title_band(&self) -> egui::Frame {
        let r = self.radius_list_item();
        self.frame_panel_header_band()
            .inner_margin(egui::Margin::symmetric(
                self.spacing_panel_title_pad_x(),
                6.0,
            ))
            .rounding(egui::Rounding {
                nw: r,
                ne: r,
                sw: 0.0,
                se: 0.0,
            })
    }

    /// 右 dock Foreground 标题带：抵消 [`right_dock_content_margin`]，横向铺满 dock 壳层。
    pub fn frame_right_dock_header_band(&self) -> egui::Frame {
        let px = self.spacing_right_dock_pad_x();
        let py = self.spacing_right_dock_pad_y();
        self.frame_panel_header_band()
            .stroke(egui::Stroke::new(1.0, self.color_panel_header_divider()))
            .outer_margin(egui::Margin {
                left: -px,
                right: -px,
                top: -py,
                bottom: 0.0,
            })
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

    // ── 字体大小（Token v2：全局 +1px，保持相对比例） ──

    /// 10px — 角标 / team 标签
    pub fn font_size_tag(&self) -> f32 {
        10.0
    }

    /// 11px — 元信息、统计、偏好分组小标题、片段命令、筛选芯片
    pub fn font_size_caption(&self) -> f32 {
        11.0
    }

    /// 12px — 菜单、搜索、状态栏、表单标签、区段标题、侧栏「连接」
    pub fn font_size_ui_control(&self) -> f32 {
        12.0
    }

    /// 13px — 正文、连接名、Tab、片段标题、表单输入
    pub fn font_size_body(&self) -> f32 {
        13.0
    }

    /// 14px — 终端输出、标题栏主文
    pub fn font_size_terminal(&self) -> f32 {
        14.0
    }

    /// 15px — 右 dock 大标题（监控、Git 等）
    pub fn font_size_dock_title(&self) -> f32 {
        15.0
    }

    /// 13px — 面板标题行（侧栏「连接」、右 dock、居中弹窗「新建会话」等，统一）
    pub fn font_size_panel_header_title(&self) -> f32 {
        self.font_size_body()
    }

    /// 居中弹窗主标题（与 [`font_size_panel_header_title`] 一致）
    pub fn font_size_modal_title(&self) -> f32 {
        self.font_size_panel_header_title()
    }

    pub fn font_size_title_bar(&self) -> f32 {
        self.font_size_terminal()
    }
    pub fn font_size_title_bar_info(&self) -> f32 {
        self.font_size_ui_control()
    }
    /// 偏好设置等 UPPERCASE 分组标题
    pub fn font_size_panel_title(&self) -> f32 {
        self.font_size_caption()
    }
    pub fn font_size_section_title(&self) -> f32 {
        self.font_size_ui_control()
    }
    pub fn font_size_form_label(&self) -> f32 {
        self.font_size_ui_control()
    }
    pub fn font_size_connection_name(&self) -> f32 {
        self.font_size_body()
    }
    pub fn font_size_connection_meta(&self) -> f32 {
        self.font_size_caption()
    }
    pub fn font_size_fragment_title(&self) -> f32 {
        self.font_size_body()
    }
    pub fn font_size_fragment_cmd(&self) -> f32 {
        self.font_size_caption()
    }
    pub fn font_size_fragment_stats(&self) -> f32 {
        self.font_size_caption()
    }
    /// 片段列表行右侧分类/标签（与统计同级，便于与标题行对齐）
    pub fn font_size_fragment_tag(&self) -> f32 {
        self.font_size_caption()
    }
    /// 标题行额外行高（标题与标签垂直居中）
    pub fn spacing_fragment_title_line_pad(&self) -> f32 {
        2.0
    }
    /// 片段列表行最小高度（由三行字号 + 行距 + 内边距派生）
    pub fn size_fragment_list_row_min_h(&self) -> f32 {
        let gap = self.spacing_fragment_row_line_gap();
        self.spacing_fragment_row_pad_y() * 2.0
            + self.font_size_fragment_title()
                .max(self.font_size_fragment_tag())
            + self.spacing_fragment_title_line_pad()
            + gap
            + self.font_size_fragment_cmd()
            + gap
            + self.font_size_fragment_stats()
    }
    /// 片段列表主栏最小宽度（保证标题/命令至少可见）
    pub fn size_fragment_list_main_min_w(&self) -> f32 {
        self.font_size_fragment_title() * 5.0
    }
    /// 片段列表标签列最多占内容区宽度比例（§5 列表）
    pub fn fragment_list_tag_max_width_frac(&self) -> f32 {
        0.45
    }
    pub fn font_size_tab_label(&self) -> f32 {
        self.font_size_body()
    }
    pub fn font_size_tab_bar_icon(&self) -> f32 {
        15.0
    }
    pub fn font_size_search_input(&self) -> f32 {
        self.font_size_control_input()
    }
    pub fn font_size_menu_item(&self) -> f32 {
        self.font_size_ui_control()
    }

    /// 菜单项标题与右侧快捷键之间的最小空隙
    pub fn spacing_menu_shortcut_gap(&self) -> f32 {
        16.0
    }
    pub fn font_size_status_bar(&self) -> f32 {
        self.font_size_ui_control()
    }
    pub fn font_size_status_bar_stats(&self) -> f32 {
        self.font_size_status_bar()
    }
    pub fn font_size_restore_btn(&self) -> f32 {
        self.font_size_caption()
    }
    pub fn font_size_tool_btn(&self) -> f32 {
        self.font_size_body()
    }
    pub fn font_size_category_label(&self) -> f32 {
        self.font_size_caption()
    }

    // ── 间距系统（按设计规范 §8） ──
    pub fn spacing_panel_gap(&self) -> f32 { 6.0 }           // 面板间 gap
    pub fn spacing_panel_title_pad_x(&self) -> f32 { 6.0 }   // 面板标题左右 padding（收紧）
    pub fn spacing_panel_title_pad_y(&self) -> f32 {
        self.spacing_panel_header_pad_y()
    }   // 面板标题上下 padding（与 Tab 条对齐）
    pub fn spacing_panel_content_x(&self) -> f32 { 4.0 }     // 面板内容左右 padding
    pub fn spacing_panel_content_y(&self) -> f32 { 4.0 }     // 面板内容上下 padding
    pub fn spacing_search_area_x(&self) -> f32 { 8.0 }       // 搜索框区域左右 padding
    pub fn spacing_search_area_y(&self) -> f32 { 6.0 }       // 搜索框区域上下 padding
    pub fn spacing_search_input_x(&self) -> f32 { 6.0 }      // 搜索框输入左右 padding（收紧）
    pub fn spacing_search_input_y(&self) -> f32 { 5.0 }      // 搜索框输入上下 padding
    pub fn spacing_list_item_x(&self) -> f32 { 10.0 }        // 列表条目左右 padding
    pub fn spacing_list_item_y(&self) -> f32 { 8.0 }         // 列表条目上下 padding
    pub fn spacing_list_item_gap(&self) -> f32 { 1.0 }       // 列表条目间距
    /// 片段列表行左右内边距
    pub fn spacing_fragment_row_pad_x(&self) -> f32 {
        self.spacing_list_item_x()
    }
    /// 片段列表行上下内边距
    pub fn spacing_fragment_row_pad_y(&self) -> f32 {
        self.spacing_list_item_y()
    }
    /// 片段列表主栏与右侧标签列间距
    pub fn spacing_fragment_row_tag_gap(&self) -> f32 {
        self.spacing_panel_gap()
    }
    /// 片段列表行内标题/命令/统计行间距
    pub fn spacing_fragment_row_line_gap(&self) -> f32 {
        self.spacing_sm()
    }
    /// 片段标签列文字左右留白（用于测量列宽）
    pub fn spacing_fragment_tag_inner_x(&self) -> f32 {
        self.spacing_search_input_x() * 0.5
    }
    pub fn spacing_tab_x(&self) -> f32 { 14.0 }              // Tab 左右 padding
    pub fn spacing_tab_y(&self) -> f32 { 7.0 }               // Tab 上下 padding
    pub fn spacing_tab_dot_text(&self) -> f32 { 6.0 }        // Tab 圆点与文字间距
    pub fn spacing_tab_icon_gap(&self) -> f32 { 8.0 }        // Tab 标题与 × 间距
    pub fn spacing_terminal_pad_x(&self) -> f32 { 4.0 }      // 终端滚动区左右 padding
    pub fn spacing_terminal_pad_y(&self) -> f32 { 8.0 }     // 终端滚动区上下 padding
    /// 主工作区左栏 / 右 dock 外框内容区内边距
    pub fn spacing_region_pad_x(&self) -> f32 { 8.0 }
    pub fn spacing_region_pad_y(&self) -> f32 { 8.0 }
    /// 右 dock 正文区内边距（比通用 region 更紧凑）
    pub fn spacing_right_dock_pad_x(&self) -> f32 { 4.0 }
    pub fn spacing_right_dock_pad_y(&self) -> f32 { 4.0 }
    /// 左栏｜终端｜右栏之间的缝隙（露出 Central 底色）
    pub fn spacing_region_gap(&self) -> f32 { 6.0 }
    /// 右 dock 与终端、相邻 dock 之间的 `bg_body` 缝宽（独立于 [`spacing_region_gap`]）。
    pub fn spacing_dock_gap(&self) -> f32 { 5.0 }
    /// 右 dock 面板与窗口右缘缝宽（细缝即可；小于 [`spacing_work_area_pad`]）
    pub fn spacing_right_dock_screen_inset(&self) -> f32 {
        // 统一左/右列宽：右 dock 不再额外吃掉可视宽度
        0.0
    }
    /// 主工作区相对 `central_work_rect` 的外圈内边距（对齐原型 `.main { padding: 8px }`）
    pub fn spacing_work_area_pad(&self) -> f32 { 4.0 }

    /// 右 `SidePanel` 外框：在屏右缘留出 `bg_body` 缝（仅 `right` 非零）
    pub fn margin_right_dock_screen_outer(&self) -> egui::Margin {
        let g = self.spacing_right_dock_screen_inset();
        egui::Margin {
            left: 0.0,
            right: g,
            top: 0.0,
            bottom: 0.0,
        }
    }

    pub fn region_content_margin(&self) -> egui::Margin {
        egui::Margin::symmetric(self.spacing_region_pad_x(), self.spacing_region_pad_y())
    }

    pub fn right_dock_content_margin(&self) -> egui::Margin {
        egui::Margin::symmetric(self.spacing_right_dock_pad_x(), self.spacing_right_dock_pad_y())
    }

    pub fn terminal_content_margin(&self) -> egui::Margin {
        egui::Margin {
            left: self.spacing_terminal_pad_x(),
            right: self.spacing_terminal_pad_x(),
            top: 4.0,
            bottom: self.spacing_terminal_pad_y(),
        }
    }
    pub fn spacing_card_x(&self) -> f32 { 8.0 }              // 卡片左右 padding
    pub fn spacing_card_y(&self) -> f32 { 7.0 }              // 卡片上下 padding
    pub fn spacing_status_bar_x(&self) -> f32 { 14.0 }       // 状态栏左右 padding
    pub fn spacing_status_bar_y(&self) -> f32 { 4.0 }        // 状态栏上下 padding
    pub fn spacing_status_left_gap(&self) -> f32 { 8.0 }     // 状态栏左侧 gap
    pub fn spacing_status_right_gap(&self) -> f32 { 4.0 }    // 状态栏右侧 gap
    pub fn spacing_tool_btn_gap(&self) -> f32 { 3.0 }        // 工具按钮间距
    /// 右 dock 标题行「＋ / ×」距面板右缘（避免贴边被裁切）
    pub fn spacing_dock_panel_trailing_pad(&self) -> f32 { 4.0 }
    /// 筛选芯片行与排序按钮间距
    pub fn spacing_filter_sort_gap(&self) -> f32 { 8.0 }
    pub fn spacing_title_bar_x(&self) -> f32 { 16.0 }        // 标题栏左右 padding
    /// 顶栏菜单行左内边距（macOS 系统已占左侧，窗口内菜单更靠左）
    pub fn spacing_menu_bar_left(&self) -> f32 {
        #[cfg(target_os = "macos")]
        {
            return 6.0;
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.spacing_title_bar_x()
        }
    }
    /// 顶栏「终端 / 视图 / 工具 / 帮助」之间的空隙
    pub fn spacing_menu_bar_gap(&self) -> f32 { 14.0 }
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

    // ── 向后兼容别名 ──
    pub fn font_size_small(&self) -> f32 {
        self.font_size_caption()
    }
    pub fn font_size_normal(&self) -> f32 {
        self.font_size_body()
    }
    pub fn font_size_medium(&self) -> f32 {
        self.font_size_terminal()
    }
    pub fn font_size_large(&self) -> f32 {
        self.font_size_title_bar()
    }
    pub fn font_size_xl(&self) -> f32 {
        self.font_size_dock_title()
    }

    pub fn spacing_xs(&self) -> f32 { 2.0 }
    pub fn spacing_sm(&self) -> f32 { self.spacing_search_input_y() }
    pub fn spacing_md(&self) -> f32 { self.spacing_body_pad() }
    pub fn spacing_lg(&self) -> f32 { 16.0 }

    // ── 组件尺寸 ──
    pub fn progress_bar_height(&self) -> f32 { 8.0 }         // 进度条高度
    pub fn panel_title_height(&self) -> f32 {
        self.size_panel_header_row_h()
    }         // 面板标题栏高度
    pub fn status_bar_height(&self) -> f32 { 36.0 }          // 状态栏（含上下内边距）
    /// 顶栏菜单行（终端 / 编辑 / 视图 / 工具 / 帮助）
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
    /// 暗夜：在表面上的前景 alpha（白字）；浅色/彩色：沿用 `fg_high` 的 RGB。
    pub fn fg_high_alpha(&self, alpha: u8) -> Color32 {
        if self.is_light_theme() || self.uses_solid_fg_palette() {
            let c = self.fg_high.to_color32();
            let [r, g, b, _] = c.to_array();
            Color32::from_rgba_unmultiplied(r, g, b, alpha)
        } else {
            Color32::from_rgba_unmultiplied(255, 255, 255, alpha)
        }
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
        self.fg_high_alpha(20)
    }

    /// 图表坐标刻度等次要标注
    pub fn subtle_label_color(&self) -> Color32 {
        self.fg_high_alpha(60)
    }

    /// 侧栏/会话列表等：行悬停底色 rgba(255,255,255,0.03)
    pub fn list_row_hover_bg(&self) -> Color32 {
        self.bg_hover_color()
    }

    /// 侧栏/会话列表等：行选中底色 rgba(102,126,234,0.05)
    pub fn list_row_selected_bg(&self) -> Color32 {
        self.bg_selected_color()
    }

    /// 创建暗夜主题（Dark）- Token v2：加强描边与表面阶梯
    pub fn dark() -> Self {
        Self {
            name: "暗夜".to_string(),
            // === 背景色 ===
            bg_body: Color32Serializable::new(18, 20, 30),            // 提亮底色，减弱“纯黑”观感
            bg_window: Color32Serializable::new(24, 28, 40),          // 面板/窗口底色
            bg_terminal: Color32Serializable::new(20, 24, 36),        // 终端区域/激活 Tab（不再发黑）
            bg_tab_bar: Color32Serializable::new(18, 18, 28), // 顶栏/底栏/Tab 条
            bg_hover: Color32Serializable::with_alpha(10, 10, 10, 10),   // rgba(255,255,255,~0.04) — 悬停
            bg_selected: Color32Serializable::with_alpha(5, 6, 12, 13), // rgba(102,126,234,0.05) — 选中背景
            // === 文字 ===
            // 暗夜 fg_* 仅使用 .a 作为白字 alpha 档位；RGB 在解析时由 fg_high_alpha 统一为白
            fg_high: Color32Serializable::with_alpha(255, 255, 255, 230), // ~90%
            fg_medium: Color32Serializable::with_alpha(255, 255, 255, 128), // ~50%
            fg_low: Color32Serializable::with_alpha(255, 255, 255, 100),  // ~39% hint/弱字
            // === 主色调 ===
            accent: Color32Serializable::new(102, 126, 234),          // #667eea
            accent_dim: Color32Serializable::with_alpha(36, 44, 82, 89), // rgba(102,126,234,0.35)
            // === 边框 ===
            // 实色描边（WCAG 对比测试按 RGB；半透明白边在测试中与底色差过小）
            border: Color32Serializable::new(98, 110, 136),      // dock 外框（暗夜须略高于面板底）
            border_divider: Color32Serializable::new(78, 88, 108), // 底缘/缝分隔
            // === 状态色 ===
            green: Color32Serializable::new(76, 175, 80),             // #4CAF50 — 成功/连接
            green_dim: Color32Serializable::with_alpha(19, 44, 20, 64), // rgba(76,175,80,0.25)
            red: Color32Serializable::new(244, 67, 54),               // #f44336
            amber: Color32Serializable::new(255, 200, 50),
        }
    }

    /// 创建晨曦主题（Light）- 实色描边，浅底对比加强
    pub fn light() -> Self {
        Self {
            name: "晨曦".to_string(),
            // === 背景色 ===
            bg_body: Color32Serializable::new(224, 226, 230),        // #e0e2e6 外框略深，层次更清晰
            bg_window: Color32Serializable::new(248, 248, 250),      // #f8f8fa 面板
            bg_terminal: Color32Serializable::new(255, 255, 255),    // #ffffff 终端区最亮
            bg_tab_bar: Color32Serializable::new(238, 240, 244),     // 顶/底栏与面板区分
            bg_hover: Color32Serializable::with_alpha(0, 0, 0, 22),
            bg_selected: Color32Serializable::with_alpha(102, 126, 234, 48),
            // === 文字（实色，浅底须更深以保证侧栏/监控可读）===
            fg_high: Color32Serializable::new(20, 22, 26),             // #14161a
            fg_medium: Color32Serializable::new(46, 50, 56),         // #2e3238
            fg_low: Color32Serializable::new(72, 78, 86),            // #484e56
            // === 主色调 ===
            accent: Color32Serializable::new(72, 92, 200),          // 浅底上 accent 略加深
            accent_dim: Color32Serializable::new(198, 208, 242),     // #c6d0f2
            // === 边框 ===
            border: Color32Serializable::new(168, 172, 180),         // #a8acb4
            border_divider: Color32Serializable::new(198, 202, 210), // #c6cad2
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
            bg_body: Color32Serializable::new(39, 61, 82),           // 提亮主背景，减少黑线错觉
            bg_window: Color32Serializable::new(31, 49, 67),         // 面板底
            bg_terminal: Color32Serializable::new(30, 48, 66),       // 终端/空白区去黑化
            bg_tab_bar: Color32Serializable::new(35, 55, 75),        // #23374b
            bg_hover: Color32Serializable::with_alpha(255, 255, 255, 12), // rgba(255,255,255,~0.05)
            bg_selected: Color32Serializable::with_alpha(70, 130, 180, 13),
            // === 文字 ===
            fg_high: Color32Serializable::new(230, 240, 250),        // #e6f0fa
            fg_medium: Color32Serializable::new(180, 200, 220),      // #b4c8dc
            fg_low: Color32Serializable::new(140, 160, 180),         // #8ca0b4
            // === 主色调 ===
            accent: Color32Serializable::new(70, 130, 180),          // steel blue
            accent_dim: Color32Serializable::new(50, 90, 130),       // dim steel blue
            // === 边框 ===
            border: Color32Serializable::new(100, 138, 172),          // 提高 dock 边框可见度
            border_divider: Color32Serializable::with_alpha(255, 255, 255, 62), // 分隔更清晰
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
            bg_hover: Color32Serializable::with_alpha(255, 255, 255, 12), // rgba(255,255,255,~0.05)
            bg_selected: Color32Serializable::with_alpha(90, 170, 100, 13),
            // === 文字 ===
            fg_high: Color32Serializable::new(230, 245, 235),        // #e6f5eb
            fg_medium: Color32Serializable::new(180, 210, 190),      // #b4d2be
            fg_low: Color32Serializable::new(140, 170, 150),         // #8caa96
            // === 主色调 ===
            accent: Color32Serializable::new(90, 170, 100),          // forest green
            accent_dim: Color32Serializable::new(70, 130, 80),       // dim forest green
            // === 边框 ===
            border: Color32Serializable::new(74, 106, 88),            // #4a6a58 实色外框
            border_divider: Color32Serializable::with_alpha(255, 255, 255, 38), // ~15% 白分隔
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
        style.visuals.panel_fill = theme.surface_panel();
        style.visuals.faint_bg_color = theme.surface_elevated();
        // TextEdit / 裸输入框底色（勿用终端色，否则侧栏表单与面板融在一起）
        style.visuals.extreme_bg_color = theme.color_text_input_fill();
        style.visuals.window_fill = theme.surface_panel();
        style.visuals.window_stroke = theme.panel_stroke();
        style.visuals.widgets.noninteractive.weak_bg_fill = theme.surface_body();

        // 按钮样式：默认透明底 + 悬停 accent 弱底（裸 `ui.checkbox` 会几乎隐形，请用 [`crate::ui::chrome::form_checkbox`]）
        style.visuals.widgets.noninteractive.bg_fill = theme.color_subtle_inset_fill();
        style.visuals.widgets.noninteractive.bg_stroke = theme.divider_stroke();
        style.visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
        style.visuals.widgets.inactive.weak_bg_fill = theme.color_subtle_inset_fill();
        style.visuals.widgets.inactive.bg_stroke =
            egui::Stroke::new(1.0, theme.color_checkbox_off_stroke_color());
        style.visuals.widgets.hovered.bg_fill = theme.accent_alpha(38);
        style.visuals.widgets.hovered.weak_bg_fill = theme.accent_alpha(51);
        style.visuals.widgets.active.bg_fill = theme.accent_color();

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
        style.visuals.widgets.noninteractive.fg_stroke =
            egui::Stroke::new(1.0, widget_label);
        style.visuals.widgets.inactive.fg_stroke =
            egui::Stroke::new(1.0, widget_label_secondary);
        style.visuals.widgets.hovered.fg_stroke =
            egui::Stroke::new(1.0, theme.text_primary());
        style.visuals.widgets.active.fg_stroke =
            egui::Stroke::new(1.0, theme.text_primary());
        
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
        ctx.request_repaint();
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

#[cfg(test)]
mod theme_semantic_tests {
    use super::Theme;

    fn contrast_ratio(fg: egui::Color32, bg: egui::Color32) -> f32 {
        fn chan(c: u8) -> f32 {
            let x = f32::from(c) / 255.0;
            if x <= 0.03928 {
                x / 12.92
            } else {
                ((x + 0.055) / 1.055).powf(2.4)
            }
        }
        let l_fg = 0.2126 * chan(fg.r()) + 0.7152 * chan(fg.g()) + 0.0722 * chan(fg.b());
        let l_bg = 0.2126 * chan(bg.r()) + 0.7152 * chan(bg.g()) + 0.0722 * chan(bg.b());
        let (hi, lo) = if l_fg > l_bg {
            (l_fg, l_bg)
        } else {
            (l_bg, l_fg)
        };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn all_builtin_themes_form_hint_readable() {
        for theme in [
            Theme::dark(),
            Theme::light(),
            Theme::ocean(),
            Theme::forest(),
        ] {
            let hint = theme.color_form_hint();
            let fill = theme.color_text_input_fill();
            assert!(
                contrast_ratio(hint, fill) >= 2.8,
                "{}: hint vs input fill contrast {:.2}",
                theme.name,
                contrast_ratio(hint, fill)
            );
        }
    }

    #[test]
    fn all_builtin_themes_input_text_brighter_than_hint() {
        for theme in [
            Theme::dark(),
            Theme::light(),
            Theme::ocean(),
            Theme::forest(),
        ] {
            let input = theme.color_text_input_text();
            let hint = theme.color_form_hint();
            let fill = theme.color_text_input_fill();
            let input_cr = contrast_ratio(input, fill);
            let hint_cr = contrast_ratio(hint, fill);
            assert!(
                input_cr > hint_cr,
                "{}: input contrast {:.2} should exceed hint {:.2}",
                theme.name,
                input_cr,
                hint_cr
            );
            assert!(
                input_cr >= 4.5,
                "{}: input text vs fill contrast {:.2}",
                theme.name,
                input_cr
            );
        }
    }

    #[test]
    fn solid_palette_classification() {
        assert!(!Theme::dark().uses_solid_fg_palette());
        assert!(Theme::light().uses_solid_fg_palette());
        assert!(Theme::ocean().uses_solid_fg_palette());
        assert!(Theme::forest().uses_solid_fg_palette());
    }

    #[test]
    fn modal_primary_text_contrast_on_accent() {
        for theme in [
            Theme::dark(),
            Theme::light(),
            Theme::ocean(),
            Theme::forest(),
        ] {
            let text = theme.color_modal_primary_text();
            let fill = theme.color_modal_primary_fill();
            assert!(
                contrast_ratio(text, fill) >= 3.0,
                "{}: primary button contrast {:.2}",
                theme.name,
                contrast_ratio(text, fill)
            );
        }
    }

    #[test]
    fn all_builtin_themes_caption_and_icon_readable() {
        for theme in [
            Theme::dark(),
            Theme::light(),
            Theme::ocean(),
            Theme::forest(),
        ] {
            let bg = theme.bg_window_color();
            assert!(
                contrast_ratio(theme.color_caption_text(), bg) >= 2.5,
                "{}: caption on panel {:.2}",
                theme.name,
                contrast_ratio(theme.color_caption_text(), bg)
            );
            assert!(
                contrast_ratio(theme.color_sidebar_header_icon(), bg) >= 2.5,
                "{}: close icon idle {:.2}",
                theme.name,
                contrast_ratio(theme.color_sidebar_header_icon(), bg)
            );
            assert!(
                contrast_ratio(theme.color_tab_offline_dot(), theme.bg_tab_bar_color()) >= 2.0,
                "{}: tab offline dot {:.2}",
                theme.name,
                contrast_ratio(theme.color_tab_offline_dot(), theme.bg_tab_bar_color())
            );
        }
    }

    #[test]
    fn all_builtin_themes_panel_stroke_nonzero() {
        for theme in [
            Theme::dark(),
            Theme::light(),
            Theme::ocean(),
            Theme::forest(),
        ] {
            assert!(
                theme.panel_stroke_color().a() > 0,
                "{}: panel stroke alpha",
                theme.name
            );
            assert!(
                theme.divider_stroke_color().a() > 0,
                "{}: divider stroke alpha",
                theme.name
            );
            let bg = theme.surface_panel();
            assert!(
                contrast_ratio(theme.panel_stroke_color(), bg) >= 1.15,
                "{}: panel border vs panel bg {:.2}",
                theme.name,
                contrast_ratio(theme.panel_stroke_color(), bg)
            );
        }
    }

    #[test]
    fn all_builtin_themes_text_tiers_readable_on_chrome() {
        for theme in [
            Theme::dark(),
            Theme::light(),
            Theme::ocean(),
            Theme::forest(),
        ] {
            for (name, bg) in [
                ("panel", theme.surface_panel()),
                ("tab_bar", theme.surface_elevated()),
            ] {
                assert!(
                    contrast_ratio(theme.text_primary(), bg) >= 4.5,
                    "{} {}: primary {:.2}",
                    theme.name,
                    name,
                    contrast_ratio(theme.text_primary(), bg)
                );
                let sec_min = if theme.is_light_theme() { 3.5 } else { 2.8 };
                let ter_min = if theme.is_light_theme() { 3.0 } else { 2.2 };
                assert!(
                    contrast_ratio(theme.text_secondary(), bg) >= sec_min,
                    "{} {}: secondary {:.2}",
                    theme.name,
                    name,
                    contrast_ratio(theme.text_secondary(), bg)
                );
                assert!(
                    contrast_ratio(theme.text_tertiary(), bg) >= ter_min,
                    "{} {}: tertiary {:.2}",
                    theme.name,
                    name,
                    contrast_ratio(theme.text_tertiary(), bg)
                );
                assert!(
                    contrast_ratio(theme.color_caption_text(), bg) >= sec_min,
                    "{} {}: caption {:.2}",
                    theme.name,
                    name,
                    contrast_ratio(theme.color_caption_text(), bg)
                );
            }
        }
    }

    #[test]
    fn all_builtin_themes_body_text_contrast() {
        for theme in [
            Theme::dark(),
            Theme::light(),
            Theme::ocean(),
            Theme::forest(),
        ] {
            let bg = theme.surface_panel();
            assert!(
                contrast_ratio(theme.text_primary(), bg) >= 4.5,
                "{}: primary text on panel {:.2}",
                theme.name,
                contrast_ratio(theme.text_primary(), bg)
            );
        }
    }

    #[test]
    fn all_builtin_themes_primary_hover_brighter_than_idle() {
        for theme in [
            Theme::dark(),
            Theme::light(),
            Theme::ocean(),
            Theme::forest(),
        ] {
            let idle = theme.color_modal_primary_fill();
            let hover = theme.color_modal_primary_fill_hover();
            let idle_lum = f32::from(idle.r()) + f32::from(idle.g()) + f32::from(idle.b());
            let hover_lum = f32::from(hover.r()) + f32::from(hover.g()) + f32::from(hover.b());
            assert!(
                hover_lum > idle_lum,
                "{}: hover fill should be brighter than idle",
                theme.name
            );
        }
    }
}