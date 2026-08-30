//! 团队 API 数据模型（与 `docs/tech/TEAM.md` §一 附录 A 对齐）。

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

/// `GET /v1/team/sync` 响应（见 `docs/tech/TEAM.md` §二）。
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

/// 团队平台下发的「可用服务器」条目（Team → Servers 列表项）。
///
/// 与本地 [`SessionConfig`] 是一对多映射：同一个团队服务器可以被多人生成本地会话，
/// 或通过 [`parse_vault_credential_path`] 共享 Vault 凭据路径。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamServer {
    /// 团队平台内稳定的服务器 ID（空字符串表示本地脱机缓存或未关联团队的旧条目）。
    #[serde(default)]
    pub id: String,
    /// 展示名称，例如「生产数据库-主」。
    pub name: String,
    /// 远端主机名或 IP（不含端口）。
    pub host: String,
    /// SSH 端口；默认 22（由 [`default_ssh_port`] 提供）。
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    /// 默认登录用户名（可为空，本地会话创建时允许覆盖）。
    #[serde(default)]
    pub username: String,
    /// 展示用标签，例如「prod」「db」「staging」，不影响连接。
    #[serde(default)]
    pub tags: Vec<String>,
    /// 团队配置的 Vault 凭据路径（如 `secret/data/ssh/db-master`），
    /// 由 [`crate::core::team::sync_config::parse_vault_credential_path`] 解析。
    #[serde(default)]
    pub vault_credential_path: String,
    /// 团队管理员设置的列表排序权重（越小越靠前；负数可置顶）。
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
    pub locked_by: String,
    #[serde(default)]
    pub locked_at: Option<String>,
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

/// `GET /v1/teams/{team_id}/fragments/analytics/members?since=7d`
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FragmentMemberAnalyticsResponse {
    #[serde(default)]
    pub members: Vec<FragmentMemberAnalyticsRow>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FragmentMemberAnalyticsRow {
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub run_count: u64,
    #[serde(default)]
    pub success_count: u64,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
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
        f.source_status = self.status.clone();
        f
    }
}

// ── Fragment version history ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentVersion {
    pub id: String,
    #[serde(default)]
    pub fragment_id: String,
    pub revision: i64,
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
    #[serde(default)]
    pub updated_by: String,
    #[serde(default)]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentVersionsResponse {
    #[serde(default)]
    pub versions: Vec<FragmentVersion>,
}

// ── External share ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalShare {
    pub id: String,
    #[serde(default)]
    pub fragment_id: String,
    #[serde(default)]
    pub team_id: String,
    pub share_token: String,
    pub title: String,
    pub command: String,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub view_count: i64,
    #[serde(default)]
    pub created_by: String,
    #[serde(default)]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateShareRequest {
    #[serde(default)]
    pub expires_in_hours: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateShareResponse {
    pub share: ExternalShare,
    pub share_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListSharesResponse {
    #[serde(default)]
    pub shares: Vec<ExternalShare>,
}

// ── Team settings ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamSettings {
    #[serde(default)]
    pub audit_retention_days: i64,
    #[serde(default)]
    pub allow_guest_access: bool,
    #[serde(default)]
    pub require_mfa: bool,
}

// ── Command-audit agents ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CmdAuditAgent {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub host: String,
    /// `active` / `offline` / 其它
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub last_seen_at: Option<String>,
    #[serde(default = "default_true_bool")]
    pub enabled: bool,
}

