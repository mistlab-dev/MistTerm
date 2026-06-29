//! 团队平台 HTTP 客户端（blocking `reqwest`）。

use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::models::{
    ApiErrorBody, CreateShareRequest, CreateShareResponse, CreateTeamFragmentRequest,
    FragmentAnalyticsResponse, FragmentMemberAnalyticsResponse, FragmentSyncRequest,
    FragmentSyncResponse, FragmentVersion, FragmentVersionsResponse, ListSharesResponse,
    RefreshResponse, RegisterResponse, TeamFragment, TeamInfo, TeamSettings,
    TeamsListResponse, TokenResponse, TeamUser, UpdateTeamFragmentRequest,
};
use super::oauth::{percent_encode_query, OAuthProvider};
use super::settings::normalize_api_base;

#[derive(Debug, Clone)]
pub struct TeamApiError {
    pub status: u16,
    pub message: String,
    pub conflict_fragment: Option<TeamFragment>,
}

impl std::fmt::Display for TeamApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP {}: {}", self.status, self.message)
    }
}

impl std::error::Error for TeamApiError {}

pub struct TeamClient {
    base_url: String,
    http: Client,
}

impl TeamClient {
    pub fn new(api_base: &str) -> Result<Self, String> {
        let base_url = normalize_api_base(api_base);
        if base_url.is_empty() {
            return Err("team API base URL is empty".into());
        }
        let http = Client::builder()
            .timeout(Duration::from_secs(45))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { base_url, http })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn register(
        &self,
        email: &str,
        username: &str,
        display_name: Option<&str>,
        password: &str,
    ) -> Result<RegisterResponse, TeamApiError> {
        let body = serde_json::json!({
            "email": email,
            "username": username,
            "display_name": display_name.unwrap_or(username),
            "password": password,
        });
        self.post_json("/v1/auth/register", None, &body)
    }

    pub fn login_email(
        &self,
        email: &str,
        password: &str,
    ) -> Result<TokenResponse, TeamApiError> {
        let body = serde_json::json!({ "email": email, "password": password });
        self.post_json("/v1/auth/login", None, &body)
    }

    pub fn login_username(
        &self,
        username: &str,
        password: &str,
    ) -> Result<TokenResponse, TeamApiError> {
        let body = serde_json::json!({ "username": username, "password": password });
        self.post_json("/v1/auth/login", None, &body)
    }

    pub fn refresh(&self, refresh_token: &str) -> Result<RefreshResponse, TeamApiError> {
        let body = serde_json::json!({ "refresh_token": refresh_token });
        self.post_json("/v1/auth/refresh", None, &body)
    }

    /// 桌面 OAuth 授权入口（在系统浏览器中打开）。
    pub fn oauth_authorize_url(api_base: &str, provider: OAuthProvider, redirect_uri: &str) -> String {
        let base = normalize_api_base(api_base);
        format!(
            "{}/v1/oauth/{}?redirect_uri={}",
            base,
            provider.path_segment(),
            percent_encode_query(redirect_uri)
        )
    }

    /// 用授权码换取 token（`GET /v1/oauth/{provider}/callback`）。
    pub fn oauth_exchange(
        &self,
        provider: OAuthProvider,
        code: &str,
        redirect_uri: &str,
    ) -> Result<TokenResponse, TeamApiError> {
        let path = format!(
            "/v1/oauth/{}/callback?code={}&redirect_uri={}",
            provider.path_segment(),
            percent_encode_query(code),
            percent_encode_query(redirect_uri)
        );
        self.get_json(&path, None)
    }

    pub fn me(&self, access_token: &str) -> Result<TeamUser, TeamApiError> {
        self.get_json("/v1/me", Some(access_token))
    }

    pub fn list_teams(&self, access_token: &str) -> Result<TeamsListResponse, TeamApiError> {
        self.get_json("/v1/teams", Some(access_token))
    }

