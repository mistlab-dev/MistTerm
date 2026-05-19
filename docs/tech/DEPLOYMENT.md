# MistTerm 部署指南

> **文档版本**: 1.0  
> **最后更新**: 2026-04-24  
> **状态**: 已实现

---

## 📋 目录

1. [系统要求](#1-系统要求)
2. [开发环境搭建](#2-开发环境搭建)
3. [编译构建](#3-编译构建)
4. [跨平台编译](#4-跨平台编译)
5. [打包发布](#5-打包发布)
6. [部署流程](#6-部署流程)
7. [故障排查](#7-故障排查)

---

## 1. 系统要求

### 1.1 开发环境

| 组件 | 最低版本 | 推荐版本 | 说明 |
|-----|---------|---------|------|
| Rust | 1.70 | 1.75+ | 通过 rustup 安装 |
| Cargo | 1.70 | 1.75+ | 随 Rust 安装 |
| pkg-config | 0.29 | latest | 依赖查找工具 |
| C 编译器 | - | latest | gcc/clang/MSVC |

### 1.2 运行时

| 平台 | 版本 | 依赖 |
|-----|------|-----|
| macOS | 10.15+ | libssh2 |
| Linux | glibc 2.17+ | libssh2 |
| Windows | 10+ | libssh2.dll |

**界面字体与图标**

- **中文**：发布构建在编译期嵌入 `assets/fonts/NotoSansSC-Regular.otf`（OFL-1.1），不依赖系统是否安装微软雅黑等字体；若嵌入与系统字体均不可用，底栏会提示「未加载中文字体」。
- **图标**：UI 工具栏/侧栏图标使用内置图集纹理（`src/ui/icons.rs`），不依赖系统 emoji 字体；克隆仓库后若缺少字体文件，请执行 `./scripts/fetch-cjk-font.sh` 再编译。

### 1.3 硬件要求

| 资源 | 最低 | 推荐 |
|-----|------|-----|
| CPU | 双核 | 四核+ |
| 内存 | 512MB | 2GB+ |
| 磁盘 | 50MB | 100MB+ |

---

## 2. 开发环境搭建

### 2.1 安装 Rust

#### 通用方法（推荐）

```bash
# 安装 rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 重启终端
source ~/.bashrc  # 或 ~/.zshrc

# 验证安装
rustc --version
cargo --version
```

#### macOS

```bash
# 使用 Homebrew（可选）
brew install rustup-init

# 初始化
rustup-init
```

#### Ubuntu/Debian

```bash
# 使用 apt（版本可能较旧）
sudo apt install rustc cargo

# 或使用 rustup（推荐）
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

#### Windows

1. 访问 https://rustup.rs
2. 下载 rustup-init.exe
3. 运行安装程序
4. 重启终端

### 2.2 安装系统依赖

#### macOS

```bash
# 安装 Xcode 命令行工具
xcode-select --install

# 安装 libssh2 和 pkg-config
brew install libssh2 pkg-config
```

#### Ubuntu/Debian

```bash
# 更新包列表
sudo apt update

# 安装依赖
sudo apt install -y \
    build-essential \
    libssh2-1-dev \
    pkg-config \
    libssl-dev
```

#### CentOS/RHEL/Fedora

```bash
# Fedora
sudo dnf install -y \
    gcc \
    libssh2-devel \
    pkg-config \
    openssl-devel

# CentOS/RHEL
sudo yum install -y \
    gcc \
    libssh2-devel \
    pkg-config \
    openssl-devel
```

#### Windows

使用 vcpkg 安装 libssh2:

```bash
# 克隆 vcpkg
git clone https://github.com/Microsoft/vcpkg.git
cd vcpkg

# 安装
./bootstrap-vcpkg.bat
./vcpkg install libssh2

# 集成
./vcpkg integrate install
```

或使用 Conan:

```bash
# 安装 Conan
pip install conan

# 添加 libssh2
conan install libssh2/1.11.0@
```

### 2.3 验证环境

```bash
# 检查 Rust
rustc --version
# 输出：rustc 1.75.0 (xxx)

# 检查 Cargo
cargo --version
# 输出：cargo 1.75.0 (xxx)

# 检查 libssh2
pkg-config --modversion libssh2
# 输出：1.11.0

# 运行测试
cargo test
```

---

## 3. 编译构建

### 3.1 克隆代码

```bash
# 克隆仓库
git clone https://github.com/your-org/MistTerm.git
cd MistTerm

# 查看状态
git status
```

### 3.2 开发版本编译

```bash
# 编译（debug 模式）
cargo build

# 输出位置
# ./target/debug/mistterm  (Unix)
# ./target/debug/mistterm.exe  (Windows)
```

**编译时间**: 首次编译约 3-5 分钟，增量编译约 30 秒

### 3.3 发布版本编译

```bash
# 编译（release 模式，优化）
cargo build --release

# 输出位置
# ./target/release/mistterm  (Unix)
# ./target/release/mistterm.exe  (Windows)
```

**优化级别**: `-C opt-level=3`

**编译时间**: 首次编译约 5-10 分钟

### 3.4 运行应用

```bash
# Debug 模式
cargo run

# Release 模式
cargo run --release

# 直接运行二进制
./target/release/mistterm
```

### 3.5 运行测试

```bash
# 运行所有测试
cargo test

# 运行特定测试
cargo test test_name

# 显示测试输出
cargo test -- --nocapture

# 运行集成测试
cargo test --test '*'
```

### 3.6 代码检查

```bash
# 格式化代码
cargo fmt

# 检查代码风格
cargo clippy

# 严格检查
cargo clippy -- -D warnings
```

### 3.7 生成文档

```bash
# 生成文档
cargo doc --no-deps

# 打开文档
cargo doc --open
```

---

## 4. 跨平台编译

### 4.1 配置目标平台

```bash
# 添加目标平台
rustup target add x86_64-unknown-linux-gnu
rustup target add x86_64-apple-darwin
rustup target add x86_64-pc-windows-msvc
rustup target add aarch64-apple-darwin  # Apple Silicon
```

### 4.2 macOS 编译

#### Intel Mac

```bash
cargo build --release --target x86_64-apple-darwin
```

#### Apple Silicon

```bash
cargo build --release --target aarch64-apple-darwin
```

#### 通用二进制

```bash
# 编译两个架构
cargo build --release --target x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin

# 合并
lipo -create \
    -output target/universal2-release/mistterm \
    target/x86_64-apple-darwin/release/mistterm \
    target/aarch64-apple-darwin/release/mistterm
```

### 4.3 Linux 编译

#### 本地编译

```bash
cargo build --release --target x86_64-unknown-linux-gnu
```

#### 使用 Docker 交叉编译

```dockerfile
# Dockerfile
FROM rust:1.75

RUN apt-get update && apt-get install -y \
    gcc-aarch64-linux-gnu \
    libc6-dev-arm64-cross

WORKDIR /app
COPY . .

RUN cargo build --release --target aarch64-unknown-linux-gnu
```

```bash
# 构建镜像
docker build -t mistterm-cross .

# 编译
docker run --rm -v $(pwd):/app mistterm-cross
```

### 4.4 Windows 编译

#### 本地编译（Windows）

```powershell
cargo build --release --target x86_64-pc-windows-msvc
```

#### 交叉编译（Linux/macOS）

需要配置交叉编译工具链：

```bash
# 安装工具链
rustup target add x86_64-pc-windows-msvc

# 配置 .cargo/config.toml
[target.x86_64-pc-windows-msvc]
linker = "x86_64-w64-mingw32-gcc"
```

### 4.5 使用 cross 交叉编译

```bash
# 安装 cross
cargo install cross

# 交叉编译
cross build --release --target x86_64-unknown-linux-musl
```

---

## 5. 打包发布

### 5.1 macOS 打包

#### 创建应用 bundle

```bash
# 目录结构
mkdir -p MistTerm.app/Contents/MacOS
mkdir -p MistTerm.app/Contents/Resources

# 复制二进制
cp target/release/mistterm MistTerm.app/Contents/MacOS/

# 创建 Info.plist
cat > MistTerm.app/Contents/Info.plist << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>MistTerm</string>
    <key>CFBundleExecutable</key>
    <string>mistterm</string>
    <key>CFBundleIdentifier</key>
    <string>com.example.mistterm</string>
    <key>CFBundleVersion</key>
    <string>1.0.0</string>
</dict>
</plist>
EOF

# 复制图标
cp icon.icns MistTerm.app/Contents/Resources/

# 压缩
tar -czf MistTerm-1.0.0-macos.tar.gz MistTerm.app
```

### 5.2 Linux 打包

#### AppImage

```bash
# 使用 appimagetool
wget -O appimagetool https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage
chmod +x appimagetool

# 创建 AppDir
mkdir -p AppDir/usr/bin
cp target/release/mistterm AppDir/usr/bin/
cp icon.png AppDir/

# 创建 .desktop
cat > AppDir/mistterm.desktop << EOF
[Desktop Entry]
Name=MistTerm
Exec=mistterm
Icon=mistterm
Type=Application
Categories=System;TerminalEmulator;
EOF

# 构建 AppImage
APPIMAGE_EXTRACT_AND_RUN=1 ./appimagetool AppDir
```

#### deb 包

```bash
# 创建目录结构
mkdir -p deb-package/DEBIAN
mkdir -p deb-package/usr/bin
mkdir -p deb-package/usr/share/icons/hicolor/256x256/apps

# 控制文件
cat > deb-package/DEBIAN/control << EOF
Package: mistterm
Version: 1.0.0
Section: utils
Priority: optional
Architecture: amd64
Maintainer: Your Name
Description: Modern SSH Terminal
EOF

# 复制文件
cp target/release/mistterm deb-package/usr/bin/
cp icon.png deb-package/usr/share/icons/hicolor/256x256/apps/mistterm.png

# 构建
dpkg-deb --build deb-package mistterm_1.0.0_amd64.deb
```

### 5.3 Windows 打包

#### 创建安装包

```powershell
# 使用 Inno Setup
# 创建脚本
@files target\release\mistterm.exe
@files icon.ico

# 编译
iscc.exe setup.iss
```

#### NSIS 示例

```nsis
; setup.nsi
OutFile "MistTerm-Setup.exe"
InstallDir "$PROGRAMFILES\MistTerm"
Section "MistTerm"
    SetOutPath "$INSTDIR"
    File "target\release\mistterm.exe"
    File "icon.ico"
SectionEnd
```

---

## 6. 部署流程

### 6.1 手动安装

#### macOS/Linux

```bash
# 解压
tar -xzf MistTerm-1.0.0-macos.tar.gz

# 移动到 /usr/local/bin
sudo mv MistTerm.app /Applications/
# 或
sudo cp target/release/mistterm /usr/local/bin/
```

#### Windows

```powershell
# 解压
Expand-Archive MistTerm-Setup.exe -DestinationPath "C:\Program Files\MistTerm"

# 添加到 PATH
$env:Path += ";C:\Program Files\MistTerm"
[Environment]::SetEnvironmentVariable("Path", $env:Path, "User")
```

### 6.2 自动更新

实现自动更新机制（待完善）：

```rust
// 检查更新
fn check_for_update() -> Result<Option<Version>, Error> {
    let latest = reqwest::blocking::get(
        "https://api.github.com/repos/your-org/MistTerm/releases/latest"
    )?;
    
    // 解析版本
    // 比较版本
    // 返回是否需要更新
}
```

### 6.3 配置迁移

```bash
# 配置文件位置
# macOS: ~/Library/Application Support/MistTerm/
# Linux: ~/.config/MistTerm/
# Windows: %APPDATA%\MistTerm\

# 备份配置
cp -r ~/.config/MistTerm ~/.config/MistTerm.backup

# 恢复配置
cp -r ~/.config/MistTerm.backup/* ~/.config/MistTerm/
```

---

## 7. 故障排查

### 7.1 编译失败

#### 问题：找不到 libssh2

```bash
# macOS
brew install libssh2

# Ubuntu
sudo apt install libssh2-1-dev

# 检查 pkg-config
pkg-config --libs --cflags libssh2
```

#### 问题：Rust 版本过低

```bash
# 更新 Rust
rustup update

# 检查版本
rustc --version  # 需要 >= 1.70
```

#### 问题：链接错误

```bash
# 清理重新编译
cargo clean
cargo build --release
```

### 7.2 运行时问题

#### 问题：无法连接服务器

```bash
# 检查网络
ping 124.220.224.223

# 检查端口
nc -zv 124.220.224.223 22

# 检查 SSH 服务
ssh -v ubuntu@124.220.224.223
```

#### 问题：认证失败

```bash
# 验证凭据
ssh ubuntu@124.220.224.223

# 检查密码
# 确保没有多余空格或特殊字符
```

#### 问题：终端显示异常

```bash
# 检查终端类型
echo $TERM

# 设置正确的终端类型
export TERM=xterm-256color
```

### 7.3 日志调试

```bash
# 启用详细日志
RUST_LOG=debug ./target/release/mistterm

# 保存到文件
RUST_LOG=debug ./target/release/mistterm 2>&1 | tee mistterm.log
```

### 7.4 性能问题

```bash
# 性能分析
cargo install flamegraph
cargo flamegraph --bin mistterm

# 内存分析
cargo install cargo-instruments
cargo instruments -t TimeProfile --bin mistterm
```

---

## 📚 相关文档

- [架构文档](./ARCHITECTURE.md)
- [技术栈](./TECH-STACK.md)
- [测试方案](./TESTING.md) (待创建)

---

**文档维护**: 技术团队  
**最后更新**: 2026-04-24  
**状态**: 已实现
