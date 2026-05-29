//! 团队 API 数据模型（与 `TEAM-PLATFORM-DEV-PLAN.md` 附录 A 对齐）。

use serde::{Deserialize, Serialize};

use crate::core::{FragmentStats, FragmentVariable};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TeamRole {
    Viewer,
    Editor,
    Admin,
}

impl TeamRole {
    pub fn parse(s: &str) -> Self {
        match s {
            "admin" => Self::Admin,
            "editor" => Self::Editor,
            _ => Self::Viewer,
        }
    }

    pub fn can_edit(&self) -> bool {
        matches!(self, Self::Editor | Self::Admin)
    }

    pub fn can_delete(&self) -> bool {
        matches!(self, Self::Admin)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamUser {
    pub id: String,
    pub email: String,
    pub username: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub email_verified: bool,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMembership {
    pub team: TeamInfo,
    pub role: String,
}

impl TeamMembership {
    pub fn role_enum(&self) -> TeamRole {
        TeamRole::parse(&self.role)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamsListResponse {
    pub teams: Vec<TeamMembership>,
}

/// `GET /v1/teams/{team_id}/members`（viewer+）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMember {
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub display_name: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMembersResponse {
    pub members: Vec<TeamMember>,
}

/// `GET /v1/team/sync` 响应（见 `docs/tech/TEAM-PLATFORM-API.md`）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TeamSyncResponse {
    #[serde(default)]
    pub teams: Vec<TeamSyncEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamSyncEntry {
    pub team_id: String,
    #[serde(default)]
    pub team_name: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub vault_config: Option<TeamVaultConfig>,
    #[serde(default)]
    pub credential: Option<TeamVaultCredential>,
    #[serde(default)]
    pub servers: Vec<TeamServer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamVaultConfig {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub team_id: String,
    pub address: String,
    #[serde(default)]
    pub kv_mount: String,
    #[serde(default)]
    pub auth_type: String,
    #[serde(default)]
    pub namespace: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamVaultCredential {
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub vault_token: String,
    #[serde(default)]
    pub approle_role_id: String,
    #[serde(default)]
    pub approle_secret_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamServer {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub host: String,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub vault_credential_path: String,
    #[serde(default)]
    pub sort_order: i32,
}

fn default_ssh_port() -> u16 {
    22
}

impl TeamServer {
    pub fn list_key(&self) -> String {
        if !self.id.is_empty() {
            return self.id.clone();
        }
        format!("{}:{}:{}", self.host, self.port, self.name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub user: TeamUser,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshResponse {
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterResponse {
    pub user: TeamUser,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiErrorBody {
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub server_version: Option<TeamFragment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamFragment {
    pub id: String,
    #[serde(default)]
    pub team_id: String,
    pub title: String,
    pub command: String,
    #[serde(default)]
    pub category: String,
    /// 服务端存 JSON 字符串
    #[serde(default)]
    pub tags: String,
    #[serde(default)]
    pub variables: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub revision: u32,
    #[serde(default)]
    pub created_by: Option<String>,
    #[serde(default)]
    pub updated_by: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub usage_count: u32,
    #[serde(default)]
    pub success_count: u32,
    #[serde(default)]
    pub total_time_ms: u64,
    #[serde(default)]
    pub last_used_at: Option<i64>,
}

/// `GET /v1/teams/{team_id}/fragments/analytics`（未部署时客户端用本地聚合）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FragmentAnalyticsResponse {
    #[serde(default)]
    pub fragments: Vec<FragmentAnalyticsRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentAnalyticsRow {
    pub fragment_id: String,
    #[serde(default)]
    pub usage_count: u32,
    #[serde(default)]
    pub success_count: u32,
    #[serde(default)]
    pub total_time_ms: u64,
    #[serde(default)]
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentSyncRequest {
    pub cursor: String,
    #[serde(default = "default_sync_limit")]
    pub limit: u32,
}

fn default_sync_limit() -> u32 {
    500
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentSyncResponse {
    pub cursor: String,
    #[serde(default)]
    pub fragments: Vec<TeamFragment>,
    #[serde(default)]
    pub deleted_ids: Vec<String>,
    #[serde(default)]
    pub server_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTeamFragmentRequest {
    pub title: String,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variables: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTeamFragmentRequest {
    pub title: String,
    pub command: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub tags: String,
    #[serde(default)]
    pub variables: String,
    #[serde(default)]
    pub status: String,
    pub revision: u32,
}

pub fn parse_tags_json(raw: &str) -> Vec<String> {
    if raw.trim().is_empty() {
        return Vec::new();
    }
    serde_json::from_str(raw).unwrap_or_default()
}

pub fn parse_variables_json(raw: &str) -> Vec<FragmentVariable> {
    if raw.trim().is_empty() {
        return Vec::new();
    }
    let v: serde_json::Value = match serde_json::from_str(raw) {
        Ok(x) => x,
        Err(_) => return Vec::new(),
    };
    if let Some(obj) = v.as_object() {
        return obj
            .iter()
            .map(|(name, val)| {
                let default = val
                    .as_str()
                    .map(|s| s.to_string())
                    .or_else(|| val.as_i64().map(|n| n.to_string()))
                    .unwrap_or_default();
                FragmentVariable::with_default(name, name, &default)
            })
            .collect();
    }
    Vec::new()
}

impl TeamFragment {
    pub fn to_fragment_stats(&self, team_name: &str) -> FragmentStats {
        let mut f = FragmentStats::new(
            self.id.clone(),
            self.title.clone(),
            self.command.clone(),
            if self.category.is_empty() {
                "team".to_string()
            } else {
                self.category.clone()
            },
        );
        f.tags = parse_tags_json(&self.tags);
        if !team_name.is_empty() {
            let label = format!("@{team_name}");
            if !f.tags.iter().any(|t| t == &label) {
                f.tags.insert(0, label);
            }
        }
        f.variables = parse_variables_json(&self.variables);
        f.usage_count = self.usage_count;
        f.success_count = self.success_count;
        f.total_time_ms = self.total_time_ms;
        f.last_used = self.last_used_at;
        f
    }
}
