//! 核心业务逻辑层

pub mod session;
mod connection;
pub mod fragment;
pub mod fragment_expr;
pub mod credential;
pub mod cloud_sync;

pub use session::{SessionConfig, SessionManager};
pub use fragment::{
    expand_command_template, expand_fragment_command_stages, list_placeholder_keys,
    substitute_angle_placeholders, FragmentManager, FragmentMergeReport, FragmentStats,
    FragmentVariable, SortBy,
};
pub use fragment_expr::{expand_rhai_blocks, merge_rhai_context};
pub use credential::{
    Credential, CredentialAuthKind, CredentialCategory, CredentialVault,
};
pub use cloud_sync::CloudSyncSettings;
