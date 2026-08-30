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

    pub fn list_cmd_audit_agents(
        &self,
        access_token: &str,
        team_id: &str,
    ) -> Result<super::models::CmdAuditAgentsResponse, TeamApiError> {
        self.get_json(
            &format!("/v1/teams/{team_id}/command-audit/agents"),
            Some(access_token),
        )
    }

    pub fn update_cmd_audit_agent(
        &self,
        access_token: &str,
        team_id: &str,
        agent_id: &str,
        enabled: bool,
    ) -> Result<super::models::CmdAuditAgent, TeamApiError> {
        self.put_json(
            &format!("/v1/teams/{team_id}/command-audit/agents/{agent_id}"),
            access_token,
            &super::models::UpdateCmdAuditAgentRequest { enabled },
        )
    }

    pub fn get_storage_usage(
        &self,
        access_token: &str,
        team_id: &str,
    ) -> Result<super::models::StorageUsageResponse, TeamApiError> {
        self.get_json(
            &format!("/v1/teams/{team_id}/storage/usage"),
            Some(access_token),
        )
    }

    pub fn get_fragment(
        &self,
        access_token: &str,
        fragment_id: &str,
    ) -> Result<super::models::TeamFragment, TeamApiError> {
        self.get_json(
            &format!("/v1/fragments/{fragment_id}"),
            Some(access_token),
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
        decode_text_as_result(status.as_u16(), &text)
    }

    fn decode_error(status: StatusCode, text: String) -> TeamApiError {
        decode_error_body(status.as_u16(), &text)
    }
}

impl TeamClient {
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

// ---- Pure helpers extracted from decode_response / decode_error.
//      Taking status as plain u16 + &str body means no reqwest Response is
//      needed for unit tests; TeamClient methods just delegate.

pub(crate) fn decode_error_body(status_u16: u16, text: &str) -> TeamApiError {
    let parsed: Option<ApiErrorBody> = serde_json::from_str(text).ok();
    let message = parsed
        .as_ref()
        .map(|b| b.error.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| text.to_string());
    // Historical naming quirk: server_version is actually the serialized
    // conflict_fragment body returned on 409 conflicts by the team server.
    let conflict_fragment = parsed.and_then(|b| b.server_version);
    TeamApiError {
        status: status_u16,
        message,
        conflict_fragment,
    }
}

pub(crate) fn decode_text_as_result<T: DeserializeOwned>(
    status_u16: u16,
    text: &str,
) -> Result<T, TeamApiError> {
    use std::num::NonZeroU16;
    // Mimic `StatusCode::is_success` range [200, 300).
    let is_success = (200..300).contains(&status_u16);
    if is_success {
        if text.trim().is_empty() {
            return Err(TeamApiError {
                status: status_u16,
                message: "empty response body".into(),
                conflict_fragment: None,
            });
        }
        return serde_json::from_str(text).map_err(|e| TeamApiError {
            status: status_u16,
            message: format!("JSON decode: {e}; body={text}"),
            conflict_fragment: None,
        });
    }
    // status_u16 of 0 would mean `StatusCode::from_u16` fails; still safe.
    let fallback = status_u16.max(NonZeroU16::MIN.get());
    Err(decode_error_body(fallback, text))
}

/// Simple string-level URL combiner (matches `TeamClient::url` exactly).
pub(crate) fn build_api_url(base: &str, path: &str) -> String {
    format!("{base}{path}")
}

#[cfg(test)]
mod pure_client_tests {
    use super::*;

    // ------------------------------------------------ normalize / TeamClient::new

    #[test]
    fn normalize_preserves_https_and_strips_trailing_slash_and_ws() {
        assert_eq!(normalize_api_base("  https://a.corp/  "), "https://a.corp");
        assert_eq!(normalize_api_base("http://insecure/"), "http://insecure");
    }

    #[test]
    fn normalize_prepends_https_for_plain_hostname() {
        assert_eq!(normalize_api_base("team.example.com"), "https://team.example.com");
        assert_eq!(normalize_api_base(""), "");
        assert_eq!(normalize_api_base("   ////  "), ""); // becomes empty after trimming
    }

    #[test]
    fn team_client_new_rejects_empty_base_and_reports_custom_message() {
        let err = match TeamClient::new("") {
            Ok(_) => panic!("expected Err for empty base"),
            Err(e) => e,
        };
        assert!(err.contains("empty"));
        let err2 = match TeamClient::new("   ///   ") {
            Ok(_) => panic!("expected Err for all-slashes base"),
            Err(e) => e,
        };
        assert!(err2.contains("empty"));
    }

    #[test]
    fn team_client_new_ok_with_valid_base_exposes_via_base_url() {
        let c = TeamClient::new("team.corp").unwrap();
        assert_eq!(c.base_url(), "https://team.corp");
    }

    // ------------------------------------------------ decode_error_body

