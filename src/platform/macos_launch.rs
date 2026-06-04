//! macOS：以 GUI 应用方式激活窗口（`open Mist.app` 或 Dock 启动时置前）。

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

/// 确保应用以常规 GUI 策略运行并置前（不依赖先打开 Terminal.app）。
pub fn activate_gui_application() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    app.activateIgnoringOtherApps(true);
}
