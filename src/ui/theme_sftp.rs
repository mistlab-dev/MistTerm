//! SFTP 文件类型色。原先写死在文件表面的 flux RGB，按四套预设取色。
//!
//! 不是插件配置，只是 [`Theme`] 上的语义令牌。暗夜保持原 flux 色值。

use super::Theme;
use eframe::egui::Color32;

/// 文件名 / 扩展名色块的语义档。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SftpFileTone {
    Dir,
    Hidden,
    Code,
    Shell,
    Data,
    Config,
    Doc,
    Plain,
}

impl Theme {
    /// 文件名颜色。暗夜是 flux 浅色字；晨曦用深色字；海洋 / 森林各自偏色，避免共用一套暗底 pastel。
    pub fn color_sftp_file_name(&self, tone: SftpFileTone) -> Color32 {
        if self.is_light_theme() {
            return match tone {
                SftpFileTone::Dir | SftpFileTone::Doc | SftpFileTone::Plain => self.text_primary(),
                SftpFileTone::Hidden => self.text_tertiary(),
                SftpFileTone::Code => Color32::from_rgb(30, 64, 175),
                SftpFileTone::Shell => Color32::from_rgb(6, 95, 70),
                SftpFileTone::Data => Color32::from_rgb(146, 64, 14),
                SftpFileTone::Config => Color32::from_rgb(91, 33, 182),
            };
        }
        if self.name == "海洋" {
            return match tone {
                SftpFileTone::Dir | SftpFileTone::Plain => self.text_primary(),
                SftpFileTone::Hidden => self.text_tertiary(),
                SftpFileTone::Code => Color32::from_rgb(186, 220, 246),
                SftpFileTone::Shell => Color32::from_rgb(167, 230, 196),
                SftpFileTone::Data => Color32::from_rgb(255, 214, 140),
                SftpFileTone::Config => Color32::from_rgb(210, 196, 245),
                SftpFileTone::Doc => Color32::from_rgb(186, 204, 216),
            };
        }
        if self.name == "森林" {
            return match tone {
                SftpFileTone::Dir | SftpFileTone::Plain => self.text_primary(),
                SftpFileTone::Hidden => self.text_tertiary(),
                SftpFileTone::Code => Color32::from_rgb(170, 220, 190),
                SftpFileTone::Shell => Color32::from_rgb(166, 236, 176),
                SftpFileTone::Data => Color32::from_rgb(240, 210, 120),
                SftpFileTone::Config => Color32::from_rgb(200, 190, 240),
                SftpFileTone::Doc => Color32::from_rgb(190, 210, 196),
            };
        }
        match tone {
            SftpFileTone::Dir | SftpFileTone::Plain => Color32::from_rgb(0xf3, 0xf4, 0xf6),
            SftpFileTone::Hidden => Color32::from_rgb(0x71, 0x71, 0x7a),
            SftpFileTone::Code => Color32::from_rgb(0xbf, 0xdb, 0xfe),
            SftpFileTone::Shell => Color32::from_rgb(0xa7, 0xf3, 0xd0),
            SftpFileTone::Data => Color32::from_rgb(0xfd, 0xe6, 0x8a),
            SftpFileTone::Config => Color32::from_rgb(0xdd, 0xd6, 0xfe),
            SftpFileTone::Doc => Color32::from_rgb(0xcb, 0xd5, 0xe1),
        }
    }

