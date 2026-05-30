//! 团队平台编排：认证、团队列表、片段同步（后台线程 + 通道回传）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use super::auth::{token_needs_refresh, TeamTokenStore};
use super::cache::TeamFragmentCache;
use super::client::{TeamApiError, TeamClient};
use super::models::{
    CreateTeamFragmentRequest, TeamFragment, TeamInfo, TeamMember, TeamMembership, TeamUser,
    UpdateTeamFragmentRequest,
};
use super::oauth::{run_browser_oauth, OAuthProvider};
use super::settings::TeamSettings;
use super::state::TeamState;

const REFRESH_SKEW_SECS: i64 = 120;

#[derive(Debug, Clone)]
pub enum TeamAsyncResult {
    LoginOk {
        user: TeamUser,
        teams: Vec<TeamMembership>,
    },
    RegisterOk {
        message: String,
    },
    SyncOk {
        team_id: String,
        count: usize,
    },
    ConfigSyncOk {
        team_count: usize,
        server_count: usize,
    },
    TeamDetailOk {
        info: TeamInfo,
    },
    MembersOk {
        members: Vec<TeamMember>,
    },
    MembersErr {
        message: String,
    },
    CmdAuditSyncOk {
        payload: crate::core::cmd_audit::CmdAuditSyncPayload,
    },
    CreateFragmentOk(TeamFragment),
    UpdateFragmentOk(TeamFragment),
    DeleteFragmentOk {
        fragment_id: String,
    },
    Err(String),
}

enum TeamJob {
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

pub struct TeamService {
    pub settings: TeamSettings,
    pub state: TeamState,
    pub cache: TeamFragmentCache,
    tokens: TeamTokenStore,
    rx: Option<Receiver<TeamAsyncResult>>,
    busy: bool,
    last_auto_sync: Option<Instant>,
    pub status_line: String,
    pending_initial_sync: bool,
    /// 刷新 token 失败，需重新登录
    pub auth_expired: bool,
    /// 当前团队详情缓存（描述等）
    pub current_team_detail: Option<super::models::TeamInfo>,
    pub team_members: Vec<TeamMember>,
    pub team_members_error: Option<String>,
    pub pending_audit_login: bool,
    pub pending_audit_sync: bool,
    pending_vault_apply: bool,
    pub pending_fragment_sync_after_config: bool,
    /// 等忙完之后再去拉 team detail
    pending_team_detail: bool,
    oauth_cancel: Arc<AtomicBool>,
    pending_cmd_audit_payload: Option<crate::core::cmd_audit::CmdAuditSyncPayload>,
}

impl TeamService {
    pub fn new(mut settings: TeamSettings) -> Self {
        settings.lock_to_product_defaults();
        Self {
            settings,
            state: TeamState::load(),
            cache: TeamFragmentCache::load(),
            tokens: TeamTokenStore::default(),
            rx: None,
            busy: false,
            // 把首次"到期"自动同步推迟到 frequency_minutes 后，避免启动瞬间打一连串请求。
            // 登录 / 切团队仍会通过 pending_initial_sync / spawn_config_sync 主动触发同步。
            last_auto_sync: Some(Instant::now()),
            status_line: String::new(),
            pending_initial_sync: false,
            auth_expired: false,
            current_team_detail: None,
            team_members: Vec::new(),
            team_members_error: None,
            pending_audit_login: false,
            pending_audit_sync: false,
            pending_vault_apply: false,
            pending_fragment_sync_after_config: false,
            pending_team_detail: false,
            oauth_cancel: Arc::new(AtomicBool::new(false)),
            pending_cmd_audit_payload: None,
        }
    }

    pub fn take_cmd_audit_sync_payload(&mut self) -> Option<crate::core::cmd_audit::CmdAuditSyncPayload> {
        self.pending_cmd_audit_payload.take()
    }

    pub fn spawn_cmd_audit_sync(&mut self) {
        if self.busy || !self.is_logged_in() {
            return;
        }
        let Some(team_id) = self.state.current_team_id.clone() else {
            return;
        };
        self.spawn_job(TeamJob::CmdAuditSync {
            api_base: self.api_base(),
            team_id,
        });
    }

