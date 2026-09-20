//! SessionConfig → SshConfig 的共享构建（CLI / GUI 共用，避免 keepalive / 转发字段漂移）。

use crate::core::secret_resolver::SecretResolver;
use crate::core::session::{
    parse_dynamic_forwards_text, parse_local_forwards_text, parse_remote_forwards_text,
    SessionConfig, SessionManager,
};
use crate::ssh::{parse_jump_chain, parse_jump_endpoint, JumpHop, SshConfig};

/// 由会话字段推导 keepalive（不套用应用级默认值）。
pub fn keepalive_from_session(session: &SessionConfig) -> (bool, u32, u8) {
    if !session.keepalive_enabled {
        return (false, 0, session.keepalive_count_max.max(1));
    }
    (
        true,
        session.keepalive_interval_secs,
        session.keepalive_count_max.max(1),
    )
}

/// 用已解析凭据 + 跳板 + keepalive 组装 [`SshConfig`]。
pub fn build_ssh_config(
    session: &SessionConfig,
    password: String,
    private_key_path: String,
    jump_hops: Vec<JumpHop>,
    keepalive_enabled: bool,
    keepalive_interval_secs: u32,
    keepalive_count_max: u8,
) -> SshConfig {
    let interval = if keepalive_enabled {
        keepalive_interval_secs.max(1)
    } else {
        0
    };
    SshConfig {
        host: session.host.clone(),
        port: session.port,
        username: session.username.clone(),
        password,
        private_key_path,
        use_ssh_agent: session.use_ssh_agent,
        keepalive_interval_secs: interval,
        keepalive_count_max: keepalive_count_max.max(1),
        proxy_jump: session.proxy_jump.clone(),
        proxy_command: session.proxy_command.clone(),
        jump_hops,
        local_forwards: parse_local_forwards_text(&session.local_forwards_text),
        remote_forwards: parse_remote_forwards_text(&session.remote_forwards_text),
        dynamic_forwards: parse_dynamic_forwards_text(&session.dynamic_forwards_text),
    }
}

/// 解析 ProxyJump 链（匹配已保存会话名/主机，或 `user@host:port`）。
pub fn resolve_proxy_jump_hops(
    session: &SessionConfig,
    sessions: &SessionManager,
    resolver: &SecretResolver,
) -> Result<Vec<JumpHop>, String> {
    let chain = parse_jump_chain(&session.proxy_jump);
    if chain.is_empty() {
        return Ok(Vec::new());
    }
    let mut hops = Vec::with_capacity(chain.len());
    for token in &chain {
        if let Some(js) = sessions.find_session_for_jump_token(token) {
            let resolved = resolver
                .resolve_session(js)
                .map_err(|e| format!("{} ({}): {}", token, js.name, e))?;
            hops.push(JumpHop {
                host: js.host.clone(),
                port: js.port,
                username: js.username.clone(),
                password: resolved.password,
                private_key_path: resolved.private_key_path,
                use_ssh_agent: js.use_ssh_agent,
            });
        } else {
            let ep = parse_jump_endpoint(token, &session.username)?;
            hops.push(JumpHop {
                host: ep.host,
                port: ep.port,
                username: ep.username,
                password: String::new(),
                private_key_path: String::new(),
                use_ssh_agent: session.use_ssh_agent,
            });
        }
    }
    Ok(hops)
}

/// 一次解析凭据 + 跳板并构建配置（CLI / 无应用默认 keepalive 场景）。
pub fn session_to_ssh_config(
    session: &SessionConfig,
    sessions: &SessionManager,
    resolver: &SecretResolver,
) -> Result<SshConfig, String> {
    let resolved = resolver
        .resolve_session(session)
        .map_err(|e| e.to_string())?;
    let jump_hops = resolve_proxy_jump_hops(session, sessions, resolver)?;
    let (ka_on, ka_int, ka_max) = keepalive_from_session(session);
    Ok(build_ssh_config(
        session,
        resolved.password,
        resolved.private_key_path,
        jump_hops,
        ka_on,
        ka_int,
        ka_max,
    ))
}
