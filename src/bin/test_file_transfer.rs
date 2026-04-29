//! 文件传输测试程序
//! 
//! 测试 SSH 通道直接文件传输（无需 lrzsz）
//! 
//! 使用方法:
//! ```bash
//! cargo run --bin test_file_transfer
//! ```

use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use ssh2::Session;

fn main() {
    println!("🚀 开始文件传输测试（SSH 通道方案）\n");
    
    // 服务器配置
    let host = "124.220.224.223";
    let port = 22;
    let username = "ubuntu";
    
    println!("📡 连接服务器:");
    println!("   主机：{}:{}", host, port);
    println!("   用户：{}", username);
    
    // 连接 SSH
    let session = match connect_ssh(host, port, username) {
        Ok(s) => s,
        Err(e) => {
            println!("❌ SSH 连接失败：{}", e);
            return;
        }
    };
    println!("✅ SSH 连接成功\n");
    
    // 创建测试目录
    let test_dir = "/tmp/ft_test_local";
    let remote_dir = format!("/tmp/ft_test_remote_{}", std::process::id());
    
    println!("📝 准备测试环境...");
    let _ = fs::create_dir_all(test_dir);
    execute_command(&session, &format!("mkdir -p {}", remote_dir)).ok();
    
    // 创建测试文件
    create_test_files(test_dir);
    println!("✅ 测试文件创建完成\n");
    
    // 测试 1: 上传单个文件
    println!("📤 测试 1: 上传单个文件");
    match upload_file(&session, &format!("{}/medium.bin", test_dir), &format!("{}/medium.bin", remote_dir)) {
        Ok(_) => println!("   ✅ 上传成功\n"),
        Err(e) => println!("   ❌ 上传失败：{}\n", e),
    }
    
    // 测试 2: 下载文件
    println!("📥 测试 2: 下载文件");
    let download_path = format!("{}/downloaded.bin", test_dir);
    match download_file(&session, &format!("{}/medium.bin", remote_dir), &download_path) {
        Ok(_) => println!("   ✅ 下载成功\n"),
        Err(e) => println!("   ❌ 下载失败：{}\n", e),
    }
    
    // 测试 3: 验证文件
    println!("🔍 测试 3: 文件验证");
    let original = fs::read(format!("{}/medium.bin", test_dir)).unwrap();
    let downloaded = fs::read(&download_path).unwrap();
    
    if original == downloaded {
        println!("   ✅ 文件内容完全一致");
    } else {
        println!("   ❌ 文件内容不一致");
    }
    
    // 测试 4: 上传多个文件
    println!("\n📤 测试 4: 批量上传");
    let files = vec![
        (format!("{}/hello.txt", test_dir), format!("{}/hello.txt", remote_dir)),
        (format!("{}/medium.bin", test_dir), format!("{}/medium2.bin", remote_dir)),
    ];
    
    let mut all_success = true;
    for (local, remote) in files {
        match upload_file(&session, &local, &remote) {
            Ok(_) => println!("   ✅ {} -> {}", local, remote),
            Err(e) => {
                println!("   ❌ {} -> {}: {}", local, remote, e);
                all_success = false;
            }
        }
    }
    if all_success {
        println!("   ✅ 批量上传全部成功");
    }
    
    // 测试 5: 检查文件
    println!("\n🔍 测试 5: 文件检查");
    let remote_file = format!("{}/hello.txt", remote_dir);
    if file_exists(&session, &remote_file) {
        println!("   ✅ 文件存在：{}", remote_file);
        
        if let Ok(size) = get_file_size(&session, &remote_file) {
            println!("   📏 文件大小：{} bytes", size);
        }
    } else {
        println!("   ❌ 文件不存在：{}", remote_file);
    }
    
    // 清理
    println!("\n🧹 清理测试文件...");
    execute_command(&session, &format!("rm -rf {}", remote_dir)).ok();
    let _ = fs::remove_dir_all(test_dir);
    println!("✅ 清理完成\n");
    
    println!("🎉 所有测试完成!");
}

fn connect_ssh(host: &str, port: u16, username: &str) -> Result<Session, String> {
    let mut session = Session::new().map_err(|e| format!("创建 SSH 会话失败：{}", e))?;
    
    let tcp = std::net::TcpStream::connect(format!("{}:{}", host, port))
        .map_err(|e| format!("连接失败：{}", e))?;
    session.set_tcp_stream(tcp);
    session.handshake().map_err(|e| format!("SSH 握手失败：{}", e))?;
    
    // 尝试密钥认证
    let home = dirs::home_dir().ok_or("无法获取家目录")?;
    let key_path = home.join(".ssh/id_rsa");
    
    if key_path.exists() {
        if session.userauth_pubkey_file(username, None, &key_path, None).is_ok() {
            return Ok(session);
        }
    }
    
    Err("密钥认证失败".to_string())
}

