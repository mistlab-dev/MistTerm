//! 团队 API 联调测试（默认打 `https://api.mistlab.dev`，需出站 HTTPS）。
//!
//! 公开探针在默认 `cargo test` 中运行；鉴权流程需环境变量：
//! `MISTTERM_TEST_TEAM_EMAIL`、`MISTTERM_TEST_TEAM_PASSWORD`
//! 可选 `MISTTERM_TEST_TEAM_API_BASE`。

use mistterm::core::team::{
    CreateTeamFragmentRequest, FragmentMemberAnalyticsResponse, OAuthProvider, TeamClient,
    DEFAULT_TEAM_API_BASE,
};
use mistterm::test_support::team_api::{api_base, login_test_session};
use reqwest::StatusCode;

fn assert_invalid_token_not_404(method: reqwest::Method, path: &str, body: Option<&str>) {
    let url = format!("{}{}", api_base().trim_end_matches('/'), path);
    let http = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .expect("http client");
    let mut req = http
        .request(method, &url)
        .header("Authorization", "Bearer invalid-token-for-smoke-test");
    if let Some(b) = body {
        req = req
            .header("Content-Type", "application/json")
            .body(b.to_string());
    }
    let resp = req
        .send()
        .unwrap_or_else(|e| panic!("request {path} failed: {e}"));
    let status = resp.status();
    assert_ne!(
        status,
        StatusCode::NOT_FOUND,
        "{path} should be deployed (expected 401/403, got {status})"
    );
}

#[test]
fn team_api_health_ok() {
    let url = format!("{}/health", api_base().trim_end_matches('/'));
    let http = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .expect("http client");
    let resp = http
        .get(&url)
        .send()
        .unwrap_or_else(|e| panic!("GET /health failed: {e}"));
    assert_eq!(resp.status(), StatusCode::OK, "GET /health");
}

#[test]
fn team_api_default_base_is_production() {
    // 文档与默认设置一致；可用 MISTTERM_TEST_TEAM_API_BASE 覆盖。
    if std::env::var("MISTTERM_TEST_TEAM_API_BASE").is_err() {
        assert_eq!(DEFAULT_TEAM_API_BASE, "https://api.mistlab.dev");
    }
}

#[test]
fn team_api_oauth_authorize_url_format() {
    let url = TeamClient::oauth_authorize_url(
        DEFAULT_TEAM_API_BASE,
        OAuthProvider::Google,
        "http://127.0.0.1:8765/callback",
    );
    assert!(url.starts_with("https://api.mistlab.dev/v1/oauth/google?redirect_uri="));
}

#[test]
fn team_api_fragment_member_analytics_serde_roundtrip() {
    let json = r#"{"members":[{"user_id":"u_1","display_name":"Alice","run_count":3,"success_count":2}]}"#;
    let parsed: FragmentMemberAnalyticsResponse = serde_json::from_str(json).unwrap();
    assert_eq!(parsed.members.len(), 1);
    assert_eq!(parsed.members[0].run_count, 3);
}

#[test]
fn team_api_p1_analytics_routes_deployed() {
    assert_invalid_token_not_404(
        reqwest::Method::GET,
        "/v1/teams/t_smoke/fragments/analytics",
        None,
    );
    assert_invalid_token_not_404(
        reqwest::Method::GET,
        "/v1/teams/t_smoke/fragments/analytics/members?since=7d",
        None,
    );
    assert_invalid_token_not_404(
        reqwest::Method::POST,
        "/v1/teams/t_smoke/fragments/f_smoke/usage",
        Some(r#"{"success":true,"duration_ms":1}"#),
    );
}

#[test]
fn team_api_members_route_deployed() {
    assert_invalid_token_not_404(reqwest::Method::GET, "/v1/teams/t_smoke/members", None);
}

#[test]
fn team_api_authenticated_analytics_and_members() {
    let Some(session) = login_test_session() else {
        eprintln!("skip: set MISTTERM_TEST_TEAM_EMAIL + MISTTERM_TEST_TEAM_PASSWORD");
        return;
    };
    let analytics = session
        .client
        .fetch_fragment_analytics(&session.access_token, &session.team_id)
        .expect("fetch_fragment_analytics");
    assert!(
        analytics.is_some(),
        "GET …/fragments/analytics should not 404 on production"
    );

    let members = session
        .client
        .fetch_fragment_member_analytics(&session.access_token, &session.team_id, 7)
        .expect("fetch_fragment_member_analytics");
    assert!(
        members.is_some(),
        "GET …/analytics/members should not 404 on production"
    );

    let roster = session
        .client
        .list_team_members(&session.access_token, &session.team_id)
        .expect("list_team_members");
    assert!(
        !roster.members.is_empty(),
        "team should have at least one member"
    );
}

#[test]
fn team_api_authenticated_usage_report() {
    let Some(session) = login_test_session() else {
        eprintln!("skip: set MISTTERM_TEST_TEAM_EMAIL + MISTTERM_TEST_TEAM_PASSWORD");
        return;
    };

    let sync = session
        .client
        .sync_fragments(&session.access_token, &session.team_id, "", 50)
        .expect("sync_fragments");

    let fragment_id = if let Some(existing) = sync.fragments.first() {
        existing.id.clone()
    } else {
        let created = session
            .client
            .create_fragment(
                &session.access_token,
                &session.team_id,
                &CreateTeamFragmentRequest {
                    title: format!(
                        "mistterm_api_smoke_{}",
                        chrono::Utc::now().timestamp_millis()
                    ),
                    command: "echo mistterm-api-smoke".into(),
                    category: Some("test".into()),
                    tags: None,
                    variables: None,
                },
            )
            .expect("create_fragment for usage smoke");
        created.id.clone()
    };

    session
        .client
        .report_fragment_usage(
            &session.access_token,
            &session.team_id,
            &fragment_id,
            true,
            42,
        )
        .expect("report_fragment_usage");

    let analytics = session
        .client
        .fetch_fragment_analytics(&session.access_token, &session.team_id)
        .expect("fetch analytics after usage")
        .expect("analytics body");
    let row = analytics
        .fragments
        .iter()
        .find(|r| r.fragment_id == fragment_id);
    assert!(
        row.is_some(),
        "analytics should include fragment {fragment_id} after usage report"
    );
}
