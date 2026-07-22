use super::{Color32Serializable, Theme};

impl Theme {
    /// 创建暗夜主题 — 中性灰表面、蓝色 accent
    pub fn dark() -> Self {
        Self {
            name: "暗夜".to_string(),
            bg_body: Color32Serializable::new(32, 32, 32), // #202020
            bg_window: Color32Serializable::new(37, 37, 37), // #252525
            bg_terminal: Color32Serializable::new(32, 32, 32),
            bg_tab_bar: Color32Serializable::new(28, 28, 28), // #1c1c1c
            bg_hover: Color32Serializable::with_alpha(255, 255, 255, 10),
            bg_selected: Color32Serializable::with_alpha(255, 255, 255, 16),
            fg_high: Color32Serializable::new(255, 255, 255),
            fg_medium: Color32Serializable::new(200, 200, 200),
            fg_low: Color32Serializable::new(160, 160, 160),
            accent: Color32Serializable::new(96, 205, 255), // #60CDFF
            accent_dim: Color32Serializable::new(0, 120, 212), // #0078D4
            border: Color32Serializable::new(61, 61, 61),   // #3d3d3d
            border_divider: Color32Serializable::new(45, 45, 45), // #2d2d2d
            green: Color32Serializable::new(108, 203, 95),
            green_dim: Color32Serializable::with_alpha(108, 203, 95, 64),
            red: Color32Serializable::new(255, 138, 128),
            amber: Color32Serializable::new(255, 196, 72),
        }
    }

    /// 创建晨曦主题（Light）- 实色描边，浅底对比加强
    pub fn light() -> Self {
        Self {
            name: "晨曦".to_string(),
            // === 背景色 ===
            bg_body: Color32Serializable::new(224, 226, 230), // #e0e2e6 外框略深，层次更清晰
            bg_window: Color32Serializable::new(248, 248, 250), // #f8f8fa 面板
            bg_terminal: Color32Serializable::new(255, 255, 255), // #ffffff 终端区最亮
            bg_tab_bar: Color32Serializable::new(238, 240, 244), // 顶/底栏与面板区分
            bg_hover: Color32Serializable::with_alpha(0, 0, 0, 22),
            bg_selected: Color32Serializable::with_alpha(102, 126, 234, 48),
            // === 文字（实色，浅底须更深以保证侧栏/监控可读）===
            fg_high: Color32Serializable::new(20, 22, 26), // #14161a
            fg_medium: Color32Serializable::new(46, 50, 56), // #2e3238
            fg_low: Color32Serializable::new(72, 78, 86),  // #484e56
            // === 主色调 ===
            accent: Color32Serializable::new(72, 92, 200), // 浅底上 accent 略加深
            accent_dim: Color32Serializable::new(198, 208, 242), // #c6d0f2
            // === 边框 ===
            border: Color32Serializable::new(168, 172, 180), // #a8acb4
            border_divider: Color32Serializable::new(198, 202, 210), // #c6cad2
            // === 状态色 ===
            green: Color32Serializable::new(76, 175, 80), // #4CAF50
            green_dim: Color32Serializable::with_alpha(76, 175, 80, 64),
            red: Color32Serializable::new(244, 67, 54), // #f44336
            amber: Color32Serializable::new(245, 124, 0),
        }
    }

    /// 创建海洋主题（Ocean）- 蓝调背景，专业冷静
    pub fn ocean() -> Self {
        Self {
            name: "海洋".to_string(),
            // === 背景色 ===
            bg_body: Color32Serializable::new(39, 61, 82), // 提亮主背景，减少黑线错觉
            bg_window: Color32Serializable::new(31, 49, 67), // 面板底
            bg_terminal: Color32Serializable::new(30, 48, 66), // 终端/空白区去黑化
            bg_tab_bar: Color32Serializable::new(35, 55, 75), // #23374b
            bg_hover: Color32Serializable::with_alpha(255, 255, 255, 12), // rgba(255,255,255,~0.05)
            bg_selected: Color32Serializable::with_alpha(70, 130, 180, 13),
            // === 文字 ===
            fg_high: Color32Serializable::new(230, 240, 250), // #e6f0fa
            fg_medium: Color32Serializable::new(180, 200, 220), // #b4c8dc
            fg_low: Color32Serializable::new(140, 160, 180),  // #8ca0b4
            // === 主色调 ===
            accent: Color32Serializable::new(70, 130, 180), // steel blue
            accent_dim: Color32Serializable::new(50, 90, 130), // dim steel blue
            // === 边框 ===
            border: Color32Serializable::new(100, 138, 172), // 提高 dock 边框可见度
            border_divider: Color32Serializable::with_alpha(255, 255, 255, 62), // 分隔更清晰
            // === 状态色 ===
            green: Color32Serializable::new(80, 200, 120), // teal green
            green_dim: Color32Serializable::with_alpha(80, 200, 120, 64),
            red: Color32Serializable::new(220, 80, 80), // coral red
            amber: Color32Serializable::new(255, 200, 100),
        }
    }

    /// 创建森林主题（Forest）- 绿色调背景，自然清新
    pub fn forest() -> Self {
        Self {
            name: "森林".to_string(),
            // === 背景色 ===
            bg_body: Color32Serializable::new(40, 60, 50), // #283c32
            bg_window: Color32Serializable::new(32, 50, 42), // #20322a
            bg_terminal: Color32Serializable::new(26, 42, 35), // #1a2a23
            bg_tab_bar: Color32Serializable::new(40, 60, 50), // #283c32
            bg_hover: Color32Serializable::with_alpha(255, 255, 255, 12), // rgba(255,255,255,~0.05)
            bg_selected: Color32Serializable::with_alpha(90, 170, 100, 13),
            // === 文字 ===
            fg_high: Color32Serializable::new(230, 245, 235), // #e6f5eb
            fg_medium: Color32Serializable::new(180, 210, 190), // #b4d2be
            fg_low: Color32Serializable::new(140, 170, 150),  // #8caa96
            // === 主色调 ===
            accent: Color32Serializable::new(90, 170, 100), // forest green
            accent_dim: Color32Serializable::new(70, 130, 80), // dim forest green
            // === 边框 ===
            border: Color32Serializable::new(74, 106, 88), // #4a6a58 实色外框
            border_divider: Color32Serializable::with_alpha(255, 255, 255, 38), // ~15% 白分隔
            // === 状态色 ===
            green: Color32Serializable::new(100, 200, 120), // bright forest green
            green_dim: Color32Serializable::with_alpha(100, 200, 120, 64),
            red: Color32Serializable::new(200, 90, 90), // muted red
            amber: Color32Serializable::new(220, 180, 60),
        }
    }
}
