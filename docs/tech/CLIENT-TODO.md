# MistTerm 客户端待办（2026-08-21）

> 服务端所有功能已交付并部署生产，以下为客户端侧需要完成的工作。
> 涉及 Rust 代码改动，需本地 cargo 环境编译验证。

---

## P0 — 命令审计客户端闭环

### 背景

服务端已实现「服务器侧强制命令审计」：在被审计的远程服务器上部署透明 agent，命令在服务器本地执行前通过 `MIST_AUDIT` 标记行回调服务端判定/记录。客户端通过解析 PTY 输出中的 `MIST_AUDIT\t{JSON}` 行获取判定结果。

**信任边界变化**：命令审计的判定权威从「客户端本地」移到「服务端 + 服务器侧 agent」。客户端本地审计（`CmdAuditEngine`）保留但降级为「本地快捷提示」，不作为安全承诺。

### 1. 服务器侧审计结果展示（已写代码，待编译验证）

**现状**：
- `src/core/cmd_audit.rs:560` — `ServerAuditProbe` 已实现，从 PTY 字节流中解析 `MIST_AUDIT\t{action}\t{rule}\t{message}\t{command}\t{token}` 标记行，剥离后返回 `ServerAuditEvent`
- `src/ui/terminal.rs:1926` — PTY feed 处已调用 `server_audit_probe.feed(&data)`，解析出的事件存入 `pending_server_audit: Vec<ServerAuditEvent>`
- `src/ui/app.rs:1055` — `poll_server_audit_from_tabs()` 已实现，每帧轮询各 tab 收集事件
- `src/ui/app.rs:1069` — `handle_server_audit_event()` 已实现：
  - `Block` → `notify_error` 展示红色错误 toast（"服务器策略：该命令被禁止: {cmd} — {msg}"）
  - `Alert` → `notify_warn` 展示黄色警告 toast（"服务器策略：该命令已上报团队审计: {cmd} — {msg}"）
  - `Confirm` → 弹出 `CmdAuditConfirmState` 确认框，source 标记为 `CmdAuditSource::Server`，携带 `approve_token`
  - `Allow` → 静默放行

**待验证**：
- [ ] `cargo build --release` 编译通过
- [ ] `cargo test` 现有测试不回归
- [ ] 实机测试：连接已安装 agent 的服务器，执行 `rm -rf /` → 应弹出 block toast
- [ ] 实机测试：执行 `ls` → 应正常放行（agent 回传 allow）
- [ ] 确认 `ServerAuditProbe::feed()` 的 partial prefix 缓存逻辑在大数据块下不丢数据

**涉及文件**：
- `src/core/cmd_audit.rs`（ServerAuditProbe + ServerAuditEvent）
- `src/ui/terminal.rs`（PTY feed + pending_server_audit）
- `src/ui/app.rs`（poll_server_audit_from_tabs + handle_server_audit_event）

### 2. 本地审计与服务器审计的文案区分

**现状**：
- `CmdAuditSource::Local` — 客户端本地引擎判定（`send_audited_command_at` 中 `CmdAuditEngine.check()`）
- `CmdAuditSource::Server` — 服务器侧 agent 判定（通过 `MIST_AUDIT` 标记行）
- 两者共用 `CmdAuditConfirmState` + `CmdAuditResult`，但 toast 文案相同，用户无法区分来源

**要做**：
- [ ] 本地审计 toast 加来源标记：
  - Block: "🔒 本地提示：该命令被禁止: {cmd}" （`cmd_audit.rs` 中 `CmdAuditAction::Block` 分支）
  - Confirm: 确认弹窗标题加"（本地检查）"
- [ ] 服务器审计 toast 已有"服务器策略"前缀（`app.rs:1069`），保持不变
- [ ] `CmdAuditConfirmState` 的确认弹窗（`app_workspace_confirm_modals.rs:215`）：
  - `source == Server` 时标题显示"⚠️ 服务器策略：确认执行"
  - `source == Local` 时标题显示"🔒 本地检查：确认执行"
  - 确认按钮文案：Server → "放行并发送"，Local → "确认发送"

**涉及文件**：
- `src/ui/app.rs:970-1050`（send_audited_command_at 中 local 分支文案）
- `src/ui/app_workspace_confirm_modals.rs:215`（from_server 判断）

### 3. Agent 不可用降级提示

**现状**：服务器侧审计依赖远端 agent 在线。agent 未安装或停止时，客户端终端顶部无任何提示，用户不知道审计已降级。

