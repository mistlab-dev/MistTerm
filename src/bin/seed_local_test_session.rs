//! 写入测试 SSH 会话到 MistTerm sessions.json（供 UI 与集成测试使用）。
//! 环境变量：`MISTTERM_TEST_SSH_HOST` / `USER` / `PASSWORD` / `PORT` / `SESSION`

use mistterm::core::session::{SessionConfig, SessionManager};

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn is_localhost(host: &str) -> bool {
    matches!(host.trim(), "127.0.0.1" | "localhost" | "::1")
}

fn main() {
    let host = env_or("MISTTERM_TEST_SSH_HOST", "127.0.0.1");
    let localhost = is_localhost(&host);
    let user = if std::env::var("MISTTERM_TEST_SSH_USER")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .is_some()
    {
        env_or("MISTTERM_TEST_SSH_USER", "root")
    } else if localhost {
        "mistterm_test".to_string()
    } else {
        "root".to_string()
    };
    let pass = env_or("MISTTERM_TEST_SSH_PASSWORD", "mistterm123");
    let name = if std::env::var("MISTTERM_TEST_SSH_SESSION")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .is_some()
    {
        env_or("MISTTERM_TEST_SSH_SESSION", "Local Test SSH")
    } else if localhost {
        "Local Test SSH".to_string()
    } else {
        "Linux Test SSH".to_string()
    };
    let port: u16 = std::env::var("MISTTERM_TEST_SSH_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(22);

    let mut mgr = SessionManager::new();
    let path = SessionManager::default_storage_path();
    println!("sessions: {}", path.display());

    if let Some((idx, mut cfg)) = mgr
        .list_sessions()
        .iter()
        .enumerate()
        .find(|(_, s)| s.host == host && s.username == user)
        .map(|(i, s)| (i, s.clone()))
    {
        cfg.name = name.clone();
        cfg.password = pass.clone();
        cfg.port = port;
        cfg.use_ssh_agent = false;
        mgr.remove_session(idx);
        mgr.add_session(cfg.clone());
        println!(
            "OK: updated session: {} ({}) -> {}@{} use_ssh_agent=false",
            cfg.name, cfg.id, user, host
        );
        return;
    }

    let mut cfg = SessionConfig::default();
    cfg.name = name.clone();
    cfg.group = "Test".to_string();
    cfg.host = host.clone();
    cfg.port = port;
    cfg.username = user.clone();
    cfg.password = pass;
    cfg.use_ssh_agent = false;

    mgr.add_session(cfg);
    println!("OK: added session \"{name}\" -> {user}@{host}");
}
