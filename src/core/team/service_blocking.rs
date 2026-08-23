use super::super::auth::{token_needs_refresh, TeamTokenStore};
use super::super::cache::TeamFragmentCache;
use super::super::client::{TeamApiError, TeamClient};
use super::super::models::{CreateTeamFragmentRequest, TeamFragment, UpdateTeamFragmentRequest};
use super::super::state::TeamState;
use super::service_jobs::do_sync;
use super::TeamService;

const REFRESH_SKEW_SECS: i64 = 120;

pub fn ensure_access_token(api_base: &str, tokens: &TeamTokenStore) -> Result<String, String> {
    let access = tokens
        .load_access_token()
        .map_err(|_| "Not signed in".to_string())?;
    if !token_needs_refresh(&access, REFRESH_SKEW_SECS) {
        return Ok(access);
    }
    force_refresh_access_token(api_base, tokens)
}

fn force_refresh_access_token(api_base: &str, tokens: &TeamTokenStore) -> Result<String, String> {
    let refresh = tokens
        .load_refresh_token()
        .map_err(|_| "Refresh token missing".to_string())?;
    let client = TeamClient::new(api_base).map_err(|e| e.to_string())?;
    let refreshed = client.refresh(&refresh);
    match &refreshed {
        Ok(_) => crate::core::audit::record_audit_blocking(crate::core::audit::AuditEvent::new(
            crate::core::audit::AuditCategory::Auth,
            "team.token_refresh",
            crate::core::audit::AuditOutcome::Success,
        )),
        Err(e) => {
            crate::core::audit::record_audit_blocking(
                crate::core::audit::AuditEvent::new(
                    crate::core::audit::AuditCategory::Auth,
                    "team.token_refresh",
                    crate::core::audit::AuditOutcome::Failure,
                )
                .with_detail(serde_json::json!({ "error": e.to_string() })),
            );
        }
    }
    let refreshed = refreshed.map_err(|e| {
        if e.status == 401 {
            tokens.clear();
            let mut state = TeamState::load();
            state.clear_session();
        }
        e.to_string()
    })?;
    tokens
        .save_tokens(&refreshed.access_token, &refreshed.refresh_token)
        .map_err(|e| e.to_string())?;
    Ok(refreshed.access_token)
}

/// 带 access token 调用 API；遇 401 时强制 refresh 并重试一次。
pub fn with_auth_retry<T, F>(api_base: &str, tokens: &TeamTokenStore, mut f: F) -> Result<T, String>
where
    F: FnMut(&str, &TeamClient) -> Result<T, TeamApiError>,
{
    let access = ensure_access_token(api_base, tokens)?;
    let client = TeamClient::new(api_base).map_err(|e| e.to_string())?;
    match f(&access, &client) {
        Ok(v) => Ok(v),
        Err(e) if e.status == 401 => {
            let access = force_refresh_access_token(api_base, tokens)?;
            f(&access, &client).map_err(|e| e.to_string())
        }
        Err(e) => Err(e.to_string()),
    }
}

/// 同步片段（供 UI 线程在已有 token 时直接调用；失败返回错误文案）。
pub fn sync_fragments_blocking(service: &mut TeamService) -> Result<usize, String> {
    let team_id = service
        .state
        .current_team_id
        .clone()
        .ok_or_else(|| "No team selected".to_string())?;
    let api_base = service.api_base();
    let count = do_sync(&api_base, &team_id, &service.tokens)?;
    service.cache = TeamFragmentCache::load();
    service.state = TeamState::load();
    Ok(count)
}

pub fn create_team_fragment_blocking(
    service: &mut TeamService,
    title: &str,
    command: &str,
    category: Option<&str>,
    status: Option<&str>,
) -> Result<TeamFragment, String> {
    let team_id = service
        .state
        .current_team_id
        .clone()
        .ok_or_else(|| "No team selected".to_string())?;
    if !service.state.current_role().can_edit() {
        return Err("Editor role required".into());
    }
    let api_base = service.api_base();
    let req = CreateTeamFragmentRequest {
        title: title.to_string(),
        command: command.to_string(),
        category: category.map(|s| s.to_string()),
        tags: Some("[]".to_string()),
        variables: Some("{}".to_string()),
        status: status.map(|s| s.to_string()),
    };
    let frag = with_auth_retry(&api_base, &service.tokens, |access, client| {
        client.create_fragment(access, &team_id, &req)
    })
    .map_err(|e| {
        if e.contains("401") {
            service.handle_auth_failure(&e);
        }
        e
    })?;
    service.cache.upsert_fragment(&team_id, frag.clone());
    let _ = service.cache.save();
    Ok(frag)
}