**要做**：
- [ ] 连接建立后（SSH 握手成功），异步查询 agent 状态：
  ```
  GET /v1/teams/{team_id}/command-audit/agents
  Header: Authorization: Bearer {access_token}
  Response: { "agents": [{ "id": "...", "host": "...", "status": "active", "last_seen_at": "..." }] }
  ```
  - 筛选匹配当前连接主机名（`agent.host == session.host`）的 agent
  - 判断 `last_seen_at` 距当前时间 > 5 分钟 → 视为离线
  - 无匹配 agent → 视为未安装
- [ ] 降级提示展示：
  - 终端 tab 顶部显示黄色横幅："⚠️ 服务器侧审计不可用，命令将仅做本地检查"
  - 横幅可手动关闭（X 按钮），关闭后不再显示直到下次连接
  - 如果 agent 恢复（定期轮询，每 60s 一次），横幅自动消失并显示绿色 toast "✅ 服务器侧审计已恢复"
- [ ] 轮询逻辑挂载到 `poll_team_service` 或独立 timer，不阻塞主线程

**涉及文件**：
- `src/core/team/client.rs`（新增 `get_cmd_audit_agents(team_id, token)` 方法）
- `src/ui/team_ui.rs`（agent 状态横幅 UI）
- `src/ui/terminal.rs`（添加 `agent_degraded_banner: bool` 字段）

### 4. 团队设置页 Agent 状态展示

**要做**：
- [ ] 团队设置页新增「命令审计 Agent」区域（在 RBAC 设置下方）
- [ ] Agent 列表表格：主机名 | 状态（🟢在线/🔴离线） | 最后心跳 | 操作
- [ ] 操作列：Admin 可点击"禁用"按钮（调用 `PUT /v1/teams/{team_id}/command-audit/agents/{agent_id}`，body: `{ "enabled": false }`）
- [ ] 禁用后 agent 状态显示为 "⏸ 已禁用"，再点"启用"恢复
- [ ] 空列表时显示引导文案："尚未安装 Agent。请联系管理员运行安装脚本。"

**涉及文件**：
- `src/ui/team_ui.rs`（新增 agent 区域）
- `src/core/team/client.rs`（新增/修改 agent API 调用）

### 5. 实机联调验证清单

| 场景 | 预期行为 | 验证方法 |
|------|----------|----------|
| 连接已装 agent 的服务器，执行 `rm -rf /` | 终端显示 block toast | 目视 |
| 执行 `ls` | 正常放行 + agent 日志有记录 | curl 查日志 |
| 执行 `cat /etc/shadow` | confirm 弹窗（服务器侧来源） | 目视 |
| 确认弹窗点"放行" | 发送 `MIST_AUDIT_APPROVE\t{token}` + 原命令 | 抓包 |
| agent 停止后连接 | 黄色降级横幅 | 目视 |
| agent 停止后执行命令 | 无 toast（agent 不可达，跳过判定） | 日志 |
| agent 恢复 | 横幅消失 + 绿色 toast | 目视 |
| 本地审计触发（服务端未启用策略） | toast 标注"本地检查" | 目视 |

---

## P1 — 片段体验补齐

### 6. 异常退出清理编辑锁

**现状**：
- 片段编辑前调 `POST /v1/fragments/{id}/lock`（`client.rs:466`），编辑完调 `POST /v1/fragments/{id}/unlock`（`client.rs:478`）
- `service_blocking.rs:219` — `lock_team_fragment_blocking` 成功后更新本地缓存 `locked_by` + `locked_at`
- **问题**：crash / 网络断开 / 强制关闭窗口时，锁未释放，其他用户编辑时被阻塞

**要做**：
- [ ] **启动时检查残留锁**：
  - 客户端启动 → 加载本地缓存的片段列表 → 筛选 `locked_by == 当前用户ID` 的片段
  - 对每个残留锁片段调 `GET /v1/fragments/{id}` 检查服务端 `locked_by` 是否仍是自己
  - 如果是 → 自动调 `POST /v1/fragments/{id}/unlock` 释放
  - 如果已被别人锁定 → 不操作
- [ ] **编辑中心跳续锁**：
  - 进入片段编辑界面时启动心跳 timer（每 30s）
  - 调 `POST /v1/fragments/{id}/lock`（幂等，已持有则刷新 `locked_at`）
  - 心跳失败（网络错误 / 403 被别人抢锁）→ 弹 toast "⚠️ 编辑锁已丢失，保存可能冲突"
  - 关闭编辑界面时调 `unlock` 并停止心跳
