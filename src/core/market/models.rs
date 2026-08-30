//! 片段市场数据模型（与团队片段字段对齐，便于 UI 复用）。

use serde::{Deserialize, Serialize};

use crate::core::fragment::FragmentStats;
use crate::core::team::{parse_tags_json, parse_variables_json};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketFragment {
    pub id: String,
    pub title: String,
    pub command: String,
    #[serde(default)]
    pub category: String,
    /// JSON 字符串数组，与团队片段一致
    #[serde(default)]
    pub tags: String,
    #[serde(default)]
    pub variables: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub revision: u32,
    #[serde(default)]
    pub install_count: u64,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MarketCatalogResponse {
    #[serde(default)]
    pub catalog_version: String,
    #[serde(default)]
    pub cursor: String,
    #[serde(default)]
    pub fragments: Vec<MarketFragment>,
}

#[derive(Debug, Clone, Default)]
pub struct MarketCatalogQuery {
    pub category: String,
    pub search: String,
    pub limit: u32,
    pub cursor: String,
}

impl MarketFragment {
    pub fn to_fragment_stats(&self) -> FragmentStats {
        let mut f = FragmentStats::new(
            format!("mkt-preview-{}", self.id),
            self.title.clone(),
            self.command.clone(),
            if self.category.is_empty() {
                "market".to_string()
            } else {
                self.category.clone()
            },
        );
        f.tags = parse_tags_json(&self.tags);
        if !f.tags.iter().any(|t| t.eq_ignore_ascii_case("market")) {
            f.tags.push("market".into());
        }
        f.tags.push(format!("mkt:{}", self.id));
        f.variables = parse_variables_json(&self.variables);
        f
    }

    pub fn market_source_tag(&self) -> String {
        format!("mkt:{}", self.id)
    }
}

pub fn install_into_personal_library(
    manager: &mut crate::core::FragmentManager,
    item: &MarketFragment,
) -> Result<(), String> {
    let source = item.market_source_tag();
    if manager.get_all().iter().any(|f| {
        f.tags
            .iter()
            .any(|t| t.eq_ignore_ascii_case(&source))
    }) {
        return Err("already_installed".into());
    }
    let mut tags = parse_tags_json(&item.tags);
    if !tags.iter().any(|t| t.eq_ignore_ascii_case("market")) {
        tags.push("market".into());
    }
    tags.push(source);
    manager.add_fragment_with_all(
        item.title.clone(),
        item.command.clone(),
        if item.category.is_empty() {
            "market".to_string()
        } else {
            item.category.clone()
        },
        tags,
        parse_variables_json(&item.variables),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::fragment::FragmentManager;

    fn sample_market_fragment() -> MarketFragment {
        MarketFragment {
            id: "frag-1".into(),
            title: "Restart Nginx".into(),
            command: "sudo systemctl restart nginx".into(),
            category: "ops".into(),
            tags: r#"["linux","web"]"#.into(),
            variables: r#"{"signal":"SIGTERM"}"#.into(),
            description: "Safely restart nginx".into(),
            author: "ops-team".into(),
            revision: 3,
            install_count: 1234,
            updated_at: Some("2025-01-02T03:04:05Z".into()),
        }
    }

    // ---------------------------------------------------------- serde roundtrip / defaults

    #[test]
    fn market_fragment_serde_roundtrip_preserves_fields() {
        let f = sample_market_fragment();
        let json = serde_json::to_string(&f).unwrap();
        let r: MarketFragment = serde_json::from_str(&json).unwrap();
        assert_eq!(r.id, "frag-1");
        assert_eq!(r.title, "Restart Nginx");
        assert_eq!(r.command, "sudo systemctl restart nginx");
        assert_eq!(r.category, "ops");
        assert_eq!(r.tags, r#"["linux","web"]"#);
        assert_eq!(r.author, "ops-team");
        assert_eq!(r.revision, 3);
        assert_eq!(r.install_count, 1234);
        assert_eq!(r.updated_at.as_deref(), Some("2025-01-02T03:04:05Z"));
    }

    #[test]
    fn market_fragment_defaults_from_minimal_json() {
        // Fields with #[serde(default)] should kick in when omitted.
        let json = r#"{"id":"x","title":"t","command":"c"}"#;
        let f: MarketFragment = serde_json::from_str(json).unwrap();
        assert_eq!(f.id, "x");
        assert_eq!(f.title, "t");
        assert_eq!(f.command, "c");
        assert_eq!(f.category, "");
        assert_eq!(f.tags, "");
        assert_eq!(f.variables, "");
        assert_eq!(f.description, "");
        assert_eq!(f.author, "");
        assert_eq!(f.revision, 0);
        assert_eq!(f.install_count, 0);
        assert_eq!(f.updated_at, None);
    }

    #[test]
    fn market_catalog_response_defaults() {
        let r: MarketCatalogResponse = serde_json::from_str("{}").unwrap();
        assert_eq!(r.catalog_version, "");
        assert_eq!(r.cursor, "");
        assert!(r.fragments.is_empty());
    }

    // ---------------------------------------------------------- to_fragment_stats conversion

    #[test]
    fn to_fragment_stats_adds_market_tag_and_source_tag() {
        let m = sample_market_fragment();
        let f = m.to_fragment_stats();
        assert_eq!(f.id, "mkt-preview-frag-1");
        assert_eq!(f.title, "Restart Nginx");
        assert_eq!(f.command, "sudo systemctl restart nginx");
        // Category falls back to "market" when original is empty; here it's "ops".
        assert_eq!(f.category, "ops");
        // Tags JSON was ["linux","web"]; expected to be parsed + "market" prepended + mkt:<id> appended.
        assert!(f.tags.iter().any(|t| t == "linux"));
        assert!(f.tags.iter().any(|t| t == "web"));
        assert!(f.tags.iter().any(|t| t.eq_ignore_ascii_case("market")));
        assert!(f.tags.iter().any(|t| t == "mkt:frag-1"));
    }

    #[test]
    fn to_fragment_stats_empty_category_falls_back_to_market() {
        let mut m = sample_market_fragment();
        m.category = String::new();
        let f = m.to_fragment_stats();
        assert_eq!(f.category, "market");
    }

    #[test]
    fn to_fragment_stats_variables_json_parsed_into_vars() {
        let m = sample_market_fragment();
        let f = m.to_fragment_stats();
        assert_eq!(f.variables.len(), 1);
        assert_eq!(f.variables[0].name, "signal");
        assert_eq!(f.variables[0].default_value.as_deref(), Some("SIGTERM"));
    }

    #[test]
    fn to_fragment_stats_stats_fields_initialized() {
        let m = sample_market_fragment();
        let f = m.to_fragment_stats();
        assert_eq!(f.usage_count, 0);
        assert_eq!(f.success_count, 0);
        assert_eq!(f.total_time_ms, 0);
        assert_eq!(f.last_used, None);
    }

    #[test]
    fn to_fragment_stats_market_tag_not_duplicated_when_already_present() {
        let mut m = sample_market_fragment();
        // tags json already includes "market" (case-insensitive test).
        m.tags = r#"["Market","linux"]"#.into();
        let f = m.to_fragment_stats();
        // Exactly one tag case-insensitively equal to "market".
        let market_count = f
            .tags
            .iter()
            .filter(|t| t.eq_ignore_ascii_case("market"))
            .count();
        assert_eq!(market_count, 1, "market tag duplicated: {:?}", f.tags);
    }

    // ---------------------------------------------------------- market_source_tag

    #[test]
    fn market_source_tag_is_mkt_prefix_plus_id() {
        let m = sample_market_fragment();
        assert_eq!(m.market_source_tag(), "mkt:frag-1");
    }

    // ---------------------------------------------------------- install_into_personal_library

    #[test]
    fn install_returns_err_when_source_tag_already_present() {
        let mut mgr = FragmentManager::new();
        let m = sample_market_fragment();
        install_into_personal_library(&mut mgr, &m).unwrap();
        // Second attempt must fail with "already_installed" because market
        // fragment's source tag is already present on an existing fragment.
        let err = install_into_personal_library(&mut mgr, &m).unwrap_err();
        assert_eq!(err, "already_installed");
    }

    #[test]
    fn install_appends_market_tag_when_missing_and_injects_source_tag() {
        let mut mgr = FragmentManager::new();
        let mut m = sample_market_fragment();
        m.tags = r#"["linux"]"#.into(); // no market tag yet
        install_into_personal_library(&mut mgr, &m).unwrap();
        let added = mgr
            .get_all()
            .iter()
            .find(|f| f.tags.iter().any(|t| t == "mkt:frag-1"))
            .expect("installed fragment not found");
        assert!(added.tags.iter().any(|t| t.eq_ignore_ascii_case("market")));
        assert!(added.tags.iter().any(|t| t == "mkt:frag-1"));
        assert_eq!(added.category, "ops");
    }

    #[test]
    fn install_parses_variables_json_from_market_fragment() {
        let mut mgr = FragmentManager::new();
        let m = sample_market_fragment();
        install_into_personal_library(&mut mgr, &m).unwrap();
        let added = mgr
            .get_all()
            .iter()
            .find(|f| f.title == "Restart Nginx")
            .unwrap();
        assert_eq!(added.variables.len(), 1);
        assert_eq!(added.variables[0].name, "signal");
    }
}