    #[test]
    fn error_body_with_api_error_takes_precedence_over_raw_text() {
        // `server_version` type is `Option<TeamFragment>`; to keep the parsed
        // ApiErrorBody valid we don't pass that field (defaults to None).
        let e = decode_error_body(400, r#"{"error":"bad request"}"#);
        assert_eq!(e.status, 400);
        assert_eq!(e.message, "bad request");
        // conflict_fragment field missing => None
        assert!(e.conflict_fragment.is_none());
    }

    #[test]
    fn error_body_raw_text_is_fallback_when_json_missing_or_empty_err() {
        let plain = decode_error_body(500, "plain text boom");
        assert_eq!(plain.message, "plain text boom");
        assert_eq!(plain.status, 500);
        // JSON object present but `error` is "" -> falls back to raw text.
        let empty_err = decode_error_body(403, r#"{"error":"","other":"x"}"#);
        assert_eq!(empty_err.message, r#"{"error":"","other":"x"}"#);
    }

    #[test]
    fn error_body_parses_conflict_fragment_payload() {
        // TeamApiError.conflict_fragment comes from ApiErrorBody.server_version
        // field (historical naming quirk for 409 conflict responses).
        let json = r#"{
          "error":"revision mismatch",
          "server_version":{
            "id":"f-1","title":"T","command":"c","category":"ops","tags":"[]","variables":"{}","status":"live","revision":5
          }
        }"#;
        let e = decode_error_body(409, json);
        let c = e.conflict_fragment.expect("missing conflict_fragment (server_version)");
        assert_eq!(c.id, "f-1");
        assert_eq!(c.revision, 5);
        assert_eq!(c.title, "T");
    }

    // ------------------------------------------------ decode_text_as_result

    #[test]
    fn decode_success_json_parses_typed_result() {
        let body = r#"{"access_token":"abc","refresh_token":"def","expires_in":3600,"user":{"id":"u","email":"a@b","username":"alice"}}"#;
        let r: TokenResponse = decode_text_as_result(200, body).unwrap();
        assert_eq!(r.access_token, "abc");
        assert_eq!(r.refresh_token, "def");
        assert_eq!(r.user.id, "u");
        assert_eq!(r.user.email, "a@b");
    }

    #[test]
    fn decode_empty_body_on_success_is_error() {
        let e = decode_text_as_result::<TokenResponse>(201, "   \n\t  ").unwrap_err();
        assert_eq!(e.status, 201);
        assert!(e.message.contains("empty"));
    }

    #[test]
    fn decode_invalid_json_on_success_is_decode_error_with_body_snippet() {
        let e = decode_text_as_result::<TokenResponse>(200, "{not json").unwrap_err();
        assert_eq!(e.status, 200);
        assert!(e.message.contains("JSON decode"));
        assert!(e.message.contains("{not json"));
    }

    #[test]
    fn decode_non_success_status_runs_decode_error_body_even_with_typed_call() {
        let e = decode_text_as_result::<TokenResponse>(
            401,
            r#"{"error":"unauthorized"}"#
        ).unwrap_err();
        assert_eq!(e.status, 401);
        assert_eq!(e.message, "unauthorized");
    }

    #[test]
    fn decode_non_success_zero_status_becomes_1() {
        // 0 is "unknown" sentinel from HTTP errors; clamp inside the pure fn.
        let e = decode_text_as_result::<TokenResponse>(0, "network failed").unwrap_err();
        assert!(e.status >= 1);
    }

    // ------------------------------------------------ build_api_url

    #[test]
    fn build_url_concatenates_base_path_without_rewrite() {
        assert_eq!(
            build_api_url("https://team.corp", "/v1/me"),
            "https://team.corp/v1/me"
        );
    }

    // ------------------------------------------------ oauth_authorize_url (team + market share the util)

    #[test]
    fn oauth_authorize_url_encodes_redirect_uri_percent() {
        let url = TeamClient::oauth_authorize_url(
            "team.corp/",
            OAuthProvider::Github,
            "myapp://cb?x=1&y=2",
        );
        assert!(url.starts_with("https://team.corp/v1/oauth/github?redirect_uri="));
        // `?` and `=` and `&` must all be percent-encoded in redirect_uri value.
        let tail = url.rsplit("redirect_uri=").next().unwrap();
        assert!(!tail.contains('='), "tail contained raw =: {tail}");
        assert!(!tail.contains('&'), "tail contained raw &: {tail}");
        assert!(tail.contains("%3F") || tail.contains("%26"));
    }

    #[test]
    fn oauth_authorize_url_google_segment_is_google_lowercase() {
        let u = TeamClient::oauth_authorize_url("a", OAuthProvider::Google, "r");
        assert!(u.contains("/oauth/google?"));
    }

    // ------------------------------------------------ TeamApiError Display

    #[test]
    fn display_team_api_error_inlines_status_and_message() {
        let e = TeamApiError {
            status: 404,
            message: "nope".into(),
            conflict_fragment: None,
        };
        assert_eq!(format!("{e}"), "HTTP 404: nope");
    }
}
