# MistTerm 客户端待办（2026-08-21）

> 服务端所有功能已交付并部署生产，以下为客户端侧需要完成的工作。
> 涉及 Rust 代码改动，需本地 cargo 环境编译验证。

**2026-08-23 更新**：P0 §1–§4、P1 §6–§7 已实现并通过自动化测试（`client_todo_acceptance_test`、`cmd_audit` 单测、团队 API 探针）；§5 实机 SSH/agent 仍建议人工抽测；§8 生产 sshd 运维操作。

---

## P0 — 命令审计客户端闭环

### 背景

服务端已实现「服务器侧强制命令审计」：在被审计的远程服务器上部署透明 agent，命令在服务器本地执行前通过 `MIST_AUDIT` 标记行回调服务端判定/记录。客户端通过解析 PTY 输出中的 `MIST_AUDIT\t{JSON}` 行获取判定结果。

**信任边界变化**：命令审计的判定权威从「客户端本地」移到「服务端 + 服务器侧 agent」。客户端本地审计（`CmdAuditEngine`）保留但降级为「本地快捷提示」，不作为安全承诺。

### 1. 服务器侧审计结果展示（已实现，待实机验证）

**现状**：
- `src/core/cmd_audit.rs` — `ServerAuditProbe` 解析 `MIST_AUDIT\t{JSON}` 标记行
- `src/ui/terminal.rs` — PTY feed 调用 probe，事件经 `take_server_audit_events` 交给宿主
- `src/ui/app.rs` — `poll_server_audit_from_tabs` / `handle_server_audit_event` 展示 block/confirm/alert

**待验证**：
- [x] `cargo build --release` / `cargo check --lib` 编译通过
- [x] `cargo test` cmd_audit 单测不回归
- [ ] 实机：连接已装 agent 主机，执行 `rm -rf /` → block toast
- [ ] 实机：`ls` 正常放行
- [ ] 大数据块 partial prefix 不丢数据（已有单测，建议 SSH 压测）

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
| 连接已装 agent，执行 `rm -rf /` | block toast | 待测 |
| 执行 `ls` | 正常放行 | 待测 |
| `cat /etc/shadow` | 服务器 confirm 弹窗 | 待测 |
| 确认后 | `MIST_AUDIT_APPROVE\t{token}` + 原命令 | 待测 |
| agent 停止后连接 | 黄色降级横幅 | 待测 |
| agent 恢复 | 横幅消失 + 绿色 toast | 待测 |
| 本地审计 | toast 标注「本地检查」 | 待测 |

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

### 8. SSH 密码登录关闭（运维，非客户端）

生产机 `sshd_config` 操作，需运维在确认全员 CA 证书可用后执行。客户端无代码改动。

---

## 服务端已就绪的接口

（同前，略）

---

## 参考文档

- `docs/tech/COMMAND-AUDIT.md`
- `docs/tech/SERVER-SIDE-AUDIT.md`

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
