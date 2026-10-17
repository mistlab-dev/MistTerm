//! SFTP 文件传输模块
//!
//! 提供 SFTP 客户端封装，支持文件浏览、上传、下载、删除等操作。

use super::SessionBlockingGuard;
use chrono::{DateTime, TimeZone, Utc};
use ssh2::{OpenFlags, OpenType, Session, Sftp};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// libssh2 在 shell 占用会话时可能返回 Would block / EAGAIN。
/// 现在 SFTP 操作走 blocking 模式（见 `SftpClient`），正常路径上不会再出现，
/// 该函数仅供 UI 识别遗留错误信息时复用。
pub fn is_sftp_would_block_message(msg: &str) -> bool {
    let msg = msg.to_lowercase();
    msg.contains("would block")
        || msg.contains("eagain")
        || msg.contains("resource temporarily unavailable")
        || msg.contains("libssh2_error_eagain")
        || msg.contains("try again")
        || msg.contains("session(-37)")
}

/// SFTP 文件条目信息
#[derive(Debug, Clone)]
pub struct SftpEntry {
    /// 文件名
    pub name: String,
    /// 是否为目录
    pub is_dir: bool,
    /// 文件大小（字节）
    pub size: u64,
    /// 权限字符串（如 "-rwxr-xr-x"）
    pub permissions: String,
    /// 修改时间
    pub modified: DateTime<Utc>,
    /// 完整路径
    pub path: PathBuf,
}

/// 上传/下载写模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    /// 截断目标并从 0 开始写。
    Overwrite,
    /// 从已有目标大小继续写（断点续传）。
    Resume,
}

/// 带进度/取消的传输结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferResult {
    Completed { bytes: u64 },
    Cancelled { bytes_done: u64 },
    /// 目标已完整（续传时大小已对齐），未再传。
    Skipped,
}

/// 传输进度回调参数。
#[derive(Debug, Clone, Copy)]
pub struct TransferProgress {
    pub done: u64,
    pub total: u64,
}

impl SftpEntry {
    /// 格式化文件大小为人类可读格式
    pub fn size_human(&self) -> String {
        const KB: u64 = 1024;
        const MB: u64 = 1024 * 1024;
        const GB: u64 = 1024 * 1024 * 1024;
        
        if self.size >= GB {
            format!("{:.2} GB", self.size as f64 / GB as f64)
        } else if self.size >= MB {
            format!("{:.2} MB", self.size as f64 / MB as f64)
        } else if self.size >= KB {
            format!("{:.2} KB", self.size as f64 / KB as f64)
        } else {
            format!("{} B", self.size)
        }
    }
}

/// SFTP 客户端。
///
/// **必须**在 shell 泵线程通过 [`crate::ssh::SshSessionHandle::enqueue_session_job`]
/// 调用，否则会与 PTY 读循环并发争用 libssh2 的 session 内部 mutex（症状：
/// `[Session(-43)] Timeout waiting for status message` 或 `EAGAIN/-37`）。
///
/// 本结构构造时把会话切到 blocking 模式（SFTP 握手需要多个 RTT，非阻塞模式
/// 单次调用推不动状态机）并设超时，Drop 时复位 non-blocking 让 shell 泵继续非阻塞
/// 读写——参考 `manager.rs::exec_on_cloned_session` 的相同模式。
pub struct SftpClient {
    // 字段按声明顺序 drop：`sftp` 须在 `_blocking` 恢复非阻塞之前关闭。
    sftp: Sftp,
    _blocking: SessionBlockingGuard,
}

impl SftpClient {
    /// 从 SSH 会话创建 SFTP 客户端。`session` 必须由 shell 泵线程传入。
    pub fn new(session: &Session) -> Result<Self, String> {
        let blocking = SessionBlockingGuard::new(session);
        let sftp = session
            .sftp()
            .map_err(|e| format!("Failed to create SFTP channel: {}", e))?;
        Ok(Self {
            sftp,
            _blocking: blocking,
        })
    }
}

impl SftpClient {

