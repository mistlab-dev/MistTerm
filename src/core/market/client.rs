//! 片段市场 HTTP 客户端。

use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::StatusCode;
use serde::de::DeserializeOwned;

use super::models::{MarketCatalogQuery, MarketCatalogResponse};
use crate::core::team::normalize_api_base;

#[derive(Debug, Clone)]
pub struct MarketApiError {
    pub status: u16,
    pub message: String,
}

impl std::fmt::Display for MarketApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP {}: {}", self.status, self.message)
    }
}

impl std::error::Error for MarketApiError {}

pub struct MarketClient {
    base_url: String,
    http: Client,
}

impl MarketClient {
    pub fn new(api_base: &str) -> Result<Self, String> {
        let base_url = normalize_api_base(api_base);
        if base_url.is_empty() {
            return Err("market API base URL is empty".into());
        }
        let http = Client::builder()
            .timeout(Duration::from_secs(45))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { base_url, http })
    }

    /// `GET /v1/market/fragments/catalog`；404 表示服务端未实现。
    pub fn fetch_catalog(
        &self,
        bearer: Option<&str>,
        query: &MarketCatalogQuery,
    ) -> Result<MarketCatalogResponse, MarketApiError> {
        let mut req = self
            .http
            .get(self.url("/v1/market/fragments/catalog"));
        if !query.category.trim().is_empty() {
            req = req.query(&[("category", query.category.trim())]);
        }
        if !query.search.trim().is_empty() {
            req = req.query(&[("search", query.search.trim())]);
        }
        if query.limit > 0 {
            req = req.query(&[("limit", &query.limit.to_string())]);
        }
        if !query.cursor.trim().is_empty() {
            req = req.query(&[("cursor", query.cursor.trim())]);
        }
        if let Some(t) = bearer {
            req = req.bearer_auth(t);
        }
        let resp = req.send().map_err(|e| MarketApiError {
            status: 0,
            message: e.to_string(),
        })?;
        if resp.status() == StatusCode::NOT_FOUND {
            return Err(MarketApiError {
                status: 404,
                message: "market catalog API not deployed".into(),
            });
        }
        Self::decode_response(resp)
    }

    /// 可选：安装计数上报 `POST /v1/market/fragments/{id}/install`
    pub fn report_install(&self, bearer: Option<&str>, fragment_id: &str) -> Result<(), MarketApiError> {
        let path = format!("/v1/market/fragments/{fragment_id}/install");
        let mut req = self.http.post(self.url(&path)).json(&serde_json::json!({}));
        if let Some(t) = bearer {
            req = req.bearer_auth(t);
        }
        let resp = req.send().map_err(|e| MarketApiError {
            status: 0,
            message: e.to_string(),
        })?;
        let status = resp.status();
        if status.is_success() || status == StatusCode::NOT_FOUND {
            return Ok(());
        }
        Err(Self::decode_error(status, resp.text().unwrap_or_default()))
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn decode_response<T: DeserializeOwned>(resp: reqwest::blocking::Response) -> Result<T, MarketApiError> {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        if status.is_success() {
            serde_json::from_str(&text).map_err(|e| MarketApiError {
                status: status.as_u16(),
                message: format!("JSON decode: {e}"),
            })
        } else {
            Err(Self::decode_error(status, text))
        }
    }

    fn decode_error(status: StatusCode, text: String) -> MarketApiError {
        let message = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| {
                v.get("error")
                    .or_else(|| v.get("message"))
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string())
            })
            .filter(|s| !s.is_empty())
            .unwrap_or(text);
        MarketApiError {
            status: status.as_u16(),
            message,
        }
    }
}
