//! 写入测试 SSH 会话到 MistTerm sessions.json（供 UI 与集成测试使用）。
//! 环境变量：`MISTTERM_TEST_SSH_HOST` / `USER` / `PASSWORD` / `PORT` / `SESSION`

use mistterm::core::credential::SecretBackend;
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

fn verify_session_password(name: &str, expected: &str) {
    let verify = SessionManager::new();
    let Some(session) = verify.list_sessions().iter().find(|s| s.name == name) else {
        eprintln!("ERROR: session {name:?} missing after save");
        std::process::exit(1);
    };
    if session.password != expected {
        eprintln!(
            "ERROR: password round-trip failed for {name:?} (got len {}, want len {})",
            session.password.len(),
            expected.len()
        );
        std::process::exit(1);
    }
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

    let pick = |sessions: &[SessionConfig]| -> Option<(usize, SessionConfig)> {
        sessions
            .iter()
            .enumerate()
            .find(|(_, s)| s.name == name)
            .map(|(i, s)| (i, s.clone()))
            .or_else(|| {
                sessions
                    .iter()
                    .enumerate()
                    .find(|(_, s)| s.host == host && s.username == user)
                    .map(|(i, s)| (i, s.clone()))
            })
    };

    if let Some((idx, mut cfg)) = pick(mgr.list_sessions()) {
        cfg.name = name.clone();
        cfg.password = pass.clone();
        cfg.port = port;
        cfg.use_ssh_agent = false;
        cfg.secret_backend = SecretBackend::LocalEncrypted;
        mgr.remove_session(idx);
        mgr.add_session(cfg.clone());
        println!(
            "OK: updated session: {} ({}) -> {}@{} use_ssh_agent=false",
            cfg.name, cfg.id, user, host
        );
        verify_session_password(&name, &pass);
        return;
    }

    let mut cfg = SessionConfig::default();
    cfg.name = name.clone();
    cfg.group = "Test".to_string();
    cfg.host = host.clone();
    cfg.port = port;
    cfg.username = user.clone();
    cfg.password = pass.clone();
    cfg.use_ssh_agent = false;
    cfg.secret_backend = SecretBackend::LocalEncrypted;

    mgr.add_session(cfg);
    println!("OK: added session \"{name}\" -> {user}@{host}");
    verify_session_password(&name, &pass);
}
