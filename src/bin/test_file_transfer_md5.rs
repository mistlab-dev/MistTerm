//! 文件传输 MD5 校验测试
//! 
//! 测试特殊字符、乱码、二进制文件的 MD5 一致性

use std::fs;
use std::io::{Read, Write};
use md5::compute;
use ssh2::Session;

fn main() {
    println!("🚀 开始 MD5 文件传输测试\n");
    
    // 连接 SSH
    let session = match connect_ssh() {
        Ok(s) => s,
        Err(e) => {
            println!("❌ SSH 连接失败：{}", e);
            return;
        }
    };
    println!("✅ SSH 连接成功\n");
    
    // 创建测试目录
    let test_dir = "/tmp/md5_test_local";
    let remote_dir = format!("/tmp/md5_test_remote_{}", std::process::id());
    
    println!("📝 准备测试环境...");
    let _ = fs::create_dir_all(test_dir);
    execute_command(&session, &format!("mkdir -p {}", remote_dir)).ok();
    
    // 创建各种类型的测试文件
    create_special_files(test_dir);
    println!("✅ 测试文件创建完成\n");
    
    // 测试每个文件
    let files = vec![
        ("special_chars.txt", "包含特殊符号和中文"),
        ("binary_data.bin", "二进制数据"),
        ("emoji_test.txt", "Emoji 表情"),
        ("newline_test.txt", "各种换行符"),
        ("null_bytes.bin", "空字节"),
        ("utf8_mixed.txt", "混合编码"),
    ];
    
    let mut passed = 0;
    let mut failed = 0;
    
    for (filename, description) in files {
        println!("📋 测试：{} ({})", filename, description);
        
        let local_path = format!("{}/{}", test_dir, filename);
        let remote_path = format!("{}/{}", remote_dir, filename);
        let download_path = format!("{}/downloaded_{}", test_dir, filename);
        
        // 计算本地 MD5
        let local_md5 = match calculate_md5(&local_path) {
            Some(md5) => md5,
            None => {
                println!("   ❌ 无法计算本地 MD5\n");
                failed += 1;
                continue;
            }
        };
        println!("   本地 MD5: {}", local_md5);
        
        // 上传
        match upload_file(&session, &local_path, &remote_path) {
            Ok(_) => println!("   ✅ 上传成功"),
            Err(e) => {
                println!("   ❌ 上传失败：{}\n", e);
                failed += 1;
                continue;
            }
        }
        
        // 获取远程 MD5
        let remote_md5 = match get_remote_md5(&session, &remote_path) {
            Some(md5) => md5,
            None => {
                println!("   ❌ 无法获取远程 MD5\n");
                failed += 1;
                continue;
            }
        };
        println!("   远程 MD5: {}", remote_md5);
        
        // 比较 MD5
        if local_md5 == remote_md5 {
            println!("   ✅ MD5 一致");
        } else {
            println!("   ❌ MD5 不一致！");
            failed += 1;
            continue;
        }
        
        // 下载
        match download_file(&session, &remote_path, &download_path) {
            Ok(_) => println!("   ✅ 下载成功"),
            Err(e) => {
                println!("   ❌ 下载失败：{}\n", e);
                failed += 1;
                continue;
            }
        }
        
        // 计算下载文件的 MD5
        let download_md5 = match calculate_md5(&download_path) {
            Some(md5) => md5,
            None => {
                println!("   ❌ 无法计算下载文件 MD5\n");
                failed += 1;
                continue;
            }
        };
        println!("   下载 MD5: {}", download_md5);
        
        // 比较下载 MD5
        if local_md5 == download_md5 {
            println!("   ✅ 下载文件 MD5 一致\n");
            passed += 1;
        } else {
            println!("   ❌ 下载文件 MD5 不一致！\n");
            failed += 1;
        }
    }
    
    // 清理
    println!("\n🧹 清理测试文件...");
    execute_command(&session, &format!("rm -rf {}", remote_dir)).ok();
    let _ = fs::remove_dir_all(test_dir);
    println!("✅ 清理完成\n");
    
    // 总结
    println!("📊 测试总结:");
    println!("   ✅ 通过：{}", passed);
    println!("   ❌ 失败：{}", failed);
    println!("   总计：{}", passed + failed);
    
    if failed == 0 {
        println!("\n🎉 所有测试通过！");
    } else {
        println!("\n⚠️  部分测试失败");
    }
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

fn execute_command(session: &Session, command: &str) -> Result<String, String> {
    let mut channel = session.channel_session().map_err(|e| format!("创建通道失败：{}", e))?;
    channel.exec(command).map_err(|e| format!("执行命令失败：{}", e))?;
    
    let mut output = Vec::new();
    channel.read_to_end(&mut output).map_err(|e| format!("读取失败：{}", e))?;
    let _ = channel.wait_close();
    
    String::from_utf8(output).map_err(|e| format!("UTF-8 错误：{}", e))
}

fn calculate_md5(path: &str) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).ok()?;
    
    let result = compute(&buffer);
    Some(format!("{:x}", result))
}

fn get_remote_md5(session: &Session, remote_path: &str) -> Option<String> {
    // 尝试 md5sum
    match execute_command(session, &format!("md5sum {}", remote_path)) {
        Ok(output) => {
            let parts: Vec<&str> = output.trim().split_whitespace().collect();
            if parts.len() >= 1 {
                return Some(parts[0].to_string());
            }
        }
        Err(_) => {}
    }
    
    // 尝试 md5
    match execute_command(session, &format!("md5 -r {}", remote_path)) {
        Ok(output) => {
            let parts: Vec<&str> = output.trim().split_whitespace().collect();
            if parts.len() >= 1 {
                return Some(parts[0].to_string());
            }
        }
        Err(_) => {}
    }
    
    None
}

