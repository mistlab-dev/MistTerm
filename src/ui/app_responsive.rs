use super::{ActiveRightDock, MistTermApp};
use eframe::egui;

/// FUNCTIONAL_SPEC §8.2 窗口宽度档位(用于提示与底栏 chip)
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ResponsiveLayoutBand {
    Narrow,
    Medium,
    Wide,
}

impl MistTermApp {
    #[inline]
    pub(super) fn layout_window_width(ctx: &egui::Context) -> f32 {
        ctx.screen_rect().width()
    }

    #[inline]
    pub(super) fn layout_band_from_width(w: f32) -> Option<ResponsiveLayoutBand> {
        if !w.is_finite() || w <= 0.0 {
            return None;
        }
        Some(if w < Self::RESP_LAYOUT_NARROW_LT_PX {
            ResponsiveLayoutBand::Narrow
        } else if w < Self::RESP_LAYOUT_WIDE_MIN_PX {
            ResponsiveLayoutBand::Medium
        } else {
            ResponsiveLayoutBand::Wide
        })
    }

    #[inline]
    pub(super) fn right_dock_open_allowed(w: f32) -> bool {
        // 单抽屉布局：中等宽度即可打开(不再要求 ≥1200)。
        w.is_finite() && w >= Self::RESP_LAYOUT_NARROW_LT_PX
    }

    /// 关闭所有右侧 `SidePanel`(不含居中 `Window` 如片段库弹窗)
    pub(super) fn close_all_right_dock_panels(&mut self) {
        self.show_fragment_panel = false;
        self.show_monitor_panel = false;
        self.show_ai_panel = false;
        self.show_sftp_panel = false;
        self.show_port_forward_panel = false;
        self.credential_panel.open = false;
        self.cloud_sync_panel.open = false;
        self.monitor_last_tab = None;
        self.sftp_last_tab = None;
        self.port_forward_last_tab = None;
        self.sync_monitor_panel_to_active_tab();
    }

    /// 打开指定右 dock；先关闭其它(全平台单抽屉互斥)。
    pub(super) fn open_right_dock_panel(&mut self, panel: ActiveRightDock) {
        self.close_all_right_dock_panels();
        match panel {
            ActiveRightDock::Fragment => self.show_fragment_panel = true,
            ActiveRightDock::Credential => self.credential_panel.open = true,
            ActiveRightDock::CloudSync => self.cloud_sync_panel.open = true,
            ActiveRightDock::Sftp => self.show_sftp_panel = true,
            ActiveRightDock::Monitor => {
                self.show_monitor_panel = true;
                self.sync_monitor_panel_to_active_tab();
                self.monitor_last_tab = self.active_tab;
            }
            ActiveRightDock::PortForward => {
                self.show_port_forward_panel = true;
                self.port_forward_last_tab = self.active_tab;
            }
            ActiveRightDock::Ai => {
                self.show_ai_panel = true;
            }
        }
    }

    /// 窄屏收起连接抽屉与右面板；宽屏不再自动展开连接栏(由 Activity Rail 控制)。
    pub(super) fn apply_responsive_layout(&mut self, ctx: &egui::Context) {
        let w = Self::layout_window_width(ctx);
        let Some(band) = Self::layout_band_from_width(w) else {
            return;
        };
        if w < Self::RESP_LAYOUT_NARROW_LT_PX {
            self.sidebar_collapsed = true;
            self.close_all_right_dock_panels();
        }

        self.last_responsive_layout_band = Some(band);
    }

    /// 窗口宽度不足以打开右侧 dock 时的提示。
    pub(super) fn narrow_window_right_dock_hint(ctx: &egui::Context, window_width: f32) -> String {
        use crate::i18n::{language, UiLanguage};
        match language(ctx) {
            UiLanguage::En => format!(
                "Window is narrow (~{:.0}px). Widen to {:.0}px+ to open the panel",
                window_width,
                Self::RESP_LAYOUT_NARROW_LT_PX,
            ),
            UiLanguage::Zh => format!(
                "窗口较窄(约 {:.0}px)，拉宽到 {:.0}px 以上可打开面板",
                window_width,
                Self::RESP_LAYOUT_NARROW_LT_PX,
            ),
        }
    }

    pub(super) fn narrow_window_fragment_panel_hint(
        ctx: &egui::Context,
        window_width: f32,
    ) -> String {
        use crate::i18n::{language, UiLanguage};
        let k = crate::platform::accel("K");
        match language(ctx) {
            UiLanguage::En => format!(
                "Window is narrow (~{:.0}px). Widen to {:.0}px+, then {k} for snippets",
                window_width,
                Self::RESP_LAYOUT_NARROW_LT_PX,
            ),
            UiLanguage::Zh => format!(
                "窗口较窄(约 {:.0}px)，拉宽到 {:.0}px 以上后再用 {k} 打开片段",
                window_width,
                Self::RESP_LAYOUT_NARROW_LT_PX,
            ),
        }
    }

    /// 打开任意右侧 dock 前调用；不允许时写状态栏并返回 false
    pub(super) fn ensure_right_dock_allowed_or_warn(&mut self, ctx: &egui::Context) -> bool {
        let w = Self::layout_window_width(ctx);
        if Self::right_dock_open_allowed(w) {
            true
        } else {
            self.notify_warn(Self::narrow_window_right_dock_hint(ctx, w));
            false
        }
    }
}

#[cfg(test)]
mod responsive_layout_tests {
    use super::MistTermApp;
    use crate::ui::layout_util::{terminal_column_width, work_area_inner_rect};

    #[test]
    fn work_area_inner_rect_terminal_width_respects_pad() {
        let work = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1200.0, 800.0));
        let inner = work_area_inner_rect(work, 8.0);
        let col_left = inner.min.x + 200.0;
        let w = terminal_column_width(col_left, inner.max.x, None);
        assert!(col_left + w <= inner.max.x + 0.01);
        assert_eq!(inner.width(), work.width() - 16.0);
    }

    #[test]
    fn right_dock_open_allowed_respects_narrow_min() {
        assert!(MistTermApp::right_dock_open_allowed(800.0));
        assert!(MistTermApp::right_dock_open_allowed(1200.0));
        assert!(MistTermApp::right_dock_open_allowed(2000.0));
        assert!(!MistTermApp::right_dock_open_allowed(799.0));
        assert!(!MistTermApp::right_dock_open_allowed(f32::NAN));
        assert!(!MistTermApp::right_dock_open_allowed(-1.0));
    }
}
