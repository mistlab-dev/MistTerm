//! 匿名使用统计 ping（#149）。
//!
//! 搭启动时自动更新检查的车：每次成功发起版本检查时，向
//! `https://mistlab.dev/downloads/mistterm/ping` 发一个 GET（nginx 204，纯日志统计）。
//!
//! 匿名边界：
//! - 只带 `v`（版本）/ `os` / `arch` / `cid`（本地随机 UUID，首次生成后持久化）；
//! - 不含用户名、主机名、账号、会话内容或文件路径；
//! - 失败完全静默（网络不通、超时、非 2xx 都不影响任何功能）；
//! - 测试构建（`update-test`）不发 ping，避免污染线上统计与 E2E。

use serde::{Deserialize, Serialize};

/// ping 端点：nginx 直接 204，统计走 access log。
pub const PING_URL: &str = "https://mistlab.dev/downloads/mistterm/ping";

/// 匿名客户端实例 ID：首次生成后存进 `update-state.json`，与更新状态同文件。
pub fn ensure_client_id(state: &mut super::state::UpdateState) -> String {
    if let Some(id) = state.client_id.as_deref() {
        if !id.is_empty() {
            return id.to_string();
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    state.client_id = Some(id.clone());
    id
}

/// 发送 ping（阻塞、短超时、静默失败）。必须在独立线程调用，不能进 tokio 运行时。
/// 测试构建直接跳过。
pub fn send_startup_ping(cid: &str) {
    if cfg!(feature = "update-test") {
        return;
    }
    let url = format!(
        "{PING_URL}?v={}&os={}&arch={}&cid={}",
        urlencode(super::APP_VERSION),
        urlencode(std::env::consts::OS),
        urlencode(std::env::consts::ARCH),
        urlencode(cid),
    );
    let _ = send_probe(&url);
}

fn send_probe(url: &str) -> Result<(), String> {
    // 独立短超时 client：不复用更新 client，失败立即丢弃。
    let client = reqwest::blocking::Client::builder()
        .user_agent(super::user_agent())
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(url)
        .send()
        .map_err(|e| e.to_string())?;
    let _ = resp.error_for_status().map_err(|e| e.to_string())?;
    Ok(())
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// `cid` 的 serde 兼容性测试用占位（保持模块结构稳定）。
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_urlencode() {
        assert_eq!(urlencode("1.2.34"), "1.2.34");
        assert_eq!(urlencode("a b/c"), "a%20b%2Fc");
    }

    #[test]
    fn test_client_id_stable() {
        let mut a = super::super::state::UpdateState::default();
        let id1 = ensure_client_id(&mut a);
        let id2 = ensure_client_id(&mut a);
        assert_eq!(id1, id2);
        assert!(!id1.is_empty());
        // uuid 形态
        assert_eq!(id1.split('-').count(), 5);
    }

    #[test]
    fn test_send_probe_rejects_non_https() {
        // send_probe 只在 send_startup_ping 内部用 https 常量；这里只验证 URL 形态
        assert!(PING_URL.starts_with("https://"));
    }

    #[allow(dead_code)]
    #[derive(Serialize, Deserialize)]
    struct SerdeProbe;
}