    /// 把 `.`、`..` 解析成服务器上的绝对路径。
    pub fn realpath(&self, path: &Path) -> Result<PathBuf, String> {
        self.sftp
            .realpath(path)
            .map_err(|e| format!("Failed to resolve {}: {e}", path.display()))
    }

    /// 列出目录内容
    pub fn list_dir(&self, path: &Path) -> Result<Vec<SftpEntry>, String> {
        let entries = self
            .sftp
            .readdir(path)
            .map_err(|e| format!("Failed to read directory {}: {}", path.display(), e))?;

        let result: Vec<SftpEntry> = entries
            .into_iter()
            .filter_map(|(p, stat)| {
                let name = p.file_name()?.to_string_lossy().to_string();
                // 过滤 . 和 ..
                if name == "." || name == ".." {
                    return None;
                }

                let is_dir = stat.is_dir();
                let size = stat.size.unwrap_or(0);
                
                // 格式化权限
                let perms = format_permissions(stat.perm.unwrap_or(0));

                let modified = stat
                    .mtime
                    .and_then(|t| Utc.timestamp_opt(t as i64, 0).single())
                    .unwrap_or_else(Utc::now);

                Some(SftpEntry {
                    name,
                    is_dir,
                    size,
                    permissions: perms,
                    modified,
                    path: p,
                })
            })
            .collect();

        Ok(result)
    }

    /// 创建目录
    pub fn mkdir(&self, path: &Path) -> Result<(), String> {
        self.sftp
            .mkdir(path, 0o755)
            .map_err(|e| format!("Failed to create directory {}: {}", path.display(), e))
    }

    /// 删除文件
    pub fn remove_file(&self, path: &Path) -> Result<(), String> {
        self.sftp
            .unlink(path)
            .map_err(|e| format!("Failed to delete file {}: {}", path.display(), e))
    }

    /// 删除目录（必须为空目录）
    pub fn remove_dir(&self, path: &Path) -> Result<(), String> {
        self.sftp
            .rmdir(path)
            .map_err(|e| format!("Failed to delete directory {}: {}", path.display(), e))
    }

    /// 删除文件或目录（递归删除目录）
    pub fn remove(&self, path: &Path) -> Result<(), String> {
        // 先尝试获取文件属性
        let stat = self
            .sftp
            .stat(path)
            .map_err(|e| format!("Failed to stat {}: {}", path.display(), e))?;

        if stat.is_dir() {
            // 目录需要递归删除
            self.remove_dir_recursive(path)?;
        } else {
            self.remove_file(path)?;
        }

        Ok(())
    }

    /// 递归删除目录
    fn remove_dir_recursive(&self, path: &Path) -> Result<(), String> {
        // 列出目录内容
        let entries = self.list_dir(path)?;

        // 先删除所有子项
        for entry in entries {
            if entry.is_dir {
                self.remove_dir_recursive(&entry.path)?;
            } else {
                self.remove_file(&entry.path)?;
            }
        }

        // 删除空目录
        self.remove_dir(path)?;
        Ok(())
    }

    /// 重命名文件或目录
    pub fn rename(&self, old_path: &Path, new_path: &Path) -> Result<(), String> {
        self.sftp
            .rename(old_path, new_path, None)
            .map_err(|e| format!(
                "Failed to rename {} to {}: {}",
                old_path.display(),
                new_path.display(),
                e
            ))
    }

    /// 下载文件到本地（整文件覆盖，无进度）。
    pub fn download(&self, remote_path: &Path, local_path: &Path) -> Result<u64, String> {
        match self.download_ex(
            remote_path,
            local_path,
            WriteMode::Overwrite,
            None,
            &mut |_| {},
        )? {
            TransferResult::Completed { bytes } => Ok(bytes),
            TransferResult::Cancelled { bytes_done } => {
                Err(format!("download cancelled after {bytes_done} bytes"))
            }
            TransferResult::Skipped => Ok(0),
        }
    }

    /// 上传文件到远程（整文件覆盖，无进度）。
    pub fn upload(&self, local_path: &Path, remote_path: &Path) -> Result<u64, String> {
        match self.upload_ex(
            local_path,
            remote_path,
            WriteMode::Overwrite,
            None,
            &mut |_| {},
        )? {
            TransferResult::Completed { bytes } => Ok(bytes),
            TransferResult::Cancelled { bytes_done } => {
                Err(format!("upload cancelled after {bytes_done} bytes"))
            }
            TransferResult::Skipped => Ok(0),
        }
    }

