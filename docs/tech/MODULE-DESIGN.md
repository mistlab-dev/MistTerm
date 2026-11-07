# MistTerm 模块设计

> 本文是**模块地图**：讲每个目录/文件管什么、层与层之间怎么走。
> 具体类型、字段、函数签名以源码为准，不在这里复制——复制出来的那份一定会过期。

## 1. 模块概览

### 1.1 顶层目录

| 目录 | 文件数 | 职责 |
|------|--------|------|
| `src/ui/` | 75 | 全部界面：窗口编排、终端视图、各面板、对话框、主题与图标 |
| `src/core/` | 88 | 领域逻辑：会话、片段、审计、凭证、AI、同步、团队、市场、更新 |
| `src/ssh/` | 19 | SSH 连接、认证、SFTP、端口转发、跳板、ZMODEM、known_hosts |
| `src/terminal/` | 8 | 终端内核与渲染适配：VTE 网格、命令块、语义 OSC、程序状态 |
| `src/cli/` | 14 | `mist` 命令行（ls / exec / ssh / rls / get / put / fwd / frag / import / sop） |
| `src/platform/` | 17 | 窗口、字体、托盘、单实例、系统集成等平台差异 |
| `src/modules/` | 6 | 可插拔扩展：输出触发规则、链接识别等 |
| `src/capability/` | 1 | 模块注册表：Host 持有并共享给各终端视图 |
| `src/security/` | 4 | 本地加密（device_key / AES-256-GCM）、钥匙串 |
| `src/i18n/` | 3 | 界面文案与内置规则的多语言 |
| `src/monitor/` | 3 | 主机指标采集（CPU / 内存 / 磁盘 / 负载） |
| `src/local_shell/` | 1 | 本机 Shell 会话（不走 SSH） |
| `src/bin/` | 10 | 辅助二进制：测试与冒烟工具、图标导出 |

### 1.2 依赖方向

```text
ui/ ─────► core/ ─────► ssh/ ─────► libssh2
 │           │            │
 │           └────► security/
 │
 ├─────► terminal/ ─────► alacritty_terminal
 ├─────► platform/
 └─────► i18n/
```

规则：

- `ui/` 不直接依赖 `ssh2`，只通过 `core/` 与 `ssh/` 暴露的类型和消息通信。
- `core/` 不依赖 `ui/`，可以从 `cli/` 复用。
- `terminal/` 不依赖 `ssh/`；收到的字节怎么来，它不关心。

## 2. UI 层

### 2.1 窗口与编排

| 文件 | 职责 |
|------|------|
| `app.rs` | `MistTermApp`：顶层状态、活动栏、右抽屉互斥、Toast、窗口事件 |
| `workspace.rs` | 面板注册顺序与三列工作区 |
| `chrome.rs` | 顶栏、Tab、Toast 绘制 |
| `sidebar.rs` | 连接列表（默认收起，rail「连接」展开） |
| `tab_pane.rs` | 单个标签的内容容器 |

布局细节见 [LAYOUT.md](../product/LAYOUT.md)，那里写了几何与注册顺序，是唯一权威。

### 2.2 终端视图

| 文件 | 职责 |
|------|------|
| `terminal.rs` | 终端视图：把 VTE 网格画出来，处理选区、滚动、跟随 |
| `terminal_keys.rs` | 键位映射：哪些键送 PTY、哪些留给 UI、IME 事件 |
| `command_palette.rs` | 命令面板（Ctrl+K / ⌘K） |
| `command_history_overlay.rs` | 命令历史浮层 |

### 2.3 面板与对话框

| 文件 | 职责 |
|------|------|
| `sftp_panel.rs` / `sftp_xfer.rs` / `sftp_file_table.rs` / `sftp_diff_ui.rs` | 文件面板：双栏、传输队列、文件表、目录对比 |
| `monitor_panel.rs` | 主机监控图表 |
| `port_forward_panel.rs` | 端口转发规则 |
| `ai_panel.rs` | AI 对话面板 |
| `fragment_library.rs` | 片段库 |
| `credential_panel.rs` | 凭证管理 |
| `cloud_sync_panel.rs` | 云端同步 |
| `preferences_dialog.rs` / `theme.rs` | 偏好设置与主题 |
| `rule_manager_dialog.rs` | 自动化规则（输出触发） |
| `help_docs_dialog.rs` / `markdown_view.rs` | 帮助文档与 Markdown 渲染 |

## 3. 核心层

### 3.1 会话与会话数据

| 文件 | 职责 |
|------|------|
| `session.rs` | `SessionConfig` 定义、字段默认值、认证模式 |
| `session_list.rs` / `session_sort.rs` | 会话列表、分组与排序 |
| `session_logger.rs` | 会话日志（本地 JSONL） |
| `reconnect.rs` | 断开重连、恢复状态 |
| `ssh_url.rs` | `ssh://` 链接解析 |
| `ssh_config_importer.rs` / `foreign_import.rs` | 从 `~/.ssh/config`、Xshell 等外部工具导入 |