pub fn update_team_fragment_blocking(
    service: &mut TeamService,
    fragment: &TeamFragment,
    title: &str,
    command: &str,
    status: Option<&str>,
) -> Result<TeamFragment, TeamApiError> {
    let api_base = service.api_base();
    let client = TeamClient::new(&api_base).map_err(|e| TeamApiError {
        status: 0,
        message: e,
        conflict_fragment: None,
    })?;
    let req = UpdateTeamFragmentRequest {
        title: title.to_string(),
        command: command.to_string(),
        category: fragment.category.clone(),
        tags: fragment.tags.clone(),
        variables: fragment.variables.clone(),
        status: status
            .map(|s| s.to_string())
            .unwrap_or(fragment.status.clone()),
        revision: fragment.revision,
    };
    let fid = fragment.id.clone();
    let access = ensure_access_token(&api_base, &service.tokens).map_err(|e| TeamApiError {
        status: 0,
        message: e,
        conflict_fragment: None,
    })?;
    match client.update_fragment(&access, &fid, &req) {
        Ok(f) => {
            if let Some(tid) = service.state.current_team_id.clone() {
                service.cache.upsert_fragment(&tid, f.clone());
                let _ = service.cache.save();
            }
            Ok(f)
        }
        Err(e) if e.status == 401 => {
            let access = force_refresh_access_token(&api_base, &service.tokens).map_err(|msg| {
                service.handle_auth_failure(&msg);
                TeamApiError {
                    status: 401,
                    message: msg,
                    conflict_fragment: None,
                }
            })?;
            match client.update_fragment(&access, &fid, &req) {
                Ok(f) => {
                    if let Some(tid) = service.state.current_team_id.clone() {
                        service.cache.upsert_fragment(&tid, f.clone());
                        let _ = service.cache.save();
                    }
                    Ok(f)
                }
                Err(e) => Err(e),
            }
        }
        Err(e) => {
            if e.status == 401 {
                service.handle_auth_failure(&e.message);
            }
            Err(e)
        }
    }
}

pub fn delete_team_fragment_blocking(
    service: &mut TeamService,
    fragment_id: &str,
) -> Result<(), String> {
    if !service.state.current_role().can_delete() {
        return Err("Admin role required".into());
    }
    let team_id = service.state.current_team_id.clone().unwrap_or_default();
    let api_base = service.api_base();
    let fid = fragment_id.to_string();
    with_auth_retry(&api_base, &service.tokens, |access, client| {
        client.delete_fragment(access, &fid).map(|_| ())
    })
    .map_err(|e| {
        if e.contains("401") {
            service.handle_auth_failure(&e);
        }
        e
    })?;
    service.cache.remove_fragment(&team_id, fragment_id);
    let _ = service.cache.save();
    Ok(())
}

/// 锁定团队片段（编辑者+）。成功后乐观更新本地缓存的 `locked_by`。
pub fn lock_team_fragment_blocking(
    service: &mut TeamService,
    fragment_id: &str,
) -> Result<(), String> {
    if !service.state.current_role().can_edit() {
        return Err("Editor role required".into());
    }
    let api_base = service.api_base();
    let fid = fragment_id.to_string();
    with_auth_retry(&api_base, &service.tokens, |access, client| {
        client.lock_fragment(access, &fid)
    })
    .map_err(|e| {
        if e.contains("401") {
            service.handle_auth_failure(&e);
        }
        e
    })?;
    let locker = service
        .state
        .user
        .as_ref()
        .map(|u| u.id.clone())
        .unwrap_or_default();
    if let Some(tid) = service.state.current_team_id.clone() {
        if let Some(mut frag) = service.cache.find_fragment(&tid, fragment_id) {
            frag.locked_by = locker;
            frag.locked_at = Some(chrono::Utc::now().to_rfc3339());
            service.cache.upsert_fragment(&tid, frag);
            let _ = service.cache.save();
        }
    }
    Ok(())
}

