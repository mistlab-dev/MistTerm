//! 更新清单 `latest.json` 的结构与校验。
//!
//! 只能在**验签通过之后**调用 [`Manifest::parse_and_validate`]（见 `verify.rs`）。
//! 未知字段会被忽略，方便以后在清单里加字段而不影响老客户端。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::error::UpdateError;
use super::PRODUCT;

/// 当前客户端能理解的清单格式版本。
pub const MANIFEST_SCHEMA: u32 = 1;

/// 清单最大字节数（正常只有 1–3 KB）；超过即视为无效，避免被塞入超大响应。
pub const MAX_MANIFEST_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub product: String,
    pub channel: String,
    pub version: String,
    /// RFC 3339 UTC，例如 `2026-10-05T12:00:00Z`。
    pub pub_date: String,
    #[serde(default)]
    pub notes_url: Option<String>,
    #[serde(default)]
    pub notes: Notes,
    pub platforms: BTreeMap<String, PlatformAsset>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notes {
    #[serde(default)]
    pub zh: String,
    #[serde(default)]
    pub en: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformAsset {
    /// `tar.gz` / `zip` / `inno-setup`。
    pub kind: String,
    /// 文件名（不含路径）。
    pub name: String,
    pub size: u64,
    /// 小写十六进制 SHA-256。
    pub sha256: String,
    /// Linux：运行所需的最低 glibc 版本，例如 `2.39`。
    #[serde(default)]
    pub min_glibc: Option<String>,
    /// 服务器端开关：`false` 时客户端只提醒、不自动安装（例如 macOS）。
    #[serde(default)]
    pub auto_update: bool,
    /// 下载地址，按顺序尝试（顺序由发布流水线决定，可配置）。
    pub urls: Vec<String>,
    /// 只提醒时引导用户打开的页面。
    #[serde(default)]
    pub manual_url: Option<String>,
}

/// 平台条目的键名。
pub mod platform_keys {
    pub const LINUX_X86_64: &str = "linux-x86_64";
    pub const WINDOWS_X86_64_SETUP: &str = "windows-x86_64-setup";
    pub const WINDOWS_X86_64_PORTABLE: &str = "windows-x86_64-portable";
    pub const MACOS_UNIVERSAL: &str = "macos-universal";
}

pub const KIND_TAR_GZ: &str = "tar.gz";
pub const KIND_ZIP: &str = "zip";
pub const KIND_INNO_SETUP: &str = "inno-setup";

impl Manifest {
    /// 解析并校验清单。`allow_loopback_http` 只在测试构建中为 true。
    pub fn parse_and_validate(
        bytes: &[u8],
        expected_channel: &str,
        allow_loopback_http: bool,
    ) -> Result<Self, UpdateError> {
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(UpdateError::InvalidManifest("too large".into()));
        }
        // 先只读 schema，格式升级时给出明确提示而不是笼统的解析错误。
        #[derive(Deserialize)]
        struct SchemaOnly {
            schema: u32,
        }
        let schema: SchemaOnly = serde_json::from_slice(bytes)
            .map_err(|e| UpdateError::InvalidManifest(format!("json: {e}")))?;
        if schema.schema != MANIFEST_SCHEMA {
            return Err(UpdateError::UnsupportedSchema(schema.schema));
        }
        let m: Manifest = serde_json::from_slice(bytes)
            .map_err(|e| UpdateError::InvalidManifest(format!("json: {e}")))?;
        m.validate(expected_channel, allow_loopback_http)?;
        Ok(m)
    }

    fn validate(&self, expected_channel: &str, allow_loopback_http: bool) -> Result<(), UpdateError> {
        let bad = |msg: String| Err(UpdateError::InvalidManifest(msg));
        if self.product != PRODUCT {
            return bad(format!("product {:?}", self.product));
        }
        if self.channel != expected_channel {
            return bad(format!("channel {:?} (expected {expected_channel:?})", self.channel));
        }
        if semver::Version::parse(&self.version).is_err() {
            return bad(format!("version {:?}", self.version));
        }
        if chrono::DateTime::parse_from_rfc3339(&self.pub_date).is_err() {
            return bad(format!("pub_date {:?}", self.pub_date));
        }
        for (key, asset) in &self.platforms {
            asset
                .validate(allow_loopback_http)
                .map_err(|e| UpdateError::InvalidManifest(format!("{key}: {e}")))?;
        }
        Ok(())
    }

    pub fn semver(&self) -> semver::Version {
        semver::Version::parse(&self.version).unwrap_or_else(|_| semver::Version::new(0, 0, 0))
    }

    pub fn pub_date_utc(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        chrono::DateTime::parse_from_rfc3339(&self.pub_date)
            .ok()
            .map(|d| d.with_timezone(&chrono::Utc))
    }

    pub fn asset(&self, key: &str) -> Option<&PlatformAsset> {
        self.platforms.get(key)
    }

    /// 更新说明：优先当前语言，缺了用另一种。
    pub fn notes_for(&self, zh: bool) -> &str {
        let (first, second) = if zh {
            (&self.notes.zh, &self.notes.en)
        } else {
            (&self.notes.en, &self.notes.zh)
        };
        if first.trim().is_empty() {
            second
        } else {
            first
        }
    }

    /// 签名的可信注释必须严格等于这一行，防止拿别的渠道或旧版的签名来替换。
    pub fn expected_trusted_comment(&self) -> String {
        format!("{} {} {} {}", self.product, self.channel, self.version, self.pub_date)
    }
}

