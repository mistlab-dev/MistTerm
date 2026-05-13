//! FUNCTIONAL_SPEC §1.4：将底层 SSH / TCP 英文错误映射为面向用户的中文说明。

/// 将连接阶段错误信息转为简短中文提示，无法识别时保留原文摘要。
pub fn format_ssh_connect_error(message: &str) -> String {
    let s = message.trim();
    let lower = s.to_lowercase();

    if lower.contains("authentication failed")
        || (lower.contains("password") && lower.contains("publickey"))
        || lower.contains("all configured authentication methods failed")
    {
        return "认证失败，请检查用户名、密码或本机 SSH 密钥".to_string();
    }

    if lower.contains("connection timed out")
        || lower.contains("timed out")
        || lower.contains("operation timed out")
    {
        return "连接超时（约 30 秒内无响应），请检查网络、防火墙与主机是否可达".to_string();
    }

    if lower.contains("connection refused") {
        return "连接被拒绝（端口未开放或目标未监听 SSH）".to_string();
    }

    if lower.contains("no route to host")
        || lower.contains("network is unreachable")
        || lower.contains("host is down")
    {
        return "网络不可达，请检查本机网络与路由".to_string();
    }

    if lower.contains("failed to resolve")
        || lower.contains("temporary failure in name resolution")
        || lower.contains("nodename nor servname")
        || lower.contains("could not resolve hostname")
    {
        return "无法解析主机地址，请检查域名或 DNS 配置".to_string();
    }

    if lower.contains("broken pipe") || lower.contains("connection reset") {
        return "连接被对端重置或已断开".to_string();
    }

    format!("连接失败：{}", s)
}

#[cfg(test)]
mod tests {
    use super::format_ssh_connect_error;

    #[test]
    fn maps_auth_failure() {
        let s = format_ssh_connect_error("Authentication failed (password and SSH keys failed)");
        assert!(s.contains("认证"));
    }

    #[test]
    fn maps_connection_refused() {
        let s = format_ssh_connect_error("TCP connection failed: Connection refused (os error 61)");
        assert!(s.contains("拒绝"));
    }
}
