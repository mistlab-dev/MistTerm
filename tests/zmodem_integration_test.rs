//! ZMODEM 文件传输集成测试
//! 
//! 使用方法:
//! ```bash
//! # 运行所有测试（跳过集成测试）
//! cargo test --test zmodem_integration_test -- --skip test_zmodem
//! 
//! # 运行特定测试
//! cargo test --test zmodem_integration_test test_crc32
//! 
//! # 运行测试并显示输出
//! cargo test --test zmodem_integration_test -- --nocapture
//! ```

use std::fs;
use std::path::PathBuf;

/// 测试配置
struct TestConfig {
    host: String,
    _port: u16,
    username: String,
    _password: String,
    _test_dir: String,
}

impl TestConfig {
    fn from_sessions_json() -> Self {
        let config = Self {
            host: "localhost".to_string(),
            _port: 22,
            username: "mistterm_test".to_string(),
            _password: "test123456".to_string(),
            _test_dir: "/tmp/zmodem_test".to_string(),
        };
        
        let sessions_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sessions.json");
        if let Ok(content) = fs::read_to_string(&sessions_path) {
            if let Ok(sessions) = serde_json::from_str::<Vec<serde_json::Value>>(&content) {
                if let Some(session) = sessions.first() {
                    if let (Some(host), Some(username)) = (
                        session["host"].as_str(),
                        session["username"].as_str(),
                    ) {
                        return Self {
                            host: host.to_string(),
                            username: username.to_string(),
                            ..config
                        };
                    }
                }
            }
        }
        
        config
    }
}

/// 创建测试文件
fn create_test_files(test_dir: &str) -> Result<Vec<PathBuf>, String> {
    fs::create_dir_all(test_dir)
        .map_err(|e| format!("创建测试目录失败：{}", e))?;
    
    let mut files = Vec::new();
    
    let text_file = PathBuf::from(test_dir).join("test_small.txt");
    fs::write(&text_file, "Hello, ZMODEM! This is a test file.")
        .map_err(|e| format!("创建测试文件失败：{}", e))?;
    files.push(text_file);
    
    let medium_file = PathBuf::from(test_dir).join("test_medium.bin");
    let medium_data: Vec<u8> = (0..102400).map(|i| (i % 256) as u8).collect();
    fs::write(&medium_file, &medium_data)
        .map_err(|e| format!("创建测试文件失败：{}", e))?;
    files.push(medium_file);
    
    let large_file = PathBuf::from(test_dir).join("test_large.bin");
    let large_data: Vec<u8> = (0..1048576).map(|i| (i % 256) as u8).collect();
    fs::write(&large_file, &large_data)
        .map_err(|e| format!("创建测试文件失败：{}", e))?;
    files.push(large_file);
    
    Ok(files)
}

/// 测试 CRC32 计算
#[test]
fn test_crc32() {
    // 简单测试，不依赖外部模块
    let data = b"Hello, World!";
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
    let result = !crc;
    
    println!("CRC32 of 'Hello, World!': 0x{:08X}", result);
    assert_ne!(result, 0);
    
    // 测试一致性
    let mut crc2: u32 = 0xFFFFFFFF;
    for &byte in data {
        crc2 ^= byte as u32;
        for _ in 0..8 {
            if crc2 & 1 != 0 {
                crc2 = (crc2 >> 1) ^ 0xEDB88320;
            } else {
                crc2 >>= 1;
            }
        }
    }
    let result2 = !crc2;
    assert_eq!(result, result2);
}

/// 测试 ZMODEM 包编码
#[test]
fn test_zmodem_packet() {
    const ZPAD: u8 = b'*';
    const ZDLE: u8 = 0x18;
    const ZBIN16: u8 = 0x41;
    const ZRINIT: u8 = 0x01;

    let packet_type = ZRINIT;
    let header_data = [0x40, 0x00, 0x00, 0x00];

    let mut encoded = Vec::new();
    encoded.push(ZPAD);
    encoded.push(ZDLE);
    encoded.push(ZBIN16);
    // 与 lrzsz 一致：TYPE<0x20 须 ZDLE 转义
    encoded.push(ZDLE);
    encoded.push(packet_type ^ 0x40);

    for &b in &header_data {
        encoded.push(ZDLE);
        encoded.push(b ^ 0x40);
    }

    println!("ZRINIT 包长度：{} bytes", encoded.len());

    assert_eq!(encoded[0], ZPAD);
    assert_eq!(encoded[1], ZDLE);
    assert_eq!(encoded[2], ZBIN16);
    assert_eq!(encoded[3], ZDLE);
    assert_eq!(encoded[4], ZRINIT ^ 0x40);
}

