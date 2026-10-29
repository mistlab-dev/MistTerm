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

    if let Some(fido2) = fido2_user_message(lang, s) {
        return fido2;
    }

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
        || lower.contains("did not properly respond")
    {
        return pick(
            "Connection timed out (~30s) — check network, firewall, and host reachability",
            "连接超时（约 30 秒内无响应），请检查网络、防火墙与主机是否可达",
        );
    }

    if lower.contains("connection refused") || lower.contains("actively refused") {
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
        || lower.contains("name or service not known")
        || lower.contains("no such host")
        || lower.contains("unknown host")
        || lower.contains("no address associated")
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

    if lower.contains("banner") || lower.contains("handshake") || lower.contains("kex") {
        return pick(
            "Could not talk to this SSH server. Check the address, port, and that SSH is running.",
            "连不上这台服务器。请核对地址和端口，并确认对方开了 SSH。",
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

/// 连接过程中显示的触摸 / PIN 提示。三端都走系统 OpenSSH agent，不在 MistTerm 里收集 PIN。
pub fn fido2_connect_hint(lang: UiLanguage) -> &'static str {
    match lang {
        UiLanguage::En => {
            #[cfg(windows)]
            {
                "Touch the security key, or enter its PIN in the Windows prompt. MistTerm signs with the OpenSSH Authentication Agent (ecdsa-sk / ed25519-sk), not Pageant, and does not ask for the PIN here."
            }
            #[cfg(target_os = "macos")]
            {
                "Touch the security key when it blinks, or enter its PIN in the macOS prompt. MistTerm signs with the system ssh-agent (ecdsa-sk / ed25519-sk) and does not ask for the PIN here."
            }
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                "Touch the security key, or enter its PIN in the system prompt. MistTerm signs with OpenSSH ssh-agent in this session (ecdsa-sk / ed25519-sk) and does not ask for the PIN here."
            }
            #[cfg(not(any(windows, unix)))]
            {
                "Touch the security key, or enter its PIN in the system prompt. MistTerm signs with the OpenSSH agent (ecdsa-sk / ed25519-sk) and does not ask for the PIN here."
            }
        }
        UiLanguage::Zh => {
            #[cfg(windows)]
            {
                "请触摸安全密钥，或在 Windows 提示里输入 PIN。MistTerm 通过 OpenSSH Authentication Agent 使用 ecdsa-sk / ed25519-sk（不是 Pageant），不会在这里索要 PIN。"
            }
            #[cfg(target_os = "macos")]
            {
                "请在安全密钥闪烁时触摸，或在 macOS 提示里输入 PIN。MistTerm 通过系统 ssh-agent 使用 ecdsa-sk / ed25519-sk，不会在这里索要 PIN。"
            }
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                "请触摸安全密钥，或在系统提示里输入 PIN。MistTerm 通过本会话的 OpenSSH ssh-agent 使用 ecdsa-sk / ed25519-sk，不会在这里索要 PIN。"
            }
            #[cfg(not(any(windows, unix)))]
            {
                "请触摸安全密钥，或在系统提示里输入 PIN。MistTerm 通过 OpenSSH agent 使用 ecdsa-sk / ed25519-sk，不会在这里索要 PIN。"
            }
        }
    }
}