/// 解锁团队片段（编辑者+）。成功后清除本地缓存的 `locked_by`。
pub fn unlock_team_fragment_blocking(
    service: &mut TeamService,
    fragment_id: &str,
) -> Result<(), String> {
    if !service.state.current_role().can_edit() {
        return Err("Editor role required".into());
    }
    let api_base = service.api_base();
    let fid = fragment_id.to_string();
    with_auth_retry(&api_base, &service.tokens, |access, client| {
        client.unlock_fragment(access, &fid)
    })
    .map_err(|e| {
        if e.contains("401") {
            service.handle_auth_failure(&e);
        }
        e
    })?;
    if let Some(tid) = service.state.current_team_id.clone() {
        if let Some(mut frag) = service.cache.find_fragment(&tid, fragment_id) {
            frag.locked_by.clear();
            frag.locked_at = None;
            service.cache.upsert_fragment(&tid, frag);
            let _ = service.cache.save();
        }
    }
    Ok(())
}

/// 拉取片段版本历史（viewer+）。
pub fn fetch_fragment_versions_blocking(
    service: &mut TeamService,
    fragment_id: &str,
) -> Result<Vec<super::super::models::FragmentVersion>, String> {
    let api_base = service.api_base();
    let fid = fragment_id.to_string();
    let resp = with_auth_retry(&api_base, &service.tokens, |access, client| {
        client.get_fragment_versions(access, &fid, 50, 0)
    })
    .map_err(|e| {
        if e.contains("401") {
            service.handle_auth_failure(&e);
        }
        e
    })?;
    Ok(resp.versions)
}

/// 创建外部分享链接（编辑者+）。`expires_in_hours` <= 0 表示永不过期。
pub fn create_fragment_share_blocking(
    service: &mut TeamService,
    fragment_id: &str,
    expires_in_hours: i64,
) -> Result<super::super::models::CreateShareResponse, String> {
    if !service.state.current_role().can_edit() {
        return Err("Editor role required".into());
    }
    let api_base = service.api_base();
    let fid = fragment_id.to_string();
    let req = super::super::models::CreateShareRequest { expires_in_hours };
    with_auth_retry(&api_base, &service.tokens, |access, client| {
        client.create_share(access, &fid, &req)
    })
    .map_err(|e| {
        if e.contains("401") {
            service.handle_auth_failure(&e);
        }
        e
    })
}

/// 列出片段的外部分享链接（viewer+）。
pub fn list_fragment_shares_blocking(
    service: &mut TeamService,
    fragment_id: &str,
) -> Result<Vec<super::super::models::ExternalShare>, String> {
    let api_base = service.api_base();
    let fid = fragment_id.to_string();
    let resp = with_auth_retry(&api_base, &service.tokens, |access, client| {
        client.list_shares(access, &fid)
    })
    .map_err(|e| {
        if e.contains("401") {
            service.handle_auth_failure(&e);
        }
        e
    })?;
    Ok(resp.shares)
}

/// 撤销外部分享链接（编辑者+）。
pub fn delete_fragment_share_blocking(
    service: &mut TeamService,
    share_id: &str,
) -> Result<(), String> {
    if !service.state.current_role().can_edit() {
        return Err("Editor role required".into());
    }
    let api_base = service.api_base();
    let sid = share_id.to_string();
    with_auth_retry(&api_base, &service.tokens, |access, client| {
        client.delete_share(access, &sid).map(|_| ())
    })
    .map_err(|e| {
        if e.contains("401") {
            service.handle_auth_failure(&e);
        }
        e
    })
}

/// 拉取团队服务端设置（viewer+）。
pub fn fetch_team_settings_blocking(
    service: &mut TeamService,
) -> Result<super::super::models::TeamSettings, String> {
    let team_id = service
        .state
        .current_team_id
        .clone()
        .ok_or_else(|| "No team selected".to_string())?;
    let api_base = service.api_base();
    with_auth_retry(&api_base, &service.tokens, |access, client| {
        client.get_team_settings(access, &team_id)
    })
    .map_err(|e| {
        if e.contains("401") {
            service.handle_auth_failure(&e);
        }
        e
    })
}

