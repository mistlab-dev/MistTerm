use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use super::super::auth::TeamTokenStore;
use super::super::cache::TeamFragmentCache;
use super::super::client::TeamClient;
use super::super::models::{TeamInfo, TeamMember, TeamMembership, TeamUser};
use super::super::oauth::{run_browser_oauth, OAuthProvider};
use super::super::state::TeamState;
use super::service_blocking::{ensure_access_token, with_auth_retry};
use super::TeamAsyncResult;

pub(super) enum TeamJob {
    Login {
        api_base: String,
        identifier: String,
        password: String,
        use_email: bool,
    },
    Register {
        api_base: String,
        email: String,
        username: String,
        password: String,
    },
    Sync {
        api_base: String,
        team_id: String,
    },
    RefreshTeams {
        api_base: String,
    },
    ConfigSync {
        api_base: String,
    },
    TeamDetail {
        api_base: String,
        team_id: String,
    },
    ListMembers {
        api_base: String,
        team_id: String,
    },
    CmdAuditSync {
        api_base: String,
        team_id: String,
    },
    OAuth {
        api_base: String,
        provider: OAuthProvider,
        cancel: Arc<AtomicBool>,
    },
}

pub(super) fn run_job(job: TeamJob, tokens: &TeamTokenStore) -> TeamAsyncResult {
    match job {
        TeamJob::Login {
            api_base,
            identifier,
            password,
            use_email,
        } => match do_login(&api_base, &identifier, &password, use_email, tokens) {
            Ok((user, teams)) => TeamAsyncResult::LoginOk { user, teams },
            Err(e) => TeamAsyncResult::Err(e),
        },
        TeamJob::Register {
            api_base,
            email,
            username,
            password,
        } => match do_register(&api_base, &email, &username, &password) {
            Ok(msg) => TeamAsyncResult::RegisterOk { message: msg },
            Err(e) => TeamAsyncResult::Err(e),
        },
        TeamJob::Sync { api_base, team_id } => match do_sync(&api_base, &team_id, tokens) {
            Ok(count) => TeamAsyncResult::SyncOk { team_id, count },
            Err(e) => TeamAsyncResult::Err(e),
        },
        TeamJob::OAuth {
            api_base,
            provider,
            cancel,
        } => match do_oauth(&api_base, provider, &cancel, tokens) {
            Ok((user, teams)) => TeamAsyncResult::LoginOk { user, teams },
            Err(e) => TeamAsyncResult::Err(e),
        },
        TeamJob::ConfigSync { api_base } => match do_team_config_sync(&api_base, tokens) {
            Ok((team_count, server_count)) => TeamAsyncResult::ConfigSyncOk {
                team_count,
                server_count,
            },
            Err(e) => TeamAsyncResult::Err(e),
        },
        TeamJob::TeamDetail { api_base, team_id } => {
            match do_team_detail(&api_base, &team_id, tokens) {
                Ok(info) => TeamAsyncResult::TeamDetailOk { info },
                Err(e) => TeamAsyncResult::Err(e),
            }
        }
        TeamJob::ListMembers { api_base, team_id } => {
            match do_list_team_members(&api_base, &team_id, tokens) {
                Ok(members) => TeamAsyncResult::MembersOk { members },
                Err(e) => TeamAsyncResult::MembersErr { message: e },
            }
        }
        TeamJob::CmdAuditSync { api_base, team_id } => {
            match do_cmd_audit_sync(&api_base, &team_id, tokens) {
                Ok(payload) => TeamAsyncResult::CmdAuditSyncOk { payload },
                Err(e) => TeamAsyncResult::Err(e),
            }
        }
        TeamJob::RefreshTeams { api_base } => match do_refresh_teams(&api_base, tokens) {
            Ok(teams) => {
                // 返回 LoginOk 形态以便复用 UI 更新 teams 列表
                let user = match tokens
                    .load_access_token()
                    .ok()
                    .and_then(|t| TeamClient::new(&api_base).ok().and_then(|c| c.me(&t).ok()))
                {
                    Some(u) => u,
                    None => TeamUser {
                        id: String::new(),
                        email: String::new(),
                        username: String::new(),
                        display_name: String::new(),
                        email_verified: false,
                        created_at: None,
                        updated_at: None,
                    },
                };
                TeamAsyncResult::LoginOk { user, teams }
            }
            Err(e) => TeamAsyncResult::Err(e),
        },
    }
}

fn complete_token_login(
    api_base: &str,
    token_resp: super::super::models::TokenResponse,
    tokens: &TeamTokenStore,
) -> Result<(TeamUser, Vec<TeamMembership>), String> {
    let client = TeamClient::new(api_base).map_err(|e| e.to_string())?;
    tokens
        .save_tokens(&token_resp.access_token, &token_resp.refresh_token)
        .map_err(|e| e.to_string())?;
    let user = client
        .me(&token_resp.access_token)
        .unwrap_or(token_resp.user);
    let teams = client
        .list_teams(&token_resp.access_token)
        .map_err(|e| e.to_string())?
        .teams;
    Ok((user, teams))
}

