//! 连接管理 - 管理 SSH 连接状态

use crate::ssh::{SshManager, SshMessage};
use crate::terminal::Terminal;
use crate::core::SessionConfig;
use std::sync::Arc;
use parking_lot::Mutex;

/// 连接状态
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

/// SSH 会话状态
pub struct SshSessionState {
    pub config: SessionConfig,
    pub state: ConnectionState,
    pub terminal: Terminal,
    pub handle: Option<crate::ssh::SshSessionHandle>,
    pub ssh_session_id: Option<usize>,
    /// 已向 SSH 层同步的 PTY 尺寸，与 `terminal` 网格一致
    pub notified_pty: Option<(u32, u32)>,
}

impl SshSessionState {
    /// 创建新的 SSH 会话状态
    pub fn new(config: SessionConfig) -> Self {
        Self {
            config,
            state: ConnectionState::Disconnected,
            terminal: Terminal::new(160, 48),
            handle: None,
            ssh_session_id: None,
            notified_pty: None,
        }
    }

    /// 获取状态文本
    pub fn status_text(&self) -> &'static str {
        match &self.state {
            ConnectionState::Disconnected => "Disconnected",
            ConnectionState::Connecting => "Connecting...",
            ConnectionState::Connected => "Connected",
            ConnectionState::Error(_) => "Error",
        }
    }
}

/// 连接管理器
pub struct ConnectionManager {
    sessions: Vec<Arc<Mutex<SshSessionState>>>,
    ssh_manager: SshManager,
    message_rx: Option<std::sync::mpsc::Receiver<SshMessage>>,
}

impl ConnectionManager {
    fn resolve_session_index(
        &self,
        session_id: usize,
        selected_session: Option<usize>,
    ) -> Option<usize> {
        if let Some((idx, _)) = self
            .sessions
            .iter()
            .enumerate()
            .find(|(_, s)| {
                let sess = s.lock();
                sess.ssh_session_id == Some(session_id)
            })
        {
            Some(idx)
        } else if let Some((idx, _)) = self
            .sessions
            .iter()
            .enumerate()
            .find(|(_, s)| {
                let sess = s.lock();
                sess.handle
                    .as_ref()
                    .map(|h| h.session_id == session_id)
                    .unwrap_or(false)
            })
        {
            Some(idx)
        } else if session_id < self.sessions.len() {
            Some(session_id)
        } else {
            selected_session.filter(|idx| *idx < self.sessions.len())
        }
    }

    /// 创建新的连接管理器
    pub fn new() -> (Self, std::sync::mpsc::Receiver<SshMessage>) {
        let (ssh_manager, message_rx) = SshManager::new();
        let mut manager = Self {
            sessions: Vec::new(),
            ssh_manager,
            message_rx: Some(message_rx),
        };
        
        let rx = manager.message_rx.take().expect("message_rx should be Some");
        (manager, rx)
    }

    /// 添加新会话
    pub fn add_session(&mut self, config: SessionConfig) -> usize {
        let idx = self.sessions.len();
        let state = SshSessionState::new(config);
        self.sessions.push(Arc::new(Mutex::new(state)));
        idx
    }

    /// 获取会话
    pub fn get_session(&self, idx: usize) -> Option<Arc<Mutex<SshSessionState>>> {
        self.sessions.get(idx).cloned()
    }

    /// 获取所有会话
    pub fn get_sessions(&self) -> &[Arc<Mutex<SshSessionState>>] {
        &self.sessions
    }

    /// 获取 SSH 管理器
    pub fn get_ssh_manager(&mut self) -> &mut SshManager {
        &mut self.ssh_manager
    }

    /// 获取消息接收器
    pub fn take_message_rx(&mut self) -> std::sync::mpsc::Receiver<SshMessage> {
        self.message_rx.take().expect("message_rx should be Some")
    }

    /// 处理 SSH 消息
    pub fn handle_ssh_message(&self, msg: SshMessage, _selected_session: Option<usize>) {
        match msg {
            SshMessage::Output { session_id, data } => {
                log::info!(
                    "[SSH-MSG] Session {} output message received ({} bytes)",
                    session_id,
                    data.len()
                );
                if let Some(idx) = self.resolve_session_index(session_id, _selected_session) {
                    if idx != session_id {
                        log::warn!(
                            "[SSH-MSG] Session id {} remapped to ui session {}",
                            session_id,
                            idx
                        );
                    }
                    if let Some(session) = self.sessions.get(idx) {
                        let mut sess = session.lock();
                        sess.terminal.feed(&data);
                    }
                }
            }
            SshMessage::Connected { session_id } => {
                if let Some(idx) = self.resolve_session_index(session_id, _selected_session) {
                    if let Some(session) = self.sessions.get(idx) {
                        let mut sess = session.lock();
                        sess.state = ConnectionState::Connected;
                    }
                }
            }
            SshMessage::Error { session_id, error } => {
                if let Some(idx) = self.resolve_session_index(session_id, _selected_session) {
                    if let Some(session) = self.sessions.get(idx) {
                        let mut sess = session.lock();
                        sess.state = ConnectionState::Error(error);
                    }
                }
            }
            SshMessage::Disconnected { session_id } => {
                if let Some(idx) = self.resolve_session_index(session_id, _selected_session) {
                    if let Some(session) = self.sessions.get(idx) {
                        let mut sess = session.lock();
                        sess.state = ConnectionState::Disconnected;
                        sess.handle = None;
                        sess.notified_pty = None;
                    }
                }
            }
        }
    }

    /// 删除会话
    pub fn remove_session(&mut self, idx: usize) {
        if idx < self.sessions.len() {
            self.sessions.remove(idx);
        }
    }
}

impl Default for ConnectionManager {
    fn default() -> Self {
        let (manager, rx) = Self::new();
        manager
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(name: &str) -> SessionConfig {
        SessionConfig {
            name: name.to_string(),
            host: "127.0.0.1".to_string(),
            port: 22,
            username: "u".to_string(),
            password: "p".to_string(),
        }
    }

    #[test]
    fn resolve_by_explicit_ssh_session_id_first() {
        let mut manager = ConnectionManager::default();
        let idx0 = manager.add_session(make_config("s0"));
        let idx1 = manager.add_session(make_config("s1"));
        assert_eq!(idx0, 0);
        assert_eq!(idx1, 1);

        // 绑定 SSH 会话 ID 到第二个 UI 会话
        if let Some(session) = manager.get_session(idx1) {
            let mut s = session.lock();
            s.ssh_session_id = Some(42);
        }

        let resolved = manager.resolve_session_index(42, Some(idx0));
        assert_eq!(resolved, Some(idx1));
    }

    #[test]
    fn resolve_falls_back_to_selected_session() {
        let mut manager = ConnectionManager::default();
        let idx0 = manager.add_session(make_config("s0"));
        let idx1 = manager.add_session(make_config("s1"));

        let resolved = manager.resolve_session_index(999, Some(idx1));
        assert_eq!(resolved, Some(idx1));

        let resolved_out_of_range = manager.resolve_session_index(999, Some(99));
        assert_eq!(resolved_out_of_range, None);

        // 若 session_id 落在 UI 索引范围内，仍可按索引命中
        let resolved_by_index = manager.resolve_session_index(idx0, None);
        assert_eq!(resolved_by_index, Some(idx0));
    }
}
