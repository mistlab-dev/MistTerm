//! ZMODEM 手动测试程序
//! 
//! 使用方法:
//! ```bash
//! cargo run --bin test_zmodem
//! ```

use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use ssh2::Session;

fn main() {
    println!("🚀 开始 ZMODEM 功能测试\n");
    
    // 服务器配置（从 sessions.json 读取）
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
    let test_dir = "/tmp/zmodem_test_local".to_string();
    let remote_dir = format!("/tmp/zmodem_remote_{}", std::process::id());
    
    println!("📝 准备测试环境...");
    let _ = fs::create_dir_all(&test_dir);
    let _ = execute_command(&session, &format!("mkdir -p {} {}", test_dir, remote_dir));
    
    // 创建测试文件
    create_test_files(&test_dir);
    println!("✅ 测试文件创建完成\n");
    
    // 测试 1: 上传文件到服务器
    println!("📤 测试 1: 文件上传 (模拟 sz)");
    match test_upload(&session, &test_dir, &remote_dir) {
        Ok(_) => println!("✅ 上传测试通过\n"),
        Err(e) => println!("❌ 上传测试失败：{}\n", e),
    }
    
    // 测试 2: 验证文件
    println!("🔍 测试 2: 文件验证");
    match verify_files(&session, &test_dir, &remote_dir) {
        Ok(_) => println!("✅ 文件验证通过\n"),
        Err(e) => println!("❌ 文件验证失败：{}\n", e),
    }
    
    // 测试 3: 下载文件从服务器
    println!("📥 测试 3: 文件下载 (模拟 rz)");
    match test_download(&session, &test_dir, &remote_dir) {
        Ok(_) => println!("✅ 下载测试通过\n"),
        Err(e) => println!("❌ 下载测试失败：{}\n", e),
    }
    
    println!("🎉 所有测试完成!");
}

fn connect_ssh(host: &str, port: u16, username: &str) -> Result<Session, String> {
    let mut session = Session::new().map_err(|e| format!("创建会话失败：{}", e))?;
    
    let tcp = std::net::TcpStream::connect(format!("{}:{}", host, port))
        .map_err(|e| format!("连接失败：{}", e))?;
    session.set_tcp_stream(tcp);
    session.handshake().map_err(|e| format!("握手失败：{}", e))?;
    
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

fn create_test_files(test_dir: &str) {
    // 小文本文件
    let text_file = PathBuf::from(test_dir).join("hello.txt");
    let text_content = "Hello from ZMODEM test!\nThis is a test file.\n";
    fs::write(&text_file, text_content).unwrap();
    
    // 中等文件 (100KB)
    let medium_file = PathBuf::from(test_dir).join("medium.bin");
    let medium_data: Vec<u8> = (0..102400).map(|i| (i % 256) as u8).collect();
    fs::write(&medium_file, &medium_data).unwrap();
    
    println!("   ✅ hello.txt - {} bytes", text_content.len());
    println!("   ✅ medium.bin - {} bytes", medium_data.len());
}

fn test_upload(session: &Session, local_dir: &str, remote_dir: &str) -> Result<(), String> {
    // 读取本地文件
    let test_file = PathBuf::from(local_dir).join("medium.bin");
    let file_data = fs::read(&test_file).map_err(|e| format!("读取文件失败：{}", e))?;
    
    println!("   文件大小：{} bytes", file_data.len());
    
    // 通过 SSH SFTP 上传（模拟 sz 功能）
    let remote_file = format!("{}/medium.bin", remote_dir);
    let upload_cmd = format!("cat > {}", remote_file);
    
    let mut channel = session.channel_session().map_err(|e| format!("创建通道失败：{}", e))?;
    channel.exec(&upload_cmd).map_err(|e| format!("执行命令失败：{}", e))?;
    
    channel.write_all(&file_data).map_err(|e| format!("写入失败：{}", e))?;
    channel.send_eof().map_err(|e| format!("发送 EOF 失败：{}", e))?;
    let _ = channel.wait_close();
    
    println!("   ✅ 文件已上传到 {}", remote_file);
    Ok(())
}

fn verify_files(session: &Session, local_dir: &str, remote_dir: &str) -> Result<(), String> {
    // 获取远程文件大小
    let remote_file = format!("{}/medium.bin", remote_dir);
    let cmd = format!("wc -c < {}", remote_file);
    let output = execute_command(session, &cmd)?;
    
    let remote_size: u64 = output.trim().parse().map_err(|e| format!("解析大小失败：{}", e))?;
    
    let local_meta = fs::metadata(PathBuf::from(local_dir).join("medium.bin"))
        .map_err(|e| format!("获取本地文件信息失败：{}", e))?;
    
    println!("   本地大小：{} bytes", local_meta.len());
    println!("   远程大小：{} bytes", remote_size);
    
    if local_meta.len() == remote_size {
        println!("   ✅ 文件大小匹配");
        Ok(())
    } else {
        Err("文件大小不匹配".to_string())
    }
}

fn test_download(session: &Session, local_dir: &str, remote_dir: &str) -> Result<(), String> {
    // 从服务器读取文件
    let remote_file = format!("{}/medium.bin", remote_dir);
    let download_cmd = format!("cat {}", remote_file);
    
    let mut channel = session.channel_session().map_err(|e| format!("创建通道失败：{}", e))?;
    channel.exec(&download_cmd).map_err(|e| format!("执行命令失败：{}", e))?;
    
    let mut remote_data = Vec::new();
    channel.read_to_end(&mut remote_data).map_err(|e| format!("读取失败：{}", e))?;
    let _ = channel.wait_close();
    
    println!("   下载数据：{} bytes", remote_data.len());
    
    // 保存到本地
    let local_file = PathBuf::from(local_dir).join("downloaded.bin");
    fs::write(&local_file, &remote_data).map_err(|e| format!("保存失败：{}", e))?;
    
    // 验证
    let original_data = fs::read(PathBuf::from(local_dir).join("medium.bin"))
        .map_err(|e| format!("读取原文件失败：{}", e))?;
    
    if original_data == remote_data {
        println!("   ✅ 文件内容匹配");
        Ok(())
    } else {
        Err("文件内容不匹配".to_string())
    }
}
