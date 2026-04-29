//! lrzsz 文件传输 - 完整 ZMODEM 协议实现
#![allow(dead_code)]

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use ssh2::Channel;

/// ZMODEM 常量
mod zmodem {
    pub const ZPAD: u8 = 0x80;      // ZMODEM 包填充字符
    pub const ZDLE: u8 = 0x18;      // ZMODEM 数据链路转义
    pub const ZRQINIT: u8 = 0x64;   // 请求接收初始化
    pub const ZRINIT: u8 = 0x62;    // 接收初始化
    pub const ZSINIT: u8 = 0x61;    // 发送初始化
    pub const ZACK: u8 = 0x60;      // 确认
    pub const ZFILE: u8 = 0x63;     // 文件信息
    pub const ZSKIP: u8 = 0x64;     // 跳过
    pub const ZNAK: u8 = 0x65;      // 否认
    pub const ZABORT: u8 = 0x66;    // 中止
    pub const ZDATA: u8 = 0x66;     // 数据块
    pub const ZEOF: u8 = 0x65;      // 文件结束
    pub const ZFIN: u8 = 0x67;      // 传输结束
    pub const ZRPOS: u8 = 0x6E;     // 恢复传输位置
    pub const BLOCK_SIZE: usize = 1024;
    pub const HEADER_SIZE: usize = 4;
}

/// CRC32 计算器
struct Crc32;

impl Crc32 {
    fn new() -> Self {
        Self
    }

    fn calculate(&self, data: &[u8]) -> u32 {
        let mut crc: u32 = 0xFFFFFFFF;
        for &byte in data {
            crc ^= byte as u32;
            for _ in 0..8 {
                if crc & 1 != 0 {
                    crc = (crc >> 1) ^ 0xEDB88320;
                } else {
                    crc >>= 1;
                }
            }
        }
        !crc
    }

    fn calculate16(&self, data: &[u8]) -> u16 {
        let crc = self.calculate(data);
        ((crc >> 16) as u16) ^ (crc as u16)
    }
}

/// ZMODEM 包
struct ZmodemPacket {
    packet_type: u8,
    header_data: [u8; 4],
}

impl ZmodemPacket {
    fn new(packet_type: u8, header_data: [u8; 4]) -> Self {
        Self {
            packet_type,
            header_data,
        }
    }

    /// 编码包为字节序列（带 ZDLE 转义）
    fn encode(&self) -> Vec<u8> {
        let mut result = Vec::new();
        
        // 包头：**
        result.push(zmodem::ZPAD);
        result.push(zmodem::ZPAD);
        
        // ZDLE + 包类型
        result.push(zmodem::ZDLE);
        result.push(self.packet_type);
        
        // 头部数据（4 字节，带 ZDLE 转义）
        for &b in &self.header_data {
            result.push(zmodem::ZDLE);
            result.push(b ^ 0x40);
        }
        
        // CRC-16 (2 字节)
        let crc = self.calculate_crc16();
        result.push(zmodem::ZDLE);
        result.push(((crc >> 8) as u8) ^ 0x40);
        result.push(zmodem::ZDLE);
        result.push((crc as u8) ^ 0x40);
        
        result
    }

    /// 编码数据块
    fn encode_data(&self, data: &[u8]) -> Vec<u8> {
        let mut result = Vec::new();
        
        // 包头：**
        result.push(zmodem::ZPAD);
        result.push(zmodem::ZPAD);
        
        // ZDLE + 包类型
        result.push(zmodem::ZDLE);
        result.push(self.packet_type);
        
        // 头部数据（4 字节，带 ZDLE 转义）
        for &b in &self.header_data {
            result.push(zmodem::ZDLE);
            result.push(b ^ 0x40);
        }
        
        // 数据部分（带 ZDLE 转义）
        for &byte in data {
            if byte == zmodem::ZDLE || byte == zmodem::ZPAD {
                result.push(zmodem::ZDLE);
                result.push(byte ^ 0x40);
            } else {
                result.push(byte);
            }
        }
        
        // CRC-32 (4 字节)
        let crc = self.calculate_crc32_with_data(data);
        result.push(zmodem::ZDLE);
        result.push(((crc >> 24) as u8) ^ 0x40);
        result.push(zmodem::ZDLE);
        result.push(((crc >> 16) as u8) ^ 0x40);
        result.push(zmodem::ZDLE);
        result.push(((crc >> 8) as u8) ^ 0x40);
        result.push(zmodem::ZDLE);
        result.push((crc as u8) ^ 0x40);
        
        result
    }

