//! 核心业务层（无 egui / 无 SSH 线程）
//!
//! ## 分层约定
//!
//! | 模块 | 职责 |
//! |------|------|
//! | `session` / `fragment` / `credential` | 数据模型与持久化 |
//! | `fragment_command` / `upload_policy` / `reconnect` | 纯业务规则与编排 |
//! | `connection` | 预留：单会话 SSH+终端状态（尚未接入 UI） |
//!
//! UI（`crate::ui`）只应调用本层与 `ssh` / `terminal` 的公开 API，避免在面板里写策略分支。

pub mod session;
mod connection;
pub mod fragment;
pub mod fragment_expr;
pub mod fragment_command;
pub mod credential;
pub mod cloud_sync;
pub mod reconnect;
pub mod upload_policy;
pub mod ssh_config_importer;
pub mod command_history;
pub mod session_logger;
pub mod session_sort;
pub mod audit;
pub mod app_settings;
pub mod vault;
pub mod secret_resolver;

pub use session::{SessionConfig, SessionManager, SESSION_COLOR_TAGS, session_color_tag_rgb};
pub use ssh_config_importer::{
    candidate_to_session, default_ssh_config_path, is_already_imported, parse_ssh_config_file,
    parse_ssh_config_str, pending_imports, SshConfigCandidate, SshConfigParseResult,
};
pub use command_history::{CommandHistory, HistoryEntry};
pub use session_logger::{
    cleanup_old_logs, default_log_base_dir, list_session_log_files, log_text_for_display,
    read_log_tail, spawn_cleanup_old_logs, SessionLogSettings, SessionLogWriter,
    DEFAULT_MAX_FILE_BYTES, DEFAULT_RETENTION_DAYS, LOG_TAIL_READ_BYTES,
};
pub use session_sort::{sort_sessions, SessionSortBy};
pub use fragment::{
    expand_command_template, expand_fragment_command_stages, list_placeholder_keys,
    substitute_angle_placeholders, FragmentManager, FragmentMergeReport, FragmentStats,
    FragmentVariable, SortBy,
};
pub use fragment_expr::{expand_rhai_blocks, merge_rhai_context};
pub use fragment_command::{build_fragment_command_preview, finalize_fragment_command_text};
pub use credential::{
    Credential, CredentialAuthKind, CredentialCategory, CredentialVault, SecretBackend,
};
pub use audit::{
    command_preview, AuditCategory, AuditEvent, AuditLogger, AuditOutcome, AuditSettings,
    HttpSinkSettings, SyslogSinkSettings,
};
pub use app_settings::AppSettings;
pub use vault::{
    HashiCorpVaultClient, VaultAuthSettings, VaultKvRef, VaultListEntry, VaultSettings,
};
pub use secret_resolver::{ResolvedSshSecrets, ResolveError, SecretResolver, TempKeyFile};
pub use cloud_sync::CloudSyncSettings;
pub use reconnect::{
    cleared_schedule, collect_tabs_due_for_reconnect, schedule_after_unexpected_disconnect,
    TabReconnectSchedule, ReconnectStatus, DEFAULT_MAX_RECONNECT_ATTEMPTS,
};
pub use upload_policy::{
    decide_upload_dispatch, format_bytes_short, UploadDispatch, LARGE_UPLOAD_THRESHOLD_BYTES,
};
