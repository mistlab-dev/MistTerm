//! SSH 文件传输模块
//! 
//! 使用 SSH 通道直接传输文件，无需服务器安装 lrzsz
//! 
//! 原理:
//! - 上传：远程执行 `cat > file`，本地通过通道写入数据
//! - 下载：远程执行 `cat file`，本地从通道读取数据

use ssh2::Session;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// 文件传输进度回调
pub type ProgressCallback = Arc<dyn Fn(u64, u64) + Send + Sync>;

/// 文件传输器
pub struct FileTransfer {
    session: Session,
}

impl FileTransfer {
    /// 创建新的文件传输器
    pub fn new(session: Session) -> Self {
        Self { session }
    }

    /// 上传文件到远程服务器
    /// 
    /// # 示例
    /// ```rust
    /// let transfer = FileTransfer::new(session);
    /// transfer.upload_file("/local/file.txt", "/remote/file.txt", None)?;
    /// ```
    pub fn upload_file(
        &self,
        local_path: &str,
        remote_path: &str,
        progress_cb: Option<ProgressCallback>,
    ) -> Result<(), String> {
        // 读取本地文件
        let data = fs::read(local_path)
            .map_err(|e| format!("读取本地文件失败：{}", e))?;
        
        let total_size = data.len() as u64;
        println!("📤 准备上传：{} ({} bytes)", local_path, total_size);

        // 远程执行：cat > remote_path
        let mut channel = self.session
            .channel_session()
            .map_err(|e| format!("创建 SSH 通道失败：{}", e))?;

        let exec_cmd = format!("cat > {}", remote_path);
        channel.exec(&exec_cmd)
            .map_err(|e| format!("执行命令失败：{} - {}", exec_cmd, e))?;

        println!("   远程命令：{}", exec_cmd);

        // 分块写入数据，支持进度回调
        let chunk_size = 8192; // 8KB 块
        let mut written = 0u64;

        for chunk in data.chunks(chunk_size) {
            channel.write_all(chunk)
                .map_err(|e| format!("写入数据失败：{}", e))?;
            
            written += chunk.len() as u64;
            
            if let Some(cb) = &progress_cb {
                cb(written, total_size);
            }
        }

        println!("   ✅ 数据写入完成");

        // 发送 EOF，表示数据传输结束
        channel.send_eof()
            .map_err(|e| format!("发送 EOF 失败：{}", e))?;

        // 等待远程命令完成
        channel.wait_close()
            .map_err(|e| format!("等待远程命令失败：{}", e))?;

        // 验证文件
        if let Some(cb) = &progress_cb {
            cb(total_size, total_size);
        }

        // 检查远程文件是否存在且大小正确
        let verify_cmd = format!("test -f {} && wc -c < {}", remote_path, remote_path);
        let output = self.execute_command(&verify_cmd)?;
        
        let remote_size: u64 = output.trim()
            .parse()
            .map_err(|e| format!("验证失败：远程文件大小获取失败 - {}", e))?;

        if remote_size == total_size {
            println!("   ✅ 上传成功：{} bytes", remote_size);
            Ok(())
        } else {
            Err(format!(
                "上传验证失败：本地 {} bytes vs 远程 {} bytes",
                total_size, remote_size
            ))
        }
    }

    /// 下载文件从远程服务器
    /// 
    /// # 示例
    /// ```rust
    /// let transfer = FileTransfer::new(session);
    /// transfer.download_file("/remote/file.txt", "/local/file.txt", None)?;
    /// ```
    pub fn download_file(
        &self,
        remote_path: &str,
        local_path: &str,
        progress_cb: Option<ProgressCallback>,
    ) -> Result<(), String> {
        // 先获取远程文件大小
        let size_cmd = format!("wc -c < {}", remote_path);
        let size_output = self.execute_command(&size_cmd)?;
        let total_size: u64 = size_output.trim()
            .parse()
            .map_err(|e| format!("获取文件大小失败：{}", e))?;

        println!("📥 准备下载：{} ({} bytes)", remote_path, total_size);

        // 远程执行：cat remote_path
        let mut channel = self.session
            .channel_session()
            .map_err(|e| format!("创建 SSH 通道失败：{}", e))?;

        let exec_cmd = format!("cat {}", remote_path);
        channel.exec(&exec_cmd)
            .map_err(|e| format!("执行命令失败：{} - {}", exec_cmd, e))?;

        println!("   远程命令：{}", exec_cmd);

        // 创建本地文件
        let mut file = fs::File::create(local_path)
            .map_err(|e| format!("创建本地文件失败：{}", e))?;

        // 分块读取数据，支持进度回调
        let mut buffer = [0u8; 8192];
        let mut downloaded = 0u64;

        loop {
            match channel.read(&mut buffer) {
                Ok(0) => break, // 传输完成
                Ok(n) => {
                    file.write_all(&buffer[..n])
                        .map_err(|e| format!("写入本地文件失败：{}", e))?;
                    
                    downloaded += n as u64;
                    
                    if let Some(cb) = &progress_cb {
                        cb(downloaded, total_size);
                    }
                }
                Err(e) => {
                    return Err(format!("读取数据失败：{}", e));
                }
            }
        }

        println!("   ✅ 数据下载完成");

        if let Some(cb) = &progress_cb {
            cb(downloaded, total_size);
        }

        // 验证文件
        let local_meta = fs::metadata(local_path)
            .map_err(|e| format!("获取本地文件信息失败：{}", e))?;

        if local_meta.len() == total_size {
            println!("   ✅ 下载成功：{} bytes", downloaded);
            Ok(())
        } else {
            Err(format!(
                "下载验证失败：远程 {} bytes vs 本地 {} bytes",
                total_size, local_meta.len()
            ))
        }
    }