fn execute_command(session: &Session, command: &str) -> Result<String, String> {
    let mut channel = session.channel_session().map_err(|e| format!("创建通道失败：{}", e))?;
    channel.exec(command).map_err(|e| format!("执行命令失败：{}", e))?;
    
    let mut output = Vec::new();
    channel.read_to_end(&mut output).map_err(|e| format!("读取失败：{}", e))?;
    let _ = channel.wait_close();
    
    String::from_utf8(output).map_err(|e| format!("UTF-8 错误：{}", e))
}

fn upload_file(session: &Session, local_path: &str, remote_path: &str) -> Result<(), String> {
    let data = fs::read(local_path).map_err(|e| format!("读取文件失败：{}", e))?;
    let total_size = data.len() as u64;
    
    println!("   上传：{} ({} bytes)", local_path, total_size);
    
    let mut channel = session.channel_session().map_err(|e| format!("创建通道失败：{}", e))?;
    let exec_cmd = format!("cat > {}", remote_path);
    channel.exec(&exec_cmd).map_err(|e| format!("执行命令失败：{}", e))?;
    
    // 分块写入
    let chunk_size = 8192;
    let mut written = 0u64;
    
    for chunk in data.chunks(chunk_size) {
        channel.write_all(chunk).map_err(|e| format!("写入失败：{}", e))?;
        written += chunk.len() as u64;
        
        let percent = (written as f64 / total_size as f64 * 100.0) as u32;
        print!("\r   进度：{}/{} ({:.1}%)", written, total_size, percent as f64);
        let _ = std::io::stdout().flush();
    }
    
    println!("\n   发送 EOF...");
    channel.send_eof().map_err(|e| format!("发送 EOF 失败：{}", e))?;
    
    // 读取可能的错误输出
    let mut err_buf = [0u8; 1024];
    let _ = channel.stderr().read(&mut err_buf);
    
    // 等待关闭（忽略状态错误）
    let _ = channel.wait_close();
    
    // 验证
    let verify_cmd = format!("test -f {} && wc -c < {}", remote_path, remote_path);
    let output = execute_command(session, &verify_cmd)?;
    let remote_size: u64 = output.trim().parse().map_err(|e| format!("验证失败：{}", e))?;
    
    if remote_size == total_size {
        println!("   ✅ 上传成功：{} bytes", remote_size);
        Ok(())
    } else {
        Err(format!("验证失败：本地 {} vs 远程 {}", total_size, remote_size))
    }
}

fn download_file(session: &Session, remote_path: &str, local_path: &str) -> Result<(), String> {
    let size_cmd = format!("wc -c < {}", remote_path);
    let size_output = execute_command(session, &size_cmd)?;
    let total_size: u64 = size_output.trim().parse().map_err(|e| format!("获取大小失败：{}", e))?;
    
    println!("   下载：{} ({} bytes)", remote_path, total_size);
    
    let mut channel = session.channel_session().map_err(|e| format!("创建通道失败：{}", e))?;
    let exec_cmd = format!("cat {}", remote_path);
    channel.exec(&exec_cmd).map_err(|e| format!("执行命令失败：{}", e))?;
    
    let mut file = fs::File::create(local_path).map_err(|e| format!("创建文件失败：{}", e))?;
    let mut buffer = [0u8; 8192];
    let mut downloaded = 0u64;
    
    loop {
        match channel.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                file.write_all(&buffer[..n]).map_err(|e| format!("写入失败：{}", e))?;
                downloaded += n as u64;
                
                let percent = (downloaded as f64 / total_size as f64 * 100.0) as u32;
                print!("\r   进度：{}/{} ({:.1}%)", downloaded, total_size, percent as f64);
                let _ = std::io::stdout().flush();
            }
            Err(e) => return Err(format!("读取失败：{}", e)),
        }
    }
    
    println!("\n   ✅ 下载成功：{} bytes", downloaded);
    Ok(())
}

fn file_exists(session: &Session, path: &str) -> bool {
    execute_command(session, &format!("test -f {}", path)).is_ok()
}

fn get_file_size(session: &Session, path: &str) -> Result<u64, String> {
    let cmd = format!("wc -c < {}", path);
    let output = execute_command(session, &cmd)?;
    output.trim().parse().map_err(|e| format!("解析失败：{}", e))
}

fn create_test_files(test_dir: &str) {
    let text_file = PathBuf::from(test_dir).join("hello.txt");
    let text_content = "Hello from SSH file transfer!\nThis is a test file.\n";
    fs::write(&text_file, text_content).unwrap();
    
    let medium_file = PathBuf::from(test_dir).join("medium.bin");
    let medium_data: Vec<u8> = (0..102400).map(|i| (i % 256) as u8).collect();
    fs::write(&medium_file, &medium_data).unwrap();
    
    println!("   ✅ hello.txt - {} bytes", text_content.len());
    println!("   ✅ medium.bin - {} bytes", medium_data.len());
}
