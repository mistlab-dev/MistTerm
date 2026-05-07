//! 核心业务逻辑层

pub mod session;
mod connection;
pub mod fragment;
pub mod credential;
pub mod cloud_sync;

pub use session::{SessionConfig, SessionManager};
pub use fragment::{
    expand_command_template, list_placeholder_keys, FragmentManager, FragmentMergeReport,
    FragmentStats, SortBy,
};
pub use credential::{
    Credential, CredentialAuthKind, CredentialCategory, CredentialVault,
};
pub use cloud_sync::CloudSyncSettings;