impl PlatformAsset {
    fn validate(&self, allow_loopback_http: bool) -> Result<(), String> {
        if !matches!(self.kind.as_str(), KIND_TAR_GZ | KIND_ZIP | KIND_INNO_SETUP) {
            // 未知类型不报错（可能是以后的新格式），只是本客户端不会自动安装它。
            log::info!("update manifest: unknown asset kind {:?}", self.kind);
        }
        if self.name.is_empty()
            || self.name.contains(['/', '\\'])
            || self.name == "."
            || self.name == ".."
        {
            return Err(format!("bad file name {:?}", self.name));
        }
        if self.sha256.len() != 64 || !self.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("bad sha256".into());
        }
        if self.size == 0 {
            return Err("size is 0".into());
        }
        if self.urls.is_empty() {
            return Err("no download urls".into());
        }
        for u in &self.urls {
            super::fetch::check_url_allowed(u, allow_loopback_http)?;
        }
        if let Some(g) = &self.min_glibc {
            if parse_glibc_version(g).is_none() {
                return Err(format!("bad min_glibc {g:?}"));
            }
        }
        Ok(())
    }
}

/// 解析 `2.39` 这样的 glibc 版本号。
pub fn parse_glibc_version(s: &str) -> Option<(u32, u32)> {
    let mut it = s.trim().split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next().unwrap_or("0").parse().ok()?;
    Some((major, minor))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn sample_json(version: &str, pub_date: &str) -> String {
        format!(
            r###"{{
  "schema": 1,
  "product": "mistterm",
  "channel": "stable",
  "version": "{version}",
  "pub_date": "{pub_date}",
  "notes_url": "https://github.com/mistlab-dev/MistTerm/releases/tag/v{version}",
  "notes": {{ "zh": "## 新功能\n- 自动更新", "en": "## New\n- Auto-update" }},
  "future_field": {{ "ignored": true }},
  "platforms": {{
    "linux-x86_64": {{
      "kind": "tar.gz", "name": "Mist-linux-x86_64.tar.gz", "size": 123,
      "sha256": "{sha}", "min_glibc": "2.39", "auto_update": true,
      "urls": ["https://github.com/mistlab-dev/MistTerm/releases/download/v{version}/Mist-linux-x86_64.tar.gz"]
    }},
    "macos-universal": {{
      "kind": "tar.gz", "name": "Mist-macos-universal.tar.gz", "size": 456,
      "sha256": "{sha}", "auto_update": false,
      "urls": ["https://github.com/mistlab-dev/MistTerm/releases/download/v{version}/Mist-macos-universal.tar.gz"],
      "manual_url": "https://mistlab.dev/download.html"
    }}
  }}
}}"###,
            sha = "a".repeat(64)
        )
    }

    #[test]
    fn parses_sample_and_ignores_unknown_fields() {
        let json = sample_json("1.2.0", "2026-10-05T12:00:00Z");
        let m = Manifest::parse_and_validate(json.as_bytes(), "stable", false).unwrap();
        assert_eq!(m.version, "1.2.0");
        assert_eq!(m.semver(), semver::Version::new(1, 2, 0));
        let linux = m.asset(platform_keys::LINUX_X86_64).unwrap();
        assert_eq!(linux.min_glibc.as_deref(), Some("2.39"));
        assert!(linux.auto_update);
        assert!(!m.asset(platform_keys::MACOS_UNIVERSAL).unwrap().auto_update);
        assert_eq!(
            m.expected_trusted_comment(),
            "mistterm stable 1.2.0 2026-10-05T12:00:00Z"
        );
        assert!(m.notes_for(true).contains("自动更新"));
        assert!(m.notes_for(false).contains("Auto-update"));
    }

    #[test]
    fn rejects_wrong_channel_and_product() {
        let json = sample_json("1.2.0", "2026-10-05T12:00:00Z");
        assert!(matches!(
            Manifest::parse_and_validate(json.as_bytes(), "beta", false),
            Err(UpdateError::InvalidManifest(_))
        ));
        let other = json.replace("\"mistterm\"", "\"other\"");
        assert!(Manifest::parse_and_validate(other.as_bytes(), "stable", false).is_err());
    }

    #[test]
    fn rejects_http_urls_and_bad_fields() {
        let json = sample_json("1.2.0", "2026-10-05T12:00:00Z");
        let http = json.replace("https://github.com", "http://github.com");
        assert!(Manifest::parse_and_validate(http.as_bytes(), "stable", false).is_err());
        let bad_sha = json.replacen(&"a".repeat(64), "zz", 1);
        assert!(Manifest::parse_and_validate(bad_sha.as_bytes(), "stable", false).is_err());
        let bad_ver = json.replace("\"1.2.0\"", "\"one\"");
        assert!(Manifest::parse_and_validate(bad_ver.as_bytes(), "stable", false).is_err());
        let bad_date = json.replace("2026-10-05T12:00:00Z", "yesterday");
        assert!(Manifest::parse_and_validate(bad_date.as_bytes(), "stable", false).is_err());
        let bad_name = json.replace("\"Mist-linux-x86_64.tar.gz\"", "\"../evil\"");
        assert!(Manifest::parse_and_validate(bad_name.as_bytes(), "stable", false).is_err());
    }

    #[test]
    fn loopback_http_only_when_allowed() {
        let json = sample_json("1.2.0", "2026-10-05T12:00:00Z").replace(
            "https://github.com/mistlab-dev/MistTerm/releases/download/v1.2.0",
            "http://127.0.0.1:8787",
        );
        assert!(Manifest::parse_and_validate(json.as_bytes(), "stable", false).is_err());
        assert!(Manifest::parse_and_validate(json.as_bytes(), "stable", true).is_ok());
    }

    #[test]
    fn newer_schema_is_reported() {
        let json = sample_json("1.2.0", "2026-10-05T12:00:00Z").replace("\"schema\": 1", "\"schema\": 2");
        assert!(matches!(
            Manifest::parse_and_validate(json.as_bytes(), "stable", false),
            Err(UpdateError::UnsupportedSchema(2))
        ));
    }

    #[test]
    fn html_is_not_a_manifest() {
        let html = b"<!doctype html><html><body>MistLab</body></html>";
        assert!(Manifest::parse_and_validate(html, "stable", false).is_err());
    }

    #[test]
    fn prerelease_versions_compare() {
        let beta = semver::Version::parse("1.2.0-beta.1").unwrap();
        let stable = semver::Version::parse("1.2.0").unwrap();
        assert!(beta < stable);
        assert!(semver::Version::parse("1.1.22").unwrap() < beta);
        assert!(semver::Version::parse("1.10.0").unwrap() > semver::Version::parse("1.9.9").unwrap());
    }

    #[test]
    fn glibc_parse() {
        assert_eq!(parse_glibc_version("2.39"), Some((2, 39)));
        assert_eq!(parse_glibc_version("2"), Some((2, 0)));
        assert_eq!(parse_glibc_version("x"), None);
    }
}
