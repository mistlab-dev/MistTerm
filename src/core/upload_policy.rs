//! 本地上传策略（FUNCTIONAL_SPEC §4.3）
//!
//! 决定直传 SCP 还是弹出大文件选择；不执行传输本身。

use std::path::Path;

/// ≥ 此大小需用户选择 SCP / ZMODEM
pub const LARGE_UPLOAD_THRESHOLD_BYTES: u64 = 10 * 1024 * 1024;

/// UI / 用例层对上传路径的处置
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UploadDispatch {
    /// 可直接 SCP
    ScpDirect { size_bytes: u64 },
    /// 需弹窗选方式
    PromptLargeFile { size_bytes: u64 },
    /// 无活动终端
    NoActiveTab,
}

/// 根据路径与是否有活动标签决定下一步
pub fn decide_upload_dispatch(path: &Path, has_active_tab: bool) -> UploadDispatch {
    if !has_active_tab {
        return UploadDispatch::NoActiveTab;
    }
    let size_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if size_bytes >= LARGE_UPLOAD_THRESHOLD_BYTES {
        UploadDispatch::PromptLargeFile { size_bytes }
    } else {
        UploadDispatch::ScpDirect { size_bytes }
    }
}

/// 状态栏 / 提示用简短体积文案
pub fn format_bytes_short(n: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    if n >= MB {
        format!("{:.1} MB", n as f64 / MB as f64)
    } else if n >= KB {
        format!("{:.1} KB", n as f64 / KB as f64)
    } else {
        format!("{} B", n)
    }
}