    /// 带进度 / 取消 / 续传的下载。
    pub fn download_ex(
        &self,
        remote_path: &Path,
        local_path: &Path,
        mode: WriteMode,
        cancel: Option<&AtomicBool>,
        on_progress: &mut dyn FnMut(TransferProgress),
    ) -> Result<TransferResult, String> {
        let remote_stat = self
            .sftp
            .stat(remote_path)
            .map_err(|e| format!("Failed to stat remote {}: {e}", remote_path.display()))?;
        let total = remote_stat.size.unwrap_or(0);
        let existing = if local_path.exists() {
            std::fs::metadata(local_path)
                .map(|m| m.len())
                .unwrap_or(0)
        } else {
            0
        };

        let resume_from = match mode {
            WriteMode::Overwrite => 0,
            WriteMode::Resume => {
                if existing >= total && total > 0 {
                    on_progress(TransferProgress {
                        done: total,
                        total,
                    });
                    return Ok(TransferResult::Skipped);
                }
                existing.min(total)
            }
        };

        let mut remote_file = self
            .sftp
            .open(remote_path)
            .map_err(|e| format!("Failed to open remote file {}: {e}", remote_path.display()))?;
        if resume_from > 0 {
            remote_file
                .seek(SeekFrom::Start(resume_from))
                .map_err(|e| format!("Failed to seek remote file: {e}"))?;
        }

        if let Some(parent) = local_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut local_file = if resume_from > 0 {
            std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .open(local_path)
                .map_err(|e| format!("Failed to open local file {}: {e}", local_path.display()))?
        } else {
            std::fs::File::create(local_path)
                .map_err(|e| format!("Failed to create local file {}: {e}", local_path.display()))?
        };
        if resume_from > 0 {
            local_file
                .seek(SeekFrom::Start(resume_from))
                .map_err(|e| format!("Failed to seek local file: {e}"))?;
        }

        const CHUNK: usize = 256 * 1024;
        let mut buf = [0u8; CHUNK];
        let mut done = resume_from;
        on_progress(TransferProgress { done, total });
        loop {
            if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                return Ok(TransferResult::Cancelled { bytes_done: done });
            }
            let n = remote_file
                .read(&mut buf)
                .map_err(|e| format!("Failed to read remote file: {e}"))?;
            if n == 0 {
                break;
            }
            local_file
                .write_all(&buf[..n])
                .map_err(|e| format!("Failed to write local file: {e}"))?;
            done += n as u64;
            on_progress(TransferProgress { done, total });
        }
        Ok(TransferResult::Completed { bytes: done })
    }

    /// 带进度 / 取消 / 续传的上传。
    pub fn upload_ex(
        &self,
        local_path: &Path,
        remote_path: &Path,
        mode: WriteMode,
        cancel: Option<&AtomicBool>,
        on_progress: &mut dyn FnMut(TransferProgress),
    ) -> Result<TransferResult, String> {
        let mut local_file = std::fs::File::open(local_path)
            .map_err(|e| format!("Failed to open local file {}: {e}", local_path.display()))?;
        let total = local_file
            .metadata()
            .map(|m| m.len())
            .map_err(|e| format!("Failed to stat local file: {e}"))?;

        let existing = self
            .sftp
            .stat(remote_path)
            .ok()
            .and_then(|s| s.size)
            .unwrap_or(0);

        let resume_from = match mode {
            WriteMode::Overwrite => 0,
            WriteMode::Resume => {
                if existing >= total && total > 0 {
                    on_progress(TransferProgress {
                        done: total,
                        total,
                    });
                    return Ok(TransferResult::Skipped);
                }
                existing.min(total)
            }
        };

        let mut remote_file = if resume_from > 0 {
            self.sftp
                .open_mode(
                    remote_path,
                    OpenFlags::WRITE | OpenFlags::CREATE,
                    0o644,
                    OpenType::File,
                )
                .map_err(|e| {
                    format!("Failed to open remote for resume {}: {e}", remote_path.display())
                })?
        } else {
            self.sftp.create(remote_path).map_err(|e| {
                format!("Failed to create remote file {}: {e}", remote_path.display())
            })?
        };
        if resume_from > 0 {
            remote_file
                .seek(SeekFrom::Start(resume_from))
                .map_err(|e| format!("Failed to seek remote file: {e}"))?;
            local_file
                .seek(SeekFrom::Start(resume_from))
                .map_err(|e| format!("Failed to seek local file: {e}"))?;
        }

        const CHUNK: usize = 256 * 1024;
        let mut buf = [0u8; CHUNK];
        let mut done = resume_from;
        on_progress(TransferProgress { done, total });
        loop {
            if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                let _ = remote_file.flush();
                let _ = remote_file.close();
                return Ok(TransferResult::Cancelled { bytes_done: done });
            }
            let n = local_file
                .read(&mut buf)
                .map_err(|e| format!("Failed to read local file: {e}"))?;
            if n == 0 {
                break;
            }
            remote_file
                .write_all(&buf[..n])
                .map_err(|e| format!("Failed to write remote file: {e}"))?;
            done += n as u64;
            on_progress(TransferProgress { done, total });
        }

        remote_file
            .flush()
            .map_err(|e| format!("Failed to flush remote file: {e}"))?;
        remote_file
            .close()
            .map_err(|e| format!("Failed to close remote file: {e}"))?;

        Ok(TransferResult::Completed { bytes: done })
    }

    /// 远端文件是否存在（用于覆盖/续传决策）。
    pub fn exists(&self, path: &Path) -> bool {
        self.sftp.stat(path).is_ok()
    }

    /// 远端文件大小；不存在则为 `None`。
    pub fn remote_size(&self, path: &Path) -> Option<u64> {
        self.sftp.stat(path).ok().and_then(|s| s.size)
    }

    /// 远端 mtime（Unix 秒）；不存在则为 `None`。
    pub fn remote_mtime(&self, path: &Path) -> Option<u64> {
        self.sftp.stat(path).ok().and_then(|s| s.mtime)
    }

    /// 修改远端权限（仅权限位，常见如 `0o644` / `0o755`）。
    pub fn chmod(&self, path: &Path, mode: u32) -> Result<(), String> {
        let mode = mode & 0o7777;
        self.sftp
            .setstat(
                path,
                ssh2::FileStat {
                    size: None,
                    uid: None,
                    gid: None,
                    perm: Some(mode),
                    atime: None,
                    mtime: None,
                },
            )
            .map_err(|e| format!("Failed to chmod {}: {e}", path.display()))
    }

    /// 递归修改权限；`cancel` 为 true 时提前停止。
    pub fn chmod_recursive(
        &self,
        path: &Path,
        mode: u32,
        cancel: Option<&AtomicBool>,
    ) -> Result<usize, String> {
        let mut n = 0usize;
        self.chmod(path, mode)?;
        n += 1;
        let stat = self
            .sftp
            .stat(path)
            .map_err(|e| format!("Failed to stat {}: {e}", path.display()))?;
        if !stat.is_dir() {
            return Ok(n);
        }
        for entry in self.list_dir(path)? {
            if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                return Ok(n);
            }
            n += self.chmod_recursive(&entry.path, mode, cancel)?;
        }
        Ok(n)
    }

    /// 逐级创建远端目录（已存在则跳过）。
    pub fn mkdir_p(&self, path: &Path) -> Result<(), String> {
        if path.as_os_str().is_empty() || path == Path::new(".") || path == Path::new("/") {
            return Ok(());
        }
        if self.exists(path) {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            if parent != path {
                self.mkdir_p(parent)?;
            }
        }
        match self.mkdir(path) {
            Ok(()) => Ok(()),
            Err(e) if self.exists(path) => {
                let _ = e;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// 递归收集远端树：`(绝对路径, 相对路径, 是否目录)`。相对路径相对 `root`。
    pub fn collect_tree(
        &self,
        root: &Path,
        cancel: Option<&AtomicBool>,
    ) -> Result<Vec<(PathBuf, PathBuf, bool)>, String> {
        let mut out = Vec::new();
        self.collect_tree_into(root, root, &mut out, cancel)?;
        Ok(out)
    }

    fn collect_tree_into(
        &self,
        root: &Path,
        dir: &Path,
        out: &mut Vec<(PathBuf, PathBuf, bool)>,
        cancel: Option<&AtomicBool>,
    ) -> Result<(), String> {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Ok(());
        }
        let entries = self.list_dir(dir)?;
        if entries.is_empty() {
            let rel = dir.strip_prefix(root).unwrap_or(dir).to_path_buf();
            if dir != root {
                out.push((dir.to_path_buf(), rel, true));
            }
            return Ok(());
        }
        for entry in entries {
            if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                return Ok(());
            }
            let rel = entry
                .path
                .strip_prefix(root)
                .unwrap_or(&entry.path)
                .to_path_buf();
            if entry.is_dir {
                let before = out.len();
                self.collect_tree_into(root, &entry.path, out, cancel)?;
                if out.len() == before {
                    // 空目录
                    out.push((entry.path, rel, true));
                }
            } else {
                out.push((entry.path, rel, false));
            }
        }
        Ok(())
    }

    /// 获取文件属性
    pub fn stat(&self, path: &Path) -> Result<SftpEntry, String> {
        let stat = self
            .sftp
            .stat(path)
            .map_err(|e| format!("Failed to stat {}: {}", path.display(), e))?;

        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());

        let modified = stat
            .mtime
            .and_then(|t| Utc.timestamp_opt(t as i64, 0).single())
            .unwrap_or_else(Utc::now);

        Ok(SftpEntry {
            name,
            is_dir: stat.is_dir(),
            size: stat.size.unwrap_or(0),
            permissions: format_permissions(stat.perm.unwrap_or(0)),
            modified,
            path: path.to_path_buf(),
        })
    }

    /// 获取当前工作目录
    pub fn get_current_dir(&self) -> Result<PathBuf, String> {
        // SFTP 协议没有直接获取 cwd 的方法，通常从 home 开始
        // 尝试从 ssh 会话获取，或者使用默认路径
        Ok(PathBuf::from("/"))
    }
}