    pub fn sync_team_config(
        &self,
        access_token: &str,
    ) -> Result<super::models::TeamSyncResponse, TeamApiError> {
        self.get_json("/v1/team/sync", Some(access_token))
    }

    pub fn get_team(&self, access_token: &str, team_id: &str) -> Result<TeamInfo, TeamApiError> {
        self.get_json(&format!("/v1/teams/{team_id}"), Some(access_token))
    }

    pub fn list_team_members(
        &self,
        access_token: &str,
        team_id: &str,
    ) -> Result<super::models::TeamMembersResponse, TeamApiError> {
        self.get_json(
            &format!("/v1/teams/{team_id}/members"),
            Some(access_token),
        )
    }

    pub fn cmd_audit_sync(
        &self,
        access_token: &str,
        team_id: &str,
    ) -> Result<crate::core::cmd_audit::CmdAuditSyncPayload, TeamApiError> {
        self.get_json(
            &format!("/v1/teams/{team_id}/command-audit/sync"),
            Some(access_token),
        )
    }

    pub fn cmd_audit_report_alert(
        &self,
        access_token: &str,
        team_id: &str,
        body: &crate::core::cmd_audit::CmdAuditAlertRequest,
    ) -> Result<(), TeamApiError> {
        self.post_json_empty(
            &format!("/v1/teams/{team_id}/command-audit/alerts"),
            Some(access_token),
            body,
        )
    }

    /// 团队片段聚合统计；404/未实现时返回 `Ok(None)` 供客户端本地回退。
    pub fn fetch_fragment_analytics(
        &self,
        access_token: &str,
        team_id: &str,
    ) -> Result<Option<FragmentAnalyticsResponse>, TeamApiError> {
        let path = format!("/v1/teams/{team_id}/fragments/analytics");
        let req = self.http.get(self.url(&path)).bearer_auth(access_token);
        let resp = req.send().map_err(|e| TeamApiError {
            status: 0,
            message: e.to_string(),
            conflict_fragment: None,
        })?;
        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if resp.status().is_success() {
            return Self::decode_response(resp).map(Some);
        }
        Err(Self::decode_error(
            resp.status(),
            resp.text().unwrap_or_default(),
        ))
    }

    /// 团队成员区间片段统计；404/未实现时返回 `Ok(None)`。
    pub fn fetch_fragment_member_analytics(
        &self,
        access_token: &str,
        team_id: &str,
        since_days: u32,
    ) -> Result<Option<FragmentMemberAnalyticsResponse>, TeamApiError> {
        let path = format!(
            "/v1/teams/{team_id}/fragments/analytics/members?since={since_days}d"
        );
        let req = self.http.get(self.url(&path)).bearer_auth(access_token);
        let resp = req.send().map_err(|e| TeamApiError {
            status: 0,
            message: e.to_string(),
            conflict_fragment: None,
        })?;
        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if resp.status().is_success() {
            return Self::decode_response(resp).map(Some);
        }
        Err(Self::decode_error(
            resp.status(),
            resp.text().unwrap_or_default(),
        ))
    }

    pub fn sync_fragments(
        &self,
        access_token: &str,
        team_id: &str,
        cursor: &str,
        limit: u32,
    ) -> Result<FragmentSyncResponse, TeamApiError> {
        let body = FragmentSyncRequest {
            cursor: cursor.to_string(),
            limit,
        };
        self.post_json(
            &format!("/v1/teams/{team_id}/fragments:sync"),
            Some(access_token),
            &body,
        )
    }

    pub fn create_fragment(
        &self,
        access_token: &str,
        team_id: &str,
        req: &CreateTeamFragmentRequest,
    ) -> Result<TeamFragment, TeamApiError> {
        self.post_json(
            &format!("/v1/teams/{team_id}/fragments"),
            Some(access_token),
            req,
        )
    }