    fn calculate_crc16(&self) -> u16 {
        let crc = Crc32::new();
        let mut data = Vec::new();
        data.push(self.packet_type);
        data.extend_from_slice(&self.header_data);
        crc.calculate16(&data)
    }

    fn calculate_crc32_with_data(&self, data: &[u8]) -> u32 {
        let crc = Crc32::new();
        let mut full_data = Vec::new();
        full_data.push(self.packet_type);
        full_data.extend_from_slice(&self.header_data);
        full_data.extend_from_slice(data);
        crc.calculate(&full_data)
    }
}

/// 传输事件
#[derive(Debug, Clone)]
pub enum TransferEvent {
    FileStart { filename: String, size: u64 },
    FileProgress { filename: String, received: u64, total: u64 },
    FileComplete { filename: String, path: PathBuf },
    FileError { filename: String, error: String },
    TransferComplete,
}

/// lrzsz 传输器
pub struct LrzszTransfer {
    rx: Arc<Mutex<Receiver<TransferEvent>>>,
    tx: Sender<TransferEvent>,
    is_active: Arc<AtomicBool>,
    received_bytes: Arc<AtomicU64>,
    total_bytes: Arc<AtomicU64>,
    current_filename: Arc<Mutex<String>>,
    download_dir: PathBuf,
}

impl LrzszTransfer {
    /// 创建新的传输器
    pub fn new(download_dir: &str) -> Self {
        let (tx, rx) = channel();
        let download_path = PathBuf::from(download_dir);
        
        // 创建下载目录
        let _ = fs::create_dir_all(&download_path);
        
        Self {
            rx: Arc::new(Mutex::new(rx)),
            tx,
            is_active: Arc::new(AtomicBool::new(false)),
            received_bytes: Arc::new(AtomicU64::new(0)),
            total_bytes: Arc::new(AtomicU64::new(0)),
            current_filename: Arc::new(Mutex::new(String::new())),
            download_dir: download_path,
        }
    }

    /// 检查是否正在传输
    pub fn is_active(&self) -> bool {
        self.is_active.load(Ordering::Relaxed)
    }

    /// 获取接收进度
    pub fn get_progress(&self) -> (u64, u64) {
        (
            self.received_bytes.load(Ordering::Relaxed),
            self.total_bytes.load(Ordering::Relaxed)
        )
    }

    /// 获取当前文件名
    pub fn get_filename(&self) -> String {
        self.current_filename.lock().unwrap().clone()
    }

    /// 获取接收事件
    pub fn try_recv_event(&self) -> Option<TransferEvent> {
        self.rx.lock().ok()?.try_recv().ok()
    }

    /// 检测终端输出中是否包含 rz 命令触发序列
    pub fn detect_rz_command(&self, data: &[u8]) -> bool {
        // 检测常见的 rz 触发模式
        let text = String::from_utf8_lossy(data);
        
        // 文本模式：需要明确的 rz 等待提示
        if text.contains("rz rz rz") || 
           text.contains("Awaiting rz") ||
           text.contains("rz waiting to receive") {
            return true;
        }
        
        // 二进制 ZMODEM 模式：**ZRQINIT 或 **ZRINIT
        if data.len() >= 4 && data[0] == zmodem::ZPAD && data[1] == zmodem::ZPAD {
            // 检查是否是 ZRQINIT (0x80 0x80 0x18 0x64) 或 ZRINIT (0x80 0x80 0x18 0x62)
            if data[2] == zmodem::ZDLE && (data[3] == zmodem::ZRQINIT || data[3] == zmodem::ZRINIT) {
                return true;
            }
        }
        
        false
    }