/// 格式化权限位为 Unix 风格字符串
fn format_permissions(mode: u32) -> String {
    /// POSIX 文件类型位（`S_IFMT` / `S_IFDIR` / `S_IFLNK`，与 `libc` 一致）
    const S_IFMT: u32 = 0o170_000;
    const S_IFDIR: u32 = 0o040_000;
    const S_IFLNK: u32 = 0o120_000;

    let mut result = String::with_capacity(10);

    // 文件类型（必须用 `S_IFMT` 掩码后再比较：`S_IFREG` 与 `S_IFLNK` 按位与会非零）
    let file_type = match mode & S_IFMT {
        S_IFDIR => 'd',
        S_IFLNK => 'l',
        _ => '-',
    };
    result.push(file_type);

    // 用户权限
    result.push(if (mode & 0o400) != 0 { 'r' } else { '-' });
    result.push(if (mode & 0o200) != 0 { 'w' } else { '-' });
    result.push(if (mode & 0o100) != 0 { 'x' } else { '-' });

    // 组权限
    result.push(if (mode & 0o040) != 0 { 'r' } else { '-' });
    result.push(if (mode & 0o020) != 0 { 'w' } else { '-' });
    result.push(if (mode & 0o010) != 0 { 'x' } else { '-' });

    // 其他权限
    result.push(if (mode & 0o004) != 0 { 'r' } else { '-' });
    result.push(if (mode & 0o002) != 0 { 'w' } else { '-' });
    result.push(if (mode & 0o001) != 0 { 'x' } else { '-' });

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssh2::Session;
    use std::net::TcpStream;

    /// 创建测试用的 SFTP 客户端
    fn create_test_sftp_client() -> SftpClient {
        // 连接到 localhost:22，使用本地测试账号
        let tcp = TcpStream::connect("localhost:22")
            .expect("Failed to connect to localhost:22");
        
        let mut session = Session::new()
            .expect("Failed to create SSH session");
        session.set_tcp_stream(tcp);
        session.handshake()
            .expect("SSH handshake failed");
        
        // 使用本地测试账号
        session.userauth_password("mistterm_test", "test123456")
            .expect("SSH authentication failed");
        
        SftpClient::new(&session)
            .expect("Failed to create SFTP client")
    }

    #[test]
    fn test_format_permissions() {
        // 测试目录权限
        let dir_perms = format_permissions(0o40755);
        assert_eq!(dir_perms, "drwxr-xr-x");
        
        // 测试文件权限
        let file_perms = format_permissions(0o100644);
        assert_eq!(file_perms, "-rw-r--r--");
        
        // 测试可执行文件权限
        let exec_perms = format_permissions(0o100755);
        assert_eq!(exec_perms, "-rwxr-xr-x");
    }

    #[test]
    fn test_sftp_entry_size_human() {
        let entry = SftpEntry {
            name: "test.txt".to_string(),
            is_dir: false,
            size: 1024,
            permissions: "-rw-r--r--".to_string(),
            modified: Utc::now(),
            path: PathBuf::from("/test.txt"),
        };
        assert_eq!(entry.size_human(), "1.00 KB");
        
        let large_entry = SftpEntry {
            name: "large.bin".to_string(),
            is_dir: false,
            size: 1024 * 1024 * 1024, // 1 GB
            permissions: "-rw-r--r--".to_string(),
            modified: Utc::now(),
            path: PathBuf::from("/large.bin"),
        };
        assert_eq!(large_entry.size_human(), "1.00 GB");
    }

    #[test]
    #[ignore] // 需要实际 SSH 连接，仅在集成测试时运行
    fn test_list_dir() {
        let client = create_test_sftp_client();
        let entries = client.list_dir(Path::new("/tmp"))
            .expect("Failed to list /tmp");
        
        // 应该有至少一个条目
        assert!(!entries.is_empty());
        
        // 检查条目结构
        for entry in &entries {
            assert!(!entry.name.is_empty());
            assert!(entry.name != ".");
            assert!(entry.name != "..");
        }
    }

    #[test]
    #[ignore] // 需要实际 SSH 连接，仅在集成测试时运行
    fn test_mkdir_and_remove() {
        let client = create_test_sftp_client();
        let test_dir = Path::new("/tmp/mistterm_test");
        
        // 创建目录
        client.mkdir(test_dir)
            .expect("Failed to create test directory");
        
        // 验证目录存在
        let stat = client.stat(test_dir)
            .expect("Failed to stat test directory");
        assert!(stat.is_dir);
        
        // 删除目录
        client.remove_dir(test_dir)
            .expect("Failed to remove test directory");
    }

    #[test]
    #[ignore] // 需要实际 SSH 连接，仅在集成测试时运行
    fn test_upload_and_download() {
        let client = create_test_sftp_client();
        let test_dir = Path::new("/tmp/mistterm_test");
        
        // 创建测试目录
        let _ = client.mkdir(test_dir);  // 忽略已存在错误
        
        // 创建本地测试文件
        let local_file = std::env::temp_dir().join("mistterm_test_upload.txt");
        std::fs::write(&local_file, b"Hello, SFTP!")
            .expect("Failed to create local test file");
        
        // 上传
        let remote_file = test_dir.join("upload.txt");
        let size = client.upload(&local_file, &remote_file)
            .expect("Failed to upload file");
        assert_eq!(size, 13);
        
        // 下载
        let download_file = std::env::temp_dir().join("mistterm_test_download.txt");
        let download_size = client.download(&remote_file, &download_file)
            .expect("Failed to download file");
        assert_eq!(download_size, 13);
        
        // 验证内容
        let content = std::fs::read_to_string(&download_file)
            .expect("Failed to read downloaded file");
        assert_eq!(content, "Hello, SFTP!");
        
        // 清理
        client.remove_file(&remote_file).ok();
        client.remove_dir(test_dir).ok();
        std::fs::remove_file(&local_file).ok();
        std::fs::remove_file(&download_file).ok();
    }
}