    /// 上传多个文件
    pub fn upload_files(
        &self,
        local_paths: &[&str],
        remote_dir: &str,
        progress_cb: Option<ProgressCallback>,
    ) -> Result<Vec<String>, String> {
        // 确保远程目录存在
        self.execute_command(&format!("mkdir -p {}", remote_dir))
            .map_err(|e| format!("创建远程目录失败：{}", e))?;

        let mut uploaded = Vec::new();

        for local_path in local_paths {
            let file_name = Path::new(local_path)
                .file_name()
                .ok_or("无效的文件路径")?
                .to_string_lossy()
                .to_string();

            let remote_path = format!("{}/{}", remote_dir, file_name);
            
            self.upload_file(local_path, &remote_path, progress_cb.clone())?;
            uploaded.push(remote_path);
        }

        Ok(uploaded)
    }

    /// 下载多个文件
    pub fn download_files(
        &self,
        remote_paths: &[&str],
        local_dir: &str,
        progress_cb: Option<ProgressCallback>,
    ) -> Result<Vec<PathBuf>, String> {
        // 确保本地目录存在
        fs::create_dir_all(local_dir)
            .map_err(|e| format!("创建本地目录失败：{}", e))?;

        let mut downloaded = Vec::new();

        for remote_path in remote_paths {
            let file_name = Path::new(remote_path)
                .file_name()
                .ok_or("无效的文件路径")?
                .to_string_lossy()
                .to_string();

            let local_path = PathBuf::from(local_dir).join(&file_name);
            
            self.download_file(remote_path, local_path.to_string_lossy().as_ref(), progress_cb.clone())?;
            downloaded.push(local_path);
        }

        Ok(downloaded)
    }

    /// 执行远程命令并返回输出
    fn execute_command(&self, command: &str) -> Result<String, String> {
        let mut channel = self.session
            .channel_session()
            .map_err(|e| format!("创建 SSH 通道失败：{}", e))?;

        channel.exec(command)
            .map_err(|e| format!("执行命令失败：{} - {}", command, e))?;

        let mut output = Vec::new();
        channel.read_to_end(&mut output)
            .map_err(|e| format!("读取输出失败：{}", e))?;

        let _ = channel.wait_close();

        String::from_utf8(output)
            .map_err(|e| format!("输出不是有效 UTF-8: {}", e))
    }

    /// 检查远程文件是否存在
    pub fn file_exists(&self, remote_path: &str) -> Result<bool, String> {
        let cmd = format!("test -f {}", remote_path);
        match self.execute_command(&cmd) {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    /// 获取远程文件大小
    pub fn get_file_size(&self, remote_path: &str) -> Result<u64, String> {
        let cmd = format!("wc -c < {}", remote_path);
        let output = self.execute_command(&cmd)?;
        output.trim()
            .parse()
            .map_err(|e| format!("解析文件大小失败：{}", e))
    }

    /// 删除远程文件
    pub fn remove_file(&self, remote_path: &str) -> Result<(), String> {
        let cmd = format!("rm -f {}", remote_path);
        self.execute_command(&cmd)?;
        Ok(())
    }

    /// 创建远程目录
    pub fn create_dir(&self, remote_path: &str) -> Result<(), String> {
        let cmd = format!("mkdir -p {}", remote_path);
        self.execute_command(&cmd)?;
        Ok(())
    }
}

/// 进度跟踪器
pub struct ProgressTracker {
    total: u64,
    current: Arc<AtomicU64>,
}

impl ProgressTracker {
    pub fn new(total: u64) -> Self {
        Self {
            total,
            current: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn callback(&self) -> ProgressCallback {
        let current = self.current.clone();
        let total = self.total;
        
        Arc::new(move |done: u64, _: u64| {
            current.store(done, Ordering::Relaxed);
            let percent = if total > 0 {
                (done as f64 / total as f64 * 100.0) as u32
            } else {
                0
            };
            print!("\r   进度：{}/{} ({:.1}%)", done, total, percent as f64);
            let _ = std::io::stdout().flush();
        })
    }

    pub fn current(&self) -> u64 {
        self.current.load(Ordering::Relaxed)
    }

    pub fn percentage(&self) -> f64 {
        if self.total > 0 {
            self.current.load(Ordering::Relaxed) as f64 / self.total as f64 * 100.0
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_tracker() {
        let tracker = ProgressTracker::new(1000);
        let cb = tracker.callback();
        
        cb(500, 1000);
        assert_eq!(tracker.current(), 500);
        assert!((tracker.percentage() - 50.0).abs() < 0.1);
        
        cb(1000, 1000);
        assert_eq!(tracker.current(), 1000);
        assert!((tracker.percentage() - 100.0).abs() < 0.1);
    }
}
