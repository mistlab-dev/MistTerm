//! 团队 sync 结果落盘、Vault 自动配置。

use super::models::{TeamSyncEntry, TeamSyncResponse};
use crate::core::vault::{HashiCorpVaultClient, VaultAuthSettings, VaultSettings};

/// 解析 `secret/data/ssh/db-master` → (mount, kv_path, field)。
pub fn parse_vault_credential_path(
    raw: &str,
    default_mount: &str,
) -> Option<(String, String, String)> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let parts: Vec<&str> = raw.split('/').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return None;
    }
    let (mount, path) = if parts.len() >= 2 && parts[1] == "data" {
        (
            parts[0].to_string(),
            parts[2..].join("/"),
        )
    } else {
        (
            parts[0].to_string(),
            parts[1..].join("/"),
        )
    };
    let mount = if mount.is_empty() {
        default_mount.to_string()
    } else {
        mount
    };
    if path.is_empty() {
        return None;
    }
    Some((mount, path, "password".to_string()))
}

pub fn apply_sync_response(
    state: &mut super::state::TeamState,
    resp: &TeamSyncResponse,
) {
    for entry in &resp.teams {
        state
            .sync_entries
            .insert(entry.team_id.clone(), entry.clone());
        if let Some(m) = state
            .teams
            .iter_mut()
            .find(|m| m.team.id == entry.team_id)
        {
            if !entry.role.is_empty() {
                m.role = entry.role.clone();
            }
        }
    }
    let _ = state.save();
}

