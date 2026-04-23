//! SSH 模块测试程序
//! 
//! 用法：cargo run --example test_ssh

use mistterm::ssh::{SshConfig, SshSession, SshMessage, SshManager};
use std::io::{Read, Write};
use std::thread;
use std::time::Duration;

fn main() {
    println!("=== SSH Module Test ===\n");
    
    // 创建 SSH 配置
    let config = SshConfig {
        host: "124.220.224.223".to_string(),
        port: 22,
        username: "ubuntu".to_string(),
        password: std::env::var("SSH_PASSWORD").expect("Please set SSH_PASSWORD environment variable"),
    };
    
    println!("Connecting to {}@{}:{}...", config.username, config.host, config.port);
    
    // 创建 SSH 会话
    let mut session = SshSession::new(config);
    
    match session.connect() {
        Ok(_) => println!("✓ SSH connection successful!\n"),
        Err(e) => {
            println!("✗ SSH connection failed: {}", e);
            return;
        }
    }
    
    // 打开通道
    println!("Opening shell channel...");
    let mut channel = match session.open_channel() {
        Ok(channel) => {
            println!("✓ Channel opened\n");
            channel
        }
        Err(e) => {
            println!("✗ Failed to open channel: {}", e);
            return;
        }
    };
    
    // 读取线程
    let (read_tx, read_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let mut channel_reader = channel.try_clone().expect("Failed to clone channel");
    
    thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        loop {
            match channel_reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    let data = buffer[..n].to_vec();
                    let _ = read_tx.send(data);
                }
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::WouldBlock {
                        thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                    break;
                }
            }
        }
    });
    
    // 等待初始输出
    thread::sleep(Duration::from_millis(500));
    
    // 读取并打印初始输出
    println!("Initial output:");
    println!("---");
    while let Ok(data) = read_rx.try_recv() {
        print!("{}", String::from_utf8_lossy(&data));
    }
    println!("\n---\n");
    
    // 发送命令
    let commands = vec!["ls\n", "pwd\n", "whoami\n", "date\n"];
    
    for cmd in commands {
        println!("Sending command: {}", cmd.trim());
        channel.write_all(cmd.as_bytes()).expect("Failed to write");
        channel.flush().expect("Failed to flush");
        
        // 等待并读取输出
        thread::sleep(Duration::from_millis(300));
        
        print!("Output: ");
        while let Ok(data) = read_rx.try_recv() {
            print!("{}", String::from_utf8_lossy(&data));
        }
        println!("\n");
    }
    
    println!("=== Test Complete ===");
}