- [ ] **异常退出恢复**：
  - `Ctrl+C` / `kill` / crash → 服务端锁有 TTL（默认 5min），超时自动释放
  - 如果服务端支持 `DELETE /v1/fragments/{id}/lock`（管理员强制解锁），团队设置页可加"强制解锁"按钮

**涉及文件**：
- `src/core/team/service_blocking.rs:219-255`（lock/unlock 逻辑）
- `src/ui/team_fragment_dialog.rs`（编辑界面心跳）
- `src/core/team/service.rs`（启动时残留锁检查）

### 7. 团队设置页接存储用量 API

**现状**：
- 服务端已有 `GET /v1/teams/{team_id}/storage/usage`（commit `2ff80e0`）
- Response 示例：
  ```json
  {
    "total_bytes": 12345678,
    "fragments": { "count": 42, "bytes": 5000000 },
    "recordings": { "count": 5, "bytes": 3000000 },
    "documents": { "count": 10, "bytes": 4000000 },
    "versions": { "count": 120, "bytes": 345678 }
  }
  ```

**要做**：
- [ ] `src/core/team/client.rs` 新增 `get_storage_usage(team_id, token)` 方法
- [ ] 团队设置页新增「存储用量」卡片：
  - 顶部：总用量 / 配额进度条（如 12.3 MB / 1 GB）
  - 下方：按类型分项显示（片段 📄、录制 🎬、文档 📝、版本 📚），每项显示数量 + 占用
  - 进度条颜色：<70% 绿色，70-90% 黄色，>90% 红色
- [ ] 缓存：切换团队时加载一次，编辑片段后刷新

**涉及文件**：
- `src/core/team/client.rs`（API 调用）
- `src/ui/team_ui.rs`（存储用量 UI）

---

## P2 — 运维/安全

### 8. SSH 密码登录关闭

**现状**：生产机已配置 Vault SSH CA 证书认证，但 `PasswordAuthentication` 仍为 `yes`。

**要做**：
- [ ] 确认所有团队成员已配置 CA 证书（`ssh -G root@85.137.247.166 | grep identity` 检查）
- [ ] 生产机 `/etc/ssh/sshd_config` 追加：
  ```
  PasswordAuthentication no
  ChallengeResponseAuthentication no
  ```
- [ ] `systemctl restart sshd`
- [ ] 验证：`ssh root@85.137.247.166` 用密码应被拒绝，用证书应正常

---

## 服务端已就绪的接口

| 接口 | 方法 | 说明 | 认证 | 备注 |
|------|------|------|------|------|
| `/v1/server/command-audit/check` | POST | 命令判定 | Agent Key | agent 包裹脚本调 |
| `/v1/server/command-audit/record` | POST | 执行记录 | Agent Key | agent 包裹脚本调 |
| `/v1/server/command-audit/enroll` | POST | Agent 注册 | Enroll Token | 一次性令牌 |
| `/v1/teams/:id/command-audit/agents` | GET | Agent 列表 | JWT | 客户端查状态用 |
| `/v1/teams/:id/command-audit/agents/:aid` | PUT | 更新 Agent | JWT (Admin) | 启用/禁用 |
| `/v1/teams/:id/command-audit/logs` | GET | 审计日志 | JWT | 分页+搜索 |
| `/v1/teams/:id/command-audit/logs/export` | GET | 审计导出 | JWT | CSV |
| `/v1/teams/:id/command-audit/approvals` | GET | 审批列表 | JWT | 状态筛选 |
| `/v1/teams/:id/command-audit/approvals/:id/approve` | PUT | 批准 | JWT (Admin) | |
| `/v1/teams/:id/command-audit/approvals/:id/reject` | PUT | 拒绝 | JWT (Admin) | |
| `/v1/teams/:id/storage/usage` | GET | 存储用量 | JWT | 片段+录制+文档+版本 |
| `/v1/teams/:id/overview` | GET | 团队概览 | JWT | 统计卡片用 |

---

## 参考文档

- `docs/tech/COMMAND-AUDIT.md` — 客户端本地审计（现行为，降级为本地提示）
- `docs/tech/SERVER-SIDE-AUDIT.md` — 服务器侧审计对接说明（客户端定位、信任边界、标记行格式）
- `/root/work/NEXT-PHASE.md` — 四阶段规划（服务端视角）
- `tests/server_audit_acceptance.sh` — 服务端端到端验收脚本（14/14 PASS）

---

## 编译验证

```bash
# 客户端编译
cd /root/work/mist
cargo build --release
cargo test

# 服务端验收（bash+curl，不依赖 Rust）
cd /root/work/mist-team-server
bash tests/server_audit_acceptance.sh
```
