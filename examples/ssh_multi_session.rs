//! SSH 多会话并发测试
//! 
//! 测试同时管理多个 SSH 会话的能力
//! 
//! 用法：SSH_HOST=127.0.0.1 SSH_USER=mistterm_test SSH_PASSWORD=… cargo run --example ssh_multi_session

use std::io::Read;
use std::net::TcpStream;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use ssh2::Session;

/// SSH 会话包装
struct SshSession {
    session: Session,
    _session_id: usize,
}

impl SshSession {
    fn new(session: Session, session_id: usize) -> Self {
        Self {
            session,
            _session_id: session_id,
        }
    }

    fn execute(&mut self, cmd: &str) -> Result<String, String> {
        let mut channel = self.session.channel_session()
            .map_err(|e| format!("Channel failed: {}", e))?;
        
        channel.exec(cmd)
            .map_err(|e| format!("Exec failed: {}", e))?;
        
        let mut output = String::new();
        channel.read_to_string(&mut output)
            .map_err(|e| format!("Read failed: {}", e))?;
        
        Ok(output)
    }
}

/// SSH 连接配置
fn create_connection(session_id: usize) -> Result<Session, String> {
    let host = std::env::var("SSH_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("SSH_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(22);
    let username = std::env::var("SSH_USER").unwrap_or_else(|_| "mistterm_test".into());
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into());
    let private_key_path = Path::new(&home).join(".ssh/id_rsa");
    
    println!("[Session {}] Connecting to {}@{}:{}...", session_id, username, host, port);
    
    let addr = format!("{}:{}", host, port);
    let stream = TcpStream::connect(&addr)
        .map_err(|e| format!("TCP failed: {}", e))?;
    
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
    
    let mut session = Session::new()
        .map_err(|e| format!("Session create failed: {}", e))?;
    
    session.set_tcp_stream(stream);
    session.handshake()
        .map_err(|e| format!("Handshake failed: {}", e))?;
    
    // 尝试密钥认证
    if session
        .userauth_pubkey_file(&username, None, &private_key_path, None)
        .is_ok()
    {
        println!("[Session {}] Auth with SSH key", session_id);
    } else {
        // 密码认证
        println!("[Session {}] Auth with password", session_id);
        let password = std::env::var("SSH_PASSWORD").map_err(|_| {
            "Key auth failed; set SSH_PASSWORD for password auth".to_string()
        })?;
        session.userauth_password(&username, &password)
            .map_err(|e| format!("Auth failed: {}", e))?;
    }
    
    println!("[Session {}] Connected successfully", session_id);
    Ok(session)
}

fn main() {
    println!("=== SSH Multi-Session Concurrent Test ===\n");
    println!("Testing {} concurrent SSH sessions...\n", 10);
    
    let num_sessions = 10;
    let results = Arc::new(Mutex::new(Vec::new()));
    let mut handles = vec![];
    
    // 创建多个并发会话
    for i in 0..num_sessions {
        let results_clone = Arc::clone(&results);
        
        let handle = thread::spawn(move || {
            let session_id = i;
            
            // 每个会话执行不同的命令
            let test_commands = vec![
                format!("echo 'Session {} Hello'", session_id),
                format!("pwd"),
                format!("whoami"),
                format!("hostname"),
                format!("date +'%H:%M:%S'"),
            ];
            
            let mut session_result = vec![];
            
            for cmd in test_commands {
                match create_connection(session_id) {
                    Ok(session) => {
                        let mut ssh_session = SshSession::new(session, session_id);
                        
                        match ssh_session.execute(&cmd) {
                            Ok(output) => {
                                session_result.push((cmd.clone(), output.trim().to_string()));
                                println!("[Session {}] ✓ Command: {} -> Output: {}", 
                                    session_id, 
                                    cmd, 
                                    output.trim());
                            }
                            Err(e) => {
                                println!("[Session {}] ✗ Command failed: {} -> Error: {}", 
                                    session_id, cmd, e);
                                session_result.push((cmd.clone(), format!("ERROR: {}", e)));
                            }
                        }
                    }
                    Err(e) => {
                        println!("[Session {}] ✗ Connection failed: {}", session_id, e);
                        session_result.push(("CONNECTION".to_string(), format!("ERROR: {}", e)));
                    }
                }
                
                // 稍微延迟，避免同时连接
                thread::sleep(Duration::from_millis(100));
            }
            
            // 保存结果
            let mut results = results_clone.lock().unwrap();
            results.push((session_id, session_result));
        });
        
        handles.push(handle);
        
        // 稍微延迟创建下一个会话
        thread::sleep(Duration::from_millis(50));
    }
    
    // 等待所有会话完成
    println!("\n=== Waiting for all sessions to complete ===\n");
    for handle in handles {
        handle.join().ok();
    }
    
    // 汇总结果
    println!("\n=== Test Results Summary ===\n");
    
    let results = results.lock().unwrap();
    let mut success_count = 0;
    let mut fail_count = 0;
    
    for (session_id, session_results) in results.iter() {
        println!("[Session {}] Results:", session_id);
        for (cmd, output) in session_results {
            if output.starts_with("ERROR") {
                println!("  ✗ {} -> {}", cmd, output);
                fail_count += 1;
            } else {
                println!("  ✓ {} -> {}", cmd, output);
                success_count += 1;
            }
        }
        println!();
    }
    
    println!("=== Summary ===");
    println!("Total commands executed: {}", success_count + fail_count);
    println!("Successful: {}", success_count);
    println!("Failed: {}", fail_count);
    
    if fail_count == 0 {
        println!("\n🎉 All {} sessions completed successfully!", num_sessions);
    } else {
        println!("\n⚠️ {} sessions had errors", fail_count);
    }
}
