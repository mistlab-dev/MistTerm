//! SFTP 文件传输模块
//!
//! 提供 SFTP 客户端封装，支持文件浏览、上传、下载、删除等操作。

use ssh2::{Session, Sftp};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use chrono::{DateTime, Utc, TimeZone};

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

/// SFTP 客户端
pub struct SftpClient {
    sftp: Sftp,
}

impl SftpClient {
    /// 从 SSH 会话创建 SFTP 客户端
    pub fn new(session: &Session) -> Result<Self, String> {
        let sftp = session
            .sftp()
            .map_err(|e| format!("Failed to create SFTP channel: {}", e))?;
        Ok(Self { sftp })
    }

    /// 列出目录内容
    pub fn list_dir(&self, path: &Path) -> Result<Vec<SftpEntry>, String> {
        let entries = self
            .sftp
            .readdir(path)
            .map_err(|e| format!("Failed to read directory {}: {}", path.display(), e))?;

        let mut result: Vec<SftpEntry> = entries
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

        // 排序：目录优先，然后按名称排序
        result.sort_by(|a, b| {
            if a.is_dir != b.is_dir {
                b.is_dir.cmp(&a.is_dir)
            } else {
                a.name.to_lowercase().cmp(&b.name.to_lowercase())
            }
        });

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

    /// 下载文件到本地
    pub fn download(&self, remote_path: &Path, local_path: &Path) -> Result<u64, String> {
        let mut remote_file = self
            .sftp
            .open(remote_path)
            .map_err(|e| format!("Failed to open remote file {}: {}", remote_path.display(), e))?;

        let mut local_file = std::fs::File::create(local_path)
            .map_err(|e| format!("Failed to create local file {}: {}", local_path.display(), e))?;

        const CHUNK: usize = 256 * 1024;
        let mut buf = [0u8; CHUNK];
        let mut total = 0u64;
        loop {
            let n = remote_file
                .read(&mut buf)
                .map_err(|e| format!("Failed to read remote file: {}", e))?;
            if n == 0 {
                break;
            }
            local_file
                .write_all(&buf[..n])
                .map_err(|e| format!("Failed to write local file: {}", e))?;
            total += n as u64;
        }

        Ok(total)
    }

    /// 上传文件到远程
    pub fn upload(&self, local_path: &Path, remote_path: &Path) -> Result<u64, String> {
        let mut local_file = std::fs::File::open(local_path)
            .map_err(|e| format!("Failed to open local file {}: {}", local_path.display(), e))?;

        let mut remote_file = self
            .sftp
            .create(remote_path)
            .map_err(|e| format!("Failed to create remote file {}: {}", remote_path.display(), e))?;

        const CHUNK: usize = 256 * 1024;
        let mut buf = [0u8; CHUNK];
        let mut file_size = 0u64;
        loop {
            let n = local_file
                .read(&mut buf)
                .map_err(|e| format!("Failed to read local file: {}", e))?;
            if n == 0 {
                break;
            }
            remote_file
                .write_all(&buf[..n])
                .map_err(|e| format!("Failed to write remote file: {}", e))?;
            file_size += n as u64;
        }

        std::io::Write::flush(&mut remote_file)
            .map_err(|e| format!("Failed to flush remote file: {}", e))?;
        remote_file
            .close()
            .map_err(|e| format!("Failed to close remote file: {}", e))?;

        Ok(file_size)
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
    /// POSIX `S_IFDIR` / `S_IFLNK`（与 `libc` 一致，避免依赖 `ssh2` 私有 `libc`）
    const S_IFDIR: u32 = 0o040_000;
    const S_IFLNK: u32 = 0o120_000;

    let mut result = String::with_capacity(10);

    // 文件类型
    let file_type = if (mode & S_IFDIR) != 0 {
        'd'
    } else if (mode & S_IFLNK) != 0 {
        'l'
    } else {
        '-'
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
        // 连接到 localhost:22
        let tcp = TcpStream::connect("localhost:22")
            .expect("Failed to connect to localhost:22");
        
        let mut session = Session::new()
            .expect("Failed to create SSH session");
        session.set_tcp_stream(tcp);
        session.handshake()
            .expect("SSH handshake failed");
        
        // 使用 root 用户认证（测试环境）
        session.userauth_password("root", "")
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
        client.mkdir(test_dir).ok();
        
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