fn default_true_bool() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CmdAuditAgentsResponse {
    #[serde(default)]
    pub agents: Vec<CmdAuditAgent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCmdAuditAgentRequest {
    pub enabled: bool,
}

// ── Team storage usage ──

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StorageUsageBucket {
    #[serde(default)]
    pub count: u64,
    #[serde(default)]
    pub bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StorageUsageResponse {
    #[serde(default)]
    pub total_bytes: u64,
    #[serde(default)]
    pub quota_bytes: Option<u64>,
    #[serde(default)]
    pub fragments: StorageUsageBucket,
    #[serde(default)]
    pub recordings: StorageUsageBucket,
    #[serde(default)]
    pub documents: StorageUsageBucket,
    #[serde(default)]
    pub versions: StorageUsageBucket,
}

impl CmdAuditAgent {
    /// `last_seen_at` 距今超过 `stale_secs`（默认 300）视为离线。
    pub fn is_online(&self, now: chrono::DateTime<chrono::Utc>, stale_secs: i64) -> bool {
        if !self.enabled {
            return false;
        }
        let status = self.status.trim().to_ascii_lowercase();
        if status == "offline" || status == "disabled" {
            return false;
        }
        let Some(raw) = self.last_seen_at.as_deref() else {
            return status == "active" || status == "online";
        };
        let Ok(seen) = chrono::DateTime::parse_from_rfc3339(raw) else {
            return status == "active" || status == "online";
        };
        (now - seen.with_timezone(&chrono::Utc)).num_seconds() <= stale_secs
    }
}

/// SSH 会话 host 是否与 agent 登记 host 匹配（hostname / FQDN 宽松匹配）。
pub fn cmd_audit_host_matches(session_host: &str, agent_host: &str) -> bool {
    let sh = session_host.trim().to_ascii_lowercase();
    let ah = agent_host.trim().to_ascii_lowercase();
    if sh.is_empty() || ah.is_empty() {
        return false;
    }
    sh == ah
        || sh.starts_with(&format!("{ah}:"))
        || ah.starts_with(&format!("{sh}:"))
        || sh.ends_with(&format!(".{ah}"))
}

/// 当前 host 是否存在在线且启用的 agent。
pub fn cmd_audit_agent_available_for_host(
    agents: &[CmdAuditAgent],
    host: &str,
    now: chrono::DateTime<chrono::Utc>,
    stale_secs: i64,
) -> bool {
    agents.iter().any(|a| {
        cmd_audit_host_matches(host, &a.host) && a.is_online(now, stale_secs)
    })
}

#[cfg(test)]
mod cmd_audit_agent_tests {
    use super::*;

    #[test]
    fn host_matches_exact_and_suffix() {
        assert!(cmd_audit_host_matches("10.0.0.1", "10.0.0.1"));
        assert!(cmd_audit_host_matches("web.prod.example", "example"));
        assert!(!cmd_audit_host_matches("10.0.0.2", "10.0.0.1"));
    }

    #[test]
    fn agent_online_respects_stale_and_enabled() {
        let now = chrono::Utc::now();
        let recent = (now - chrono::Duration::seconds(30)).to_rfc3339();
        let agent = CmdAuditAgent {
            id: "a1".into(),
            host: "srv1".into(),
            status: "active".into(),
            last_seen_at: Some(recent),
            enabled: true,
        };
        assert!(agent.is_online(now, 300));
        let mut disabled = agent.clone();
        disabled.enabled = false;
        assert!(!disabled.is_online(now, 300));
    }

    #[test]
    fn agent_available_for_host() {
        let now = chrono::Utc::now();
        let agents = vec![CmdAuditAgent {
            id: "a1".into(),
            host: "prod-1".into(),
            status: "active".into(),
            last_seen_at: Some(now.to_rfc3339()),
            enabled: true,
        }];
        assert!(cmd_audit_agent_available_for_host(&agents, "prod-1", now, 300));
        assert!(!cmd_audit_agent_available_for_host(&agents, "prod-2", now, 300));
    }
}

#[cfg(test)]
mod serde_and_business_logic_tests {
    use super::*;

    // ------------------------------------------------------------------ TeamRole

    #[test]
    fn team_role_parse_admin_editor_and_default_viewer() {
        assert_eq!(TeamRole::parse("admin"), TeamRole::Admin);
        assert_eq!(TeamRole::parse("editor"), TeamRole::Editor);
        assert_eq!(TeamRole::parse("viewer"), TeamRole::Viewer);
        assert_eq!(TeamRole::parse("garbage"), TeamRole::Viewer);
        assert_eq!(TeamRole::parse(""), TeamRole::Viewer);
    }

    #[test]
    fn team_role_permission_grid() {
        use TeamRole::*;
        // Viewer
        assert!(!Viewer.can_edit());
        assert!(!Viewer.can_delete());
        // Editor
        assert!(Editor.can_edit());
        assert!(!Editor.can_delete());
        // Admin
        assert!(Admin.can_edit());
        assert!(Admin.can_delete());
    }

    #[test]
    fn team_role_serde_lowercase() {
        let j = serde_json::to_string(&TeamRole::Admin).unwrap();
        assert_eq!(j, "\"admin\"");
        let r: TeamRole = serde_json::from_str("\"editor\"").unwrap();
        assert_eq!(r, TeamRole::Editor);
    }

    // -------------------------------------------------------------- TeamMembership role_enum

    #[test]
    fn membership_role_enum_matches_string() {
        let m = TeamMembership {
            team: TeamInfo {
                id: "t".into(),
                name: "n".into(),
                description: String::new(),
                created_at: None,
                updated_at: None,
            },
            role: "admin".into(),
        };
        assert_eq!(m.role_enum(), TeamRole::Admin);
    }

    // -------------------------------------------------------------- TeamServer defaults + list_key

    #[test]
    fn team_server_default_port_22_via_serde() {
        // JSON omits `port`; should fallback via default_ssh_port = 22.
        let s: TeamServer = serde_json::from_str(
            r#"{"name":"srv","host":"example.com"}"#
        ).unwrap();
        assert_eq!(s.port, 22);
        assert_eq!(s.name, "srv");
        assert_eq!(s.host, "example.com");
    }

    #[test]
    fn team_server_list_key_uses_id_when_present_or_host_port_name() {
        let with_id = TeamServer {
            id: "id-123".into(),
            name: "ignored".into(),
            host: "ignored".into(),
            port: 9999,
            username: String::new(),
            tags: vec![],
            vault_credential_path: String::new(),
            sort_order: 0,
        };
        assert_eq!(with_id.list_key(), "id-123");

        let without_id = TeamServer {
            id: String::new(),
            name: "bastion".into(),
            host: "10.0.0.1".into(),
            port: 2222,
            username: String::new(),
            tags: vec![],
            vault_credential_path: String::new(),
            sort_order: 0,
        };
        assert_eq!(without_id.list_key(), "10.0.0.1:2222:bastion");
    }

    #[test]
    fn team_server_serde_roundtrip_preserves_fields() {
        let s = TeamServer {
            id: "s1".into(),
            name: "db".into(),
            host: "db.corp".into(),
            port: 2222,
            username: "app".into(),
            tags: vec!["prod".into(), "db".into()],
            vault_credential_path: "kv/env/app".into(),
            sort_order: -5,
        };
        let json = serde_json::to_string(&s).unwrap();
        let r: TeamServer = serde_json::from_str(&json).unwrap();
        assert_eq!(r.id, "s1");
        assert_eq!(r.port, 2222);
        assert_eq!(r.tags, vec!["prod", "db"]);
        assert_eq!(r.sort_order, -5);
        assert_eq!(r.vault_credential_path, "kv/env/app");
    }

    // -------------------------------------------------------------- parse_tags_json / parse_variables_json

    #[test]
    fn parse_tags_json_empty_inputs() {
        assert!(parse_tags_json("").is_empty());
        assert!(parse_tags_json("   \t\n").is_empty());
    }

    #[test]
    fn parse_tags_json_array_parsed_ordered() {
        assert_eq!(
            parse_tags_json(r#"["a","b","c"]"#),
            vec!["a".to_string(), "b".into(), "c".into()]
        );
    }

    #[test]
    fn parse_tags_json_invalid_json_returns_empty() {
        assert!(parse_tags_json("NOT JSON").is_empty());
    }

    #[test]
    fn parse_variables_json_empty_inputs() {
        assert!(parse_variables_json("").is_empty());
        assert!(parse_variables_json(" \n").is_empty());
        assert!(parse_variables_json("NOT JSON").is_empty());
    }

    #[test]
    fn parse_variables_json_object_with_string_defaults() {
        let vars = parse_variables_json(
            r#"{"host":"default.example","port":"2222"}"#
        );
        assert_eq!(vars.len(), 2);
        assert_eq!(vars[0].name, "host");
        assert_eq!(vars[0].default_value.as_deref(), Some("default.example"));
        assert_eq!(vars[1].name, "port");
        assert_eq!(vars[1].default_value.as_deref(), Some("2222"));
    }

    #[test]
    fn parse_variables_json_object_with_integer_defaults_coerced_to_str() {
        let vars = parse_variables_json(r#"{"n":42}"#);
        assert_eq!(vars.len(), 1);
        assert_eq!(vars[0].name, "n");
        assert_eq!(vars[0].default_value.as_deref(), Some("42"));
    }

    #[test]
    fn parse_variables_json_primitive_values_return_empty() {
        // Only object form is supported; array/string primitives should be empty.
        assert!(parse_variables_json(r#"["a","b"]"#).is_empty());
        assert!(parse_variables_json(r#""str""#).is_empty());
    }

    // -------------------------------------------------------------- default_sync_limit helper

    #[test]
    fn default_sync_limit_is_500() {
        assert_eq!(default_sync_limit(), 500);
    }

    #[test]
    fn fragment_sync_request_default_limit_when_missing() {
        let r: FragmentSyncRequest =
            serde_json::from_str(r#"{"cursor":"c"}"#).unwrap();
        assert_eq!(r.cursor, "c");
        assert_eq!(r.limit, 500);
    }

    // -------------------------------------------------------------- TeamsListResponse + ApiErrorBody serde defaults

    #[test]
    fn teams_list_response_requires_teams_field_in_json() {
        // TeamsListResponse has no #[serde(default)] on teams; empty JSON must fail.
        let err = serde_json::from_str::<TeamsListResponse>("{}").unwrap_err();
        assert!(err.to_string().contains("missing field `teams`"));
    }

    #[test]
    fn teams_list_response_parses_list_ordered() {
        let json = r#"{
          "teams": [
            {"team":{"id":"t1","name":"Ops"}, "role":"admin"},
            {"team":{"id":"t2","name":"Qa"},  "role":"viewer"}
          ]
        }"#;
        let r: TeamsListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.teams.len(), 2);
        assert_eq!(r.teams[0].team.id, "t1");
        assert_eq!(r.teams[0].role, "admin");
        assert_eq!(r.teams[1].team.name, "Qa");
    }

    #[test]
    fn api_error_body_defaults_apply() {
        // `error` missing -> ""; `server_version` missing -> None.
        let e: ApiErrorBody = serde_json::from_str("{}").unwrap();
        assert_eq!(e.error, "");
        assert!(e.server_version.is_none());
    }
}
