//! 共享小工具：输出 / 历史记录。

use crate::core::command_history::CommandHistory;

/// 记录命令历史（与 GUI 共用同一份 history 文件）。
/// 失败静默降级——CLI 不为历史记录失败而中断。
pub fn record_history(command: &str, session_id: Option<&str>, session_name: Option<&str>, success: bool) {
    let mut h = CommandHistory::new();
    // 触发同步等待后台加载完成（默认 new() 是异步加载）
    for _ in 0..100 {
        if h.is_loaded() {
            break;
        }
        if h.poll_background_load() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    h.record(command, session_id, session_name, success);
    let _ = h.save();
}

/// 远端目标 spec 拆分：`target:path`（Windows 盘符除外）。
/// 返回 `(target, remote_path)`；`None` 表示没有冒号分隔。
pub fn split_target_path(spec: &str) -> Option<(String, String)> {
    // IPv6 简化：不支持 [::1]:path 形式，先用普通冒号切
    let (t, p) = spec.split_once(':')?;
    if t.is_empty() || p.is_empty() {
        return None;
    }
    Some((t.to_string(), p.to_string()))
}
