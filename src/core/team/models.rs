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

pub fn tags_to_json(tags: &[String]) -> String {
    serde_json::to_string(tags).unwrap_or_else(|_| "[]".to_string())
}

pub fn variables_to_json(vars: &[FragmentVariable]) -> String {
    let mut map = serde_json::Map::new();
    for v in vars {
        let val = v
            .default_value
            .clone()
            .unwrap_or_default();
        map.insert(v.name.clone(), serde_json::Value::String(val));
    }
    serde_json::to_string(&map).unwrap_or_else(|_| "{}".to_string())
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
        f
    }
}
