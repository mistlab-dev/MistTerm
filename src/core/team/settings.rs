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

/// 与 `docs/tech/TEAM.md` §一 CORS 一致，桌面 OAuth 优先监听端口。
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

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------- constants
    #[test]
    fn constants_have_expected_format() {
        assert!(DEFAULT_TEAM_API_BASE.starts_with("https://api."));
        assert!(DEFAULT_TEAM_WEB_ORIGIN.starts_with("https://mistlab"));
        assert_eq!(team_web_register_url(), "https://mistlab.dev/register");
        assert_eq!(
            team_web_forgot_password_url(),
            "https://mistlab.dev/forgot-password"
        );
        assert_eq!(
            team_web_oauth_desktop_callback_url(),
            "https://mistlab.dev/oauth/desktop-callback.html"
        );
        assert_eq!(OAUTH_LOCAL_PORT, 8765);
    }

    // --------------------------------------------------- TeamSettings
    #[test]
    fn default_uses_product_api_base() {
        let s = TeamSettings::default();
        assert_eq!(s.api_base, DEFAULT_TEAM_API_BASE);
        assert!(s.is_configured());
    }

    #[test]
    fn normalized_base_always_returns_product_default() {
        let mut s = TeamSettings::default();
        s.api_base = "https://evil.example.com".to_string();
        // Runtime normalizer ignores the user file override.
        assert_eq!(s.normalized_api_base(), DEFAULT_TEAM_API_BASE);
        s.lock_to_product_defaults();
        assert_eq!(s.api_base, DEFAULT_TEAM_API_BASE);
    }

    #[test]
    fn serde_defaults_roundtrip() {
        // Empty JSON -> api_base should be the product default.
        let s: TeamSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.api_base, DEFAULT_TEAM_API_BASE);
        // Partial override deserializes cleanly (but normalizer ignores it anyway).
        let s: TeamSettings = serde_json::from_str(
            r#"{"api_base":"https://staging.example/"}"#,
        )
        .unwrap();
        assert_eq!(s.api_base, "https://staging.example/");
        let roundtrip = serde_json::to_string(&s).unwrap();
        let s2: TeamSettings = serde_json::from_str(&roundtrip).unwrap();
        assert_eq!(s2.api_base, s.api_base);
    }

    // ------------------------------------------------ normalize_api_base
    #[test]
    fn normalize_empty_variants_are_empty() {
        assert_eq!(normalize_api_base(""), "");
        assert_eq!(normalize_api_base("   "), "");
        assert_eq!(normalize_api_base(" /////  "), "");
        assert_eq!(normalize_api_base("////////"), "");
    }

    #[test]
    fn normalize_preserves_scheme_prefixes_and_strips_trailing_slashes() {
        assert_eq!(
            normalize_api_base("https://api.example.com/"),
            "https://api.example.com"
        );
        assert_eq!(
            normalize_api_base("  http://api.example.com/team////  "),
            "http://api.example.com/team"
        );
    }

    #[test]
    fn normalize_prepends_https_for_bare_hostnames() {
        assert_eq!(normalize_api_base("api.example"), "https://api.example");
        assert_eq!(
            normalize_api_base("  localhost:8080/  "),
            "https://localhost:8080"
        );
    }
}