/// 更新团队服务端设置（管理员）。
pub fn update_team_settings_blocking(
    service: &mut TeamService,
    settings: &super::super::models::TeamSettings,
) -> Result<super::super::models::TeamSettings, String> {
    if !service.state.current_role().can_delete() {
        return Err("Admin role required".into());
    }
    let team_id = service
        .state
        .current_team_id
        .clone()
        .ok_or_else(|| "No team selected".to_string())?;
    let api_base = service.api_base();
    with_auth_retry(&api_base, &service.tokens, |access, client| {
        client.update_team_settings(access, &team_id, settings)
    })
    .map_err(|e| {
        if e.contains("401") {
            service.handle_auth_failure(&e);
        }
        e
    })
}

/// 拉取命令审计 Agent 列表。
pub fn list_cmd_audit_agents_blocking(
    service: &mut TeamService,
) -> Result<Vec<super::super::models::CmdAuditAgent>, String> {
    let team_id = service
        .state
        .current_team_id
        .clone()
        .ok_or_else(|| "No team selected".to_string())?;
    let api_base = service.api_base();
    with_auth_retry(&api_base, &service.tokens, |access, client| {
        client.list_cmd_audit_agents(access, &team_id)
    })
    .map(|r| r.agents)
    .map_err(|e| {
        if e.contains("401") {
            service.handle_auth_failure(&e);
        }
        e
    })
}

/// 启用/禁用命令审计 Agent（管理员）。
pub fn update_cmd_audit_agent_blocking(
    service: &mut TeamService,
    agent_id: &str,
    enabled: bool,
) -> Result<super::super::models::CmdAuditAgent, String> {
    if !service.state.current_role().can_delete() {
        return Err("Admin role required".into());
    }
    let team_id = service
        .state
        .current_team_id
        .clone()
        .ok_or_else(|| "No team selected".to_string())?;
    let api_base = service.api_base();
    let aid = agent_id.to_string();
    with_auth_retry(&api_base, &service.tokens, |access, client| {
        client.update_cmd_audit_agent(access, &team_id, &aid, enabled)
    })
    .map_err(|e| {
        if e.contains("401") {
            service.handle_auth_failure(&e);
        }
        e
    })
}

/// 拉取团队存储用量。
pub fn fetch_storage_usage_blocking(
    service: &mut TeamService,
) -> Result<super::super::models::StorageUsageResponse, String> {
    let team_id = service
        .state
        .current_team_id
        .clone()
        .ok_or_else(|| "No team selected".to_string())?;
    let api_base = service.api_base();
    with_auth_retry(&api_base, &service.tokens, |access, client| {
        client.get_storage_usage(access, &team_id)
    })
    .map_err(|e| {
        if e.contains("401") {
            service.handle_auth_failure(&e);
        }
        e
    })
}

/// 启动/登录后：释放当前用户在本地缓存中仍持有的残留编辑锁。
pub fn release_residual_fragment_locks_blocking(service: &mut TeamService) -> usize {
    let Some(uid) = service.state.user.as_ref().map(|u| u.id.clone()) else {
        return 0;
    };
    if uid.is_empty() || !service.state.current_role().can_edit() {
        return 0;
    }
    let Some(tid) = service.state.current_team_id.clone() else {
        return 0;
    };
    let candidates: Vec<String> = service
        .cache
        .fragments_for_team(&tid)
        .iter()
        .filter(|f| f.locked_by == uid)
        .map(|f| f.id.clone())
        .collect();
    if candidates.is_empty() {
        return 0;
    }
    let api_base = service.api_base();
    let mut released = 0usize;
    for fid in candidates {
        let still_mine = with_auth_retry(&api_base, &service.tokens, |access, client| {
            client.get_fragment(access, &fid)
        });
        match still_mine {
            Ok(remote) if remote.locked_by == uid => {
                if unlock_team_fragment_blocking(service, &fid).is_ok() {
                    released += 1;
                }
            }
            Ok(remote) => {
                if let Some(mut frag) = service.cache.find_fragment(&tid, &fid) {
                    frag.locked_by = remote.locked_by;
                    frag.locked_at = remote.locked_at;
                    service.cache.upsert_fragment(&tid, frag);
                    let _ = service.cache.save();
                }
            }
            Err(_) => {}
        }
    }
    released
}
