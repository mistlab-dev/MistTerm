# MistTerm 客户端待办（2026-08-21）

> 服务端所有功能已交付并部署生产，以下为客户端侧需要完成的工作。
> 涉及 Rust 代码改动，需本地 cargo 环境编译验证。

---

## P0 — 命令审计客户端闭环

服务端已实现服务器侧强制审计（agent 包裹脚本 + forcecommand），客户端需要配合展示结果。

### 1. 解析服务器侧判定结果

**现状**：`terminal.rs` 已写了 `pending_server_audit_block` 检测逻辑（检测 `[mist-agent]` + `拦截` 标记），`app.rs` 已有 `server_audit_toasts` 收集和 `notify_warn` 展示。**未编译验证**。

**要做**：
- `src/ui/terminal.rs`：PTY feed 处解析 `[mist-agent]` 前缀的输出，提取 action（block/confirm/alert）+ 命令内容
- `src/ui/app.rs`：`poll_connect_audit_from_tabs` 收集各 tab 的审计结果，展示 toast
- 确认 `take_pending_server_audit_block()` 在拦截场景下正确触发一次

**涉及文件**：
- `src/ui/terminal.rs`
- `src/ui/app.rs`
- `src/ui/app_notifications.rs`（复用通知展示）

### 2. 本地审计文案定位调整

**现状**：客户端本地审计（`CmdAuditEngine`）的 toast/弹窗定位可能与服务器侧结果冲突。

**要做**：
- 本地审计文案加"（本地提示）"后缀，明确区分来源
- 服务器侧拦截结果加"（服务器侧）"前缀
- 定位：本地提示不遮挡服务器侧结果

**涉及文件**：
- `src/core/cmd_audit.rs`
- `src/ui/app_notifications.rs`

### 3. Agent 不可用降级提示

**现状**：服务器侧审计依赖远端 agent，agent 不可用时客户端无提示。

**要做**：
- 连接服务器时检查 agent 状态（GET `/v1/teams/:team_id/command-audit/agents`）
- agent 不在线/未安装：终端顶部显示黄色提示条「⚠️ 服务器侧审计不可用，命令将仅做本地检查」
- agent 恢复后自动消失

**涉及文件**：
- `src/core/team/client.rs`（查询 agent 状态）
- `src/ui/team_ui.rs`（提示条展示）

### 4. 团队/主机设置页 Agent 状态展示

**要做**：
- 团队设置页显示已注册 agent 列表（主机名、状态、最后心跳时间）
- 支持标记 agent 为 disabled（调 PUT `/v1/teams/:team_id/command-audit/agents/:agent_id`）

**涉及文件**：
- `src/ui/team_ui.rs`
- `src/ui/team_fragment_dialog.rs`

### 5. 实机联调

- 服务端部署生产，agent 安装到测试服务器
- 客户端连接 → 执行危险命令 → 验证拦截 toast
- 执行普通命令 → 验证正常放行 + 记录
- agent 停止 → 验证降级提示
- agent 恢复 → 验证提示消失

---

## P1 — 片段体验补齐

### 6. 异常退出清理编辑锁

**现状**：用户编辑片段时客户端持有编辑锁（`fragments/:id/lock`），异常退出（crash/网络断开）后锁残留，其他用户无法编辑。

**要做**：
- 客户端启动时检查是否有残留锁（调 GET `/v1/fragments/:id/lock`）
- 如果是自己之前持有的锁，自动释放
- 编辑界面添加心跳机制（每 30s 续锁），超时自动释放

**涉及文件**：
- `src/core/team/client.rs`（锁 API 调用）
- `src/ui/team_fragment_dialog.rs`（编辑界面）

### 7. 团队设置页接存储用量 API

**现状**：服务端已有 `GET /v1/teams/:team_id/storage/usage`（`2ff80e0`），返回片段+录制+文档+版本的用量明细。

**要做**：
- 团队设置页新增「存储用量」区域
- 展示总用量 / 配额进度条
- 按类型（片段/录制/文档/版本）分项显示

**涉及文件**：
- `src/core/team/client.rs`（API 调用）
- `src/ui/team_ui.rs`（UI 展示）

---

## P2 — 运维/安全

### 8. SSH 密码登录关闭

**现状**：生产机已配置 Vault SSH CA 证书认证，但密码登录仍开启。

**要做**：
- 确认所有用户已配置 CA 证书登录
- 生产机 `/etc/ssh/sshd_config` 设置 `PasswordAuthentication no`
- 重启 sshd

**前置条件**：需先在所有客户端验证证书登录正常。

---

## 服务端已就绪的接口（供客户端调用）

| 接口 | 方法 | 说明 | 认证 |
|------|------|------|------|
| `/v1/server/command-audit/check` | POST | 命令判定（agent 调） | Agent Key |
| `/v1/server/command-audit/record` | POST | 执行记录（agent 调） | Agent Key |
| `/v1/server/command-audit/enroll` | POST | Agent 注册（一次性令牌） | Enroll Token |
| `/v1/teams/:id/command-audit/agents` | GET | Agent 列表 | JWT |
| `/v1/teams/:id/command-audit/logs` | GET | 审计日志 | JWT |
| `/v1/teams/:id/command-audit/approvals` | GET | 审批列表 | JWT |
| `/v1/teams/:id/command-audit/approvals/:id/approve` | PUT | 批准 | JWT (Admin) |
| `/v1/teams/:id/command-audit/approvals/:id/reject` | PUT | 拒绝 | JWT (Admin) |
| `/v1/teams/:id/storage/usage` | GET | 存储用量 | JWT |

---

## 参考文档

- `docs/tech/COMMAND-AUDIT.md` — 客户端本地审计（现行为）
- `docs/tech/SERVER-SIDE-AUDIT.md` — 服务器侧审计对接说明
- `/root/work/NEXT-PHASE.md` — 阶段规划（服务端视角）

---

## 编译验证

所有改动完成后：
```bash
cargo build --release
cargo test
```

服务端验收脚本（bash+curl，不依赖 Rust）：
```bash
bash tests/server_audit_acceptance.sh
```
