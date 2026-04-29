# 替代文件传输方案调研

## 🎯 问题

服务器没有安装 `lrzsz`，无法使用 `rz`/`sz` 命令：

```bash
$ rz
bash: rz: command not found

$ sz file.txt
bash: sz: command not found
```

## 💡 解决方案

### 方案 1: SSH 通道直接传输（推荐）

**原理**: 利用 SSH 通道直接读写，绕过 PTY 和 lrzsz

**上传 (本地 -> 远程)**:
```bash
# 远程执行：cat > /tmp/file.txt
# 本地通过 SSH 通道写入文件数据
# 远程 cat 接收数据并写入文件
```

**下载 (远程 -> 本地)**:
```bash
# 远程执行：cat /tmp/file.txt
# 远程输出文件内容到 SSH 通道
# 本地从通道读取并保存
```

**优势**:
- ✅ 不需要服务器安装任何软件
- ✅ `cat` 是 POSIX 标准命令，100% 可用
- ✅ 利用现有 SSH 通道，无需额外连接
- ✅ 简单可靠

### 方案 2: SFTP 协议

**原理**: 使用 SSH2 的 SFTP 子协议

**优势**:
- ✅ 标准协议，支持断点续传
- ✅ 支持目录操作、权限设置

**劣势**:
- ❌ 需要服务器支持 SFTP（大多数支持）
- ❌ 实现复杂度较高

### 方案 3: Base64 编码传输

**原理**: 文件转 Base64 后通过 SSH 传输

```bash
# 上传
cat file.txt | base64 | ssh user@host "base64 -d > file.txt"

# 下载
ssh user@host "cat file.txt | base64" | base64 -d > file.txt
```

**优势**:
- ✅ 处理二进制文件安全
- ✅ 避免特殊字符问题

**劣势**:
- ❌ 增加 33% 传输开销
- ❌ 需要编码/解码时间

## 🏆 推荐方案：SSH 通道直接传输

### 实现思路

```rust
// 上传文件
fn upload_file(session: &Session, local_path: &str, remote_path: &str) -> Result<()> {
    // 1. 远程执行：cat > remote_path
    let mut channel = session.channel_session()?;
    channel.exec(&format!("cat > {}", remote_path))?;
    
    // 2. 本地读取文件
    let data = fs::read(local_path)?;
    
    // 3. 通过通道写入
    channel.write_all(&data)?;
    channel.send_eof()?;
    channel.wait_close()?;
    
    Ok(())
}

// 下载文件
fn download_file(session: &Session, remote_path: &str, local_path: &str) -> Result<()> {
    // 1. 远程执行：cat remote_path
    let mut channel = session.channel_session()?;
    channel.exec(&format!("cat {}", remote_path))?;
    
    // 2. 从通道读取
    let mut data = Vec::new();
    channel.read_to_end(&mut data)?;
    
    // 3. 保存到本地
    fs::write(local_path, &data)?;
    
    Ok(())
}
```

### 完整实现

见 `src/ssh/file_transfer.rs`

## 📊 方案对比

| 方案 | 服务器要求 | 实现难度 | 性能 | 可靠性 |
|-----|----------|---------|------|--------|
| **lrzsz (rz/sz)** | 需安装 lrzsz | 中 | 中 | 高 |
| **SSH 通道 (cat)** | 无 | 低 | 高 | 高 |
| **SFTP** | 需 SFTP 支持 | 高 | 高 | 高 |
| **Base64** | 无 | 低 | 中 | 高 |

## ✅ 结论

**推荐采用 SSH 通道直接传输方案**，理由：

1. **零依赖**: 不需要服务器安装任何软件
2. **简单**: 实现代码少，易于维护
3. **可靠**: 利用 SSH 通道，传输稳定
4. **快速**: 无额外编码开销

## 🚀 下一步

1. 实现 `file_transfer.rs` 模块
2. 集成到 UI 层
3. 添加进度显示
4. 支持批量传输
5. 添加错误重试机制