fn upload_file(session: &Session, local_path: &str, remote_path: &str) -> Result<(), String> {
    let data = fs::read(local_path).map_err(|e| format!("读取文件失败：{}", e))?;
    let total_size = data.len() as u64;
    
    let mut channel = session.channel_session().map_err(|e| format!("创建通道失败：{}", e))?;
    let exec_cmd = format!("cat > {}", remote_path);
    channel.exec(&exec_cmd).map_err(|e| format!("执行命令失败：{}", e))?;
    
    for chunk in data.chunks(8192) {
        channel.write_all(chunk).map_err(|e| format!("写入失败：{}", e))?;
    }
    
    channel.send_eof().ok();
    let _ = channel.wait_close();
    
    println!("   ✅ 上传：{} bytes", total_size);
    Ok(())
}

fn download_file(session: &Session, remote_path: &str, local_path: &str) -> Result<(), String> {
    let mut channel = session.channel_session().map_err(|e| format!("创建通道失败：{}", e))?;
    let exec_cmd = format!("cat {}", remote_path);
    channel.exec(&exec_cmd).map_err(|e| format!("执行命令失败：{}", e))?;
    
    let mut file = fs::File::create(local_path).map_err(|e| format!("创建文件失败：{}", e))?;
    let mut buffer = [0u8; 8192];
    
    loop {
        match channel.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                file.write_all(&buffer[..n]).map_err(|e| format!("写入失败：{}", e))?;
            }
            Err(e) => return Err(format!("读取失败：{}", e)),
        }
    }
    
    Ok(())
}

fn create_special_files(test_dir: &str) {
    // 1. 特殊字符文件
    let special_content = "特殊字符测试：\n\
        中文：你好世界\n\
        日文：こんにちは\n\
        韩文：안녕하세요\n\
        俄文：Привет\n\
        阿拉伯：مرحبا\n\
        希伯来：שלום\n\
        表情：😀😂🎉🚀💻\n\
        特殊符号：@#$%^&*()_+-=[]{}|;':\",./<>?\n\
        控制字符：\t\n\r\n\
        引号：\"double\" and 'single'\n\
        反斜杠：\\backslash\\n\
        美元：$VAR ${HOME}\n\
        反引号：`command`\n\
        管道：| && ||\n\
        重定向：> >> <\n\
        分号：;";
    
    fs::write(format!("{}/special_chars.txt", test_dir), special_content).unwrap();
    println!("   ✅ special_chars.txt");
    
    // 2. 二进制数据（包含所有字节值）
    let mut binary_data: Vec<u8> = (0..256).map(|i| i as u8).collect();
    // 重复多次
    let mut repeated = binary_data.clone();
    for _ in 0..100 {
        repeated.extend_from_slice(&binary_data);
    }
    binary_data = repeated;
    fs::write(format!("{}/binary_data.bin", test_dir), &binary_data).unwrap();
    println!("   ✅ binary_data.bin ({} bytes)", binary_data.len());
    
    // 3. Emoji 测试
    let emoji_content = "😀 😂 🎉 🚀 💻 🔥 ❤️ 👍 ✨ 🌟\n\
        🐛 🐕 🐈 🐉 🦄 🦊 🐼 🐨\n\
        🍎 🍊 🍇 🍉 🍌 🍒 🍓 🥝\n\
        ⚽ 🏀 🏈 ⚾ 🎾 🏐 🎱 🏓\n\
        🚗 🚕 🚙 🚌 🚎 🏎️ 🚓 🚑";
    
    fs::write(format!("{}/emoji_test.txt", test_dir), emoji_content).unwrap();
    println!("   ✅ emoji_test.txt");
    
    // 4. 各种换行符
    let newline_content = "Unix LF\n\
        Windows CRLF\r\n\
        Old Mac CR\r\
        Mixed\n\r\n\r\n\
        End";
    
    fs::write(format!("{}/newline_test.txt", test_dir), newline_content).unwrap();
    println!("   ✅ newline_test.txt");
    
    // 5. 包含空字节
    let mut null_data = Vec::new();
    null_data.extend_from_slice(b"Before null byte");
    null_data.push(0);
    null_data.extend_from_slice(b"After null byte");
    null_data.push(0);
    null_data.push(0);
    null_data.extend_from_slice(b"End of file");
    
    fs::write(format!("{}/null_bytes.bin", test_dir), &null_data).unwrap();
    println!("   ✅ null_bytes.bin");
    
    // 6. UTF-8 混合编码
    let utf8_content = "ASCII: Hello World\n\
        UTF-8 BOM: \u{FEFF}Start with BOM\n\
        Chinese: 简体中文 繁體中文\n\
        Japanese: ひらがな カタカナ\n\
        Korean: 한글\n\
        Thai: สวัสดี\n\
        Greek: Γειά σου\n\
        Hebrew: עברית\n\
        Arabic: العربية\n\
        Emoji: 🇨🇳 🇺🇸 🇯🇵 🇰🇷";
    
    fs::write(format!("{}/utf8_mixed.txt", test_dir), utf8_content).unwrap();
    println!("   ✅ utf8_mixed.txt");
}