    /// 扩展名色块：(填充, 字色)。色块是实心的，字色按填充取对比，不跟面板底走。
    pub fn color_sftp_badge(&self, tone: SftpFileTone) -> (Color32, Color32) {
        if self.is_light_theme() {
            let ink = Color32::WHITE;
            return match tone {
                SftpFileTone::Dir => (self.amber_color(), Color32::from_rgb(0x1c, 0x14, 0x03)),
                SftpFileTone::Hidden | SftpFileTone::Plain => (Color32::from_rgb(72, 78, 86), ink),
                SftpFileTone::Code => (Color32::from_rgb(30, 64, 175), ink),
                SftpFileTone::Shell => (Color32::from_rgb(6, 95, 70), ink),
                SftpFileTone::Data => (Color32::from_rgb(146, 64, 14), ink),
                SftpFileTone::Config => (Color32::from_rgb(91, 33, 182), ink),
                SftpFileTone::Doc => (Color32::from_rgb(70, 84, 102), ink),
            };
        }
        if self.name == "海洋" {
            return match tone {
                SftpFileTone::Dir => (
                    Color32::from_rgb(255, 196, 72),
                    Color32::from_rgb(28, 20, 3),
                ),
                SftpFileTone::Code => (
                    Color32::from_rgb(125, 185, 230),
                    Color32::from_rgb(8, 20, 32),
                ),
                SftpFileTone::Shell => (
                    Color32::from_rgb(110, 210, 170),
                    Color32::from_rgb(5, 32, 24),
                ),
                SftpFileTone::Data => (
                    Color32::from_rgb(255, 196, 110),
                    Color32::from_rgb(28, 20, 3),
                ),
                SftpFileTone::Config => (
                    Color32::from_rgb(186, 170, 240),
                    Color32::from_rgb(20, 16, 40),
                ),
                SftpFileTone::Doc => (
                    Color32::from_rgb(170, 196, 214),
                    Color32::from_rgb(12, 22, 32),
                ),
                SftpFileTone::Hidden | SftpFileTone::Plain => (
                    Color32::from_rgb(130, 156, 176),
                    Color32::from_rgb(8, 14, 20),
                ),
            };
        }
        if self.name == "森林" {
            return match tone {
                SftpFileTone::Dir => (
                    Color32::from_rgb(220, 180, 60),
                    Color32::from_rgb(28, 22, 4),
                ),
                SftpFileTone::Code => (
                    Color32::from_rgb(130, 200, 160),
                    Color32::from_rgb(6, 28, 16),
                ),
                SftpFileTone::Shell => (
                    Color32::from_rgb(120, 210, 140),
                    Color32::from_rgb(5, 32, 14),
                ),
                SftpFileTone::Data => (
                    Color32::from_rgb(230, 190, 80),
                    Color32::from_rgb(28, 22, 4),
                ),
                SftpFileTone::Config => (
                    Color32::from_rgb(180, 170, 230),
                    Color32::from_rgb(22, 16, 36),
                ),
                SftpFileTone::Doc => (
                    Color32::from_rgb(170, 196, 180),
                    Color32::from_rgb(10, 24, 16),
                ),
                SftpFileTone::Hidden | SftpFileTone::Plain => (
                    Color32::from_rgb(120, 150, 132),
                    Color32::from_rgb(8, 16, 12),
                ),
            };
        }
        match tone {
            SftpFileTone::Dir => (
                Color32::from_rgb(0xea, 0xb3, 0x08),
                Color32::from_rgb(0x1c, 0x14, 0x03),
            ),
            SftpFileTone::Code => (
                Color32::from_rgb(0x60, 0xa5, 0xfa),
                Color32::from_rgb(0x0b, 0x12, 0x20),
            ),
            SftpFileTone::Shell => (
                Color32::from_rgb(0x34, 0xd3, 0x99),
                Color32::from_rgb(0x05, 0x2e, 0x1a),
            ),
            SftpFileTone::Data => (
                Color32::from_rgb(0xfb, 0xbf, 0x24),
                Color32::from_rgb(0x1c, 0x14, 0x03),
            ),
            SftpFileTone::Config => (
                Color32::from_rgb(0xc4, 0xb5, 0xfd),
                Color32::from_rgb(0x1a, 0x10, 0x30),
            ),
            SftpFileTone::Doc => (
                Color32::from_rgb(0x94, 0xa3, 0xb8),
                Color32::from_rgb(0x0f, 0x17, 0x2a),
            ),
            SftpFileTone::Hidden | SftpFileTone::Plain => (
                Color32::from_rgb(0x64, 0x74, 0x8b),
                Color32::from_rgb(0x0b, 0x0c, 0x0f),
            ),
        }
    }

    /// 目录图标色。暗夜保持 flux 黄；其它预设用主题 amber。
    pub fn color_sftp_dir_icon(&self) -> Color32 {
        if self.uses_modern_palette() {
            Color32::from_rgb(0xea, 0xb3, 0x08)
        } else {
            self.amber_color()
        }
    }

    /// 文件列表尺寸 / 时间。暗夜是 flux zinc；其它预设用三级文字。
    pub fn color_sftp_meta_text(&self) -> Color32 {
        if self.uses_modern_palette() {
            Color32::from_rgb(0x71, 0x71, 0x7a)
        } else {
            self.text_tertiary()
        }
    }

    /// 暗夜文件行选中底（flux 蓝，低 alpha）。其它预设沿用列表选中底。
    pub fn color_sftp_row_selected_fill(&self) -> Color32 {
        if self.uses_modern_palette() {
            Color32::from_rgba_unmultiplied(96, 165, 250, 31)
        } else {
            self.list_row_selected_bg()
        }
    }

    /// 暗夜文件行左侧强调条。其它预设用主题 accent。
    pub fn color_sftp_row_selected_accent(&self) -> Color32 {
        if self.uses_modern_palette() {
            Color32::from_rgb(0x60, 0xa5, 0xfa)
        } else {
            self.accent_color()
        }
    }

    /// 暗夜文件行悬停底。其它预设用文件列表悬停令牌。
    pub fn color_sftp_row_hover_fill(&self) -> Color32 {
        if self.uses_modern_palette() {
            Color32::from_rgba_unmultiplied(255, 255, 255, 11)
        } else {
            self.color_file_list_row_hover_bg()
        }
    }
}
