# ZMODEM 文件传输

MistTerm ZMODEM 发送实现的协议要点、`rz` 兼容性排障，以及无 `lrzsz` 时的兜底方案。

---

## 1. 概述与特性

MistTerm 内置 `zmodem2`（fork）发送，支持远端 `rz`（接收）。常见命令：

```bash
rz -bye          # 推荐入口；MistTerm 自动检测并接管
sz filename.txt  # 接收侧（如启用）
```

实现特性：

- 完整 ZMODEM 状态机（ZRINIT / ZFILE / ZDATA / ZEOF / ZACK / ZFIN）
- CRC16 / CRC32 校验
- ESCCTL 转义（`rz -bye` 必需，2026-05 修复）
- SSH 通道共享：交互式 shell 与 ZMODEM 帧统一经 Tokio 泵 + 有界 mpsc 队列下发
- 自动检测 `rz` 触发序列（文本模式 + 二进制 ZRQINIT）
- 进度跟踪、错误回显

---

## 2. 协议流程（发送侧）

```
本地 (sz / Mist)                    远端 (rz)
  |                                   |
  |<------ ZRQINIT ------------------ |   (rz 启动后发起)
  |------ ZRINIT (peer caps) -------> |
  |------ ZFILE (filename, size) ---> |
  |<----- ZRPOS (offset 0) ---------- |
  |------ ZDATA (block 1) ----------> |
  |<----- ZACK (or implicit) -------- |
  |   ...                             |
  |------ ZEOF ---------------------> |
  |<----- ZRINIT -------------------- |
  |------ ZFIN ---------------------> |
  |<----- ZFIN ---------------------- |
  |------ "OO" ---------------------> |
```

包类型速查：

| 包类型 | 代码 | 说明 |
|---|---|---|
| ZRQINIT | 0x00 | 请求接收初始化（多以 `**\x18B00` 文本暴露） |
| ZRINIT | 0x01 | 接收方初始化（含 ESCCTL/ESC8/CRC32 等能力位） |
| ZSINIT | 0x02 | 发送方初始化 |
| ZFILE | 0x04 | 文件元信息 |
| ZRPOS | 0x09 | 接收方告知发送恢复位置 |
| ZDATA | 0x0A | 数据块 |
| ZEOF | 0x0B | 文件结束 |
| ZACK | 0x03 | 确认 |
| ZFIN | 0x08 | 传输结束 |
| ZNAK / ZSKIP / ZABORT | 0x05 / 0x06 / 0x07 | 否认 / 跳过 / 中止 |

> 注意：协议字面量为 `ZSE`（0x18B0）转义后的真实代码点，与早期 docs 中的 0x60+ 形式不同。

---

## 3. ZDLE / ESCCTL 关键实现

### 3.1 默认 ZDLE 转义表

仅转义控制流劫持字符：`ZDLE(0x18)`、`ZPAD(0x2A)`、`XON(0x11)`、`XOFF(0x13)`、`CR(0x0D)`、`LF(0x0A)` 及高位等价。其他字节直传。

### 3.2 ESCCTL 增强转义表（`rz -bye` 必需）

`rz -bye` 在 ZRINIT 中带 `ESCCTL` 标志，要求发送方对 `0x00..0x1F` 与 `0x80..0x9F` 全段控制字符做 ZDLE 转义；否则 PTY/驱动可能截留或解释字节，导致 CRC 校验失败、远端回退到 X/YMODEM（在 PTY 中表现为反复 `C`）或 `command not found`。

实现见 `vendor/zmodem2/src/zdle.rs`：

```rust
pub const ZDLE_TABLE: [u8; 256] = compute_default_table();
pub const ZDLE_TABLE_ESCCTL: [u8; 256] = compute_escctl_table();

thread_local! { static ESCCTL_ENABLED: Cell<bool> = const { Cell::new(false) }; }
pub fn escctl_enabled() -> bool { ESCCTL_ENABLED.with(|c| c.get()) }
pub fn set_escctl(v: bool) { ESCCTL_ENABLED.with(|c| c.set(v)); }
```

发送方在握手期收到带 `Zrinit::ESCCTL` 的 ZRINIT 后，立即 `zdle::set_escctl(true)`，后续所有 `write_byte_escaped` 自动改用增强表（`vendor/zmodem2/src/transmission.rs::update_receiver_caps`）。

集成测试 `vendor/zmodem2/tests/integration.rs::test_batch_to_rz_bye_escctl` 跑真实 `rz -b -y -e` 校验该路径。

---

## 4. SSH 通道共享

ZMODEM 与键盘输入复用同一条 `ssh2::Channel`，全部经 Tokio 泵收发，避免在 `.await` 之间持有 SSH 锁。

```rust
pub enum ShellPumpCommand {
    PtyInput(Vec<u8>),
    ZmodemWrite(Vec<u8>),
}

pub struct SshSessionHandle {
    pub session_id: SshSessionId,
    pump_tx: tokio::sync::mpsc::Sender<ShellPumpCommand>,
    resize_tx: tokio::sync::mpsc::Sender<(u32, u32)>,
}
```

阻塞 `read`/`write` 一律 `spawn_blocking` 出去，保证 GUI 不卡。

