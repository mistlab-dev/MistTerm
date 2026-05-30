//! 侧栏会话排序

use super::session::SessionConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SessionSortBy {
    Name,
    NameDesc,
    LastConnected,
    CreatedAt,
}

impl Default for SessionSortBy {
    fn default() -> Self {
        SessionSortBy::LastConnected
    }
}

impl SessionSortBy {
    pub const ALL: &'static [SessionSortBy] = &[
        SessionSortBy::Name,
        SessionSortBy::NameDesc,
        SessionSortBy::LastConnected,
        SessionSortBy::CreatedAt,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SessionSortBy::Name => "名称 (A→Z)",
            SessionSortBy::NameDesc => "名称 (Z→A)",
            SessionSortBy::LastConnected => "最近连接",
            SessionSortBy::CreatedAt => "创建时间",
        }
    }

    /// 侧栏标题行窄位展示（避免 ComboBox 内换行）
    pub fn short_label(self) -> &'static str {
        match self {
            SessionSortBy::Name => "A→Z",
            SessionSortBy::NameDesc => "Z→A",
            SessionSortBy::LastConnected => "最近",
            SessionSortBy::CreatedAt => "创建",
        }
    }
}

/// 排序会话列表（先过滤后调用；不改变在线/离线分组外的顺序规则）
pub fn sort_sessions(sessions: &mut [SessionConfig], sort_by: SessionSortBy) {
    sessions.sort_by(|a, b| compare_key(a, b, sort_by));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_desc_order() {
        let mut v = vec![
            SessionConfig {
                name: "b".into(),
                ..SessionConfig::default()
            },
            SessionConfig {
                name: "a".into(),
                ..SessionConfig::default()
            },
        ];
        sort_sessions(&mut v, SessionSortBy::NameDesc);
        assert_eq!(v[0].name, "b");
    }
}

fn compare_key(a: &SessionConfig, b: &SessionConfig, sort_by: SessionSortBy) -> std::cmp::Ordering {
    match sort_by {
        SessionSortBy::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        SessionSortBy::NameDesc => b.name.to_lowercase().cmp(&a.name.to_lowercase()),
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