    /// 命令审计告警上报（不占用 `busy`，避免阻塞其它团队任务）
    pub fn spawn_cmd_audit_report_alert(
        &self,
        team_id: &str,
        request: crate::core::cmd_audit::CmdAuditAlertRequest,
    ) {
        if !self.is_logged_in() || team_id.is_empty() {
            return;
        }
        let api_base = self.api_base();
        let team_id = team_id.to_string();
        thread::spawn(move || {
            let tokens = TeamTokenStore::default();
            let _ = do_cmd_audit_report_alert(&api_base, &team_id, &request, &tokens);
        });
    }

    pub fn take_pending_initial_sync(&mut self) -> bool {
        std::mem::take(&mut self.pending_initial_sync)
    }

    pub fn take_pending_vault_apply(&mut self) -> bool {
        std::mem::take(&mut self.pending_vault_apply)
    }

    pub fn current_team_servers(&self) -> Vec<super::models::TeamServer> {
        let Some(tid) = self.state.current_team_id.as_deref() else {
            return Vec::new();
        };
        let mut servers = self.state.servers_for_team(tid);
        servers.sort_by_key(|s| s.sort_order);
        servers
    }

    pub fn reload_settings(&mut self, settings: TeamSettings) {
        let mut settings = settings;
        settings.lock_to_product_defaults();
        self.settings = settings;
    }

    pub fn is_configured(&self) -> bool {
        self.settings.is_configured()
    }

    pub fn is_logged_in(&self) -> bool {
        self.state.user.is_some() && self.tokens.has_tokens()
    }

    pub fn api_base(&self) -> String {
        self.settings.normalized_api_base()
    }

    pub fn audit_events_url(&self) -> String {
        format!("{}/v1/audit/events", self.api_base())
    }

    pub fn current_access_token(&self) -> Option<String> {
        self.tokens.load_access_token().ok()
    }

    pub fn logout(&mut self) {
        self.tokens.clear();
        self.state.clear_session();
        self.current_team_detail = None;
        self.team_members.clear();
        self.team_members_error = None;
        self.auth_expired = false;
        self.status_line = "Logged out".into();
    }

    pub fn current_team_name(&self) -> String {
        self.state
            .current_membership()
            .map(|m| m.team.name.clone())
            .unwrap_or_else(|| "Team".to_string())
    }

    pub fn find_team_fragment(&self, fragment_id: &str) -> Option<TeamFragment> {
        let team_id = self.state.current_team_id.as_deref()?;
        self.cache.find_fragment(team_id, fragment_id)
    }

    /// 异步刷新当前团队详情；在后台线程中跑 HTTP，不阻塞 UI。
    /// 如果当前已有任务在跑，会留下 pending 标记，等 poll() 中 busy 清掉后再触发。
    pub fn spawn_list_team_members(&mut self) {
        if !self.is_logged_in() {
            self.team_members_error = Some("Not signed in".into());
            return;
        }
        let Some(team_id) = self.state.current_team_id.clone() else {
            self.team_members_error = Some("No team selected".into());
            return;
        };
        if self.busy {
            return;
        }
        self.team_members_error = None;
        self.spawn_job(TeamJob::ListMembers {
            api_base: self.api_base(),
            team_id,
        });
        self.status_line = "Loading team members…".into();
    }

