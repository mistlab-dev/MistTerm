# MistTerm 客户端待办（2026-09-03）

> 服务端所有功能已交付并部署生产；本文用于记录客户端验收状态和后续体验项。
> 涉及 Rust 代码改动时，需在本地 cargo 环境编译验证；本次仅同步文档，未执行编译。

**2026-08-23 更新**：P0 §1–§4、P1 §6–§7 已实现并通过自动化测试（`client_todo_acceptance_test`、`cmd_audit` 单测、团队 API 探针）；§5 实机 SSH/agent 仍建议人工抽测；§8 生产 sshd 运维操作。

**2026-09-03 更新（文档同步，未编译）**：
- 基线 release 已到 **v1.1.2**（v1.0.22 → v1.1.2），本条目所有「已实现」功能均已发布。
- CLI/v1.1 期间的稳定修复一并随版本发布：**ZMODEM 接收管道缓冲区溢出丢弃 bug 修复**、**SSH shell pump 防 Tab/输入阻塞加固**、core 公开 API 文档补全与遗留中文 label 函数弃用。
- §5 实机点验：自动化部分已 PASS；GUI 人工点验（confirm 弹窗 / 黄横幅）仍建议登录团队后补一次。
- **§8 已完成**：生产机已关闭 SSH 密码登录（`PasswordAuthentication no`）并依赖 CA 证书，客户端无代码改动。

**2026-08-29 更新**：
- 客户端自动化验收仍通过（`client_todo_acceptance_test` 9/9）。
- **§5 已在 `124.220.224.223` 落地 mist-agent 并完成 SSH ForceCommand 联调**（见下表）。
- 说明：内置规则将 `cat /etc/shadow` 标为 **dangerous→block**（非 confirm）；confirm 场景用团队自定义规则 `mist_confirm_demo` 验证。
- Agent 为 bash+curl（非二进制），API 基址指向本机 `http://127.0.0.1:8080/v1`（与 `mist-team-server` 同机）。
- MistTerm GUI toast / 黄横幅仍建议在客户端登录对应团队后做一次人工点看。
- **hotfix**：`interactive.bash` / wrapper 内 `case` 曾写成 `"trap "*"`（坏引号）→ 登录即 `syntax error near unexpected token '*'`；已改为 `trap\ *|source\ *|.\ *)` 并同步修 wrapper 内嵌 HOOK，避免再次覆盖。

---

## P0 — 命令审计客户端闭环

### 背景

服务端已实现「服务器侧强制命令审计」：在被审计的远程服务器上部署透明 agent，命令在服务器本地执行前通过 `MIST_AUDIT` 标记行回调服务端判定/记录。客户端通过解析 PTY 输出中的 `MIST_AUDIT\t{JSON}` 行获取判定结果。

**信任边界变化**：命令审计的判定权威从「客户端本地」移到「服务端 + 服务器侧 agent」。客户端本地审计（`CmdAuditEngine`）保留但降级为「本地快捷提示」，不作为安全承诺。

### 1. 服务器侧审计结果展示（已实现；仅剩 GUI 人工点验）

**现状**：
- `src/core/cmd_audit.rs` — `ServerAuditProbe` 解析 `MIST_AUDIT\t{JSON}` 标记行
- `src/ui/terminal.rs` — PTY feed 调用 probe，事件经 `take_server_audit_events` 交给宿主
- `src/ui/app.rs` — `poll_server_audit_from_tabs` / `handle_server_audit_event` 展示 block/confirm/alert

**验收状态**：
- [x] `cargo build --release` / `cargo check --lib` 编译通过（此前已验证）
- [x] `cargo test` cmd_audit 单测不回归（此前已验证）
- [x] 实机：agent 主机执行危险命令 → `MIST_AUDIT` block（`rm -rf /`）
- [x] 实机：无害命令正常放行
- [x] 实机：自定义 confirm 规则和 Agent 列表 API
- [ ] 大数据块 partial prefix 不丢数据（已有单测；可选 SSH 压测）
- [ ] MistTerm GUI 上人工点验 confirm 弹窗 / Agent 黄横幅（代码已实现，需登录团队后点验）

### 2. 本地审计与服务器审计的文案区分（已实现）

- [x] 本地 block：`本地检查：该命令被禁止`
- [x] 服务器 block/alert：保留「服务器策略」前缀
- [x] 确认弹窗标题：本地「本地检查：确认执行」/ 服务器「服务器策略：确认执行」
- [x] 确认按钮：Local「确认发送」/ Server「放行并发送」

### 3. Agent 不可用降级提示（已实现）

- [x] 每 60s `GET /v1/teams/{team_id}/command-audit/agents`（404 时不显示横幅）
- [x] 按 `session.host` 匹配 agent，`last_seen_at` > 5min 或离线 → 终端顶栏黄色横幅（可关闭）
- [x] agent 恢复 → 绿色 toast「服务器侧命令审计已恢复」

