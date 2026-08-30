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
        decode_market_text(status.as_u16(), &text)
    }

    fn decode_error(status: StatusCode, text: String) -> MarketApiError {
        decode_market_error(status.as_u16(), &text)
    }
}

// ---- Pure market helpers (same pattern as team client)

pub(crate) fn decode_market_error(status_u16: u16, text: &str) -> MarketApiError {
    let message = serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| {
            v.get("error")
                .or_else(|| v.get("message"))
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| text.to_string());
    MarketApiError {
        status: status_u16,
        message,
    }
}

pub(crate) fn decode_market_text<T: DeserializeOwned>(
    status_u16: u16,
    text: &str,
) -> Result<T, MarketApiError> {
    if (200..300).contains(&status_u16) {
        return serde_json::from_str(text).map_err(|e| MarketApiError {
            status: status_u16,
            message: format!("JSON decode: {e}"),
        });
    }
    Err(decode_market_error(status_u16, text))
}

/// Catalog query → list of (key, value) pairs that will be passed to
/// `reqwest::RequestBuilder::query`. Returned as a plain `Vec<String>`
/// (serialized key=value) so tests can verify order & content.
pub(crate) fn catalog_query_pairs(q: &MarketCatalogQuery) -> Vec<(&'static str, String)> {
    let mut out: Vec<(&'static str, String)> = Vec::new();
    if !q.category.trim().is_empty() {
        out.push(("category", q.category.trim().to_string()));
    }
    if !q.search.trim().is_empty() {
        out.push(("search", q.search.trim().to_string()));
    }
    if q.limit > 0 {
        out.push(("limit", q.limit.to_string()));
    }
    if !q.cursor.trim().is_empty() {
        out.push(("cursor", q.cursor.trim().to_string()));
    }
    out
}

pub(crate) fn build_market_path(path: &str) -> String {
    // Market API lives under /v1/market/...
    format!("/v1/market/{path}")
}

#[cfg(test)]
mod pure_tests {
    use super::*;

    // ------------------------------------------------ new + base_url

    #[test]
    fn new_rejects_empty_or_all_slashes_base() {
        let e = match MarketClient::new("") {
            Ok(_) => panic!("expected Err for empty base"),
            Err(e) => e,
        };
        assert!(e.contains("empty"));
        let e2 = match MarketClient::new(" //// ") {
            Ok(_) => panic!("expected Err for all-slashes base"),
            Err(e) => e,
        };
        assert!(e2.contains("empty"));
    }

    #[test]
    fn new_ok_adds_https_for_plain_host() {
        let c = MarketClient::new("mkt.example.com").unwrap();
        assert_eq!(c.base_url, "https://mkt.example.com");
    }

    // ------------------------------------------------ decode_market_error

    #[test]
    fn market_error_prefers_error_key_then_message_key() {
        let a = decode_market_error(400, r#"{"error":"rate limited"}"#);
        assert_eq!(a.status, 400);
        assert_eq!(a.message, "rate limited");
        let b = decode_market_error(500, r#"{"message":"oops"}"#);
        assert_eq!(b.message, "oops");
    }

    #[test]
    fn market_error_falls_back_to_raw_body() {
        let a = decode_market_error(502, "gateway timeout");
        assert_eq!(a.message, "gateway timeout");
        let b = decode_market_error(422, r#"{"error":""}"#);
        assert_eq!(b.message, r#"{"error":""}"#);
    }

    // ------------------------------------------------ decode_market_text

    #[test]
    fn decode_market_success_roundtrip_catalog() {
        let body = r#"{"catalog_version":"cv1","cursor":"c","fragments":[{"id":"x","title":"t","command":"c"}]}"#;
        let r: MarketCatalogResponse = decode_market_text(200, body).unwrap();
        assert_eq!(r.catalog_version, "cv1");
        assert_eq!(r.cursor, "c");
        assert_eq!(r.fragments.len(), 1);
        assert_eq!(r.fragments[0].id, "x");
    }

    #[test]
    fn decode_market_success_invalid_json_returns_decode_error() {
        let e = decode_market_text::<MarketCatalogResponse>(200, "{invalid").unwrap_err();
        assert!(e.message.contains("JSON decode"));
        assert_eq!(e.status, 200);
    }

    #[test]
    fn decode_market_failure_uses_error_decoder() {
        let e = decode_market_text::<MarketCatalogResponse>(
            403,
            r#"{"message":"denied"}"#
        ).unwrap_err();
        assert_eq!(e.status, 403);
        assert_eq!(e.message, "denied");
    }

    // ------------------------------------------------ catalog_query_pairs

    #[test]
    fn catalog_empty_query_emits_no_pairs() {
        let q = MarketCatalogQuery::default();
        assert!(catalog_query_pairs(&q).is_empty());
    }

    #[test]
    fn catalog_pairs_appear_in_declared_order_and_trim() {
        let q = MarketCatalogQuery {
            category: "  ops  ".into(),
            search: " nginx  ".into(),
            limit: 100,
            cursor: "  cur-0 ".into(),
        };
        let pairs = catalog_query_pairs(&q);
        assert_eq!(pairs.len(), 4);
        assert_eq!(pairs[0], ("category", "ops".into()));
        assert_eq!(pairs[1], ("search", "nginx".into()));
        assert_eq!(pairs[2], ("limit", "100".into()));
        assert_eq!(pairs[3], ("cursor", "cur-0".into()));
    }

    #[test]
    fn catalog_zero_limit_is_omitted() {
        let q = MarketCatalogQuery {
            limit: 0,
            search: "any".into(),
            ..Default::default()
        };
        let pairs = catalog_query_pairs(&q);
        // limit=0 is skipped; only search pair is present.
        assert!(!pairs.iter().any(|(k, _)| *k == "limit"));
        assert_eq!(pairs.len(), 1);
    }

    // ------------------------------------------------ build_market_path

    #[test]
    fn build_market_path_prefixes_v1_market() {
        assert_eq!(
            build_market_path("fragments/catalog"),
            "/v1/market/fragments/catalog"
        );
    }

    // ------------------------------------------------ MarketApiError Display

    #[test]
    fn market_api_error_display_format() {
        let e = MarketApiError { status: 404, message: "gone".into() };
        assert_eq!(format!("{e}"), "HTTP 404: gone");
    }
}
