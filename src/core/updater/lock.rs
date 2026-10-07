//! 更新锁：防止 GUI 和 CLI（或两个窗口）同时更新。
//!
//! 用操作系统的文件锁（`File::try_lock`），进程崩溃或退出时自动释放，不会留下「死锁文件」。

use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;

use super::error::UpdateError;

pub struct UpdateLock {
    _file: File,
}

impl UpdateLock {
    /// 在 `dir/update.lock` 上加独占锁；已被占用时返回 [`UpdateError::Locked`]。
    pub fn acquire_in(dir: &Path) -> Result<Self, UpdateError> {
        std::fs::create_dir_all(dir).map_err(|e| UpdateError::Install(e.to_string()))?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(dir.join("update.lock"))
            .map_err(|e| UpdateError::Install(e.to_string()))?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(UpdateError::Locked),
            Err(TryLockError::Error(e)) => Err(UpdateError::Install(e.to_string())),
        }
    }

    pub fn acquire() -> Result<Self, UpdateError> {
        Self::acquire_in(&super::paths::cache_dir())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_lock_fails_until_released() {
        let dir = tempfile::tempdir().unwrap();
        let first = UpdateLock::acquire_in(dir.path()).unwrap();
        assert!(matches!(UpdateLock::acquire_in(dir.path()), Err(UpdateError::Locked)));
        drop(first);
        assert!(UpdateLock::acquire_in(dir.path()).is_ok());
    }
}
