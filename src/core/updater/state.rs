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
    /// 匿名统计用的随机实例 ID（#149）：首次生成后持久化，不含任何身份信息。
    #[serde(default)]
    pub client_id: Option<String>,
}

/// 状态文件完整性（#417：不再把损坏/不可读静默当成首次安装）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateIntegrity {
    /// 文件不存在：真·首次运行。
    FirstRun,
    /// 读取与解析都成功。
    Ok,
    /// 文件存在但内容不是合法 JSON / 结构不符。
    Corrupt,
    /// 文件存在但读取失败（权限/IO）。
    Unreadable,
}

impl UpdateState {
    pub fn load() -> Self {
        Self::load_from(&super::paths::state_file())
    }

    /// #417：默认状态文件路径的完整性感知加载。
    pub fn load_with_integrity() -> (Self, StateIntegrity) {
        Self::load_from_with_integrity(&super::paths::state_file())
    }

    pub fn load_from(path: &Path) -> Self {
        Self::load_from_with_integrity(path).0
    }

    /// 加载并给出完整性判定（#417 架构锁定第 2 条）。
    /// 返回 `(状态, 完整性)`；损坏/不可读时状态为 `Default` 但调用方必须据此告警，
    /// 且在检查失败时**不得**用空状态覆盖坏文件。
    pub fn load_from_with_integrity(path: &Path) -> (Self, StateIntegrity) {
        match std::fs::read(path) {
            // 文件不存在才是首次运行；存在但读失败（权限/IO/EISDIR）= Unreadable。
            Err(_) if !path.exists() => (Self::default(), StateIntegrity::FirstRun),
            Err(_) => (Self::default(), StateIntegrity::Unreadable),
            Ok(bytes) => match serde_json::from_slice::<UpdateState>(&bytes) {
                Ok(st) => (st, StateIntegrity::Ok),
                Err(_) => (Self::default(), StateIntegrity::Corrupt),
            },
        }
    }

    /// 文件是否存在（区分 FirstRun 与 Unreadable 用）。
    pub fn file_exists(path: &Path) -> bool {
        path.exists()
    }