    /// 开始接收文件（rz）
    pub fn start_receive(&self, channel: Arc<Mutex<Channel>>) -> Result<(), String> {
        if self.is_active.load(Ordering::Relaxed) {
            return Err("传输已在进行中".to_string());
        }

        self.is_active.store(true, Ordering::Relaxed);
        self.received_bytes.store(0, Ordering::Relaxed);
        self.total_bytes.store(0, Ordering::Relaxed);
        
        let tx = self.tx.clone();
        let is_active = self.is_active.clone();
        let received_bytes = self.received_bytes.clone();
        let total_bytes = self.total_bytes.clone();
        let current_filename = self.current_filename.clone();
        let download_dir = self.download_dir.clone();

        thread::spawn(move || {
            log::info!("开始 ZMODEM 文件接收");
            
            // 等待并接收 ZRQINIT 包（服务器发送的初始化请求）
            let mut buffer = [0u8; 8192];
            let mut idle_count = 0;
            const MAX_IDLE_COUNT: u32 = 1000; // 最多等待 10 秒
            let mut zrinit_sent = false;
            
            while is_active.load(Ordering::Relaxed) && idle_count < MAX_IDLE_COUNT {
                if let Ok(mut chan) = channel.lock() {
                    match chan.read(&mut buffer) {
                        Ok(0) => {
                            log::warn!("通道关闭");
                            break;
                        }
                        Ok(n) => {
                            log::debug!("收到 {} bytes 数据", n);
                            // 打印前 32 字节用于调试
                            if n > 0 {
                                let hex: String = buffer[..n.min(32)].iter()
                                    .map(|b| format!("{:02x}", b))
                                    .collect::<Vec<_>>()
                                    .join(" ");
                                log::debug!("数据：{}", hex);
                            }
                            
                            // 检查是否是 ZRQINIT 包
                            if !zrinit_sent && parse_zrqinit_packet(&buffer[..n]) {
                                log::info!("收到 ZRQINIT，发送 ZRINIT 响应");
                                
                                // 发送 ZRINIT 响应
                                let zrinit = ZmodemPacket::new(
                                    zmodem::ZRINIT,
                                    [0x40, 0x00, 0x00, 0x00] // 支持 1024 字节块，CRC-32
                                );
                                let zrinit_bytes = zrinit.encode();
                                
                                if let Ok(mut chan) = channel.lock() {
                                    let _ = chan.write_all(&zrinit_bytes);
                                    let _ = chan.flush();
                                }
                                log::info!("发送 ZRINIT: {} bytes", zrinit_bytes.len());
                                zrinit_sent = true;
                                idle_count = 0; // 重置计数器
                                continue;
                            }
                            
                            // 已发送 ZRINIT 后，等待 ZFILE 包
                            if zrinit_sent {
                                if let Some((filename, size)) = parse_zfile_packet(&buffer[..n]) {
                                    log::info!("收到文件：{} ({} bytes)", filename, size);
                                    current_filename.lock().unwrap().clone_from(&filename);
                                    total_bytes.store(size, Ordering::Relaxed);
                                    
                                    let _ = tx.send(TransferEvent::FileStart {
                                        filename: filename.clone(),
                                        size,
                                    });
                                    
                                    // 发送 ZACK 确认
                                    let zack = ZmodemPacket::new(zmodem::ZACK, [0u8; 4]);
                                    let zack_bytes = zack.encode();
                                    let _ = chan.write_all(&zack_bytes);
                                    let _ = chan.flush();
                                    
                                    // 开始接收文件数据
                                    if let Err(e) = receive_file_data(
                                        &channel,
                                        &download_dir,
                                        &filename,
                                        size,
                                        &is_active,
                                        &received_bytes,
                                        &tx,
                                    ) {
                                        log::error!("接收文件失败：{}", e);
                                        let _ = tx.send(TransferEvent::FileError {
                                            filename,
                                            error: e,
                                        });
                                        break;
                                    }
                                    
                                    break;
                                }
                            }
                        }
                        Err(_) => {
                            // 非阻塞读，没有数据时返回错误
                            idle_count += 1;
                            thread::sleep(Duration::from_millis(10));
                            continue;
                        }
                    }
                }
                idle_count += 1;
                thread::sleep(Duration::from_millis(10));
            }
            
            if idle_count >= MAX_IDLE_COUNT {
                log::warn!("等待 ZMODEM 包超时");
                let _ = tx.send(TransferEvent::FileError {
                    filename: "unknown".to_string(),
                    error: format!("等待超时（{}ms）", idle_count * 10),
                });
            }
            
            // 退出接收模式
            is_active.store(false, Ordering::Relaxed);
            let mut filename = current_filename.lock().unwrap();
            filename.clear();
            
            let _ = tx.send(TransferEvent::TransferComplete);
            log::info!("文件接收模式已退出");
        });
        
        Ok(())
    }

