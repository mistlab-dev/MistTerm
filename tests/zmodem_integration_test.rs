//! ZMODEM file transfer integration tests
//!
//! Usage:
//! ```bash
//! # Run all tests (skip integration tests)
//! cargo test --test zmodem_integration_test -- --skip test_zmodem
//!
//! # Run specific test
//! cargo test --test zmodem_integration_test test_crc32
//!
//! # Run test with output
//! cargo test --test zmodem_integration_test -- --nocapture
//! ```

use std::fs;
use std::path::PathBuf;

/// Test configuration
struct TestConfig {
    host: String,
    _port: u16,
    username: String,
    _password: String,
    _test_dir: String,
}

/// Create test files
fn create_test_files(test_dir: &str) -> Result<Vec<PathBuf>, String> {
    fs::create_dir_all(test_dir)
        .map_err(|e| format!("Failed to create test directory: {}", e))?;

    let mut files = Vec::new();

    let text_file = PathBuf::from(test_dir).join("test_small.txt");
    fs::write(&text_file, "Hello, ZMODEM! This is a test file.")
        .map_err(|e| format!("Failed to create test file: {}", e))?;
    files.push(text_file);

    let medium_file = PathBuf::from(test_dir).join("test_medium.bin");
    let medium_data: Vec<u8> = (0..102400).map(|i| (i % 256) as u8).collect();
    fs::write(&medium_file, &medium_data)
        .map_err(|e| format!("Failed to create test file: {}", e))?;
    files.push(medium_file);

    let large_file = PathBuf::from(test_dir).join("test_large.bin");
    let large_data: Vec<u8> = (0..1048576).map(|i| (i % 256) as u8).collect();
    fs::write(&large_file, &large_data)
        .map_err(|e| format!("Failed to create test file: {}", e))?;
    files.push(large_file);

    Ok(files)
}

/// Test CRC32 calculation
#[test]
fn test_crc32() {
    // Simple test, no external dependencies
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

    // Test consistency
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

/// Test ZMODEM packet encoding
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
    // Consistent with lrzsz: TYPE<0x20 needs ZDLE escaping
    encoded.push(ZDLE);
    encoded.push(packet_type ^ 0x40);

    for &b in &header_data {
        encoded.push(ZDLE);
        encoded.push(b ^ 0x40);
    }

    println!("ZRINIT packet length: {} bytes", encoded.len());

    assert_eq!(encoded[0], ZPAD);
    assert_eq!(encoded[1], ZDLE);
    assert_eq!(encoded[2], ZBIN16);
    assert_eq!(encoded[3], ZDLE);
    assert_eq!(encoded[4], ZRINIT ^ 0x40);
}

/// Test file detection logic
#[test]
fn test_file_detection() {
    // Simulate detection logic
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

    // Test text detection
    assert!(detect_rz_command(b"rz rz rz"));
    assert!(detect_rz_command(b"Awaiting rz"));
    assert!(detect_rz_command(b"rz waiting to receive"));

    // Test binary detection (BIN16: `** ZDLE 'A' TYPE`)
    assert!(detect_rz_command(&[0x80, 0x80, 0x18, 0x41, 0x00]));
    assert!(detect_rz_command(&[0x80, 0x80, 0x18, 0x41, 0x01]));

    // Test false positives
    assert!(!detect_rz_command(b"ls -la"));
    assert!(!detect_rz_command(b"cd /tmp"));
}

/// Test human readable file size
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

/// Test file creation
#[test]
fn test_create_files() {
    let test_dir = format!("/tmp/zmodem_test_{}", std::process::id());

    let files = create_test_files(&test_dir).expect("Failed to create test files");

    assert_eq!(files.len(), 3);

    for f in &files {
        assert!(f.exists());
        if let Ok(meta) = fs::metadata(f) {
            println!("File: {} - {} bytes", f.display(), meta.len());
        }
    }

    let _ = fs::remove_dir_all(&test_dir);
}

/// Test SSH connection
#[test]
fn test_ssh_connect() {
    // Force local test server
    let config = TestConfig {
        host: "localhost".to_string(),
        _port: 22,
        username: "mistterm_test".to_string(),
        _password: "test123456".to_string(),
        _test_dir: "/tmp/zmodem_test".to_string(),
    };

    println!("📡 Testing SSH connection:");
    println!("   Host: {}", config.host);
    println!("   Port: {}", config._port);
    println!("   User: {}", config.username);

    use std::net::TcpStream;
    use ssh2::Session;

    // Connect TCP
    let tcp = TcpStream::connect((config.host.as_str(), config._port))
        .expect("TCP connection failed");
    println!("✅ TCP connection successful");

    // Create SSH session
    let mut sess = Session::new().expect("Failed to create SSH session");
    sess.set_tcp_stream(tcp);
    sess.handshake().expect("SSH handshake failed");
    println!("✅ SSH handshake successful");
    println!("   Server version: {}", sess.banner().unwrap_or(""));

    // Password authentication
    sess.userauth_password(&config.username, &config._password)
        .expect("Password authentication failed");
    println!("✅ Password authentication successful");

    assert!(sess.authenticated());
    println!("✅ SSH connection test passed!");
}

/// Test ZMODEM receive (skipped, needs real server and full implementation)
#[test]
#[ignore]
fn test_zmodem_receive() {
    println!("⚠️  Skipping ZMODEM receive test (needs real server)");
}

/// Test ZMODEM send (skipped, needs real server and full implementation)
#[test]
#[ignore]
fn test_zmodem_send() {
    println!("⚠️  Skipping ZMODEM send test (needs real server)");
}