fn fido2_user_message(lang: UiLanguage, message: &str) -> Option<String> {
    let code = message.split(':').next()?.trim();
    let (en, zh) = match code {
        "FIDO2_AGENT_UNAVAILABLE" => (
            "SSH agent is not available, so the security key cannot sign. Windows: start the OpenSSH Authentication Agent service (Pageant cannot sign sk keys). macOS: the system ssh-agent should already be running. Linux: start OpenSSH ssh-agent in this session, export SSH_AUTH_SOCK, then run ssh-add. Login did not succeed.",
            "连不上 SSH 代理，安全密钥无法签名。Windows 请启动 OpenSSH Authentication Agent 服务 (Pageant 不能签 sk 密钥)。macOS 使用系统 ssh-agent。Linux 请在本会话启动 OpenSSH 的 ssh-agent 并设置 SSH_AUTH_SOCK，然后执行 ssh-add。这次没有登录成功。",
        ),
        "FIDO2_NO_SECURITY_KEY" => (
            "No OpenSSH security key (ecdsa-sk / ed25519-sk) is available. Plug the key in and load it with ssh-add, then retry. Login did not succeed.",
            "没有可用的 OpenSSH 安全密钥 (ecdsa-sk / ed25519-sk)。请插入密钥并执行 ssh-add，然后重试。这次没有登录成功。",
        ),
        "FIDO2_KEY_FILE_MISSING" => (
            "Security key file was not found. Check the path (for example ~/.ssh/id_ed25519_sk), or leave it empty to use any sk key already loaded in the agent. Login did not succeed.",
            "找不到安全密钥文件。请检查路径 (例如 ~/.ssh/id_ed25519_sk)，或留空以使用代理里已加载的 sk 密钥。这次没有登录成功。",
        ),
        "FIDO2_TOUCH_CANCELLED" => (
            "Security key touch was cancelled or timed out. Touch the key when it blinks, or confirm the system prompt, then retry. Login did not succeed.",
            "安全密钥触摸已取消或等待超时。请在密钥闪烁时触摸，或确认系统提示后重试。这次没有登录成功。",
        ),
        "FIDO2_PIN_INCORRECT" => (
            "Security key PIN was rejected. Enter the FIDO2 PIN in the system prompt and retry. Too many failures can lock the key. Login did not succeed.",
            "安全密钥 PIN 不正确。请在系统提示里重新输入 FIDO2 PIN。连续错误可能会锁住密钥。这次没有登录成功。",
        ),
        "FIDO2_SIGN_FAILED" => (
            "Security key sign-in failed and login did not succeed. If you dismissed the touch prompt, the touch was cancelled — touch the key and retry. If the system prompt said the PIN was wrong, retry with the FIDO2 PIN. If the key never blinked and there was no prompt, it is missing or not loaded — plug it in and run ssh-add -l.",
            "安全密钥登录失败，没有登录成功。若你关掉了触摸提示，这是取消触摸，请触摸密钥后重试。若系统提示 PIN 错误，请用正确的 FIDO2 PIN 重试。若密钥没有闪烁且没有任何提示，说明密钥未插入或未加载，请插入后执行 ssh-add -l。",
        ),
        _ => return None,
    };
    Some(match lang {
        UiLanguage::En => en.to_string(),
        UiLanguage::Zh => zh.to_string(),
    })
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
    fn maps_handshake_banner_zh() {
        let s = format_ssh_connect_error(
            UiLanguage::Zh,
            "SSH handshake failed: [Session(-13)] Failed getting banner",
        );
        assert!(s.contains("连不上"));
        assert!(!s.contains("Session"));
    }

    #[test]
    fn maps_linux_dns_and_macos_timeout() {
        let dns = format_ssh_connect_error(
            UiLanguage::Zh,
            "failed to lookup address information: Name or service not known",
        );
        assert!(dns.contains("无法解析"));
        let timeout = format_ssh_connect_error(
            UiLanguage::Zh,
            "TCP connection failed: Operation timed out (os error 60)",
        );
        assert!(timeout.contains("超时"));
        let refused = format_ssh_connect_error(
            UiLanguage::Zh,
            "No connection could be made because the target machine actively refused it. (os error 10061)",
        );
        assert!(refused.contains("拒绝"));
    }

    #[test]
    fn maps_fido2_failures_without_claiming_success() {
        let pin = format_ssh_connect_error(
            UiLanguage::Zh,
            "FIDO2_PIN_INCORRECT: incorrect PIN",
        );
        assert!(pin.contains("PIN"));
        assert!(pin.contains("没有登录成功"));
        assert!(!pin.contains("FIDO2_"));

        let touch = format_ssh_connect_error(
            UiLanguage::En,
            "FIDO2_TOUCH_CANCELLED: user presence check timed out",
        );
        assert!(touch.to_lowercase().contains("touch"));
        assert!(touch.to_lowercase().contains("did not succeed"));

        let missing = format_ssh_connect_error(
            UiLanguage::Zh,
            "FIDO2_NO_SECURITY_KEY: no sk key",
        );
        assert!(missing.contains("安全密钥"));

        let generic = format_ssh_connect_error(
            UiLanguage::En,
            "FIDO2_SIGN_FAILED: agent refused operation",
        );
        assert!(generic.to_lowercase().contains("touch"));
        assert!(generic.to_lowercase().contains("pin"));
        assert!(generic.to_lowercase().contains("not loaded"));

        let agent = format_ssh_connect_error(
            UiLanguage::Zh,
            "FIDO2_AGENT_UNAVAILABLE: socket",
        );
        assert!(agent.contains("Windows"));
        assert!(agent.contains("macOS"));
        assert!(agent.contains("Linux"));

        let file = format_ssh_connect_error(
            UiLanguage::En,
            "FIDO2_KEY_FILE_MISSING: nope",
        );
        assert!(file.to_lowercase().contains("not found"));
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
