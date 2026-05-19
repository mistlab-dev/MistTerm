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