---

## 5. 自动检测 `rz`

```rust
pub fn detect_rz_command(&self, data: &[u8]) -> bool {
    let text = String::from_utf8_lossy(data);
    if text.contains("rz rz rz")
        || text.contains("Awaiting rz")
        || text.contains("rz waiting to receive")
    {
        return true;
    }
    // 二进制 ZRQINIT: 0x80 0x80 0x18 0x64/0x62
    matches!(
        data,
        [0x80, 0x80, 0x18, 0x64 | 0x62, ..]
    )
}
```

---

## 6. `rz` 兼容性排障经验（2026-05）

### 6.1 已确认事实

1. **链路不是问题**：`strace` 显示 `rz` 的 `read(0, ...)` 能收到 ZMODEM 字节。
2. **失败可能在协议解析阶段**：某些包形态触发 `no.name/ZMODEM: got error`，`exit_group(128)`。
3. **也可能进入 fallback**：`rz` 周期性输出单字节 `C`（X/YMODEM 风格等待），会话僵住。
4. **`command not found` 不是根因**：是 `rz` 已退出后协议字节落到 shell 的副作用。

### 6.2 关键定位证据

- `strace`：
  - `read(0, "**\30A...1111.txt...") = ...`（收到发送流）
  - `sendto(... "no.name/ZMODEM: got error", ...)`
  - `exit_group(128)`
- 会话僵住时 `write(1, "C", 1)` 反复出现 → 已落入 fallback。
- 本地日志 `text='**010004...: command not found'` → 已回到 shell。

### 6.3 落地的稳定策略

| 项 | 做法 |
|---|---|
| **ESCCTL 自动协商** | `Zrinit::ESCCTL` 命中后切换 `ZDLE_TABLE_ESCCTL`（最关键，2026-05 修复 `rz -bye`） |
| **不主动回 sender ZRINIT** | 默认关，可经 `MISTTERM_ZMODEM_SENDER_REPLY_ZRINIT=1` 打开做对照 |
| **ZFILE 编码可切换** | `MISTTERM_ZMODEM_ZFILE_BIN32` 默认 false（优先 ZBIN16） |
| **关闭激进重发轮换** | 不在同一会话快速切换多种线材形态 |
| **可观测性增强** | 检测 shell 回退标记 / 连续 `C` 立即失败；`feed 后无待发帧` 日志带回显文本片段 |

### 6.4 推荐操作

1. 远端 `rz -bye`（推荐，已支持 ESCCTL）。
2. MistTerm 中选择上传文件。
3. 失败时优先看错误提示中的：
   - `远端已回到 shell...` → `rz` 已退出。
   - `已切换到 X/YMODEM（收到连续 'C'）` → fallback。

### 6.5 复现 / 复盘流程

```bash
# 本地：可选打开 TX dump
export MISTTERM_ZMODEM_DUMP_TX=1

# 远端：strace 抓 rz
strace -ff -tt -s 256 -o /tmp/rz.trace rz -bye 2>/tmp/rz.err

# 分析
rg -n "read\\(0|write\\(1|write\\(2|TIMEOUT|exit_group|got error|SIG" /tmp/rz.trace*
sed -n '1,200p' /tmp/rz.err
```

判断分支：

- 看到 `read(0, ...)` 字节 → 不是链路丢包；
- `got error` + `exit_group(128)` → 协议解析失败；
- 反复 `write(1, "C", 1)` → fallback 卡住。

---

## 7. 兜底：服务器无 `lrzsz` 时的方案

部分受控/精简镜像没有 `rz`/`sz`，回退顺序：

| 方案 | 服务器要求 | 实现难度 | 性能 | 备注 |
|---|---|---|---|---|
| **SSH 通道 `cat` 透传** | 无 | 低 | 高 | POSIX 必备，纯 SSH，已实现于 `src/ssh/scp.rs` 兼容路径 |
| **SFTP 子协议** | 需 sftp-server | 中 | 高 | 支持目录、权限、断点续传，主路径 |
| **Base64 透传** | 无 | 低 | 中 | 二进制安全，但 +33% 开销 |

参考实现（思路）：

```rust
fn upload(session: &Session, local: &Path, remote: &str) -> Result<()> {
    let mut ch = session.channel_session()?;
    ch.exec(&format!("cat > {remote}"))?;
    ch.write_all(&fs::read(local)?)?;
    ch.send_eof()?;
    ch.wait_close()?;
    Ok(())
}
```

实际产品里 SFTP 是主路径（`src/ui/sftp_panel.rs`），ZMODEM 是终端中显式触发的便捷通道。

---

## 8. 测试

```bash
cargo test --lib ssh::lrzsz                         # 单元测试
cargo test -p zmodem2 --test integration            # 真实 rz/sz 集成测试（需 lrzsz 安装）
cargo test -p zmodem2 --test integration -- test_batch_to_rz_bye_escctl
```

---

## 9. 参考

- ZMODEM 协议规范（Hayes/Forsberg 1988）
- [`vendor/zmodem2/`](../../vendor/zmodem2/)（fork，含 ESCCTL 修复）
- [ssh2-rs](https://github.com/alexcrichton/ssh2-rs)
