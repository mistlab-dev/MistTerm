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

/// 打开文档目录成功后的状态栏文案（随平台区分 Finder / 资源管理器等）。
pub fn reveal_docs_folder_success_message() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "已在 Finder 中打开文档文件夹"
    }
    #[cfg(target_os = "windows")]
    {
        "已在资源管理器中打开文档文件夹"
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        "已在文件管理器中打开文档文件夹"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
    {
        "已打开文档文件夹"
    }
}

/// 帮助文案中的菜单路径提示（与 [`reveal_docs_folder_menu_action_label`] 一致）。
pub fn reveal_docs_folder_menu_hint() -> String {
    format!(
        "帮助 → {}",
        reveal_docs_folder_menu_action_label()
    )
}

/// 各平台「打开文档文件夹」菜单项名称。
pub fn reveal_docs_folder_menu_action_label() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "在 Finder 中打开文档文件夹"
    }
    #[cfg(target_os = "windows")]
    {
        "在资源管理器中打开文档文件夹"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "打开文档文件夹"
    }
}
