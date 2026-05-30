//! 安全模块
//!
//! 提供系统密钥链集成，安全存储密码和敏感信息

pub mod device_key;
pub mod encrypted_file;
mod keyring;

pub use keyring::{CredentialManager, CredentialStore, KeyringError};
