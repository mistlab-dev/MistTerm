//! 写入本地 OpenSSH 测试会话到 MistTerm sessions.json（供 UI 与集成测试使用）。

use mistterm::core::session::{SessionConfig, SessionManager};

const HOST: &str = "127.0.0.1";
const USER: &str = "mistterm_test";
const PASS: &str = "mistterm123";
const NAME: &str = "Local Test SSH";

fn main() {
    let mut mgr = SessionManager::new();
    let path = SessionManager::default_storage_path();
    println!("sessions: {}", path.display());

    if let Some((idx, mut cfg)) = mgr
        .list_sessions()
        .iter()
        .enumerate()
        .find(|(_, s)| s.host == HOST && s.username == USER)
        .map(|(i, s)| (i, s.clone()))
    {
        cfg.password = PASS.to_string();
        cfg.use_ssh_agent = false;
        mgr.remove_session(idx);
        mgr.add_session(cfg.clone());
        println!(
            "OK: updated session: {} ({}) use_ssh_agent=false",
            cfg.name, cfg.id
        );
        return;
    }

    let mut cfg = SessionConfig::default();
    cfg.name = NAME.to_string();
    cfg.group = "Test".to_string();
    cfg.host = HOST.to_string();
    cfg.port = 22;
    cfg.username = USER.to_string();
    cfg.password = PASS.to_string();
    cfg.use_ssh_agent = false;

    mgr.add_session(cfg);
    println!("OK: added session \"{NAME}\" -> {USER}@{HOST}");
}
