//! ZMODEM 协议实现
//!
//! 实现 ZMODEM 文件传输协议，支持终端内文件上传下载

use std::path::PathBuf;
use tokio::sync::mpsc::Sender;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use thiserror::Error;

/// ZMODEM 错误
#[derive(Error, Debug)]
pub enum ZmodemError {
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),

    #[error("协议错误：{0}")]
    Protocol(String),

    #[error("传输超时")]
    Timeout,

    #[error("文件不存在：{0}")]
    FileNotFound(String),
}

/// ZMODEM 传输状态
#[derive(Debug, Clone, PartialEq)]
pub enum ZmodemState {
    /// 空闲状态
    Idle,
    /// 等待接收文件头
    WaitingHeader,
    /// 发送文件头
    SendingHeader,
    /// 发送数据块
    SendingData,
    /// 接收数据块
    ReceivingData,
    /// 验证 CRC
    Verifying,
    /// 传输完成
    Finished,
    /// 传输错误
    Error(String),
}

/// 文件信息
#[derive(Debug, Clone)]
pub struct FileInfo {
    pub name: String,
    pub size: u64,
    pub modified_time: u64,
    pub mode: u32,
}

/// 传输进度
#[derive(Debug, Clone)]
pub struct TransferProgress {
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub filename: String,
    pub speed_bytes_per_sec: f64,
}

/// ZMODEM 传输器
pub struct ZmodemTransfer {
    state: ZmodemState,
    file_path: Option<PathBuf>,
    file_size: u64,
    bytes_transferred: u64,
    crc: u32,
}

impl ZmodemTransfer {
    /// 创建新的传输器
    pub fn new() -> Self {
        ZmodemTransfer {
            state: ZmodemState::Idle,
            file_path: None,
            file_size: 0,
            bytes_transferred: 0,
            crc: 0,
        }
    }

    /// 获取当前状态
    pub fn state(&self) -> &ZmodemState {
        &self.state
    }

    /// 获取传输进度（百分比）
    pub fn progress(&self) -> f32 {
        if self.file_size == 0 {
            0.0
        } else {
            (self.bytes_transferred as f32 / self.file_size as f32) * 100.0
        }
    }

    /// 获取已传输字节数
    pub fn bytes_transferred(&self) -> u64 {
        self.bytes_transferred
    }

    /// 获取总字节数
    pub fn total_bytes(&self) -> u64 {
        self.file_size
    }

    // ==================== 上传相关方法 ====================

    /// 准备上传文件
    pub fn prepare_upload(&mut self, file_path: PathBuf) -> Result<FileInfo, ZmodemError> {
        if !file_path.exists() {
            return Err(ZmodemError::FileNotFound(
                file_path.to_string_lossy().to_string(),
            ));
        }

        let metadata = std::fs::metadata(&file_path)?;
        let file_info = FileInfo {
            name: file_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            size: metadata.len(),
            modified_time: metadata
                .modified()
                .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs())
                .unwrap_or(0),
            mode: 0o644, // 默认权限
        };

        self.file_path = Some(file_path);
        self.file_size = file_info.size;
        self.bytes_transferred = 0;
        self.state = ZmodemState::WaitingHeader;

