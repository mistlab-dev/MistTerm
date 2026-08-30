//! 市场片段目录本地缓存。

use std::io;
use std::path::PathBuf;

use super::models::MarketCatalogResponse;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct MarketFragmentCache {
    #[serde(default)]
    pub catalog_version: String,
    #[serde(default)]
    pub cursor: String,
    #[serde(default)]
    pub fragments: Vec<super::models::MarketFragment>,
    #[serde(default)]
    pub fetched_at: Option<i64>,
}

impl MarketFragmentCache {
    pub fn cache_path() -> PathBuf {
        let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        p.push("mistterm");
        p.push("market_fragments_cache.json");
        p
    }

    pub fn load() -> Self {
        crate::security::encrypted_file::load_encrypted_json(&Self::cache_path())
    }

    pub fn save(&self) -> io::Result<()> {
        crate::security::encrypted_file::save_encrypted_json(&Self::cache_path(), self)
    }

    pub fn apply_response(&mut self, resp: &MarketCatalogResponse) {
        self.catalog_version = resp.catalog_version.clone();
        self.cursor = resp.cursor.clone();
        self.fragments = resp.fragments.clone();
        self.touch_fetched();
    }

    /// 追加下一页（按 `id` 去重）。
    pub fn append_response(&mut self, resp: &MarketCatalogResponse) {
        if !resp.catalog_version.is_empty() {
            self.catalog_version = resp.catalog_version.clone();
        }
        self.cursor = resp.cursor.clone();
        for frag in &resp.fragments {
            if let Some(i) = self.fragments.iter().position(|f| f.id == frag.id) {
                self.fragments[i] = frag.clone();
            } else {
                self.fragments.push(frag.clone());
            }
        }
        self.touch_fetched();
    }

    fn touch_fetched(&mut self) {
        self.fetched_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::market::models::{MarketCatalogResponse, MarketFragment};

    fn mf(id: &str, title: &str) -> MarketFragment {
        MarketFragment {
            id: id.to_string(),
            title: title.to_string(),
            command: format!("echo {id}"),
            category: "ops".to_string(),
            tags: "[]".to_string(),
            variables: "{}".to_string(),
            description: String::new(),
            author: String::new(),
            revision: 1,
            install_count: 0,
            updated_at: None,
        }
    }

    // --------------------------------------------------- default / serde
    #[test]
    fn default_fields_are_all_empty_or_none() {
        let c = MarketFragmentCache::default();
        assert_eq!(c.catalog_version, "");
        assert_eq!(c.cursor, "");
        assert!(c.fragments.is_empty());
        assert!(c.fetched_at.is_none());
    }

    #[test]
    fn serde_default_roundtrip() {
        let c: MarketFragmentCache = serde_json::from_str("{}").unwrap();
        assert!(c.fragments.is_empty());
        assert_eq!(c.catalog_version, "");
        let rt = serde_json::to_string(&MarketFragmentCache::default()).unwrap();
        let c2: MarketFragmentCache = serde_json::from_str(&rt).unwrap();
        assert!(c2.fragments.is_empty());
        assert!(c2.fetched_at.is_none());
    }

    // --------------------------------------------------- apply_response
    #[test]
    fn apply_response_replaces_fragments_and_updates_metadata() {
        let mut c = MarketFragmentCache::default();
        c.apply_response(&MarketCatalogResponse {
            catalog_version: "v1".into(),
            cursor: "cur-1".into(),
            fragments: vec![mf("a", "A"), mf("b", "B")],
        });
        assert_eq!(c.catalog_version, "v1");
        assert_eq!(c.cursor, "cur-1");
        assert_eq!(c.fragments.len(), 2);
        assert!(c.fetched_at.is_some(), "fetched_at not stamped by apply_response");

        let before = c.fetched_at;
        // Re-apply should replace the list (not append) and update stamp.
        // (sleep small to ensure timestamp changes on systems with 1s granularity)
        std::thread::sleep(std::time::Duration::from_millis(1100));
        c.apply_response(&MarketCatalogResponse {
            catalog_version: "v2".into(),
            cursor: "cur-2".into(),
            fragments: vec![mf("c", "C")],
        });
        assert_eq!(c.fragments.len(), 1);
        assert_eq!(c.fragments[0].id, "c");
        assert!(c.fetched_at > before, "fetched_at should have advanced");
    }

    // --------------------------------------------------- append_response
    #[test]
    fn append_response_dedupes_preserves_new_and_updates_cursor() {
        let mut c = MarketFragmentCache::default();
        c.apply_response(&MarketCatalogResponse {
            catalog_version: "v-initial".into(),
            cursor: "p0".into(),
            fragments: vec![mf("a", "A"), mf("b", "B")],
        });
        let first_fetch = c.fetched_at.expect("must stamp");

        std::thread::sleep(std::time::Duration::from_millis(1100));
        let mut b_rev = mf("b", "B'");
        b_rev.revision = 99;
        c.append_response(&MarketCatalogResponse {
            catalog_version: String::new(), // empty -> keep original v-initial
            cursor: "p1".into(),
            fragments: vec![b_rev, mf("c", "C")],
        });
        assert_eq!(c.catalog_version, "v-initial", "empty version should not clobber");
        assert_eq!(c.cursor, "p1", "cursor always updates");
        assert_eq!(c.fragments.len(), 3, "dedupe expected: a,b,c = 3 unique");
        let mut ids: Vec<&str> = c.fragments.iter().map(|f| f.id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(ids, vec!["a", "b", "c"]);
        let b = c.fragments.iter().find(|f| f.id == "b").unwrap();
        assert_eq!(b.title, "B'");
        assert_eq!(b.revision, 99);
        assert!(
            c.fetched_at.expect("fetched_at missing") >= first_fetch,
            "fetched_at should have been touched by append"
        );
    }

    #[test]
    fn append_response_with_new_catalog_version_overwrites_it() {
        let mut c = MarketFragmentCache::default();
        c.apply_response(&MarketCatalogResponse {
            catalog_version: "old-v".into(),
            cursor: "p0".into(),
            fragments: vec![],
        });
        c.append_response(&MarketCatalogResponse {
            catalog_version: "new-v".into(),
            cursor: "p1".into(),
            fragments: vec![],
        });
        assert_eq!(c.catalog_version, "new-v");
    }
}
