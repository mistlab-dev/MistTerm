//! 产品文档目录（编译时相对仓库根路径）。

use std::path::PathBuf;

/// `docs/` 目录（开发构建时有效；发布包若未携带则为空目录）。
pub fn docs_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs")
}

/// 在系统文件管理器中打开文档目录。
pub fn reveal_docs_directory() -> bool {
    crate::platform::reveal_directory(&docs_directory())
}

/// 打开文档目录成功后的状态栏文案 `(英文, 简体中文)`，供 [`crate::i18n::tr`]。
pub fn reveal_docs_folder_success_pair() -> (&'static str, &'static str) {
    #[cfg(target_os = "macos")]
    {
        ("Opened the docs folder in Finder.", "已在 Finder 中打开文档文件夹")
    }
    #[cfg(target_os = "windows")]
    {
        (
            "Opened the docs folder in File Explorer.",
            "已在资源管理器中打开文档文件夹",
        )
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        (
            "Opened the docs folder in the file manager.",
            "已在文件管理器中打开文档文件夹",
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
    {
        ("Opened the docs folder.", "已打开文档文件夹")
    }
}

/// 各平台「打开文档文件夹」菜单项 `(英文, 简体中文)`。
pub fn reveal_docs_folder_menu_action_label_pair() -> (&'static str, &'static str) {
    #[cfg(target_os = "macos")]
    {
        ("Open docs folder in Finder", "在 Finder 中打开文档文件夹")
    }
    #[cfg(target_os = "windows")]
    {
        ("Open docs folder in File Explorer", "在资源管理器中打开文档文件夹")
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        ("Open docs folder", "打开文档文件夹")
    }
}

/// 帮助文案中的菜单路径提示（英 / 中由 UI 层 `tr` 选择）。
pub fn reveal_docs_folder_menu_hint_en() -> String {
    format!(
        "Help → {}",
        reveal_docs_folder_menu_action_label_pair().0
    )
}

pub fn reveal_docs_folder_menu_hint_zh() -> String {
    format!(
        "帮助 → {}",
        reveal_docs_folder_menu_action_label_pair().1
    )
}