fn do_oauth(
    api_base: &str,
    provider: OAuthProvider,
    cancel: &Arc<AtomicBool>,
    tokens: &TeamTokenStore,
) -> Result<(TeamUser, Vec<TeamMembership>), String> {
    let token_resp = run_browser_oauth(api_base, provider, Arc::clone(cancel))?;
    complete_token_login(api_base, token_resp, tokens)
}

fn do_login(
    api_base: &str,
    identifier: &str,
    password: &str,
    use_email: bool,
    tokens: &TeamTokenStore,
) -> Result<(TeamUser, Vec<TeamMembership>), String> {
    let client = TeamClient::new(api_base).map_err(|e| e.to_string())?;
    let token_resp = if use_email {
        client.login_email(identifier, password)
    } else {
        client.login_username(identifier, password)
    }
    .map_err(|e| e.to_string())?;
    complete_token_login(api_base, token_resp, tokens)
}

fn do_register(
    api_base: &str,
    email: &str,
    username: &str,
    password: &str,
) -> Result<String, String> {
    let client = TeamClient::new(api_base).map_err(|e| e.to_string())?;
    let resp = client
        .register(email, username, None, password)
        .map_err(|e| e.to_string())?;
    Ok(if resp.message.is_empty() {
        "Account created. Please sign in.".into()
    } else {
        resp.message
    })
}

fn do_refresh_teams(
    api_base: &str,
    tokens: &TeamTokenStore,
) -> Result<Vec<TeamMembership>, String> {
    let access = ensure_access_token(api_base, tokens)?;
    let client = TeamClient::new(api_base).map_err(|e| e.to_string())?;
    client
        .list_teams(&access)
        .map(|r| r.teams)
        .map_err(|e| e.to_string())
}

fn do_team_detail(
    api_base: &str,
    team_id: &str,
    tokens: &TeamTokenStore,
) -> Result<TeamInfo, String> {
    with_auth_retry(api_base, tokens, |access, client| {
        client.get_team(access, team_id)
    })
}

fn do_list_team_members(
    api_base: &str,
    team_id: &str,
    tokens: &TeamTokenStore,
) -> Result<Vec<TeamMember>, String> {
    let resp = with_auth_retry(api_base, tokens, |access, client| {
        client.list_team_members(access, team_id)
    })?;
    Ok(resp.members)
}

fn do_cmd_audit_sync(
    api_base: &str,
    team_id: &str,
    tokens: &TeamTokenStore,
) -> Result<crate::core::cmd_audit::CmdAuditSyncPayload, String> {
    with_auth_retry(api_base, tokens, |access, client| {
        client.cmd_audit_sync(access, team_id)
    })
}

pub(super) fn do_cmd_audit_report_alert(
    api_base: &str,
    team_id: &str,
    request: &crate::core::cmd_audit::CmdAuditAlertRequest,
    tokens: &TeamTokenStore,
) -> Result<(), String> {
    with_auth_retry(api_base, tokens, |access, client| {
        client.cmd_audit_report_alert(access, team_id, request)
    })
}

pub(super) fn do_report_fragment_usage(
    api_base: &str,
    team_id: &str,
    fragment_id: &str,
    success: bool,
    duration_ms: u64,
    tokens: &TeamTokenStore,
) -> Result<(), String> {
    match with_auth_retry(api_base, tokens, |access, client| {
        client.report_fragment_usage(access, team_id, fragment_id, success, duration_ms)
    }) {
        Ok(()) => Ok(()),
        Err(e) if e.contains("404") || e.contains("Not Found") => Ok(()),
        Err(e) => {
            log::debug!("fragment usage report: {}", e);
            Ok(())
        }
    }
}

fn do_team_config_sync(api_base: &str, tokens: &TeamTokenStore) -> Result<(usize, usize), String> {
    let resp = match with_auth_retry(api_base, tokens, |access, client| {
        client.sync_team_config(access)
    }) {
        Ok(r) => r,
        Err(e) if e.contains("404") || e.contains("Not Found") => {
            super::super::models::TeamSyncResponse { teams: vec![] }
        }
        Err(e) => return Err(e),
    };
    let team_count = resp.teams.len();
    let server_count: usize = resp.teams.iter().map(|t| t.servers.len()).sum();
    let mut state = TeamState::load();
    super::super::sync_config::apply_sync_response(&mut state, &resp);
    Ok((team_count, server_count))
}

pub fn do_sync(api_base: &str, team_id: &str, tokens: &TeamTokenStore) -> Result<usize, String> {
    let mut state = TeamState::load();
    let cursor = state.cursor_for(team_id);
    let resp = with_auth_retry(api_base, tokens, |access, client| {
        client.sync_fragments(access, team_id, &cursor, 500)
    })
    .map_err(|e| e.to_string())?;
    let count = resp.fragments.len() + resp.deleted_ids.len();
    let mut cache = TeamFragmentCache::load();
    cache.apply_sync(team_id, &resp);
    state.set_cursor(team_id, resp.cursor);
    state.last_sync_unix = Some(chrono::Utc::now().timestamp());
    state.last_error.clear();
    let _ = state.save();
    let _ = cache.save();
    Ok(count)
}