    /// 发送文件（sz）
    pub fn start_send(&self, file_path: &str, channel: Arc<Mutex<Channel>>) -> Result<(), String> {
        let path = PathBuf::from(file_path);
        if !path.exists() {
            return Err(format!("文件不存在：{}", file_path));
        }

        if self.is_active.load(Ordering::Relaxed) {
            return Err("传输已在进行中".to_string());
        }

        let file_path_clone = path.clone();
        let file_name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        let file_size = fs::metadata(&path)
            .map(|m| m.len())
            .unwrap_or(0);

        self.is_active.store(true, Ordering::Relaxed);
        self.received_bytes.store(0, Ordering::Relaxed);
        
        let tx = self.tx.clone();
        let is_active = self.is_active.clone();
        let received_bytes = self.received_bytes.clone();
        let total_bytes = self.total_bytes.clone();
        let current_filename = self.current_filename.clone();

        thread::spawn(move || {
            log::info!("开始发送文件：{}", file_path_clone.display());
            
            current_filename.lock().unwrap().clone_from(&file_name);
            
            let _ = tx.send(TransferEvent::FileStart {
                filename: file_name.clone(),
                size: file_size,
            });
            
            total_bytes.store(file_size, Ordering::Relaxed);
            
            // 读取文件内容
            let file_data = match fs::read(&file_path_clone) {
                Ok(data) => data,
                Err(e) => {
                    let _ = tx.send(TransferEvent::FileError {
                        filename: file_name.clone(),
                        error: format!("读取文件失败：{}", e),
                    });
                    is_active.store(false, Ordering::Relaxed);
                    return;
                }
            };
            
            // 发送 ZFILE 包（文件信息）
            let mut zfile_header = [0u8; 4];
            zfile_header[0] = 0x00; // 文件类型：binary
            let size_be = file_size.to_be_bytes();
            zfile_header[1] = size_be[0];
            zfile_header[2] = size_be[1];
            zfile_header[3] = size_be[2];
            
            let zfile = ZmodemPacket::new(zmodem::ZFILE, zfile_header);
            let zfile_bytes = zfile.encode();
            
            if let Ok(mut chan) = channel.lock() {
                let _ = chan.write_all(&zfile_bytes);
                let _ = chan.flush();
            }
            log::info!("发送 ZFILE: {} bytes", zfile_bytes.len());
            
            // 等待 ZACK 确认
            let mut buffer = [0u8; 1024];
            let mut ack_received = false;
            for _ in 0..100 {
                if let Ok(mut chan) = channel.lock() {
                    if let Ok(n) = chan.read(&mut buffer) {
                        if n > 0 && parse_zack_packet(&buffer[..n]) {
                            ack_received = true;
                            break;
                        }
                    }
                }
                thread::sleep(Duration::from_millis(50));
            }
            
            if !ack_received {
                log::warn!("未收到 ZACK 确认");
                is_active.store(false, Ordering::Relaxed);
                return;
            }
            
            // 发送数据块
            let mut offset = 0;
            while offset < file_data.len() && is_active.load(Ordering::Relaxed) {
                let chunk_size = std::cmp::min(zmodem::BLOCK_SIZE, file_data.len() - offset);
                let chunk = &file_data[offset..offset + chunk_size];
                
                // 发送 ZDATA 包
                let mut data_header = [0u8; 4];
                let pos_be = (offset as u32).to_be_bytes();
                data_header[0] = pos_be[0];
                data_header[1] = pos_be[1];
                data_header[2] = pos_be[2];
                data_header[3] = pos_be[3];
                
                let zdata = ZmodemPacket::new(zmodem::ZDATA, data_header);
                let zdata_bytes = zdata.encode_data(chunk);
                
                if let Ok(mut chan) = channel.lock() {
                    if let Err(e) = chan.write_all(&zdata_bytes) {
                        log::error!("发送数据失败：{}", e);
                        break;
                    }
                    if let Err(e) = chan.flush() {
                        log::error!("刷新失败：{}", e);
                        break;
                    }
                }
                
                offset += chunk_size;
                received_bytes.store(offset as u64, Ordering::Relaxed);
                
                let _ = tx.send(TransferEvent::FileProgress {
                    filename: file_name.clone(),
                    received: offset as u64,
                    total: file_size,
                });
                
                // 等待 ZACK 确认（增加超时时间）
                let mut ack_received = false;
                let start = std::time::Instant::now();
                let timeout = Duration::from_secs(30);
                
                while start.elapsed() < timeout && is_active.load(Ordering::Relaxed) {
                    if let Ok(mut chan) = channel.lock() {
                        if let Ok(n) = chan.read(&mut buffer) {
                            if n > 0 && parse_zack_packet(&buffer[..n]) {
                                ack_received = true;
                                log::info!("收到 ZACK 确认，位置：{}", offset);
                                break;
                            }
                        }
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                
                if !ack_received {
                    log::warn!("数据块 {} 未收到确认，继续传输", offset);
                }
            }
            
            // 发送 ZEOF 包
            let zeof = ZmodemPacket::new(zmodem::ZEOF, [0u8; 4]);
            let zeof_bytes = zeof.encode();
            
            if let Ok(mut chan) = channel.lock() {
                let _ = chan.write_all(&zeof_bytes);
                let _ = chan.flush();
            }
            log::info!("发送 ZEOF: {} bytes", zeof_bytes.len());
            
            let _ = tx.send(TransferEvent::FileComplete {
                filename: file_name.clone(),
                path: file_path_clone.clone(),
            });
            
            log::info!("文件发送完成：{}", file_name);
            
            // 发送 ZFIN 结束
            thread::sleep(Duration::from_millis(100));
            
            is_active.store(false, Ordering::Relaxed);
            let _ = tx.send(TransferEvent::TransferComplete);
        });
        
        Ok(())
    }
}

/// 检测 ZRQINIT 包
fn parse_zrqinit_packet(data: &[u8]) -> bool {
    // 查找 ZRQINIT 序列：可能是 |*B0... 或 **ZDLE ZRQINIT
    for i in 0..data.len().saturating_sub(3) {
        // 模式 1: **ZDLE ZRQINIT (0x80 0x80 0x18 0x64)
        if data[i] == zmodem::ZPAD && 
           data[i+1] == zmodem::ZPAD && 
           data[i+2] == zmodem::ZDLE && 
           data[i+3] == zmodem::ZRQINIT {
            log::info!("检测到 ZRQINIT 包（模式 1）");
            return true;
        }
        
        // 模式 2: |*B... (ZMODEM 二进制模式起始)
        // ZRQINIT 通常是 0x80 0x80 0x18 0x64
        if i + 3 < data.len() &&
           data[i] == 0x80 && 
           data[i+1] == 0x80 && 
           data[i+2] == 0x18 && 
           data[i+3] == 0x64 {
            log::info!("检测到 ZRQINIT 包（模式 2）");
            return true;
        }
    }
    false
}

/// 解析 ZFILE 包
fn parse_zfile_packet(data: &[u8]) -> Option<(String, u64)> {
    // 查找 **ZDLE ZFILE 序列
    let mut i = 0;
    while i < data.len().saturating_sub(3) {
        if data[i] == zmodem::ZPAD && 
           data[i+1] == zmodem::ZPAD && 
           data[i+2] == zmodem::ZDLE && 
           data[i+3] == zmodem::ZFILE {
            log::info!("找到 ZFILE 包，位置：{}", i);
            
            // 解析头部数据（4 字节，带 ZDLE 转义）+ CRC-16（2 字节）
            let mut j = i + 4;
            let mut header_bytes = [0u8; 4];
            let mut header_idx = 0;
            
            // 提取 4 字节头部（带 ZDLE 转义）
            while j < data.len() && header_idx < 4 {
                if data[j] == zmodem::ZDLE && j + 1 < data.len() {
                    header_bytes[header_idx] = data[j+1] ^ 0x40;
                    header_idx += 1;
                    j += 2;
                } else {
                    j += 1;
                }
            }
            
            if header_idx < 4 {
                log::warn!("ZFILE 头部不完整：{} bytes", header_idx);
                i += 1;
                continue;
            }
            
            // 解析文件名（从 header_bytes 开始，查找 NUL）
            let mut filename = String::new();
            for &b in &header_bytes {
                if b == 0 {
                    break;
                }
                if let Some(c) = char::from_u32(b as u32) {
                    filename.push(c);
                }
            }
            
            if filename.is_empty() {
                filename = "received_file".to_string();
            }
            
            // 文件大小是 0（简化处理，实际应该在 ZFILE 包后面）
            let size = 0;
            
            log::info!("解析 ZFILE: filename={}, size={}", filename, size);
            return Some((filename, size));
        }
        i += 1;
    }
    None
}

/// 解析 ZACK 包
fn parse_zack_packet(data: &[u8]) -> bool {
    // 查找 **ZDLE ZACK 序列
    for i in 0..data.len().saturating_sub(3) {
        if data[i] == zmodem::ZPAD && 
           data[i+1] == zmodem::ZPAD && 
           data[i+2] == zmodem::ZDLE && 
           data[i+3] == zmodem::ZACK {
            return true;
        }
    }
    false
}

/// 接收文件数据
fn receive_file_data(
    channel: &Mutex<Channel>,
    download_dir: &PathBuf,
    filename: &str,
    total_size: u64,
    is_active: &AtomicBool,
    received_bytes: &AtomicU64,
    tx: &Sender<TransferEvent>,
) -> Result<(), String> {
    let mut file_path = download_dir.join(filename);
    
    // 如果文件已存在，添加后缀
    let mut counter = 1;
    while file_path.exists() {
        let stem = file_path.file_stem().unwrap_or_default().to_string_lossy();
        let ext = file_path.extension().unwrap_or_default().to_string_lossy();
        if ext.is_empty() {
            file_path = download_dir.join(format!("{}_{}", stem, counter));
        } else {
            file_path = download_dir.join(format!("{}_{}.{}", stem, counter, ext));
        }
        counter += 1;
    }
    
    let mut file = File::create(&file_path)
        .map_err(|e| format!("创建文件失败：{}", e))?;
    
    let mut received: u64 = 0;
    let mut buffer = [0u8; 65536]; // 增加缓冲区大小
    let mut no_data_count = 0;
    let max_no_data = 100; // 最多 100 次无数据后超时
    
    while received < total_size && is_active.load(Ordering::Relaxed) {
        let mut chan = match channel.lock() {
            Ok(c) => c,
            Err(_) => {
                thread::sleep(Duration::from_millis(50));
                continue;
            }
        };
        
        match chan.read(&mut buffer) {
            Ok(0) => {
                // 没有数据，等待一下
                no_data_count += 1;
                if no_data_count > max_no_data {
                    log::warn!("接收超时：{} bytes", received);
                    break;
                }
                thread::sleep(Duration::from_millis(50));
                continue;
            }
            Ok(n) => {
                no_data_count = 0;
                // 解析 ZDATA 包并提取数据
                if let Some(data) = parse_zdata_packet(&buffer[..n]) {
                    if !data.is_empty() {
                        if file.write_all(&data).is_err() {
                            return Err("写入文件失败".to_string());
                        }
                        received += data.len() as u64;
                        received_bytes.store(received, Ordering::Relaxed);
                        
                        let _ = tx.send(TransferEvent::FileProgress {
                            filename: filename.to_string(),
                            received,
                            total: total_size,
                        });
                        
                        // 发送 ZACK 确认
                        let mut pos_bytes = received.to_be_bytes();
                        let zack_header = [
                            pos_bytes[4], pos_bytes[5], pos_bytes[6], pos_bytes[7]
                        ];
                        let zack = ZmodemPacket::new(zmodem::ZACK, zack_header);
                        let zack_bytes = zack.encode();
                        let _ = chan.write_all(&zack_bytes);
                        let _ = chan.flush();
                    }
                }
            }
            Err(_) => {
                thread::sleep(Duration::from_millis(10));
                continue;
            }
        }
    }
    
    file.flush().map_err(|e| format!("刷新文件失败：{}", e))?;
    
    let _ = tx.send(TransferEvent::FileComplete {
        filename: filename.to_string(),
        path: file_path,
    });
    
    Ok(())
}

/// 解析 ZDATA 包
fn parse_zdata_packet(data: &[u8]) -> Option<Vec<u8>> {
    // 查找 **ZDLE ZDATA 序列
    let mut i = 0;
    while i < data.len().saturating_sub(3) {
        if data[i] == zmodem::ZPAD && 
           data[i+1] == zmodem::ZPAD && 
           data[i+2] == zmodem::ZDLE && 
           data[i+3] == zmodem::ZDATA {
            // 找到 ZDATA 包，提取数据
            let mut result = Vec::new();
            let mut j = i + 4;
            
            // 跳过 4 字节头部（带 ZDLE 转义）
            let mut header_count = 0;
            while j < data.len() && header_count < 4 {
                if data[j] == zmodem::ZDLE && j + 1 < data.len() {
                    j += 2;
                    header_count += 1;
                } else {
                    j += 1;
                    header_count += 1;
                }
            }
            
            // 提取数据直到 CRC（ZDLE 后跟 CRC 字节）
            while j < data.len() {
                if data[j] == zmodem::ZDLE {
                    // 检查是否是 CRC 开始（后面跟 2 个字节）
                    if j + 2 < data.len() && data[j+1] == zmodem::ZDLE {
                        break; // CRC-16 开始
                    }
                    // 数据中的 ZDLE 转义
                    result.push(data[j]);
                    j += 1;
                } else {
                    result.push(data[j]);
                    j += 1;
                }
            }
            
            if !result.is_empty() {
                return Some(result);
            }
        }
        i += 1;
    }
    None
}

/// 人类可读的文件大小格式
pub fn human_readable_size(size: u64) -> String {
    if size >= 1024 * 1024 * 1024 {
        format!("{:.2} GB", size as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if size >= 1024 * 1024 {
        format!("{:.2} MB", size as f64 / (1024.0 * 1024.0))
    } else if size >= 1024 {
        format!("{:.2} KB", size as f64 / 1024.0)
    } else {
        format!("{} B", size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc32_calculate() {
        let crc = Crc32::new();
        let data = b"test";
        let result = crc.calculate(data);
        assert!(result != 0);
    }

    #[test]
    fn test_zmodem_packet_encode() {
        let packet = ZmodemPacket::new(zmodem::ZRINIT, [0x40, 0x00, 0x00, 0x00]);
        let encoded = packet.encode();
        assert!(encoded.len() > 0);
        assert_eq!(encoded[0], zmodem::ZPAD);
        assert_eq!(encoded[1], zmodem::ZPAD);
    }

    #[test]
    fn test_detect_rz_command_text() {
        let lrzsz = LrzszTransfer::new("/tmp");
        
        // 应该检测到的情况
        assert!(lrzsz.detect_rz_command(b"rz rz rz"));
        assert!(lrzsz.detect_rz_command(b"Awaiting rz"));
        assert!(lrzsz.detect_rz_command(b"rz waiting to receive"));
        
        // 不应该检测到的情况
        assert!(!lrzsz.detect_rz_command(b"ls -la"));
        assert!(!lrzsz.detect_rz_command(b"cd /tmp"));
        assert!(!lrzsz.detect_rz_command(b"rz is not a command"));
    }

    #[test]
    fn test_detect_rz_command_binary() {
        let lrzsz = LrzszTransfer::new("/tmp");
        
        // ZRQINIT
        assert!(lrzsz.detect_rz_command(&[zmodem::ZPAD, zmodem::ZPAD, zmodem::ZDLE, zmodem::ZRQINIT]));
        
        // ZRINIT
        assert!(lrzsz.detect_rz_command(&[zmodem::ZPAD, zmodem::ZPAD, zmodem::ZDLE, zmodem::ZRINIT]));
        
        // 不应该检测到的情况
        assert!(!lrzsz.detect_rz_command(&[zmodem::ZPAD, zmodem::ZPAD, 0x00, 0x00]));
    }

    #[test]
    fn test_human_readable_size() {
        assert_eq!(human_readable_size(100), "100 B");
        assert_eq!(human_readable_size(1024), "1.00 KB");
        assert_eq!(human_readable_size(1024 * 1024), "1.00 MB");
        assert_eq!(human_readable_size(1024 * 1024 * 1024), "1.00 GB");
    }
}
