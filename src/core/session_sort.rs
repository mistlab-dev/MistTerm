//! 侧栏会话排序

use std::collections::HashSet;

use super::session::SessionConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum SessionSortBy {
    #[default]
    Name,
    LastConnected,
    CreatedAt,
}

impl SessionSortBy {
    pub const ALL: &'static [SessionSortBy] = &[
        SessionSortBy::Name,
        SessionSortBy::LastConnected,
        SessionSortBy::CreatedAt,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SessionSortBy::Name => "名称",
            SessionSortBy::LastConnected => "最近连接",
            SessionSortBy::CreatedAt => "创建时间",
        }
    }

    /// 侧栏标题行窄位展示（避免 ComboBox 内换行）
    pub fn short_label(self) -> &'static str {
        match self {
            SessionSortBy::Name => "名称",
            SessionSortBy::LastConnected => "最近",
            SessionSortBy::CreatedAt => "创建",
        }
    }
}

/// 排序会话列表（在线优先，再按选定键）
pub fn sort_sessions(
    sessions: &mut [SessionConfig],
    sort_by: SessionSortBy,
    connected: &HashSet<String>,
) {
    sessions.sort_by(|a, b| {
        let a_on = connected.contains(&a.id);
        let b_on = connected.contains(&b.id);
        b_on.cmp(&a_on).then_with(|| compare_key(a, b, sort_by))
    });
}

fn compare_key(a: &SessionConfig, b: &SessionConfig, sort_by: SessionSortBy) -> std::cmp::Ordering {
    match sort_by {
        SessionSortBy::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        SessionSortBy::LastConnected => b
            .last_connected_at
            .unwrap_or(0)
            .cmp(&a.last_connected_at.unwrap_or(0)),
        SessionSortBy::CreatedAt => b
            .created_at
            .unwrap_or(0)
            .cmp(&a.created_at.unwrap_or(0)),
    }
}