    pub fn update_fragment(
        &self,
        access_token: &str,
        fragment_id: &str,
        req: &UpdateTeamFragmentRequest,
    ) -> Result<TeamFragment, TeamApiError> {
        self.put_json(
            &format!("/v1/fragments/{fragment_id}"),
            access_token,
            req,
        )
    }

    pub fn delete_fragment(
        &self,
        access_token: &str,
        fragment_id: &str,
    ) -> Result<(), TeamApiError> {
        self.delete(&format!("/v1/fragments/{fragment_id}"), Some(access_token))
    }

    pub fn post_audit_events(
        &self,
        access_token: &str,
        body: &serde_json::Value,
    ) -> Result<(), TeamApiError> {
        self.post_json_empty("/v1/audit/events", Some(access_token), body)
    }

    /// 片段执行统计上报；404/未实现时静默成功。
    pub fn report_fragment_usage(
        &self,
        access_token: &str,
        team_id: &str,
        fragment_id: &str,
        success: bool,
        duration_ms: u64,
    ) -> Result<(), TeamApiError> {
        let path = format!("/v1/teams/{team_id}/fragments/{fragment_id}/usage");
        let body = serde_json::json!({
            "success": success,
            "duration_ms": duration_ms,
        });
        let req = self
            .http
            .post(self.url(&path))
            .bearer_auth(access_token)
            .json(&body);
        let resp = req.send().map_err(|e| TeamApiError {
            status: 0,
            message: e.to_string(),
            conflict_fragment: None,
        })?;
        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(());
        }
        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }
        Err(Self::decode_error(status, resp.text().unwrap_or_default()))
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        bearer: Option<&str>,
    ) -> Result<T, TeamApiError> {
        let mut req = self.http.get(self.url(path));
        if let Some(t) = bearer {
            req = req.bearer_auth(t);
        }
        let resp = req.send().map_err(|e| TeamApiError {
            status: 0,
            message: e.to_string(),
            conflict_fragment: None,
        })?;
        Self::decode_response(resp)
    }

    fn post_json<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        bearer: Option<&str>,
        body: &B,
    ) -> Result<T, TeamApiError> {
        let mut req = self.http.post(self.url(path)).json(body);
        if let Some(t) = bearer {
            req = req.bearer_auth(t);
        }
        let resp = req.send().map_err(|e| TeamApiError {
            status: 0,
            message: e.to_string(),
            conflict_fragment: None,
        })?;
        Self::decode_response(resp)
    }

    fn post_json_empty<B: Serialize>(
        &self,
        path: &str,
        bearer: Option<&str>,
        body: &B,
    ) -> Result<(), TeamApiError> {
        let mut req = self.http.post(self.url(path)).json(body);
        if let Some(t) = bearer {
            req = req.bearer_auth(t);
        }
        let resp = req.send().map_err(|e| TeamApiError {
            status: 0,
            message: e.to_string(),
            conflict_fragment: None,
        })?;
        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }
        Err(Self::decode_error(status, resp.text().unwrap_or_default()))
    }

    fn put_json<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        bearer: &str,
        body: &B,
    ) -> Result<T, TeamApiError> {
        let resp = self
            .http
            .put(self.url(path))
            .bearer_auth(bearer)
            .json(body)
            .send()
            .map_err(|e| TeamApiError {
                status: 0,
                message: e.to_string(),
                conflict_fragment: None,
            })?;
        Self::decode_response(resp)
    }

    fn delete(&self, path: &str, bearer: Option<&str>) -> Result<(), TeamApiError> {
        let mut req = self.http.delete(self.url(path));
        if let Some(t) = bearer {
            req = req.bearer_auth(t);
        }
        let resp = req.send().map_err(|e| TeamApiError {
            status: 0,
            message: e.to_string(),
            conflict_fragment: None,
        })?;
        let status = resp.status();
        if status.is_success() {
            Ok(())
        } else {
            Err(Self::decode_error(status, resp.text().unwrap_or_default()))
        }
    }

    fn decode_response<T: DeserializeOwned>(resp: reqwest::blocking::Response) -> Result<T, TeamApiError> {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        if status.is_success() {
            if text.trim().is_empty() {
                return Err(TeamApiError {
                    status: status.as_u16(),
                    message: "empty response body".into(),
                    conflict_fragment: None,
                });
            }
            serde_json::from_str(&text).map_err(|e| TeamApiError {
                status: status.as_u16(),
                message: format!("JSON decode: {e}; body={text}"),
                conflict_fragment: None,
            })
        } else {
            Err(Self::decode_error(status, text))
        }
    }

    fn decode_error(status: StatusCode, text: String) -> TeamApiError {
        let parsed: Option<ApiErrorBody> = serde_json::from_str(&text).ok();
        let message = parsed
            .as_ref()
            .map(|b| b.error.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| text.clone());
        TeamApiError {
            status: status.as_u16(),
            message,
            conflict_fragment: parsed.and_then(|b| b.server_version),
        }
    }

    // ── Fragment lock/unlock ──

    pub fn lock_fragment(
        &self,
        access_token: &str,
        fragment_id: &str,
    ) -> Result<(), TeamApiError> {
        self.post_json_empty(
            &format!("/v1/fragments/{fragment_id}/lock"),
            Some(access_token),
            &serde_json::json!({}),
        )
    }

    pub fn unlock_fragment(
        &self,
        access_token: &str,
        fragment_id: &str,
    ) -> Result<(), TeamApiError> {
        self.post_json_empty(
            &format!("/v1/fragments/{fragment_id}/unlock"),
            Some(access_token),
            &serde_json::json!({}),
        )
    }

    // ── Fragment version history ──

    pub fn get_fragment_versions(
        &self,
        access_token: &str,
        fragment_id: &str,
        limit: u32,
        offset: u32,
    ) -> Result<FragmentVersionsResponse, TeamApiError> {
        let path = format!(
            "/v1/fragments/{fragment_id}/versions?limit={limit}&offset={offset}"
        );
        self.get_json(&path, Some(access_token))
    }

    pub fn get_fragment_version(
        &self,
        access_token: &str,
        fragment_id: &str,
        revision: i64,
    ) -> Result<FragmentVersion, TeamApiError> {
        let path = format!("/v1/fragments/{fragment_id}/versions/{revision}");
        self.get_json(&path, Some(access_token))
    }

    // ── External shares ──

    pub fn create_share(
        &self,
        access_token: &str,
        fragment_id: &str,
        req: &CreateShareRequest,
    ) -> Result<CreateShareResponse, TeamApiError> {
        self.post_json(
            &format!("/v1/fragments/{fragment_id}/shares"),
            Some(access_token),
            req,
        )
    }

    pub fn list_shares(
        &self,
        access_token: &str,
        fragment_id: &str,
    ) -> Result<ListSharesResponse, TeamApiError> {
        self.get_json(
            &format!("/v1/fragments/{fragment_id}/shares"),
            Some(access_token),
        )
    }

    pub fn delete_share(
        &self,
        access_token: &str,
        share_id: &str,
    ) -> Result<(), TeamApiError> {
        self.delete(&format!("/v1/shares/{share_id}"), Some(access_token))
    }

    // ── Team settings ──

    pub fn get_team_settings(
        &self,
        access_token: &str,
        team_id: &str,
    ) -> Result<TeamSettings, TeamApiError> {
        self.get_json(
            &format!("/v1/teams/{team_id}/settings"),
            Some(access_token),
        )
    }

    pub fn update_team_settings(
        &self,
        access_token: &str,
        team_id: &str,
        settings: &TeamSettings,
    ) -> Result<TeamSettings, TeamApiError> {
        self.put_json(
            &format!("/v1/teams/{team_id}/settings"),
            access_token,
            settings,
        )
    }
}
