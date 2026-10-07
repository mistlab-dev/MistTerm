//! `update-state.json`：频繁变化、但不敏感的更新状态。
//!
//! 和用户偏好（加密的 `settings.json`）分开存放，避免每次检查都重写加密文件。

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateState {
    /// 上次成功检查的时间（Unix 秒）。
    #[serde(default)]
    pub last_check_unix: Option<i64>,
    /// 用户选择「跳过此版本」的版本号。
    #[serde(default)]
    pub skipped_version: Option<String>,
    /// 见过的最高清单版本与发布时间（防止被旧清单冻结）。
    #[serde(default)]
    pub highest_seen_version: Option<String>,
    #[serde(default)]
    pub highest_seen_pub_date: Option<String>,
    /// 已经装好、等重启后生效的版本。
    #[serde(default)]
    pub installed_pending_restart: Option<String>,
}

impl UpdateState {
    pub fn load() -> Self {
        Self::load_from(&super::paths::state_file())
    }

    pub fn load_from(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&super::paths::state_file())
    }

    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(&tmp, body)?;
        std::fs::rename(&tmp, path)
    }

    /// 清单是否比见过的还旧（版本更低，或同版本但发布时间更早）。
    pub fn is_stale(&self, version: &semver::Version, pub_date: Option<chrono::DateTime<chrono::Utc>>) -> bool {
        let Some(seen) = self
            .highest_seen_version
            .as_deref()
            .and_then(|v| semver::Version::parse(v).ok())
        else {
            return false;
        };
        if *version < seen {
            return true;
        }
        if *version == seen {
            let seen_date = self
                .highest_seen_pub_date
                .as_deref()
                .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
                .map(|d| d.with_timezone(&chrono::Utc));
            if let (Some(seen_date), Some(date)) = (seen_date, pub_date) {
                return date < seen_date;
            }
        }
        false
    }

    /// 记录一份验签通过的清单（只会往高处走）。
    pub fn record_seen(&mut self, version: &semver::Version, pub_date: &str) {
        let newer = match self
            .highest_seen_version
            .as_deref()
            .and_then(|v| semver::Version::parse(v).ok())
        {
            None => true,
            Some(seen) => {
                *version > seen
                    || (*version == seen && is_later(pub_date, self.highest_seen_pub_date.as_deref()))
            }
        };
        if newer {
            self.highest_seen_version = Some(version.to_string());
            self.highest_seen_pub_date = Some(pub_date.to_string());
        }
    }
}

fn is_later(date: &str, than: Option<&str>) -> bool {
    let parse = |s: &str| chrono::DateTime::parse_from_rfc3339(s).ok();
    match (parse(date), than.and_then(parse)) {
        (Some(a), Some(b)) => a > b,
        (Some(_), None) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> semver::Version {
        semver::Version::parse(s).unwrap()
    }

    fn d(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
        Some(chrono::DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&chrono::Utc))
    }

    #[test]
    fn stale_detection() {
        let mut st = UpdateState::default();
        assert!(!st.is_stale(&v("1.0.0"), d("2026-01-01T00:00:00Z")));
        st.record_seen(&v("1.2.0"), "2026-10-05T12:00:00Z");
        assert!(st.is_stale(&v("1.1.9"), d("2026-10-06T00:00:00Z")));
        assert!(st.is_stale(&v("1.2.0"), d("2026-10-01T00:00:00Z")));
        assert!(!st.is_stale(&v("1.2.0"), d("2026-10-05T12:00:00Z")));
        assert!(!st.is_stale(&v("1.2.1"), d("2026-09-01T00:00:00Z")));
    }

    #[test]
    fn record_seen_only_moves_up() {
        let mut st = UpdateState::default();
        st.record_seen(&v("1.2.0"), "2026-10-05T12:00:00Z");
        st.record_seen(&v("1.1.0"), "2026-11-05T12:00:00Z");
        assert_eq!(st.highest_seen_version.as_deref(), Some("1.2.0"));
        st.record_seen(&v("1.3.0"), "2026-11-05T12:00:00Z");
        assert_eq!(st.highest_seen_version.as_deref(), Some("1.3.0"));
        assert_eq!(st.highest_seen_pub_date.as_deref(), Some("2026-11-05T12:00:00Z"));
    }

    #[test]
    fn roundtrip_and_tolerates_garbage() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sub/update-state.json");
        let mut st = UpdateState::default();
        st.skipped_version = Some("1.2.0".into());
        st.save_to(&p).unwrap();
        assert_eq!(UpdateState::load_from(&p), st);
        std::fs::write(&p, b"{not json").unwrap();
        assert_eq!(UpdateState::load_from(&p), UpdateState::default());
    }
}
