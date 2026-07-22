//! 团队平台编排：认证、团队列表、片段同步（后台线程 + 通道回传）。

#[path = "service_blocking.rs"]
mod service_blocking;
#[path = "service_jobs.rs"]
mod service_jobs;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use self::service_jobs::{
    do_cmd_audit_report_alert, do_report_fragment_usage, run_job, TeamJob,
};
pub use self::service_blocking::{
    create_fragment_share_blocking, create_team_fragment_blocking,
    delete_fragment_share_blocking, delete_team_fragment_blocking, ensure_access_token,
    fetch_fragment_versions_blocking, fetch_team_settings_blocking, list_fragment_shares_blocking,
    lock_team_fragment_blocking, sync_fragments_blocking, unlock_team_fragment_blocking,
    update_team_fragment_blocking, update_team_settings_blocking,
};
pub use self::service_jobs::do_sync;

use super::auth::TeamTokenStore;
use super::cache::TeamFragmentCache;
use super::client::TeamClient;
use super::models::{
    TeamFragment, TeamInfo, TeamMember, TeamMembership, TeamUser,
};
use super::oauth::OAuthProvider;
use super::settings::TeamSettings;
use super::state::TeamState;

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

    /// 异步上报片段执行统计（不占用 `busy`；404 静默）。
    pub fn spawn_report_fragment_usage(
        &self,
        team_id: &str,
        fragment_id: &str,
        success: bool,
        dur_ms: u64,
    ) {
        if !self.is_logged_in() || team_id.is_empty() || fragment_id.is_empty() {
            return;
        }
        let api_base = self.api_base();
        let team_id = team_id.to_string();
        let fragment_id = fragment_id.to_string();
        thread::spawn(move || {
            let tokens = TeamTokenStore::default();
            let _ = do_report_fragment_usage(
                &api_base,
                &team_id,
                &fragment_id,
                success,
                dur_ms,
                &tokens,
            );
        });
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
        let mut dash = if range.cutoff_unix().is_some() {
            let team_id = self.state.current_team_id.clone();
            crate::core::build_dashboard_with_events(
                personal,
                &team_all,
                usage_log.all_events(),
                range,
                api_ok,
                team_id.as_deref(),
                &self.team_members,
            )
        } else {
            let personal = range.filter_fragments(personal);
            let team = range.filter_fragments(&team_all);
            crate::core::build_dashboard(&personal, &team, api_ok)
        };
        if let (Some(days), Some(tid)) = (range.since_days(), self.state.current_team_id.as_deref())
        {
            let api_base = self.api_base();
            if let Ok(token) = ensure_access_token(&api_base, &self.tokens) {
                if let Ok(client) = TeamClient::new(&api_base) {
                    match client.fetch_fragment_member_analytics(&token, tid, days) {
                        Ok(Some(resp)) if !resp.members.is_empty() => {
                            dash.member_rows = crate::core::member_rows_from_api(&resp.members);
                            dash.member_stats_from_server = true;
                        }
                        Ok(_) => {}
                        Err(e) => log::debug!("fragment member analytics API: {}", e),
                    }
                }
            }
        }
        dash
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
