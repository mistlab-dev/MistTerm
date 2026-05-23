//! Map low-level SSH / TCP errors to short user-facing messages (English / 简体中文).

use crate::i18n::UiLanguage;

/// Map connect-stage errors to a short localized hint; unknown messages keep a brief summary.
pub fn format_ssh_connect_error(lang: UiLanguage, message: &str) -> String {
    let s = message.trim();
    let lower = s.to_lowercase();

    let pick = |en: &str, zh: &str| -> String {
        match lang {
            UiLanguage::En => en.to_string(),
            UiLanguage::Zh => zh.to_string(),
        }
    };

    if lower.contains("authentication failed")
        || (lower.contains("password") && lower.contains("publickey"))
        || lower.contains("all configured authentication methods failed")
    {
        return pick(
            "Authentication failed — check username, password, or SSH keys",
            "认证失败，请检查用户名、密码或本机 SSH 密钥",
        );
    }

    if lower.contains("connection timed out")
        || lower.contains("timed out")
        || lower.contains("operation timed out")
    {
        return pick(
            "Connection timed out (~30s) — check network, firewall, and host reachability",
            "连接超时（约 30 秒内无响应），请检查网络、防火墙与主机是否可达",
        );
    }

    if lower.contains("connection refused") {
        return pick(
            "Connection refused — port closed or SSH not listening",
            "连接被拒绝（端口未开放或目标未监听 SSH）",
        );
    }

    if lower.contains("no route to host")
        || lower.contains("network is unreachable")
        || lower.contains("host is down")
    {
        return pick(
            "Network unreachable — check local network and routing",
            "网络不可达，请检查本机网络与路由",
        );
    }

    if lower.contains("failed to resolve")
        || lower.contains("temporary failure in name resolution")
        || lower.contains("nodename nor servname")
        || lower.contains("could not resolve hostname")
        || lower.contains("failed to resolve host address")
    {
        return pick(
            "Could not resolve hostname — check DNS or the host name",
            "无法解析主机地址，请检查域名或 DNS 配置",
        );
    }

    if lower.contains("broken pipe") || lower.contains("connection reset") {
        return pick(
            "Connection reset or closed by peer",
            "连接被对端重置或已断开",
        );
    }

    if lower.contains("no resolvable address") {
        return pick(
            "No resolvable address — check hostname and port",
            "无可解析的地址（请检查主机名与端口）",
        );
    }

    if lower.contains("tcp") && lower.contains("failed") {
        return pick(
            &format!("TCP connect failed: {s}"),
            &format!("TCP 连接失败：{s}"),
        );
    }

    match lang {
        UiLanguage::En => format!("Connection failed: {s}"),
        UiLanguage::Zh => format!("连接失败：{s}"),
    }
}

#[cfg(test)]
mod tests {
    use super::format_ssh_connect_error;
    use crate::i18n::UiLanguage;

    #[test]
    fn maps_auth_failure_zh() {
        let s = format_ssh_connect_error(
            UiLanguage::Zh,
            "Authentication failed (password and SSH keys failed)",
        );
        assert!(s.contains("认证"));
    }

    #[test]
    fn maps_auth_failure_en() {
        let s = format_ssh_connect_error(
            UiLanguage::En,
            "Authentication failed (password and SSH keys failed)",
        );
        assert!(s.to_lowercase().contains("authentication"));
    }

    #[test]
    fn maps_connection_refused() {
        let s = format_ssh_connect_error(
            UiLanguage::Zh,
            "TCP connection failed: Connection refused (os error 61)",
        );
        assert!(s.contains("拒绝"));
    }
}
