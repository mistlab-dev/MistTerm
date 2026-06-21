//! macOS 应用显示名（菜单栏首项、Dock 等）。

use objc2::MainThreadMarker;
use objc2_app_kit::NSApplication;
use objc2_foundation::{NSProcessInfo, NSString};

pub use super::app_name::APP_DISPLAY_NAME;

/// 设置进程名，并修正菜单栏首项标题（`cargo run` 时系统默认显示可执行文件名）。
pub fn set_application_display_name() {
    let _mtm = MainThreadMarker::new().expect("set_application_display_name 须在主线程");
    let info = NSProcessInfo::processInfo();
    let name = NSString::from_str(APP_DISPLAY_NAME);
    info.setProcessName(&name);
    fix_menu_bar_application_title();
}

/// 将主菜单应用项及其子菜单标题设为 [`APP_DISPLAY_NAME`]。
///
/// 菜单栏首项文字在 macOS 上来自 CFBundleName / 进程名；仍同步 NSMenuItem 与子菜单标题。
pub fn fix_menu_bar_application_title() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let Some(main_menu) = app.mainMenu() else {
        return;
    };
    let title = NSString::from_str(APP_DISPLAY_NAME);
    let count = main_menu.numberOfItems();
    for i in 0..count {
        let Some(item) = main_menu.itemAtIndex(i) else {
            continue;
        };
        let Some(submenu) = item.submenu() else {
            continue;
        };
        // 应用菜单：含「关于 / 偏好设置 / 退出」等项
        if submenu.itemAtIndex(0).is_some_and(|m| {
            let t = m.title().to_string();
            t.contains("关于") || t.contains("偏好") || t.contains("退出")
        }) {
            item.setTitle(&title);
            submenu.setTitle(&title);
            return;
        }
    }
    if let Some(first) = main_menu.itemAtIndex(0) {
        first.setTitle(&title);
        if let Some(submenu) = first.submenu() {
            submenu.setTitle(&title);
        }
    }
}
