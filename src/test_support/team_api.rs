//! 团队 API 联调辅助（`api.mistlab.dev` 或 `MISTTERM_TEST_TEAM_API_BASE`）。

use crate::core::team::{TeamClient, DEFAULT_TEAM_API_BASE};

pub struct TeamTestSession {
    pub client: TeamClient,
    pub access_token: String,
    pub team_id: String,
}

pub fn api_base() -> String {
    std::env::var("MISTTERM_TEST_TEAM_API_BASE")
        .unwrap_or_else(|_| DEFAULT_TEAM_API_BASE.to_string())
}

/// 无凭证或登录失败时返回 `None`（测试应直接 return）。
pub fn login_test_session() -> Option<TeamTestSession> {
    let email = std::env::var("MISTTERM_TEST_TEAM_EMAIL").ok()?;
    let password = std::env::var("MISTTERM_TEST_TEAM_PASSWORD").ok()?;
    let client = TeamClient::new(&api_base()).ok()?;
    let token = client.login_email(&email, &password).ok()?;
    let teams = client.list_teams(&token.access_token).ok()?;
    let team_id = teams.teams.first()?.team.id.clone();
    Some(TeamTestSession {
        client,
        access_token: token.access_token,
        team_id,
    })
}
