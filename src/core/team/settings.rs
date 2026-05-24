//! 团队平台连接配置（持久化在 `settings.json`）。

use serde::{Deserialize, Serialize};

/// 桌面客户端调用的团队 REST API（用户不可改）。
pub const DEFAULT_TEAM_API_BASE: &str = "https://api.mistlab.dev";

/// 账户注册、找回密码等浏览器入口（无 `api` 子域；**不会**写入桌面端 token）。
pub const DEFAULT_TEAM_WEB_ORIGIN: &str = "https://mistlab.dev";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamSettings {
    /// 历史字段：仅用于兼容旧 `settings.json`，运行时始终使用 [`DEFAULT_TEAM_API_BASE`]。
    #[serde(default = "default_team_api_base")]
    pub api_base: String,
}

fn default_team_api_base() -> String {
    DEFAULT_TEAM_API_BASE.to_string()
}

impl Default for TeamSettings {
    fn default() -> Self {
        Self {
            api_base: default_team_api_base(),
        }
    }
}

impl TeamSettings {
    /// 团队功能是否可用（产品内置 API，恒为 true）。
    pub fn is_configured(&self) -> bool {
        true
    }

    /// 实际请求使用的 API 根地址（忽略用户配置文件中的覆盖）。
    pub fn normalized_api_base(&self) -> String {
        DEFAULT_TEAM_API_BASE.to_string()
    }

    pub fn lock_to_product_defaults(&mut self) {
        self.api_base = DEFAULT_TEAM_API_BASE.to_string();
    }
}

#[inline]
pub fn team_web_register_url() -> &'static str {
    "https://mistlab.dev/register"
}

#[inline]
pub fn team_web_forgot_password_url() -> &'static str {
    "https://mistlab.dev/forgot-password"
}

/// 部署在 mistlab.dev 的桌面 OAuth 桥接页（将 token 转发到本机 `127.0.0.1:8765`）。
/// 见 `docs/product/oauth-desktop-callback.html`。
#[inline]
pub fn team_web_oauth_desktop_callback_url() -> &'static str {
    "https://mistlab.dev/oauth/desktop-callback.html"
}

/// 与 `TEAM-PLATFORM-DEV-PLAN.md` CORS 一致，桌面 OAuth 优先监听端口。
pub const OAUTH_LOCAL_PORT: u16 = 8765;

pub fn normalize_api_base(raw: &str) -> String {
    let s = raw.trim().trim_end_matches('/');
    if s.is_empty() {
        return String::new();
    }
    if s.starts_with("http://") || s.starts_with("https://") {
        s.to_string()
    } else {
        format!("https://{s}")
    }
}
