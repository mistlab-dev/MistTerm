//! 命令片段市场（catalog 拉取 + 本地缓存 + 安装到个人库）。

mod cache;
mod client;
mod models;

pub use cache::MarketFragmentCache;
pub use client::{MarketApiError, MarketClient};
pub use models::{
    install_into_personal_library, MarketCatalogQuery, MarketCatalogResponse, MarketFragment,
};

#[derive(Debug, Clone, Default)]
pub struct MarketCatalogState {
    pub cache: MarketFragmentCache,
    pub last_error: Option<String>,
    pub api_available: bool,
    pub loading_more: bool,
}

impl MarketCatalogState {
    pub fn load() -> Self {
        Self {
            cache: MarketFragmentCache::load(),
            ..Default::default()
        }
    }

    pub fn fragments(&self) -> &[models::MarketFragment] {
        &self.cache.fragments
    }

    pub fn has_more(&self) -> bool {
        !self.cache.cursor.trim().is_empty()
    }

    pub fn refresh_blocking(
        &mut self,
        api_base: &str,
        bearer: Option<&str>,
        query: &MarketCatalogQuery,
    ) {
        self.fetch_page_blocking(api_base, bearer, query, false);
    }

    pub fn load_more_blocking(
        &mut self,
        api_base: &str,
        bearer: Option<&str>,
        query: &MarketCatalogQuery,
    ) {
        self.fetch_page_blocking(api_base, bearer, query, true);
    }

    fn fetch_page_blocking(
        &mut self,
        api_base: &str,
        bearer: Option<&str>,
        query: &MarketCatalogQuery,
        append: bool,
    ) {
        if append {
            self.loading_more = true;
        } else {
            self.last_error = None;
        }
        let client = match MarketClient::new(api_base) {
            Ok(c) => c,
            Err(e) => {
                self.last_error = Some(e);
                self.loading_more = false;
                return;
            }
        };
        let mut q = query.clone();
        if append {
            q.cursor = self.cache.cursor.clone();
            if q.cursor.is_empty() {
                self.loading_more = false;
                return;
            }
        }
        match client.fetch_catalog(bearer, &q) {
            Ok(resp) => {
                self.api_available = true;
                if append {
                    self.cache.append_response(&resp);
                } else {
                    self.cache.apply_response(&resp);
                }
                if let Err(e) = self.cache.save() {
                    log::warn!("market cache save: {e}");
                }
            }
            Err(e) if e.status == 404 => {
                self.api_available = false;
                self.last_error = Some(format!("catalog_not_deployed: {}", e.message));
            }
            Err(e) => {
                self.last_error = Some(e.to_string());
            }
        }
        self.loading_more = false;
    }

    pub fn to_fragment_stats_list(&self) -> Vec<crate::core::FragmentStats> {
        self.cache
            .fragments
            .iter()
            .map(|f| f.to_fragment_stats())
            .collect()
    }

    pub fn report_install_blocking(
        &self,
        api_base: &str,
        bearer: Option<&str>,
        fragment_id: &str,
    ) {
        if let Ok(client) = MarketClient::new(api_base) {
            let _ = client.report_install(bearer, fragment_id);
        }
    }
}
