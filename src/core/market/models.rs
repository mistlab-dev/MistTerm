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
