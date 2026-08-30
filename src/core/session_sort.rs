//! 侧栏会话列表排序：策略枚举 + 排序函数
//!
//! UI 层应调用 [`crate::i18n::session_sort_popup_row`] / [`crate::i18n::session_sort_chip_short`]
//! 获取当前语言的标签，而不是直接使用本模块的 [`SessionSortBy::label`]（已标记弃用）。

use super::session::SessionConfig;

/// 会话排序的四种策略（侧栏筛选区右上 chip 循环切换）。
///
/// 内部比较逻辑在 [`sort_sessions`] 中实现：
/// - 排序前不会改动「在线/离线」的大分组顺序（分组逻辑由 UI 侧先处理）；
/// - 同一分组内按选择的字段排序，`None` 时间戳下沉到末尾。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SessionSortBy {
    /// 按会话名称的字典序升序（A→Z），数字与符号按 Unicode 排序规则。
    Name,
    /// 按会话名称的字典序降序（Z→A）。
    NameDesc,
    /// 按最后连接时间降序（最近连接过的排在最前）。
    LastConnected,
    /// 按创建时间升序（最早创建的会话排在最前）。
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

    /// 侧栏/设置菜单中用于 ComboBox/说明的长标签。
    ///
    /// ⚠️ **已弃用**：返回值硬编码中文，不支持 UI 语言切换。
    /// UI 层应调用 [`crate::i18n::session_sort_popup_row`] 获取当前语言的标签。
    #[deprecated(
        since = "1.1.1",
        note = "硬编码中文；请改用 crate::i18n::session_sort_popup_row(ctx, variant)"
    )]
    pub fn label(self) -> &'static str {
        match self {
            SessionSortBy::Name => "名称 (A→Z)",
            SessionSortBy::NameDesc => "名称 (Z→A)",
            SessionSortBy::LastConnected => "最近连接",
            SessionSortBy::CreatedAt => "创建时间",
        }
    }

    /// 侧栏标题行窄位展示（避免 ComboBox 内换行）。
    ///
    /// ⚠️ **已弃用**：返回值硬编码中文，不支持 UI 语言切换。
    /// UI 层应调用 [`crate::i18n::session_sort_chip_short`] 获取当前语言的短标签。
    #[deprecated(
        since = "1.1.1",
        note = "硬编码中文；请改用 crate::i18n::session_sort_chip_short(ctx, variant)"
    )]
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

    fn sc(name: &str, last: Option<i64>, created: Option<i64>) -> SessionConfig {
        SessionConfig {
            name: name.to_string(),
            last_connected_at: last,
            created_at: created,
            ..SessionConfig::default()
        }
    }

    #[test]
    fn name_desc_order() {
        let mut v = vec![
            sc("b", None, None),
            sc("a", None, None),
        ];
        sort_sessions(&mut v, SessionSortBy::NameDesc);
        assert_eq!(v[0].name, "b");
    }

    // ------------------------------------------------ default + constants
    #[test]
    fn default_is_last_connected_and_all_has_four_variants() {
        assert_eq!(SessionSortBy::default(), SessionSortBy::LastConnected);
        assert_eq!(SessionSortBy::ALL.len(), 4);
        assert!(SessionSortBy::ALL.contains(&SessionSortBy::Name));
        assert!(SessionSortBy::ALL.contains(&SessionSortBy::NameDesc));
        assert!(SessionSortBy::ALL.contains(&SessionSortBy::LastConnected));
        assert!(SessionSortBy::ALL.contains(&SessionSortBy::CreatedAt));
    }

    #[test]
    fn labels_have_no_duplicates_and_all_nonempty() {
        for s in SessionSortBy::ALL {
            let l = s.label();
            let sl = s.short_label();
            assert!(!l.is_empty(), "label empty for {:?}", s);
            assert!(!sl.is_empty(), "short_label empty for {:?}", s);
        }
        let mut longs: Vec<&str> = SessionSortBy::ALL.iter().map(|s| s.label()).collect();
        let mut shorts: Vec<&str> = SessionSortBy::ALL.iter().map(|s| s.short_label()).collect();
        let l_orig_len = longs.len();
        longs.sort_unstable();
        longs.dedup();
        assert_eq!(longs.len(), l_orig_len, "label() has duplicates");
        shorts.sort_unstable();
        shorts.dedup();
        assert_eq!(shorts.len(), l_orig_len, "short_label() has duplicates");
    }

    #[test]
    fn label_matches_expected_strings() {
        assert_eq!(SessionSortBy::Name.label(), "名称 (A→Z)");
        assert_eq!(SessionSortBy::NameDesc.label(), "名称 (Z→A)");
        assert_eq!(SessionSortBy::LastConnected.label(), "最近连接");
        assert_eq!(SessionSortBy::CreatedAt.label(), "创建时间");

        assert_eq!(SessionSortBy::Name.short_label(), "A→Z");
        assert_eq!(SessionSortBy::NameDesc.short_label(), "Z→A");
        assert_eq!(SessionSortBy::LastConnected.short_label(), "最近");
        assert_eq!(SessionSortBy::CreatedAt.short_label(), "创建");
    }

    // ------------------------------------------------ serde
    #[test]
    fn serde_round_trip_all_variants() {
        for s in SessionSortBy::ALL {
            let json = serde_json::to_string(s).unwrap();
            let back: SessionSortBy = serde_json::from_str(&json).unwrap();
            assert_eq!(*s, back);
        }
    }

    // ------------------------------------------------ Name (case-insensitive)
    #[test]
    fn name_sort_asc_case_insensitive_and_stable_same() {
        let mut v = vec![
            sc("B", None, None),
            sc("a", None, None),
            sc("C", None, None),
        ];
        sort_sessions(&mut v, SessionSortBy::Name);
        let names: Vec<&str> = v.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["a", "B", "C"]);
    }

    #[test]
    fn name_sort_desc_case_insensitive() {
        let mut v = vec![
            sc("a", None, None),
            sc("Zebra", None, None),
            sc("Monkey", None, None),
        ];
        sort_sessions(&mut v, SessionSortBy::NameDesc);
        let names: Vec<&str> = v.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["Zebra", "Monkey", "a"]);
    }

    // ------------------------------------------------ LastConnected (None → 0, largest first)
    #[test]
    fn last_connected_puts_largest_first_and_none_last() {
        let mut v = vec![
            sc("a", None, None),
            sc("b", Some(500), None),
            sc("c", Some(100), None),
        ];
        sort_sessions(&mut v, SessionSortBy::LastConnected);
        let names: Vec<&str> = v.iter().map(|s| s.name.as_str()).collect();
        // Descending: b=500, c=100, a=0 (None as 0)
        assert_eq!(names, vec!["b", "c", "a"]);
    }

    #[test]
    fn last_connected_equal_values_preserve_relative_order_via_stable_sort() {
        let mut v = vec![
            sc("x", Some(1000), None),
            sc("y", Some(1000), None),
            sc("z", Some(1000), None),
        ];
        sort_sessions(&mut v, SessionSortBy::LastConnected);
        let names: Vec<&str> = v.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["x", "y", "z"]);
    }

    // ------------------------------------------------ CreatedAt
    #[test]
    fn created_at_newest_first_none_treated_as_zero() {
        let mut v = vec![
            sc("s1", None, Some(10)),
            sc("s2", None, None),
            sc("s3", None, Some(100)),
            sc("s4", None, Some(50)),
        ];
        sort_sessions(&mut v, SessionSortBy::CreatedAt);
        let names: Vec<&str> = v.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["s3", "s4", "s1", "s2"]);
    }

    // ------------------------------------------------ Copy semantics
    #[test]
    fn enum_is_copy_clone_eq() {
        let a = SessionSortBy::Name;
        let b = SessionSortBy::CreatedAt;
        assert_ne!(a, b);
        let c = a;
        assert_eq!(c, SessionSortBy::Name);
        let cl = a.clone();
        assert_eq!(cl, a);
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
