# 🌫️ MistTerm

> 异步 SSH 终端 - 现代 Rust 实现的终端模拟器

基于 Rust 构建的现代化 SSH 终端，支持异步连接、多会话管理和交互式 shell。

## ✨ 核心功能

- 🚀 **纯 Rust 实现** - 性能与安全兼备
- 🔐 **SSH 连接** - 密码/密钥双认证支持
- 🔄 **异步架构** - 基于 tokio + ssh2，非阻塞操作
- 🖥️ **GUI 界面** - eframe/egui 现代化终端界面
- 📂 **多会话管理** - 支持同时管理多个 SSH 会话
- 💾 **配置持久化** - 会话配置自动保存到 `sessions.json`
- ⌨️ **完整键盘支持** - Enter 发送命令，支持所有终端操作

## 🚀 快速开始

### 构建与运行

```bash
# 克隆仓库
git clone https://github.com/c-wind/MistTerm.git
cd MistTerm

# 构建 release 版本
cargo build --release

# 运行
./target/release/mistterm
```

### 使用流程

1. **启动程序** - 运行 `./target/release/mistterm`
2. **连接服务器** - 点击 "Connect" 按钮
3. **填写信息** - 输入主机、端口、用户名、密码
4. **开始使用** - 连接成功后在输入框输入命令

### 会话管理

- **保存会话** - 点击 "Save Sessions" 手动保存
- **自动保存** - 创建会话时自动保存到 `sessions.json`
- **加载会话** - 程序启动时自动加载已保存的会话
- **删除会话** - 连接状态下点击会话后的 "X" 按钮

## 📖 功能说明

### 连接对话框

| 字段 | 说明 | 示例 |
|------|------|------|
| Name | 会话名称 | My Server |
| Host | 服务器地址 | 192.168.1.100 |
| Port | SSH 端口 | 22 |
| Username | 用户名 | ubuntu |
| Password | 密码 | your_password |

### 键盘操作

| 按键 | 功能 |
|------|------|
| `Enter` | 发送命令 |
| `Ctrl+C` | 中断当前命令 |
| `Ctrl+D` | 发送 EOF |

### 终端显示

- **绿色等宽字体** - 清晰的代码显示
- **黑色背景** - 减少视觉疲劳
- **自动滚动** - 输出自动滚动到底部
- **状态指示** - 连接状态实时显示（连接中/已连接/错误）

## 🛠️ 技术架构

### 核心组件

```
MistTerm/
├── src/
│   ├── main.rs          # GUI 主界面 (eframe/egui)
│   └── ssh.rs           # SSH 模块 (异步连接、认证、会话管理)
├── examples/
│   ├── ssh_test.rs      # 单会话测试
│   └── ssh_multi_session.rs  # 多会话并发测试
└── Cargo.toml           # 项目配置
```

### 依赖库

- **[eframe](https://github.com/emilk/egui)** - GUI 框架
- **[egui](https://github.com/emilk/egui)** - 即时模式 GUI
- **[tokio](https://github.com/tokio-rs/tokio)** - 异步运行时
- **[ssh2](https://github.com/alexcrichton/ssh2-rs)** - SSH 协议实现
- **[serde](https://github.com/serde-rs/serde)** - 序列化/反序列化
- **[parking_lot](https://github.com/Amanieu/parking_lot)** - 线程同步

## 📊 性能测试

### 多会话并发测试

```
测试配置：10 个并发 SSH 会话
每个会话执行：5 条命令
总命令数：50 条

结果:
✅ 成功：50
❌ 失败：0
📈 成功率：100%
```

每个会话独立执行，无连接冲突或数据混乱。

## 🚧 开发状态

### 已完成

- [x] SSH 异步连接架构
- [x] 密码/密钥认证
- [x] 交互式 shell 启动
- [x] 命令执行 (exec)
- [x] 多会话并发管理
- [x] GUI 界面
- [x] 会话配置持久化
- [x] 单元测试

### 待完善

- [ ] ANSI 转义码解析
- [ ] 终端渲染优化
- [ ] 复制/粘贴支持
- [ ] 会话配置编辑
- [ ] 快捷键自定义
- [ ] 主题切换
- [ ] 文件传输 (SFTP)

## 🤝 贡献

欢迎提交 Issue 和 Pull Request！

## 📄 许可证

MIT License - 详见 [LICENSE](LICENSE)

---

Made with 🦀 and ☕
