# MistTerm CLI 设计

> **状态：已全部落地（2026-09-21）**
> 二进制：`mist`（`src/bin/mist.rs` + `src/cli/`），与 GUI 共享 `~/.config/mistterm/sessions.json`。
> 另：GUI 二进制 `Mist` 在**无子命令**裸调用时默认直接拉起桌面 GUI（`ff9b479`），
> 不再打印 help 后退出；`mist` CLI 与 `Mist` GUI 是两个独立 bin。

目标:为 MistTerm 增加一个命令行入口 `mist`,复用现有 `mistterm` lib 的会话存储、SSH、SFTP、批量执行、端口转发能力,让服务器上保存的连接配置可以直接在终端/脚本里用,也作为 GUI 之外的无头(headless)使用方式。

## 定位与边界

- **不是**要替代系统 `ssh`/`scp`,而是替代"打开 GUI → 找会话 → 点连接"这条链路在命令行场景的等价物。
- 与 GUI **共享同一份数据**:`~/.config/mistterm/sessions.json`(device_key 加密存储)、credentials、fragments、team sync 配置。CLI 只读为主,写操作限定在明确子命令。
- GUI 二进制 `Mist` 保持不变;CLI 是独立 bin,通过 lib 复用,不拆 crate。

## 可复用的现有模块

| 能力 | 现有代码 | 复用方式 |
|------|----------|----------|
| 会话存储/解密 | `core::session::SessionManager`(device_key 加解密 sessions.json) | 直接 `SessionManager::new()` |
| 密码解析 | `core::secret_resolver::SecretResolver`(本地/Vault/凭据库) | 直接调用 |
| SSH 连接 | `ssh::client::{SshClient, SshConfig}` | connect + open_shell/exec |
| 非交互执行 | `SshClient::exec_command` → (stdout, exit_code) | `mist exec` 核心 |
| 批量执行 | `core::batch_exec::{run_batch_parallel, run_batch_serial_fail_fast}` | `mist exec --group/--all` |
| SFTP | `ssh::sftp::SftpClient`(list/upload/download/remove/stat) | `mist ls/get/put` |
| 端口转发 | `ssh::port_forward` + `core::session::parse_*_forwards_text` | `mist fwd` |
| 跳板链 | `ssh::jump::parse_jump_chain` | 连接路径 |
| 片段/ snippets | `core::fragment::{FragmentManager, expand_*}` | `mist run <fragment>` |
| 团队同步 | `core::team::*` | P3 再做 |

## 命令设计(clap derive)

```text
mist <command>

连接与执行
  mist ls                          列出已保存会话(--group 过滤, --json)
  mist ssh <target>                交互式 shell(target = 会话名/id,或 user@host[:port])
  mist exec <target> <cmd...>      在单台机器执行命令,输出 stdout,退出码透传
  mist exec --group <g> <cmd...>   按分组批量执行(--serial 串行, --json 结构化输出)
  mist exec --all <cmd...>

文件传输(基于 SftpClient)
  mist get <target>:<remote> <local>     下载
  mist put <local> <target>:<remote>     上传
  mist rls <target>:<path>               列远端目录

端口转发
  mist fwd <target>                       按会话配置启动全部转发(前台运行,Ctrl+C 停止)
  mist fwd <target> -L 8080:host:80       临时 local forward
  mist fwd <target> -R 9090:127.0.0.1:3000
  mist fwd <target> -D 1080               SOCKS5

片段
  mist frag list                          列出片段(--json)
  mist frag run <name> <target> [--var k=v]   变量展开后 exec
  mist frag run <name> --group <g>

其他
  mist import-ssh-config                 复用 ssh_config_importer 导入 ~/.ssh/config
  mist --version / mist doctor           版本 / 环境自检(配置文件、device_key、网络)
```

### target 解析规则

1. 先按 `sessions.json` 的 name/id 精确匹配;
2. 匹配不到再按 `user@host[:port]`(或 `host`,取当前用户名)临时构造 `SessionConfig`;
3. 临时连接也走 `SecretResolver`:依次 ssh-agent → `~/.ssh/id_*` → 交互询问密码(仅 TTY;非 TTY 报错提示用 `--password-file` 或 keyring)。

## 关键实现点

### 1. 新 bin,不碰 GUI

```toml
[[bin]]
name = "mist"
path = "src/bin/mist_cli.rs"
```

