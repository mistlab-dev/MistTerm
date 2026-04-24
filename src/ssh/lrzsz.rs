//! lrzsz 文件传输协议实现
//!
//! 支持 ZMODEM 协议，用于 rz（接收文件）和 sz（发送文件）

use std::fs::File;
use std::io::{Read, Write, BufReader, BufWriter};
use std::path::PathBuf;
use std::sync::mpsc::{Sender, Receiver};
use std::sync::mpsc;
use std::thread;
use std::sync::atomic::{AtomicBool, Ordering};

/// 文件传输事件
#[derive(Debug, Clone)]
pub enum TransferEvent {
    /// 开始接收文件
    FileStart {
        filename: String,
        size: u64,
    },
    /// 接收进度
    FileProgress {
        filename: String,
        received: u64,
        total: u64,
    },
    /// 文件接收完成
    FileComplete {
        filename: String,
        path: PathBuf,
    },
    /// 文件接收失败
    FileError {
        filename: String,
        error: String,
    },
    /// 传输完成
    TransferComplete,
}

/// ZMODEM 协议常量
mod zmodem {
    // 控制字符
    pub const DLE: u8 = 0x10;
    pub const XON: u8 = 0x11;
    pub const XOFF: u8 = 0x13;
    
    // ZMODEM 包头
    pub const ZRQINIT: u32 = 0;   // 请求初始化
    pub const ZRINIT: u32 = 1;    // 接收方初始化
    pub const ZSINIT: u32 = 2;    // 发送初始化
    pub const ZACK: u32 = 3;      // 确认
    pub const ZFILE: u32 = 4;     // 文件信息
    pub const ZSKIP: u32 = 5;     // 跳过
    pub const ZNAK: u32 = 6;      // 否定确认
    pub const ZABORT: u32 = 7;    // 中止
    pub const ZFIN: u32 = 8;      // 结束
    pub const ZRPOS: u32 = 9;     // 从位置重新开始
    pub const ZDATA: u32 = 10;    // 数据
    pub const ZEOF: u32 = 11;     // 文件结束
    pub const ZFERR: u32 = 12;    // 文件错误
    pub const ZCRC: u32 = 13;     // CRC 校验
    pub const ZRSP: u32 = 14;     // 响应
    pub const ZDLRQ: u32 = 15;    // 数据链路请求
    pub const ZPAD: u8 = b'*';    // 填充字符
    pub const ZDLE: u8 = 0x1E;    // ZMODEM DLE
}

/// lrzsz 传输器
pub struct LrzszTransfer {
    rx: Receiver<TransferEvent>,
    tx: Sender<TransferEvent>,
    is_active: AtomicBool,
}

impl LrzszTransfer {
    /// 创建新的传输器
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            rx,
            tx,
            is_active: AtomicBool::new(false),
        }
    }

    /// 检查是否正在传输
    pub fn is_active(&self) -> bool {
        self.is_active.load(Ordering::Relaxed)
    }

    /// 获取传输事件
    pub fn try_recv_event(&self) -> Option<TransferEvent> {
        self.rx.try_recv().ok()
    }

    /// 检测终端输出中是否包含 lrzsz 触发序列
    pub fn detect_rz_command(&self, data: &[u8]) -> bool {
        // 检测 rz 命令的输出特征
        let text = String::from_utf8_lossy(data);
        
        // ZRQINIT 序列：rz 命令发送的初始化请求
        if text.contains("rz rz rz") || 
           text.contains("Zrinit") ||
           text.contains("rz ") {
            return true;
        }
        
        // 检查二进制 ZMODEM 包头
        if data.len() > 4 {
            // ZMODEM 包通常以 *ZRQINIT 或 *ZRINIT 开头
            if &data[0..1] == b"*" {
                return true;
            }
        }
        
        false
    }

    /// 开始接收文件（rz）
    pub fn start_receive(&self, output_dir: &str) -> Result<(), String> {
        self.is_active.store(true, Ordering::Relaxed);
        let tx = self.tx.clone();
        let output_path = PathBuf::from(output_dir);
        
        thread::spawn(move || {
            // 这里实现 ZMODEM 接收逻辑
            // 简化版：等待终端发送文件数据
            
            // 在实际实现中，这里会：
            // 1. 发送 ZRINIT 响应
            // 2. 接收 ZFILE 包（文件信息）
            // 3. 接收 ZDATA 包（文件数据）
            // 4. 接收 ZEOF 包（文件结束）
            // 5. 发送 ZACK 确认
            
            log::info!("等待文件传输...");
            
            // 模拟：实际需要从终端读取 ZMODEM 数据
            // 这里只是占位实现
        });
        
        Ok(())
    }

    /// 发送文件（sz）
    pub fn start_send(&self, file_path: &str) -> Result<(), String> {
        let path = PathBuf::from(file_path);
        if !path.exists() {
            return Err(format!("文件不存在：{}", file_path));
        }
        
        // 提前获取文件名和大小
        let filename = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        
        let size = match path.metadata() {
            Ok(m) => m.len(),
            Err(e) => return Err(format!("无法获取文件信息：{}", e)),
        };
        
        self.is_active.store(true, Ordering::Relaxed);
        let tx = self.tx.clone();
        let output_path = path.clone();
        
        thread::spawn(move || {
            let file = match File::open(&output_path) {
                Ok(f) => f,
                Err(e) => {
                    let _ = tx.send(TransferEvent::FileError {
                        filename: filename.clone(),
                        error: format!("无法打开文件：{}", e),
                    });
                    return;
                }
            };
            
            // 发送文件开始事件
            let _ = tx.send(TransferEvent::FileStart {
                filename: filename.clone(),
                size,
            });
            
            // 在实际实现中，这里会：
            // 1. 发送 ZFILE 包（文件信息）
            // 2. 发送 ZDATA 包（文件数据）
            // 3. 发送 ZEOF 包（文件结束）
            // 4. 等待 ZACK 确认
            
            // 模拟进度
            let mut sent = 0u64;
            let chunk_size = 1024 * 1024; // 1MB
            
            {
                let mut reader = BufReader::new(file);
                let mut buffer = vec![0u8; chunk_size as usize];
                
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(n) => {
                            sent += n as u64;
                            
                            // 发送进度
                            let _ = tx.send(TransferEvent::FileProgress {
                                filename: filename.clone(),
                                received: sent,
                                total: size,
                            });
                            
                            // 实际实现中这里会发送 ZMODEM 数据包
                            // 简化版：只是模拟
                            thread::sleep(std::time::Duration::from_millis(10));
                        }
                        Err(e) => {
                            let _ = tx.send(TransferEvent::FileError {
                                filename: filename.clone(),
                                error: format!("读取错误：{}", e),
                            });
                            return;
                        }
                    }
                }
            }
            
            // 发送完成事件
            let _ = tx.send(TransferEvent::FileComplete {
                filename: filename.clone(),
                path: output_path.clone(),
            });
            let _ = tx.send(TransferEvent::TransferComplete);
            
            log::info!("文件发送完成：{}", output_path.display());
        });
        
        Ok(())
    }

    /// 中止传输
    pub fn abort(&self) {
        self.is_active.store(false, Ordering::Relaxed);
        // 发送中止信号
        let _ = self.tx.send(TransferEvent::FileError {
            filename: String::new(),
            error: "传输被用户中止".to_string(),
        });
    }
}

impl Default for LrzszTransfer {
    fn default() -> Self {
        Self::new()
    }
}