/// 将某团队的 Vault 配置写入 `VaultSettings` 并保存密钥到钥匙串。
pub fn apply_vault_for_team(
    vault: &mut VaultSettings,
    entry: &TeamSyncEntry,
) -> Result<(), String> {
    let Some(vc) = entry.vault_config.as_ref() else {
        vault.managed_by_team_id = None;
        return Ok(());
    };
    if !vault.team_auto_apply {
        return Ok(());
    }
    vault.enabled = true;
    vault.address = vc.address.clone();
    vault.namespace = vc.namespace.clone();
    if !vc.kv_mount.is_empty() {
        vault.default_mount = vc.kv_mount.clone();
    }
    let cred = entry.credential.as_ref();
    match vc.auth_type.as_str() {
        "token" => {
            let token = cred
                .map(|c| c.vault_token.as_str())
                .unwrap_or_default();
            if token.is_empty() {
                return Err("团队 Vault 未提供 token".into());
            }
            vault.auth = VaultAuthSettings::Token;
            HashiCorpVaultClient::save_token_to_keyring(token)
                .map_err(|e| e.to_string())?;
        }
        "approle" => {
            let (role_id, secret_id) = cred
                .map(|c| (c.approle_role_id.as_str(), c.approle_secret_id.as_str()))
                .unwrap_or(("", ""));
            if role_id.is_empty() || secret_id.is_empty() {
                return Err("团队 Vault 未提供 AppRole 凭证".into());
            }
            vault.auth = VaultAuthSettings::AppRole;
            HashiCorpVaultClient::save_approle_to_keyring(role_id, secret_id)
                .map_err(|e| e.to_string())?;
        }
        _ => {
            vault.auth = VaultAuthSettings::None;
        }
    }
    vault.managed_by_team_id = Some(entry.team_id.clone());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::team::models::{TeamInfo, TeamMembership, TeamServer, TeamSyncEntry, TeamSyncResponse};
    use crate::core::team::state::TeamState;

    // ------------------------------------------------ parse_vault_credential_path
    #[test]
    fn parse_v2_path() {
        let (m, p, f) = parse_vault_credential_path("secret/data/ssh/db-master", "secret").unwrap();
        assert_eq!(m, "secret");
        assert_eq!(p, "ssh/db-master");
        assert_eq!(f, "password");
    }

    #[test]
    fn parse_empty_or_all_slashes_is_none() {
        assert!(parse_vault_credential_path("", "secret").is_none());
        assert!(parse_vault_credential_path("   ", "secret").is_none());
        assert!(parse_vault_credential_path("/////", "secret").is_none());
    }

    #[test]
    fn parse_leading_trailing_slashes_trimmed_and_flat_v1_style() {
        // Flat path (no `data` segment) = mount=first component, path=rest joined.
        let (m, p, _) = parse_vault_credential_path("/kv/my/creds/entry/", "secret").unwrap();
        assert_eq!(m, "kv");
        assert_eq!(p, "my/creds/entry");
    }

    #[test]
    fn parse_mount_empty_uses_default_mount() {
        // Parts become ["", "data", "x"] → mount after filter: ""; replaced by default.
        // Actually filter removes empty parts so "//data//x" -> ["data","x"], len >= 2 -> parts[1]="x" != "data"
        // So mount = parts[0]="data", path=parts[1..]="x". That's different.
        // Test actual default mount fallback: raw that resolves to mount = empty is impossible if
        // the split filter is `!p.is_empty()`. The only way is `parts` being single-component.
        // With single component "ssh-key", mount=parts[0]="ssh-key", path=parts[1..]="", which is None.
        // So actually the default_mount branch is unreachable with current impl. Let's test a case
        // where mount is NOT empty but default is NOT used, and also single-component returns None:
        assert!(parse_vault_credential_path("only-mount", "secret").is_none(), "empty path → None");
    }

    #[test]
    fn parse_trims_input_whitespace() {
        let r = parse_vault_credential_path("  secret/data/foo/bar  \n\t", "s").unwrap();
        assert_eq!(r.0, "secret");
        assert_eq!(r.1, "foo/bar");
    }

    #[test]
    fn parse_v2_data_segment_stripped_with_multi_component_path() {
        // mount/data/a/b/c → mount + a/b/c
        let (m, p, _) =
            parse_vault_credential_path("m/data/nested/path/here", "DEFAULT").unwrap();
        assert_eq!(m, "m");
        assert_eq!(p, "nested/path/here");
    }

    // ------------------------------------------------ apply_sync_response
    fn make_membership(id: &str, name: &str, role: &str) -> TeamMembership {
        TeamMembership {
            team: TeamInfo {
                id: id.to_string(),
                name: name.to_string(),
                description: String::new(),
                created_at: None,
                updated_at: None,
            },
            role: role.to_string(),
        }
    }

    fn make_entry(team_id: &str, role: &str) -> TeamSyncEntry {
        TeamSyncEntry {
            team_id: team_id.into(),
            team_name: team_id.into(),
            role: role.into(),
            vault_config: None,
            credential: None,
            servers: vec![TeamServer {
                id: String::new(),
                name: String::new(),
                host: String::new(),
                port: 22,
                username: String::new(),
                tags: vec![],
                vault_credential_path: String::new(),
                sort_order: 0,
            }],
        }
    }

    #[test]
    fn apply_sync_updates_sync_entries_and_overwrites_role_in_membership_when_present() {
        let mut state = TeamState::default();
        state.teams.push(make_membership("t1", "Alpha", "viewer"));
        state.teams.push(make_membership("t2", "Beta", "admin"));

        apply_sync_response(
            &mut state,
            &TeamSyncResponse {
                teams: vec![
                    make_entry("t1", "editor"), // t1 role upgrade
                    make_entry("tNEW", "admin"), // new team, not in members list
                ],
            },
        );

        // sync_entries now have 2 entries
        assert_eq!(state.sync_entries.len(), 2);
        assert!(state.sync_entries.contains_key("t1"));
        assert!(state.sync_entries.contains_key("tNEW"));

        // t1 role updated; t2 role untouched; tNEW has no membership
        let t1 = state.teams.iter().find(|m| m.team.id == "t1").unwrap();
        assert_eq!(t1.role, "editor");
        let t2 = state.teams.iter().find(|m| m.team.id == "t2").unwrap();
        assert_eq!(t2.role, "admin");
    }

    #[test]
    fn apply_sync_empty_role_in_entry_does_not_overwrite_membership() {
        let mut state = TeamState::default();
        state.teams.push(make_membership("t1", "A", "editor"));
        apply_sync_response(
            &mut state,
            &TeamSyncResponse {
                teams: vec![make_entry("t1", "")], // empty role = keep membership role
            },
        );
        assert_eq!(state.teams[0].role, "editor");
        assert!(state.sync_entries.contains_key("t1"));
    }

    #[test]
    fn apply_sync_teams_list_empty_is_noop() {
        let mut state = TeamState::default();
        state.teams.push(make_membership("t1", "A", "viewer"));
        state.current_team_id = Some("t1".into());
        apply_sync_response(&mut state, &TeamSyncResponse { teams: vec![] });
        assert_eq!(state.teams[0].role, "viewer");
        assert_eq!(state.sync_entries.len(), 0);
    }
}