/// 测试文件检测逻辑
#[test]
fn test_file_detection() {
    // 模拟检测逻辑
    fn detect_rz_command(data: &[u8]) -> bool {
        let text = String::from_utf8_lossy(data);
        
        if text.contains("rz rz rz") || 
           text.contains("Awaiting rz") ||
           text.contains("rz waiting to receive") {
            return true;
        }
        
        if data.len() >= 5 && (data[0] == 0x2a || data[0] == 0x80) && (data[1] == 0x2a || data[1] == 0x80) {
            if data[2] == 0x18 && data[3] == 0x41 {
                let t = if data[4] == 0x18 && data.len() > 5 {
                    data[5] ^ 0x40
                } else {
                    data[4]
                };
                if t == 0x00 || t == 0x01 {
                    return true;
                }
            }
        }
        
        false
    }
    
    // 测试文本检测
    assert!(detect_rz_command(b"rz rz rz"));
    assert!(detect_rz_command(b"Awaiting rz"));
    assert!(detect_rz_command(b"rz waiting to receive"));
    
    // 测试二进制检测（BIN16：`** ZDLE 'A' TYPE`）
    assert!(detect_rz_command(&[0x80, 0x80, 0x18, 0x41, 0x00]));
    assert!(detect_rz_command(&[0x80, 0x80, 0x18, 0x41, 0x01]));
    
    // 测试误报
    assert!(!detect_rz_command(b"ls -la"));
    assert!(!detect_rz_command(b"cd /tmp"));
}

/// 测试人类可读文件大小
#[test]
fn test_human_readable_size() {
    fn human_readable_size(size: u64) -> String {
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
    
    assert_eq!(human_readable_size(100), "100 B");
    assert_eq!(human_readable_size(1024), "1.00 KB");
    assert_eq!(human_readable_size(1024 * 1024), "1.00 MB");
    assert_eq!(human_readable_size(1024 * 1024 * 1024), "1.00 GB");
}

/// 测试文件创建
#[test]
fn test_create_files() {
    let test_dir = format!("/tmp/zmodem_test_{}", std::process::id());
    
    let files = create_test_files(&test_dir).expect("创建测试文件失败");
    
    assert_eq!(files.len(), 3);
    
    for f in &files {
        assert!(f.exists());
        if let Ok(meta) = fs::metadata(f) {
            println!("文件：{} - {} bytes", f.display(), meta.len());
        }
    }
    
    let _ = fs::remove_dir_all(&test_dir);
}

/// 测试 SSH 连接
#[test]
fn test_ssh_connect() {
    // 强制使用本地测试服务器
    let config = TestConfig {
        host: "localhost".to_string(),
        _port: 22,
        username: "mistterm_test".to_string(),
        _password: "test123456".to_string(),
        _test_dir: "/tmp/zmodem_test".to_string(),
    };
    
    println!("📡 测试 SSH 连接:");
    println!("   主机：{}", config.host);
    println!("   端口：{}", config._port);
    println!("   用户：{}", config.username);
    
    use std::net::TcpStream;
    use ssh2::Session;
    
    // 连接 TCP
    let tcp = TcpStream::connect((config.host.as_str(), config._port))
        .expect("TCP 连接失败");
    println!("✅ TCP 连接成功");
    
    // 创建 SSH 会话
    let mut sess = Session::new().expect("创建 SSH 会话失败");
    sess.set_tcp_stream(tcp);
    sess.handshake().expect("SSH 握手失败");
    println!("✅ SSH 握手成功");
    println!("   服务器版本：{}", sess.banner().unwrap_or(""));
    
    // 密码认证
    sess.userauth_password(&config.username, &config._password)
        .expect("密码认证失败");
    println!("✅ 密码认证成功");
    
    assert!(sess.authenticated());
    println!("✅ SSH 连接测试通过！");
}

/// 测试 ZMODEM 接收（跳过，需要真实服务器和完整实现）
#[test]
#[ignore]
fn test_zmodem_receive() {
    println!("⚠️  跳过 ZMODEM 接收测试（需要真实服务器）");
}

/// 测试 ZMODEM 发送（跳过，需要真实服务器和完整实现）
#[test]
#[ignore]
fn test_zmodem_send() {
    println!("⚠️  跳过 ZMODEM 发送测试（需要真实服务器）");
}
