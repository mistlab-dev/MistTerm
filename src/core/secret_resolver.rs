//! 连接前解析密码 / SSH 私钥（本地加密库或 Vault KV）

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use crate::core::credential::{Credential, CredentialAuthKind, SecretBackend};
use crate::core::session::SessionConfig;
use crate::core::vault::{HashiCorpVaultClient, VaultKvRef, VaultSettings};

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("{0}")]
    Message(String),
    #[error("Vault: {0}")]
    Vault(#[from] crate::core::vault::VaultError),
}

/// 解析后的 SSH 凭据（私钥路径可能为临时文件）
pub struct ResolvedSshSecrets {
    pub password: String,
    pub private_key_path: String,
    /// 连接结束后应删除的临时私钥
    pub temp_key_file: Option<TempKeyFile>,
}

/// 受限权限的临时私钥文件，Drop 时删除
pub struct TempKeyFile(PathBuf);

impl TempKeyFile {
    pub fn path(&self) -> &PathBuf {
        &self.0
    }

    pub fn write_pem(pem: &str) -> Result<Self, ResolveError> {
        let mut path = std::env::temp_dir();
        path.push(format!("mistterm_key_{}.pem", uuid::Uuid::new_v4()));
        {
            let mut f = fs::File::create(&path)
                .map_err(|e| ResolveError::Message(format!("Failed to create temp key file: {e}")))?;
            f.write_all(pem.as_bytes())
                .map_err(|e| ResolveError::Message(format!("Failed to write temp key file: {e}")))?;
        }
        restrict_key_permissions(&path);
        Ok(Self(path))
    }
}

impl Drop for TempKeyFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(unix)]
fn restrict_key_permissions(path: &PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_mode(0o600);
        let _ = fs::set_permissions(path, perms);
    }
}

#[cfg(not(unix))]
fn restrict_key_permissions(_path: &PathBuf) {}

pub struct SecretResolver {
    vault_settings: VaultSettings,
}

impl SecretResolver {
    pub fn new(vault_settings: VaultSettings) -> Self {
        Self { vault_settings }
    }

    pub fn resolve_session(&self, session: &SessionConfig) -> Result<ResolvedSshSecrets, ResolveError> {
        let mut password = session.password.clone();
        let mut key_path = session.private_key_path.clone();
        let mut temp_key = None;

        if let SecretBackend::VaultKv {
            mount,
            path,
            field,
            version: _,
        } = &session.secret_backend
        {
            let secret = self.read_vault_field(mount, path, field)?;
            if !session.private_key_path.is_empty() || secret.contains("BEGIN") {
                let tmp = TempKeyFile::write_pem(&secret)?;
                key_path = tmp.path().display().to_string();
                temp_key = Some(tmp);
            } else {
                password = secret;
            }
        }

        Ok(ResolvedSshSecrets {
            password,
            private_key_path: key_path,
            temp_key_file: temp_key,
        })
    }

    pub fn resolve_credential(&self, cred: &Credential) -> Result<ResolvedSshSecrets, ResolveError> {
        let mut password = String::new();
        let mut key_path = String::new();
        let mut temp_key = None;

        match &cred.secret_backend {
            SecretBackend::LocalEncrypted => match cred.auth {
                CredentialAuthKind::Password | CredentialAuthKind::Token => {
                    password = cred.secret.clone();
                }
                CredentialAuthKind::SshKey => {
                    if cred.secret.contains("BEGIN") {
                        let tmp = TempKeyFile::write_pem(&cred.secret)?;
                        key_path = tmp.path().display().to_string();
                        temp_key = Some(tmp);
                    } else {
                        key_path = cred.secret.clone();
                    }
                }
            },
            SecretBackend::VaultKv {
                mount,
                path,
                field,
                version: _,
            } => {
                let secret = self.read_vault_field(mount, path, field)?;
                match cred.auth {
                    CredentialAuthKind::Password | CredentialAuthKind::Token => {
                        password = secret;
                    }
                    CredentialAuthKind::SshKey => {
                        if secret.contains("BEGIN") {
                            let tmp = TempKeyFile::write_pem(&secret)?;
                            key_path = tmp.path().display().to_string();
                            temp_key = Some(tmp);
                        } else {
                            key_path = secret;
                        }
                    }
                }
            }
        }

        Ok(ResolvedSshSecrets {
            password,
            private_key_path: key_path,
            temp_key_file: temp_key,
        })
    }

    fn read_vault_field(
        &self,
        mount: &str,
        path: &str,
        field: &str,
    ) -> Result<String, ResolveError> {
        if !self.vault_settings.enabled {
            return Err(ResolveError::Message("Vault is not enabled".into()));
        }
        let client = HashiCorpVaultClient::new(self.vault_settings.clone())?;
        let reference = VaultKvRef {
            mount: mount.to_string(),
            path: path.to_string(),
            field: if field.is_empty() {
                "password".to_string()
            } else {
                field.to_string()
            },
            version: None,
        };
        client.read_kv(&reference).map_err(ResolveError::from)
    }
}