涉及：`src/core/team/client.rs`、`service_blocking.rs`、`src/ui/app.rs`、`workspace.rs`、`terminal.rs`

### 4. 团队设置页 Agent 状态展示（已实现）

- [x] 团队设置弹窗「命令审计 Agent」表格（主机 / 状态 / 最后心跳 / 启用禁用）
- [x] Admin 可调 `PUT …/agents/{id}`；空列表引导文案

涉及：`src/ui/team_fragment_extras_dialog.rs`

### 5. 实机联调验证清单

| 场景 | 预期行为 | 状态 |
|------|----------|------|
| 连接已装 agent，执行 `rm -rf /` | block + `MIST_AUDIT` | **PASS**（SSH ForceCommand） |
| 执行无害命令（如 `echo …`） | 正常放行 | **PASS** |
| 触发 confirm 规则（`mist_confirm_demo`） | confirm + `MIST_AUDIT` | **PASS** |
| 确认后 `MIST_AUDIT_APPROVE` | 客户端放行链路 | **PASS**（自动化链路）；GUI 待人工点看 |
| agent 列表 API | 返回 active agent | **PASS** |
| agent 停止后黄横幅 | 客户端降级 UI | 代码已实现；登录团队后人工点验 |
| 本地审计文案「本地检查」 | toast 前缀 | 自动化覆盖（`client_todo_acceptance_test` 9/9） |

> 注：文档原稿中的 `cat /etc/shadow`→confirm 与当前内置规则不符（CREAD-006 = dangerous→block）。实机以规则引擎为准。

#### 实机联调步骤（约 15 分钟）

**前置**：登录团队账号；目标主机已安装 command-audit agent；团队设置 → Agent 列表显示「在线」。

1. **block**：SSH 连 agent 主机 → 输入 `rm -rf /` 回车 → 红色 toast「服务器策略：该命令被禁止」（含规则 message 或 rule）；命令未执行。
2. **allow**：输入 `ls` → 无 block toast；正常输出目录列表。
3. **confirm**：输入 `cat /etc/shadow` → 弹出「服务器策略：确认执行」对话框，按钮「放行并发送」→ 确认后终端应先收到 `MIST_AUDIT_APPROVE\t{token}` 再发送原命令。
4. **降级**：在主机上 `systemctl stop mist-audit-agent`（或等心跳超时 >5min）→ 终端顶栏黄色横幅「服务器侧审计不可用…」；可点 × 关闭。
5. **恢复**：重启 agent → 横幅消失 + 绿色 toast「服务器侧命令审计已恢复」。
6. **本地 vs 服务器**：断开 agent 或连未装 agent 的主机 → 执行本地 block 规则 → toast 前缀为「本地检查：」而非「服务器策略：」。

**记录**：每项 PASS/FAIL + 截图或 toast 原文；失败时附 `MIST_AUDIT` 原始行（若可见）与团队 ID / 主机名。

---

## P1 — 片段体验补齐

### 6. 异常退出清理编辑锁（已实现）

- [x] 启动 / 登录后 `release_residual_fragment_locks_blocking`：释放当前用户残留锁
- [x] 编辑团队片段时自动 lock，关闭时 unlock
- [x] 编辑中心跳每 30s 续锁；失败 toast「编辑锁已丢失…」

涉及：`service_blocking.rs`、`team_fragment_dialog.rs`

### 7. 团队设置页接存储用量 API（已实现）

- [x] `GET /v1/teams/{team_id}/storage/usage`
- [x] 团队设置弹窗「存储用量」卡片（总量 / 配额条 / 分项）

---

## P2 — 运维/安全

### 8. SSH 密码登录关闭（已完成）

生产机已执行：`sshd_config` 关闭 `PasswordAuthentication`，依赖 Vault SSH CA 证书，并配置 `AuthorizedPrincipalsFile`。客户端无需改动。

---

## 服务端已就绪的接口

（同前，略）

---

## 参考文档

- `docs/tech/COMMAND-AUDIT.md`
- `docs/tech/SERVER-SIDE-AUDIT.md`
- 下一版（概念稿 v4 对齐）：`docs/product/NEXT-VERSION-TODO.md`

---

## 编译验证

```powershell
$env:CARGO_BUILD_JOBS='1'
$env:CARGO_INCREMENTAL='0'
cargo check -p mistterm --lib
cargo test -p mistterm --lib cmd_audit -- --test-threads=1
cargo test -p mistterm --test client_todo_acceptance_test -- --test-threads=1
cargo test -p mistterm --test cmd_audit_test -- --test-threads=1
cargo test -p mistterm --test team_api_test team_api_cmd_audit -- --test-threads=1
```
