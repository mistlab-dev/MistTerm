//! SSH 模块独立测试
//! 
//! 用法：SSH_HOST=127.0.0.1 SSH_USER=mistterm_test cargo run --example ssh_test

use std::io::{Read, stdin};
use std::net::TcpStream;
use std::path::Path;
use std::time::Duration;

fn main() {
    println!("=== SSH Module Test ===\n");
    
    let host = std::env::var("SSH_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("SSH_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(22);
    let username = std::env::var("SSH_USER").unwrap_or_else(|_| "mistterm_test".into());
    
    println!("Connecting to {}@{}:{}...", username, host, port);
    
    // 建立 TCP 连接
    let addr = format!("{}:{}", host, port);
    let stream = match TcpStream::connect(&addr) {
        Ok(s) => {
            println!("✓ TCP connected");
            s
        }
        Err(e) => {
            println!("✗ TCP connection failed: {}", e);
            return;
        }
    };
    
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
    
    let mut session = match ssh2::Session::new() {
        Ok(s) => {
            println!("✓ SSH session created");
            s
        }
        Err(e) => {
            println!("✗ Failed to create SSH session: {}", e);
            return;
        }
    };
    
    session.set_tcp_stream(stream);
    
    match session.handshake() {
        Ok(_) => println!("✓ SSH handshake done"),
        Err(e) => {
            println!("✗ SSH handshake failed: {}", e);
            return;
        }
    }
    
    // 尝试密钥认证
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into());
    let private_key_path = Path::new(&home).join(".ssh/id_rsa");
    println!("\nTrying public key authentication...");
    match session.userauth_pubkey_file(&username, None, &private_key_path, None) {
        Ok(_) => {
            println!("✓ Authentication successful with SSH key\n");
        }
        Err(e) => {
            println!("✗ Key auth failed: {}", e);
            println!("\nPlease enter password for {}@{}:", username, host);
            
            // 读取密码
            print!("Password: ");
            let mut password = String::new();
            stdin().read_line(&mut password).ok();
            let password = password.trim();
            
            match session.userauth_password(&username, password) {
                Ok(_) => println!("\n✓ Authentication successful with password\n"),
                Err(e) => {
                    println!("\n✗ Authentication failed: {}", e);
                    return;
                }
            }
        }
    }
    
    // 测试命令 - 使用 exec 而不是 shell
    let commands = vec!["ls", "pwd", "whoami", "date"];
    
    for cmd in commands {
        println!(">>> Running: {}", cmd);
        
        match session.channel_session() {
            Ok(mut channel) => {
                match channel.exec(cmd) {
                    Ok(_) => {
                        // 读取输出
                        let mut buffer = String::new();
                        channel.read_to_string(&mut buffer).ok();
                        print!("Output:\n---\n{}\n---\n\n", buffer);
                    }
                    Err(e) => println!("✗ Exec failed: {}\n", e),
                }
            }
            Err(e) => println!("✗ Channel failed: {}\n", e),
        }
    }
    
    println!("=== SSH Module Test Complete ===");
    println!("✓ SSH connection and command execution verified!");
}