    /// #417：损坏状态的安全重建——先把坏文件备份为 `<name>.bak-corrupt`，
    /// 备份失败则**不写盘**（保留坏原件，调用方告警）。
    pub fn rebuild_over_corrupt(&self, path: &Path) -> std::io::Result<()> {
        let bak = path.with_extension("json.bak-corrupt");
        match std::fs::copy(path, &bak) {
            Ok(_) => self.save_to(path),
            // 备份失败：不强行覆盖，保留现场。
            Err(e) => Err(std::io::Error::other(format!(
                "backup corrupt state failed ({}), not overwriting",
                e
            ))),
        }
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
    ///
    /// #417 架构锁定：下限改为 `max(最高已见, 当前运行版本)`——状态被删/损坏
    /// 重置后，「最高已见」丢失，当前运行版本仍兜住发现下限。
    /// `allow_downgrade` 时不以当前版本做下限（CLI 手动回退语义不变）。
    pub fn is_stale(
        &self,
        version: &semver::Version,
        pub_date: Option<chrono::DateTime<chrono::Utc>>,
        current_running: Option<&semver::Version>,
        allow_downgrade: bool,
    ) -> bool {
        let highest: Option<semver::Version> = self
            .highest_seen_version
            .as_deref()
            .and_then(|v| semver::Version::parse(v).ok());
        // 下限 = max(highest_seen, current_running)（allow_downgrade 时去掉 current）。
        let floor = match (&highest, current_running.filter(|_| !allow_downgrade)) {
            (Some(h), Some(c)) => Some(std::cmp::max(h.clone(), c.clone())),
            (Some(h), None) => Some(h.clone()),
            (None, Some(c)) => Some(c.clone()),
            (None, None) => None,
        };
        let Some(seen) = floor else {
            return false;
        };
        if *version < seen {
            return true;
        }
        if *version == seen && highest.as_ref() == Some(version) {
            // 仅当「下限来自最高已见」时才做同版本时间比较；当前运行版本
            // 兜底的场景没有 pub_date 可比（不该因时间误伤同版本清单）。
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
        let cur = v("1.0.0");
        assert!(!st.is_stale(&v("1.0.0"), d("2026-01-01T00:00:00Z"), Some(&cur), false));
        st.record_seen(&v("1.2.0"), "2026-10-05T12:00:00Z");
        assert!(st.is_stale(&v("1.1.9"), d("2026-10-06T00:00:00Z"), Some(&cur), false));
        assert!(st.is_stale(&v("1.2.0"), d("2026-10-01T00:00:00Z"), Some(&cur), false));
        assert!(!st.is_stale(&v("1.2.0"), d("2026-10-05T12:00:00Z"), Some(&cur), false));
        assert!(!st.is_stale(&v("1.2.1"), d("2026-09-01T00:00:00Z"), Some(&cur), false));
    }

    /// #417：状态重置后当前运行版本仍兜住发现下限。
    #[test]
    fn current_version_floors_after_state_loss() {
        let st = UpdateState::default(); // 最高已见丢失（状态被删/损坏）
        let cur = v("1.2.0"); // 正在运行 1.2.0
        assert!(
            st.is_stale(&v("1.1.9"), d("2026-10-06T00:00:00Z"), Some(&cur), false),
            "低于当前运行版本的清单必须 stale"
        );
        assert!(!st.is_stale(&v("1.2.0"), d("2026-10-06T00:00:00Z"), Some(&cur), false));
        // allow_downgrade：不用当前版本做下限（CLI 手动回退语义不变）。
        assert!(!st.is_stale(&v("1.1.9"), d("2026-10-06T00:00:00Z"), Some(&cur), true));
    }

    /// #417：状态损坏（Corrupt）与不存在（FirstRun）区分；损坏不被静默当首次。
    #[test]
    fn corrupt_state_is_detected_not_silent() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("update-state.json");
        // FirstRun：文件不存在。
        let (st, integrity) = UpdateState::load_from_with_integrity(&p);
        assert_eq!(st, UpdateState::default());
        assert_eq!(integrity, StateIntegrity::FirstRun);
        // Unreadable：目录当文件（读必失败）。
        let dirp = dir.path().join("unreadable");
        std::fs::create_dir(&dirp).unwrap();
        let (_, integrity2) = UpdateState::load_from_with_integrity(&dirp);
        assert_eq!(integrity2, StateIntegrity::Unreadable);
        // Corrupt：坏 JSON。
        std::fs::write(&p, b"{not json").unwrap();
        let (_, integrity3) = UpdateState::load_from_with_integrity(&p);
        assert_eq!(integrity3, StateIntegrity::Corrupt);
        // 损坏重建：备份坏文件 + 写入新状态；备份可读。
        let mut rebuilt = UpdateState::default();
        rebuilt.highest_seen_version = Some("1.3.0".into());
        rebuilt.rebuild_over_corrupt(&p).unwrap();
        assert_eq!(
            UpdateState::load_from_with_integrity(&p).1,
            StateIntegrity::Ok
        );
        let bak = p.with_extension("json.bak-corrupt");
        assert!(bak.exists(), "坏文件必须备份");
        let (bak_state, bak_integrity) = UpdateState::load_from_with_integrity(&bak);
        assert_eq!(bak_integrity, StateIntegrity::Corrupt);
        let _ = bak_state;
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
        // #417：损坏不再静默当 Default——load_from 仍是兼容入口（Default），
        // 但 load_from_with_integrity 必须报 Corrupt。
        assert_eq!(UpdateState::load_from(&p), UpdateState::default());
        let (st, integrity) = UpdateState::load_from_with_integrity(&p);
        assert_eq!(st, UpdateState::default());
        assert_eq!(integrity, StateIntegrity::Corrupt);
    }
}
