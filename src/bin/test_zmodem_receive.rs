//! ZMODEM 接收测试程序 - 独立测试 rz 功能
//! 
//! 使用方法:
//! ```bash
//! cargo run --bin test_zmodem_receive
//! ```

use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use ssh2::Session;

// 导入 lrzsz 模块的代码（复制过来以便独立测试）
mod zmodem_impl;

fn main() {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_target(true)
        .init();
    
    println!("🚀 开始 ZMODEM 接收测试\n");
    
    // 连接 SSH
    let session = match connect_ssh() {
        Ok(s) => s,
        Err(e) => {
            println!("❌ SSH 连接失败：{}", e);
            return;
        }
    };
    println!("✅ SSH 连接成功\n");
    
    // 创建测试文件（在服务器上）
    let test_file = "/tmp/test_upload.csv";
    let test_content = "id,name,value,timestamp\n1,test1,100,2026-04-29\n2,test2,200,2026-04-29\n3,test3,300,2026-04-29\n";
    
    println!("📝 创建测试文件...");
    let mut channel = session.channel_session().unwrap();
    channel.exec(&format!("cat > {} && echo '{}'", test_file, test_content)).unwrap();
    let mut data = test_content.as_bytes();
    channel.write_all(&test_content.as_bytes()).unwrap();
    channel.send_eof().unwrap();
    let _ = channel.wait_close();
    println!("✅ 测试文件已创建 ({} bytes)", test_content.len());
    
    // 现在测试 rz 接收
    println!("\n📥 测试 ZMODEM 接收...");
    println!("   请等待服务器发送文件...");
    
    // 模拟 rz 接收流程
    let download_dir = "/tmp/zmodem_download_test";
    let _ = fs::create_dir_all(download_dir);
    
    let is_active = Arc::new(AtomicBool::new(true));
    let received_bytes = Arc::new(AtomicU64::new(0));
    let (tx, rx): (Sender<String>, Receiver<String>) = std::sync::mpsc::channel();
    
    // 获取 SSH 通道
    let mut chan = session.channel_session().unwrap();
    
    // 1. 等待 ZRQINIT
    println!("   等待 ZRQINIT...");
    let mut buffer = [0u8; 8192];
    let mut zrinit_sent = false;
    
    for i in 0..100 {
        match chan.read(&mut buffer) {
            Ok(0) => {
                println!("   等待中... ({}s)", i);
                thread::sleep(Duration::from_millis(100));
                continue;
            }
            Ok(n) => {
                println!("   收到 {} bytes", n);
                
                // 检查是否是 ZRQINIT
                if !zrinit_sent && zmodem_impl::is_zrqinit(&buffer[..n]) {
                    println!("   ✅ 收到 ZRQINIT，发送 ZRINIT");
                    
                    // 发送 ZRINIT
                    let zrinit = zmodem_impl::encode_zrinit();
                    chan.write_all(&zrinit).unwrap();
                    chan.flush().unwrap();
                    zrinit_sent = true;
                    continue;
                }
                
                // 检查是否是 ZFILE
                if zrinit_sent {
                    if let Some(filename) = zmodem_impl::parse_zfile(&buffer[..n]) {
                        println!("   ✅ 收到 ZFILE: {}", filename);
                        
                        // 发送 ZACK
                        let zack = zmodem_impl::encode_zack(0);
                        chan.write_all(&zack).unwrap();
                        chan.flush().unwrap();
                        
                        // 开始接收数据
                        println!("   开始接收文件数据...");
                        let mut file = fs::File::create(format!("{}/{}", download_dir, filename)).unwrap();
                        let mut total_received = 0u64;
                        
                        loop {
                            match chan.read(&mut buffer) {
                                Ok(0) => break,
                                Ok(n) => {
                                    if let Some(data) = zmodem_impl::parse_zdata(&buffer[..n]) {
                                        file.write_all(&data).unwrap();
                                        total_received += data.len() as u64;
                                        println!("   已接收：{} bytes", total_received);
                                        
                                        // 发送 ZACK
                                        let zack = zmodem_impl::encode_zack(total_received);
                                        chan.write_all(&zack).unwrap();
                                        chan.flush().unwrap();
                                    }
                                }
                                Err(_) => {
                                    thread::sleep(Duration::from_millis(10));
                                }
                            }
                        }
                        
                        println!("   ✅ 文件接收完成：{} bytes", total_received);
                        break;
                    }
                }
            }
            Err(_) => {
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
    
    // 验证文件
    println!("\n🔍 验证文件...");
    let downloaded_path = format!("{}/test_upload.csv", download_dir);
    if let Ok(downloaded) = fs::read_to_string(&downloaded_path) {
        if downloaded == test_content {
            println!("   ✅ 文件内容完全一致！");
        } else {
            println!("   ❌ 文件内容不一致");
            println!("   原始：{} bytes", test_content.len());
            println!("   下载：{} bytes", downloaded.len());
        }
    } else {
        println!("   ❌ 无法读取下载的文件");
    }
    
    // 清理
    let _ = session.channel_session().and_then(|mut c| {
        c.exec("rm -f /tmp/test_upload.csv").ok();
        Ok(())
    });
    let _ = fs::remove_dir_all(download_dir);
    
    println!("\n🎉 测试完成!");
}

fn connect_ssh() -> Result<Session, String> {
    let mut session = Session::new().map_err(|e| format!("创建 SSH 会话失败：{}", e))?;
    
    let tcp = std::net::TcpStream::connect("124.220.224.223:22")
        .map_err(|e| format!("连接失败：{}", e))?;
    session.set_tcp_stream(tcp);
    session.handshake().map_err(|e| format!("SSH 握手失败：{}", e))?;
    
    let home = dirs::home_dir().ok_or("无法获取家目录")?;
    let key_path = home.join(".ssh/id_rsa");
    
    if key_path.exists() {
        if session.userauth_pubkey_file("ubuntu", None, &key_path, None).is_ok() {
            return Ok(session);
        }
    }
    
    Err("密钥认证失败".to_string())
}