逻辑放 `src/cli/` 模块(`src/cli/mod.rs` + 各子命令文件),`mist_cli.rs` 只做 clap 解析和分发,保证可测试。新增依赖仅 `clap = { version = "4", features = ["derive"] }` 和 `crossterm`(raw mode / 终端尺寸),都是纯 Rust。

### 2. 交互式 shell(`mist ssh`)

- `SshClient::connect` → `open_shell(cols, rows)`,cols/rows 用 `crossterm::terminal::size()` 取真实终端尺寸。
- 本地 `crossterm::terminal::enable_raw_mode()`,stdin 单独线程 → `channel.write`;channel `set_blocking(false)` 轮询读 → stdout 直写(不做终端仿真,远端 PTY 自己处理,GUI 的 alacritty grid 不需要)。
- 监听 `SIGWINCH` → `channel.request_pty_size` 调整窗口。
- 退出路径:channel EOF / `~.` 本地 escape / Ctrl+C 透传(0x03)而非杀进程。
- 非 TTY 场景 stdin 是管道时自动退化:pipe stdin → channel,等价 `ssh host cmd`。

### 3. 输出约定(脚本友好)

- 默认人类可读;`--json` 给结构化输出(exec 批量结果直接序列化 `BatchExecRow`)。
- stdout 只放数据,诊断/进度走 stderr + `tracing`(默认 warn,`-v` 提升)。
- 退出码:`exec` 透传远端退出码;批量时任一失败返回最高非零码;连接失败统一 2,参数错误 1。

### 4. 与 GUI 的数据一致性

- CLI 读 `SessionManager::default_storage_path()` 同一份文件;device_key 来自 `security::device_key`(与 GUI 相同派生),加密会话开箱可读。
- `exec`/`ssh` 成功后更新 `last_connected_at` 并 `save()` —— 与 GUI 行为一致,最近连接排序保持同步。
- 写历史:`exec`/`frag run` 记录进 `core::command_history`,GUI 历史面板可见。

## 实际落地命令（`src/bin/mist.rs`）

```text
mist ls [--group <g>]                 列出已保存会话
mist ssh <target>                     交互式 shell
mist exec <target> <cmd...>           单机执行（退出码透传）
mist exec --group <g> <cmd...>        按分组批量（默认并行 8）
mist exec --all <cmd...>              全量批量
     --serial                         串行 + 失败熔断
     --parallel <1-16>                并行度
mist rls <target>:<path>              列远端目录
mist get <target>:<remote> <local>    下载
mist put <local> <target>:<remote>    上传
mist fwd <target> [-L ...] [-R ...] [-D ...]   端口转发（前台，Ctrl+C 停止）
mist frag list                        列出片段
mist frag run <name> [target] [-v k=v] [--group <g>|--all] [--serial] [--parallel N]
mist import-ssh-config [-f <file>] [--dry-run] [--overwrite]
mist sop extract [-n <N>] [-t <title>]  从 exec-history.jsonl 提炼 Markdown 排错 SOP

全局：--json（结构化输出）、-v/-vv（日志级别，输出到 stderr）
```

模块划分：`src/cli/{context,ls,exec,ssh_cmd,sftp_cmds,fwd,frag,import_ssh,session_log}.rs`。

## 分期（历史计划 → 实际状态）

- **P1** ✅：`ls` / `exec`(单机+批量) / `get` / `put` / `rls` / `--json` / target 解析。
- **P2** ✅：`mist ssh` 交互 shell（raw mode + 窗口 resize + escape）、`mist fwd`（含 `-L`/`-R`/`-D`）。
- **P3** ✅：`frag run`、`import-ssh-config`、`sop extract`。
  仍待办：team sync 登录态复用（`TeamTokenStore`）、shell 补全脚本（`clap_complete`）。

## 风险点

- `ssh2` 是同步阻塞模型:交互 shell 需要非阻塞轮询 + 单独 stdin 线程;批量执行已有 `run_batch_parallel` 的线程模型可套。
- device_key 派生若依赖 GUI 运行环境(如 keyring 回退),无 GUI 的服务器上要确认 `security::device_key` 路径可独立工作 —— P1 开工前先验证。
- Windows:`mist ssh` 的 raw mode 用 crossterm 跨平台没问题;`~.` escape 行为按 OpenSSH 惯例实现。
- 命名冲突:bin 名 `mist` 小写,GUI 是 `Mist`;Windows 文件系统不区分大小写,产物名要错开(如 CLI 产物叫 `mist-cli` 或确认 Windows 打包时区分目录)。