### 3.2 片段与命令

| 文件 | 职责 |
|------|------|
| `fragment.rs` / `fragment_command.rs` / `fragment_expr.rs` | 片段模型、变量替换、表达式求值 |
| `fragment_shortcuts.rs` / `fragment_analytics.rs` / `fragment_usage_log.rs` / `fragment_recommendations.rs` | 快捷入口、统计、使用记录、推荐 |
| `send_sequence.rs` | 发送序列：按行下发、消息延迟 |
| `paste_history.rs` | 粘贴历史与私钥/密码过滤 |
| `command_history.rs` / `exec_history.rs` | 命令历史、执行历史 |
| `batch_exec.rs` / `interactive_exec.rs` | 批量执行、交互式执行 |

### 3.3 审计与安全

| 文件 | 职责 |
|------|------|
| `audit.rs` / `audit_timeline.rs` | 审计事件与时间线 |
| `cmd_audit.rs` / `cmd_audit_personal.rs` | 命令审计（团队策略、个人规则） |
| `secret_resolver.rs` | 凭证引用解析 |
| `credential.rs` | 凭证模型与存储 |

### 3.4 AI 与知识

| 文件 | 职责 |
|------|------|
| `ai_client.rs` / `ai_enhanced.rs` | OpenAI 兼容接口调用、增强能力 |
| `knowledge.rs` / `runbook_recall.rs` | 知识注入、运维手册召回 |
| `postmortem.rs` | 事后复盘草稿 |

### 3.5 同步与团队

| 文件 | 职责 |
|------|------|
| `host_sync.rs` | 主机列表与凭证跨设备同步 |
| `cloud_sync.rs` | 配置备份到 Git 仓库 |
| `agent/` | 服务端命令审计 Agent 的对接 |
| `controlplane/` | 控制面接口 |
| `team/` | 团队账号、片段共享、审计上报 |
| `market/` | 片段市场 |
| `vault/` | Vault 凭证读取 |
| `updater/` | 自动更新检查与下载 |

### 3.6 其它

`crash_reporter.rs` / `hang_reporter.rs` 负责崩溃与卡死报告，写 `logs/crash-reports/`、`logs/hang-reports/`。

## 4. SSH 层

| 文件 | 职责 |
|------|------|
| `client.rs` | `SshConfig` / `SshClient`：建连、认证、开通道 |
| `manager.rs` | `SshManager` / `SshSessionHandle`：会话表、消息泵 |
| `fido2.rs` | FIDO2 sk 密钥（走 OpenSSH agent） |
| `known_hosts.rs` | 主机密钥校验与首次信任 |
| `jump.rs` / `proxy_command.rs` | ProxyJump 多跳、ProxyCommand 子进程桥接 |
| `port_forward.rs` / `socks_proxy.rs` | 本地 / 远程 / 动态转发 |
| `sftp.rs` / `scp_upload.rs` / `file_transfer.rs` | 文件传输与进度 |
| `lrzsz.rs` / `lrzsz_zmodem2_send.rs` / `lrzsz_external_sz.rs` / `zmodem_pty_*.rs` | rz/sz（ZMODEM）传输 |
| `user_facing.rs` | 把底层错误转成用户能读的文案 |

连接与认证细节见 [SERVER-SIDE-AUDIT.md](./SERVER-SIDE-AUDIT.md)、[ZMODEM.md](./ZMODEM.md)。

## 5. 终端层

| 文件 | 职责 |
|------|------|
| `alacritty.rs` | alacritty_terminal 适配：网格 → egui 渲染、选区、滚动、OSC 分块 |
| `emulator.rs` | 自带终端模拟器：网格、滚屏、备用屏幕、光标、样式 |
| `command_block.rs` | 命令块索引、块导航、块复制 |
| `sem_osc.rs` | 语义 OSC：目录（9;9）、徽标（7501）、命令块（133） |
| `program_status.rs` | 程序状态：等待 / 完成 |
| `style.rs` | 行级样式判定（用户错误 / 警告 / 成功行） |
| `paste.rs` | 粘贴处理与括号粘贴 |

时序与转义序列细节见 [TERMINAL-BEHAVIOR.md](./TERMINAL-BEHAVIOR.md)。

## 6. 其它层

- `cli/`：`mist` 子命令实现，复用 `core/`；不依赖 `ui/`。
- `platform/`：窗口 frame、字体加载、托盘、单实例锁、系统主题。
- `modules/`：输出触发规则与链接识别，注册进 `capability::ModuleRegistry`。
- `security/`：`device_key` 派生、AES-256-GCM 信封、钥匙串读写。
- `i18n/`：界面文案、菜单、内置规则的多语言表。

## 7. 错误处理约定

- 底层错误在 `ssh/user_facing.rs` 转成面向用户的文案，UI 只显示转换后的结果。
- 需要用户决策的错误（主机密钥变更、认证失败、传输中断）走对话框或 Toast，不弹裸错误串。
- 崩溃与卡死写文件报告，下次启动提示一次，不打断正常使用。
