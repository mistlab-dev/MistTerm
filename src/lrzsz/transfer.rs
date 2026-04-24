//! 传输管理器
//!
//! 管理文件传输会话，处理 lrzsz 事件

use std::path::PathBuf;
use tokio::sync::mpsc;
use crate::lrzsz::{LrzszEvent, ZmodemTransfer, TransferProgress};

/// 传输状态
#[derive(Debug, Clone, PartialEq)]
pub enum TransferState {
    Pending,
    Transferring,
    Completed,
    Failed(String),
}

/// 活动传输
pub struct ActiveTransfer {
    pub id: usize,
    pub file_path: PathBuf,
    pub state: TransferState,
    pub progress: f32,
}

/// 传输管理器
pub struct TransferManager {
    /// 活动传输列表
    transfers: Vec<ActiveTransfer>,
    /// 下一个传输 ID
    next_id: usize,
}

impl TransferManager {
    /// 创建新的传输管理器
    pub fn new() -> Self {
        TransferManager {
            transfers: Vec::new(),
            next_id: 0,
        }
    }

    /// 处理 lrzsz 事件
    pub async fn handle_event(
        &mut self,
        event: LrzszEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match event {
            LrzszEvent::UploadReady => {
                self.handle_upload().await?;
            }
            LrzszEvent::DownloadReady(filename) => {
                self.handle_download(&filename).await?;
            }
        }
        Ok(())
    }

    /// 处理上传请求
    async fn handle_upload(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // 弹出文件选择器
        if let Some(file_path) = show_upload_dialog() {
            let id = self.next_id;
            self.next_id += 1;

            self.transfers.push(ActiveTransfer {
                id,
                file_path: file_path.clone(),
                state: TransferState::Transferring,
                progress: 0.0,
            });

            // TODO: 启动后台传输任务
            // tokio::spawn(async move {
            //     // 执行上传
            // });

            tracing::info!("开始上传文件：{:?}", file_path);
        }

        Ok(())
    }

    /// 处理下载请求
    async fn handle_download(&mut self, filename: &str) -> Result<(), Box<dyn std::error::Error>> {
        // 弹出保存对话框
        if let Some(save_path) = show_download_dialog(filename) {
            let id = self.next_id;
            self.next_id += 1;

            self.transfers.push(ActiveTransfer {
                id,
                file_path: save_path.clone(),
                state: TransferState::Transferring,
                progress: 0.0,
            });

            // TODO: 启动后台传输任务
            // tokio::spawn(async move {
            //     // 执行下载
            // });

            tracing::info!("开始下载文件到：{:?}", save_path);
        }

        Ok(())
    }

    /// 获取所有传输
    pub fn get_transfers(&self) -> &[ActiveTransfer] {
        &self.transfers
    }

    /// 获取单个传输
    pub fn get_transfer(&self, id: usize) -> Option<&ActiveTransfer> {
        self.transfers.iter().find(|t| t.id == id)
    }

    /// 更新传输进度
    pub fn update_progress(&mut self, id: usize, progress: f32) {
        if let Some(transfer) = self.transfers.iter_mut().find(|t| t.id == id) {
            transfer.progress = progress;
        }
    }

    /// 标记传输完成
    pub fn mark_completed(&mut self, id: usize) {
        if let Some(transfer) = self.transfers.iter_mut().find(|t| t.id == id) {
            transfer.state = TransferState::Completed;
            transfer.progress = 100.0;
        }
    }

    /// 标记传输失败
    pub fn mark_failed(&mut self, id: usize, error: String) {
        if let Some(transfer) = self.transfers.iter_mut().find(|t| t.id == id) {
            transfer.state = TransferState::Failed(error);
        }
    }

    /// 移除传输
    pub fn remove_transfer(&mut self, id: usize) {
        self.transfers.retain(|t| t.id != id);
    }

    /// 清除完成的传输
    pub fn clear_completed(&mut self) {
        self.transfers.retain(|t| t.state != TransferState::Completed);
    }
}

impl Default for TransferManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 显示上传文件对话框
fn show_upload_dialog() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("选择要上传的文件")
        .pick_file()
}

/// 显示下载保存对话框
fn show_download_dialog(filename: &str) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("选择保存位置")
        .set_file_name(filename)
        .save_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transfer_manager() {
        let mut manager = TransferManager::new();

        assert!(manager.get_transfers().is_empty());
        assert_eq!(manager.next_id, 0);
    }

    #[test]
    fn test_update_progress() {
        let mut manager = TransferManager::new();

        // 添加一个传输
        manager.transfers.push(ActiveTransfer {
            id: 0,
            file_path: PathBuf::from("test.txt"),
            state: TransferState::Transferring,
            progress: 0.0,
        });

        manager.update_progress(0, 50.0);

        let transfer = manager.get_transfer(0).unwrap();
        assert_eq!(transfer.progress, 50.0);
    }
}
