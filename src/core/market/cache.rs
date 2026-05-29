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
