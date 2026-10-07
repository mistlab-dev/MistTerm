//! 网络请求：只允许 HTTPS（测试构建另外允许 `http://127.0.0.1`），多地址按顺序回退。
//!
//! 使用 `reqwest::blocking`，**必须在独立的 `std::thread` 中调用**，不能放进 tokio 运行时。
//! 代理：自动遵循 `HTTPS_PROXY` / `ALL_PROXY` 等环境变量（reqwest 默认行为）。

use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::error::UpdateError;
use super::verify::hex_lower;

/// 是否允许回环地址上的 http（只有 `update-test` 测试构建为 true）。
pub fn allow_loopback_http() -> bool {
    cfg!(feature = "update-test")
}

/// 校验单个地址：必须 https；测试构建允许 http 到 127.0.0.1 / localhost / [::1]。
pub fn check_url_allowed(url: &str, allow_loopback_http: bool) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("bad url {url:?}: {e}"))?;
    match parsed.scheme() {
        "https" => Ok(()),
        "http" if allow_loopback_http && is_loopback_host(&parsed) => Ok(()),
        other => Err(format!("url scheme {other:?} not allowed: {url}")),
    }
}

fn is_loopback_host(url: &reqwest::Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
}

pub struct Fetcher {
    client: reqwest::blocking::Client,
    allow_loopback_http: bool,
}

impl Fetcher {
    /// 清单请求：每个地址 10 秒连接超时、20 秒总超时。
    pub fn for_manifest() -> Result<Self, UpdateError> {
        Self::build(Duration::from_secs(20))
    }

    /// 安装包下载：总超时放宽到 30 分钟（慢速网络下载 100 MB 级文件）。
    pub fn for_download() -> Result<Self, UpdateError> {
        Self::build(Duration::from_secs(30 * 60))
    }

    fn build(total_timeout: Duration) -> Result<Self, UpdateError> {
        let allow = allow_loopback_http();
        let policy = reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 10 {
                return attempt.error("too many redirects");
            }
            let ok = check_url_allowed(attempt.url().as_str(), allow).is_ok();
            if ok {
                attempt.follow()
            } else {
                attempt.error("redirect to a non-https url refused")
            }
        });
        let client = reqwest::blocking::Client::builder()
            .user_agent(super::user_agent())
            .connect_timeout(Duration::from_secs(10))
            .timeout(total_timeout)
            .https_only(!allow)
            .redirect(policy)
            .build()
            .map_err(|e| UpdateError::Network(e.to_string()))?;
        Ok(Self {
            client,
            allow_loopback_http: allow,
        })
    }

    pub fn allow_loopback_http(&self) -> bool {
        self.allow_loopback_http
    }

    /// 下载小文件（清单、签名）。非 2xx、HTML 页面或超过 `max_bytes` 都算失败。
    pub fn get_small(&self, url: &str, max_bytes: usize) -> Result<Vec<u8>, UpdateError> {
        check_url_allowed(url, self.allow_loopback_http).map_err(UpdateError::Network)?;
        let resp = self
            .client
            .get(url)
            .send()
            .map_err(|e| UpdateError::Network(short_reqwest_error(&e)))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(UpdateError::Network(format!("HTTP {} from {}", status.as_u16(), host_of(url))));
        }
        // mistlab.dev 对不存在的路径会返回首页 HTML（200）：直接判失败，不必读完整页面。
        if let Some(ct) = resp.headers().get(reqwest::header::CONTENT_TYPE) {
            if ct.to_str().unwrap_or("").to_ascii_lowercase().contains("text/html") {
                return Err(UpdateError::InvalidManifest(format!("{} returned an HTML page", host_of(url))));
            }
        }
        if resp.content_length().is_some_and(|n| n > max_bytes as u64) {
            return Err(UpdateError::InvalidManifest("response too large".into()));
        }
        let mut body = Vec::new();
        resp.take(max_bytes as u64 + 1)
            .read_to_end(&mut body)
            .map_err(|e| UpdateError::Network(e.to_string()))?;
        if body.len() > max_bytes {
            return Err(UpdateError::InvalidManifest("response too large".into()));
        }
        Ok(body)
    }

    /// 下载到 `dest`，边下边算 SHA-256；大小超过 `expected_size` 立即中止。返回实际 SHA-256。
    pub fn download_to(
        &self,
        url: &str,
        dest: &Path,
        expected_size: u64,
        progress: &mut dyn FnMut(u64, u64),
        cancel: &AtomicBool,
    ) -> Result<String, UpdateError> {
        check_url_allowed(url, self.allow_loopback_http).map_err(UpdateError::Network)?;
        let mut resp = self
            .client
            .get(url)
            .send()
            .map_err(|e| UpdateError::Network(short_reqwest_error(&e)))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(UpdateError::Network(format!("HTTP {} from {}", status.as_u16(), host_of(url))));
        }
        if resp.content_length().is_some_and(|n| n != expected_size) {
            return Err(UpdateError::SizeMismatch);
        }
        let mut file = std::fs::File::create(dest).map_err(|e| UpdateError::Install(e.to_string()))?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        let mut done: u64 = 0;
        progress(0, expected_size);
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(UpdateError::Cancelled);
            }
            let n = resp
                .read(&mut buf)
                .map_err(|e| UpdateError::Network(e.to_string()))?;
            if n == 0 {
                break;
            }
            done += n as u64;
            if done > expected_size {
                return Err(UpdateError::SizeMismatch);
            }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n])
                .map_err(|e| UpdateError::Install(e.to_string()))?;
            progress(done, expected_size);
        }
        if done != expected_size {
            return Err(UpdateError::SizeMismatch);
        }
        file.sync_all().map_err(|e| UpdateError::Install(e.to_string()))?;
        Ok(hex_lower(&hasher.finalize()))
    }
}

fn host_of(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_else(|| "server".into())
}

fn short_reqwest_error(e: &reqwest::Error) -> String {
    let host = e
        .url()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_else(|| "server".into());
    if e.is_timeout() {
        format!("{host}: timed out")
    } else if e.is_connect() {
        format!("{host}: connection failed")
    } else if e.is_redirect() {
        format!("{host}: redirect refused")
    } else {
        format!("{host}: {e}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_only_unless_loopback_test() {
        assert!(check_url_allowed("https://mistlab.dev/x", false).is_ok());
        assert!(check_url_allowed("http://mistlab.dev/x", false).is_err());
        assert!(check_url_allowed("http://127.0.0.1:8787/x", false).is_err());
        assert!(check_url_allowed("http://127.0.0.1:8787/x", true).is_ok());
        assert!(check_url_allowed("http://localhost:8787/x", true).is_ok());
        assert!(check_url_allowed("http://[::1]:8787/x", true).is_ok());
        assert!(check_url_allowed("http://10.0.0.1/x", true).is_err());
        assert!(check_url_allowed("http://127.0.0.1.evil.com/x", true).is_err());
        assert!(check_url_allowed("file:///etc/passwd", true).is_err());
        assert!(check_url_allowed("ftp://mistlab.dev/x", false).is_err());
        assert!(check_url_allowed("not a url", false).is_err());
    }
}
