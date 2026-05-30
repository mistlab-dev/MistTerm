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

    #[test]
    fn parse_v2_path() {
        let (m, p, f) = parse_vault_credential_path("secret/data/ssh/db-master", "secret").unwrap();
        assert_eq!(m, "secret");
        assert_eq!(p, "ssh/db-master");
        assert_eq!(f, "password");
    }
}