        Ok(file_info)
    }

    /// 发送 ZRQINIT 初始化包
    pub async fn send_zrqinit<W: tokio::io::AsyncWrite + Unpin>(&self, writer: &mut W) -> Result<(), ZmodemError> {
        // ZRQINIT: ZRQINIT 0x80 0x80 0x80 0x80 0x04
        let packet = b"\x80\x80\x80\x80\x04";
        writer.write_all(packet).await?;
        writer.flush().await?;
        Ok(())
    }

    /// 发送文件头
    pub async fn send_file_header<W: tokio::io::AsyncWrite + Unpin>(
        &self,
        writer: &mut W,
        file_info: &FileInfo,
    ) -> Result<(), ZmodemError> {
        // D0 格式的文件头：D0 filename size mtime mode 0 0
        let header = format!(
            "D0 {} {} {} {} 0 0\n",
            file_info.name, file_info.size, file_info.modified_time, file_info.mode
        );
        writer.write_all(header.as_bytes()).await?;
        writer.flush().await?;
        Ok(())
    }

    /// 发送数据块（简化版，实际 ZMODEM 需要完整的帧格式）
    pub async fn send_data_blocks<R: tokio::io::AsyncRead + Unpin, W: tokio::io::AsyncWrite + Unpin>(
        &mut self,
        reader: &mut R,
        writer: &mut W,
        progress_tx: Option<Sender<TransferProgress>>,
        block_size: usize,
    ) -> Result<(), ZmodemError> {
        let mut buffer = vec![0u8; block_size];
        let start_time = std::time::Instant::now();

        loop {
            let n = reader.read(&mut buffer).await?;
            if n == 0 {
                break;
            }

            // 发送数据块（简化，实际需要 ZMODEM 帧格式包含 CRC）
            writer.write_all(&buffer[..n]).await?;
            writer.flush().await?;

            self.bytes_transferred += n as u64;

            // 发送进度更新
            if let Some(tx) = &progress_tx {
                let elapsed = start_time.elapsed().as_secs_f64();
                let speed = if elapsed > 0.0 {
                    self.bytes_transferred as f64 / elapsed
                } else {
                    0.0
                };

                let progress = TransferProgress {
                    bytes_transferred: self.bytes_transferred,
                    total_bytes: self.file_size,
                    filename: self
                        .file_path
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    speed_bytes_per_sec: speed,
                };

                tx.send(progress).await.ok();
            }
        }

        self.state = ZmodemState::Verifying;
        Ok(())
    }

    /// 发送 EOF 结束标记
    pub async fn send_eof<W: tokio::io::AsyncWrite + Unpin>(&self, writer: &mut W) -> Result<(), ZmodemError> {
        // ZEOF: 0x04
        writer.write_all(&[0x04]).await?;
        writer.flush().await?;
        Ok(())
    }

    /// 执行完整上传流程
    pub async fn upload<R: tokio::io::AsyncBufRead + Unpin, W: tokio::io::AsyncWrite + Unpin>(
        &mut self,
        file_path: PathBuf,
        reader: &mut R,
        writer: &mut W,
        progress_tx: Option<Sender<TransferProgress>>,
    ) -> Result<(), ZmodemError> {
        // 1. 准备文件
        let file_info = self.prepare_upload(file_path.clone())?;

        // 2. 发送 ZRQINIT
        self.send_zrqinit(writer).await?;

        // 3. 等待远程响应后发送文件头
        // （实际实现需要等待 YASN 响应）
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        self.send_file_header(writer, &file_info).await?;

        // 4. 发送数据块
        self.send_data_blocks(reader, writer, progress_tx, 1024).await?;

        // 5. 发送 EOF
        self.send_eof(writer).await?;

        self.state = ZmodemState::Finished;
        Ok(())
    }

    // ==================== 下载相关方法 ====================

    /// 准备下载文件
    pub fn prepare_download(&mut self, save_path: PathBuf) -> Result<(), ZmodemError> {
        self.file_path = Some(save_path);
        self.file_size = 0;
        self.bytes_transferred = 0;
        self.state = ZmodemState::ReceivingData;
        Ok(())
    }

    /// 接收文件头
    pub async fn recv_file_header<R: tokio::io::AsyncRead + Unpin>(
        &mut self,
        reader: &mut R,
    ) -> Result<FileInfo, ZmodemError> {
        // 简化实现：读取一行作为文件头
        let mut header = String::new();
        use tokio::io::AsyncBufReadExt;
        use tokio::io::AsyncReadExt;
        // 使用 buf_read 包装器
        let mut buf_reader = tokio::io::BufReader::new(reader);
        buf_reader.read_line(&mut header).await?;

        // 解析文件头格式：D0 filename size mtime mode 0 0
        let parts: Vec<&str> = header.trim().split_whitespace().collect();
        if parts.len() >= 4 {
            let file_info = FileInfo {
                name: parts[1].to_string(),
                size: parts[2].parse().unwrap_or(0),
                modified_time: parts[3].parse().unwrap_or(0),
                mode: 0o644,
            };
            self.file_size = file_info.size;
            return Ok(file_info);
        }

        Err(ZmodemError::Protocol("Invalid file header".to_string()))
    }

    /// 接收数据块
    pub async fn recv_data_blocks<W: tokio::io::AsyncWrite + Unpin>(
        &mut self,
        writer: &mut W,
        reader: &mut (impl tokio::io::AsyncRead + Unpin),
        progress_tx: Option<Sender<TransferProgress>>,
        block_size: usize,
    ) -> Result<(), ZmodemError> {
        let mut buffer = vec![0u8; block_size];
        let start_time = std::time::Instant::now();

        loop {
            let n = reader.read(&mut buffer).await?;
            if n == 0 {
                break;
            }

            writer.write_all(&buffer[..n]).await?;
            writer.flush().await?;

            self.bytes_transferred += n as u64;

            // 发送进度更新
            if let Some(tx) = &progress_tx {
                let elapsed = start_time.elapsed().as_secs_f64();
                let speed = if elapsed > 0.0 {
                    self.bytes_transferred as f64 / elapsed
                } else {
                    0.0
                };

                let progress = TransferProgress {
                    bytes_transferred: self.bytes_transferred,
                    total_bytes: self.file_size,
                    filename: self
                        .file_path
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    speed_bytes_per_sec: speed,
                };

                tx.send(progress).await.ok();
            }
        }

        self.state = ZmodemState::Verifying;
        Ok(())
    }

    /// 验证 CRC
    pub fn verify_crc<R: tokio::io::AsyncRead + Unpin>(&mut self, _reader: &mut R) -> Result<(), ZmodemError> {
        // 简化实现：实际 ZMODEM 需要解析 CRC 包
        // 这里假设传输成功
        self.state = ZmodemState::Finished;
        Ok(())
    }

    /// 执行完整下载流程
    pub async fn download<R: tokio::io::AsyncBufRead + Unpin, W: tokio::io::AsyncWrite + Unpin>(
        &mut self,
        save_path: PathBuf,
        reader: &mut R,
        writer: &mut W,
        progress_tx: Option<Sender<TransferProgress>>,
    ) -> Result<(), ZmodemError> {
        // 1. 准备下载
        self.prepare_download(save_path)?;

        // 2. 接收文件头
        let _file_info = self.recv_file_header(reader).await?;

        // 3. 发送 YASN 确认
        writer.write_all(b"YASN\n").await?;
        writer.flush().await?;

        // 4. 接收数据块
        self.recv_data_blocks(writer, reader, progress_tx, 1024).await?;

        // 5. 验证 CRC（简化）
        self.verify_crc(reader)?;

        Ok(())
    }
}

impl Default for ZmodemTransfer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_transfer() {
        let transfer = ZmodemTransfer::new();
        assert_eq!(transfer.state(), &ZmodemState::Idle);
        assert_eq!(transfer.progress(), 0.0);
    }

    #[test]
    fn test_prepare_upload() {
        let mut transfer = ZmodemTransfer::new();
        let temp_file = std::env::temp_dir().join("test_zmodem.txt");
        std::fs::write(&temp_file, b"test content").unwrap();

        let result = transfer.prepare_upload(temp_file.clone());
        assert!(result.is_ok());

        let file_info = result.unwrap();
        assert_eq!(file_info.name, "test_zmodem.txt");
        assert_eq!(file_info.size, 12);

        // 清理
        std::fs::remove_file(temp_file).ok();
    }
}