    pub fn spawn_refresh_current_team_detail(&mut self) {
        if !self.is_logged_in() {
            return;
        }
        let Some(team_id) = self.state.current_team_id.clone() else {
            return;
        };
        if self.busy {
            self.pending_team_detail = true;
            return;
        }
        self.spawn_job(TeamJob::TeamDetail {
            api_base: self.api_base(),
            team_id,
        });
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    pub fn spawn_login(
        &mut self,
        identifier: String,
        password: String,
        use_email: bool,
    ) {
        if self.busy {
            return;
        }
        let api_base = self.api_base();
        self.spawn_job(TeamJob::Login {
            api_base,
            identifier,
            password,
            use_email,
        });
        self.status_line = "Signing in…".into();
    }

    pub fn spawn_oauth_login(&mut self, provider: OAuthProvider) {
        if self.busy {
            return;
        }
        self.oauth_cancel.store(false, Ordering::Relaxed);
        let api_base = self.api_base();
        let cancel = Arc::clone(&self.oauth_cancel);
        self.spawn_job(TeamJob::OAuth {
            api_base,
            provider,
            cancel,
        });
        self.status_line = match provider {
            OAuthProvider::Google => {
                "① 已在浏览器打开 Google 授权；② 完成后应看到「登录成功」页；③ 若只进了控制台，请点「取消」后重试。"
            }
            OAuthProvider::Github => {
                "① 已在浏览器打开 GitHub 授权；② 完成后应看到「登录成功」页；③ 若只进了控制台，请点「取消」后重试。"
            }
        }
        .into();
    }

    pub fn cancel_oauth_login(&mut self) {
        self.oauth_cancel.store(true, Ordering::Relaxed);
        if self.busy {
            self.status_line = "正在取消…".into();
        }
    }

    pub fn spawn_register(
        &mut self,
        email: String,
        username: String,
        password: String,
    ) {
        if self.busy {
            return;
        }
        let api_base = self.api_base();
        self.spawn_job(TeamJob::Register {
            api_base,
            email,
            username,
            password,
        });
        self.status_line = "Registering…".into();
    }

    pub fn spawn_sync_current_team(&mut self) {
        let Some(team_id) = self.state.current_team_id.clone() else {
            self.state.last_error = "No team selected".into();
            return;
        };
        self.spawn_sync_team(&team_id);
    }

    pub fn spawn_sync_team(&mut self, team_id: &str) {
        if self.busy || !self.is_logged_in() {
            return;
        }
        self.spawn_job(TeamJob::Sync {
            api_base: self.api_base(),
            team_id: team_id.to_string(),
        });
        self.status_line = "Syncing team fragments…".into();
    }

    pub fn spawn_refresh_teams(&mut self) {
        if self.busy || !self.is_logged_in() {
            return;
        }
        self.spawn_job(TeamJob::RefreshTeams {
            api_base: self.api_base(),
        });
    }

    pub fn set_current_team(&mut self, team_id: String) {
        self.state.current_team_id = Some(team_id.clone());
        self.current_team_detail = None;
        let _ = self.state.save();
        self.pending_vault_apply = true;
        // ConfigSync 会在 busy 清掉后由 poll() 收尾时触发，TeamDetail 标 pending 让 poll() 串行触发。
        self.pending_team_detail = true;
        self.spawn_config_sync();
    }

    pub fn spawn_config_sync(&mut self) {
        if self.busy || !self.is_logged_in() {
            return;
        }
        self.spawn_job(TeamJob::ConfigSync {
            api_base: self.api_base(),
        });
        self.status_line = "Syncing team config…".into();
    }

    pub fn handle_auth_failure(&mut self, message: &str) {
        self.tokens.clear();
        self.state.clear_session();
        self.current_team_detail = None;
        self.auth_expired = true;
        self.state.last_error = message.to_string();
        self.status_line = "Session expired — sign in again".into();
        let _ = self.state.save();
    }

    /// 在 UI 帧中调用：处理异步结果、按需自动同步。
    pub fn poll(&mut self, frequency_minutes: u32) -> bool {
        let mut changed = false;
        let mut inbox = Vec::new();
        if let Some(rx) = &self.rx {
            while let Ok(msg) = rx.try_recv() {
                inbox.push(msg);
            }
        }
        for msg in inbox {
                changed = true;
                self.busy = false;
                match msg {
                    TeamAsyncResult::LoginOk { user, teams } => {
                        self.state.user = Some(user);
                        self.state.teams = teams;
                        self.state.ensure_default_team();
                        self.state.last_error.clear();
                        self.status_line = "Signed in".into();
                        self.pending_initial_sync = true;
                        self.pending_audit_login = true;
                        let _ = self.state.save();
                    }
                    TeamAsyncResult::RegisterOk { message } => {
                        self.status_line = message;
                    }
                    TeamAsyncResult::ConfigSyncOk {
                        team_count,
                        server_count,
                    } => {
                        self.state = TeamState::load();
                        self.state.last_error.clear();
                        self.status_line = format!(
                            "Team config synced ({team_count} teams, {server_count} servers)"
                        );
                        self.pending_vault_apply = true;
                        self.pending_fragment_sync_after_config = true;
                        let _ = (team_count, server_count);
                    }
                    TeamAsyncResult::SyncOk { team_id, count } => {
                        self.state.last_sync_unix = Some(chrono::Utc::now().timestamp());
                        self.state.last_error.clear();
                        self.status_line = format!("Synced {count} fragment change(s)");
                        self.cache = TeamFragmentCache::load();
                        self.state = TeamState::load();
                        let _ = self.state.save();
                        let _ = team_id;
                        self.auth_expired = false;
                        self.pending_audit_sync = true;
                    }
                    TeamAsyncResult::CreateFragmentOk(frag) => {
                        if let Some(tid) = self.state.current_team_id.clone() {
                            self.cache.upsert_fragment(&tid, frag);
                            let _ = self.cache.save();
                        }
                        self.status_line = "Fragment created".into();
                    }
                    TeamAsyncResult::UpdateFragmentOk(frag) => {
                        if let Some(tid) = self.state.current_team_id.clone() {
                            self.cache.upsert_fragment(&tid, frag);
                            let _ = self.cache.save();
                        }
                        self.status_line = "Fragment updated".into();
                    }
                    TeamAsyncResult::DeleteFragmentOk { .. } => {
                        self.status_line = "Fragment deleted".into();
                    }
                    TeamAsyncResult::TeamDetailOk { info } => {
                        self.current_team_detail = Some(info);
                    }
                    TeamAsyncResult::MembersOk { members } => {
                        self.team_members = members;
                        self.team_members_error = None;
                        self.status_line.clear();
                    }
                    TeamAsyncResult::MembersErr { message } => {
                        self.team_members.clear();
                        self.team_members_error = Some(message);
                        self.status_line.clear();
                    }
                    TeamAsyncResult::CmdAuditSyncOk { payload } => {
                        self.pending_cmd_audit_payload = Some(payload);
                    }
                    TeamAsyncResult::Err(e) => {
                        if e.contains("401") || e.contains("Not signed in") {
                            self.handle_auth_failure(&e);
                        } else {
                            self.state.last_error = e.clone();
                            self.status_line = e;
                            let _ = self.state.save();
                        }
                    }
                }
        }

        if !self.busy && self.pending_team_detail && self.is_logged_in() {
            self.pending_team_detail = false;
            self.spawn_refresh_current_team_detail();
        }

        if self.is_logged_in()
            && frequency_minutes > 0
            && !self.busy
            && self.state.current_team_id.is_some()
        {
            let interval = Duration::from_secs(frequency_minutes as u64 * 60);
            let due = self
                .last_auto_sync
                .map(|t| t.elapsed() >= interval)
                .unwrap_or(true);
            if due {
                self.last_auto_sync = Some(Instant::now());
                self.spawn_sync_current_team();
            }
        }
        changed
    }

    pub fn team_fragments_as_stats(&self) -> Vec<crate::core::FragmentStats> {
        let Some(tid) = self.state.current_team_id.as_deref() else {
            return Vec::new();
        };
        let name = self.current_team_name();
        self.cache.to_fragment_stats(tid, &name)
    }

    pub fn record_fragment_usage(&mut self, fragment_id: &str, success: bool, dur_ms: u64) {
        self.cache.record_usage(fragment_id, success, dur_ms);
        let _ = self.cache.save();
    }

    /// 尝试拉取团队分析 API 并合并到本地 overlay；失败静默。
    pub fn refresh_fragment_analytics_from_api(&mut self) -> bool {
        let Some(tid) = self.state.current_team_id.clone() else {
            return false;
        };
        let api_base = self.api_base();
        let Ok(token) = ensure_access_token(&api_base, &self.tokens) else {
            return false;
        };
        let client = match TeamClient::new(&api_base) {
            Ok(c) => c,
            Err(_) => return false,
        };
        match client.fetch_fragment_analytics(&token, &tid) {
            Ok(Some(resp)) => {
                self.cache.apply_analytics_rows(&resp.fragments);
                let _ = self.cache.save();
                true
            }
            Ok(None) => false,
            Err(e) => {
                log::debug!("fragment analytics API: {}", e);
                false
            }
        }
    }

    pub fn build_fragment_analytics_dashboard(
        &mut self,
        personal: &[crate::core::FragmentStats],
        range: crate::core::FragmentAnalyticsTimeRange,
        usage_log: &crate::core::FragmentUsageLog,
    ) -> crate::core::FragmentAnalyticsDashboard {
        let api_ok = self.refresh_fragment_analytics_from_api();
        let team_all = self.team_fragments_as_stats();
        if range.cutoff_unix().is_some() {
            let team_id = self.state.current_team_id.clone();
            return crate::core::build_dashboard_with_events(
                personal,
                &team_all,
                usage_log.all_events(),
                range,
                api_ok,
                team_id.as_deref(),
                &self.team_members,
            );
        }
        let personal = range.filter_fragments(personal);
        let team = range.filter_fragments(&team_all);
        crate::core::build_dashboard(&personal, &team, api_ok)
    }

    fn spawn_job(&mut self, job: TeamJob) {
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.busy = true;
        let tokens = TeamTokenStore::default();
        thread::spawn(move || {
            let result = run_job(job, &tokens);
            let _ = tx.send(result);
        });
    }
}

fn run_job(job: TeamJob, tokens: &TeamTokenStore) -> TeamAsyncResult {
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
                let user = match tokens.load_access_token().ok().and_then(|t| {
                    TeamClient::new(&api_base)
                        .ok()
                        .and_then(|c| c.me(&t).ok())
                }) {
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
    token_resp: super::models::TokenResponse,
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

fn do_refresh_teams(api_base: &str, tokens: &TeamTokenStore) -> Result<Vec<TeamMembership>, String> {
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
    with_auth_retry(api_base, tokens, |access, client| client.cmd_audit_sync(access, team_id))
}

fn do_cmd_audit_report_alert(
    api_base: &str,
    team_id: &str,
    request: &crate::core::cmd_audit::CmdAuditAlertRequest,
    tokens: &TeamTokenStore,
) -> Result<(), String> {
    with_auth_retry(api_base, tokens, |access, client| {
        client.cmd_audit_report_alert(access, team_id, request)
    })
}

fn do_team_config_sync(
    api_base: &str,
    tokens: &TeamTokenStore,
) -> Result<(usize, usize), String> {
    let resp = match with_auth_retry(api_base, tokens, |access, client| {
        client.sync_team_config(access)
    }) {
        Ok(r) => r,
        Err(e) if e.contains("404") || e.contains("Not Found") => super::models::TeamSyncResponse {
            teams: vec![],
        },
        Err(e) => return Err(e),
    };
    let team_count = resp.teams.len();
    let server_count: usize = resp.teams.iter().map(|t| t.servers.len()).sum();
    let mut state = TeamState::load();
    super::sync_config::apply_sync_response(&mut state, &resp);
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

pub fn ensure_access_token(api_base: &str, tokens: &TeamTokenStore) -> Result<String, String> {
    let access = tokens
        .load_access_token()
        .map_err(|_| "Not signed in".to_string())?;
    if !token_needs_refresh(&access, REFRESH_SKEW_SECS) {
        return Ok(access);
    }
    force_refresh_access_token(api_base, tokens)
}

fn force_refresh_access_token(
    api_base: &str,
    tokens: &TeamTokenStore,
) -> Result<String, String> {
    let refresh = tokens
        .load_refresh_token()
        .map_err(|_| "Refresh token missing".to_string())?;
    let client = TeamClient::new(api_base).map_err(|e| e.to_string())?;
    let refreshed = client.refresh(&refresh);
    match &refreshed {
        Ok(_) => crate::core::audit::record_audit_blocking(
            crate::core::audit::AuditEvent::new(
                crate::core::audit::AuditCategory::Auth,
                "team.token_refresh",
                crate::core::audit::AuditOutcome::Success,
            ),
        ),
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
pub fn sync_fragments_blocking(
    service: &mut TeamService,
) -> Result<usize, String> {
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
        status: fragment.status.clone(),
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

