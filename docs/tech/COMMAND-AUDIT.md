# 命令审计

---

## 1. 背景与目标

### 1.1 要解决的问题

| 场景 | 说明 |
|------|------|
| 危险命令拦截 | 用户在终端输入 `rm -rf /`、`dd if=/dev/zero of=/dev/sda` 等破坏性命令时，**在发送到 SSH 通道之前**拦截 |
| 敏感操作确认 | 读取 `/etc/shadow`、`/proc/*/environ` 等敏感路径时，要求二次确认 |
| 团队级策略管控 | 管理员可为团队配置哪些命令需要 block / confirm / alert |
| 审计与合规 | 所有被拦截或确认执行的命令记录到团队审计，供管理员查询 |

### 1.2 系统架构概览

```
用户输入命令
    │
    ▼
┌─────────────────────────────────────────────┐
│ MistTerm 客户端                              │
│                                              │
│  ┌──────────────┐    ┌───────────────────┐  │
│  │ Terminal      │    │ CmdAuditEngine    │  │
│  │ send_command()│───►│ (本地匹配引擎)    │  │
│  │              │    │                   │  │
│  │   ↕ 拦截/放行 │◄───│ 1. 自定义规则     │  │
│  └──────┬───────┘    │ 2. 内置模式库     │  │
│         │            │ 3. 团队策略       │  │
│         │            └───────┬───────────┘  │
│         │                    │ sync          │
│         │            ┌───────▼───────────┐  │
│         │            │ 本地缓存           │  │
│         │            │ - policy          │  │
│         │            │ - rules           │  │
│         │            │ - builtin_stats   │  │
│         │            └───────┬───────────┘  │
│         │                    │               │
└─────────┼────────────────────┼───────────────┘
          │                    │ HTTPS
          ▼                    ▼
    SSH Server          ┌──────────────┐
                        │ 团队服务端    │
                        │ - 策略 CRUD   │
                        │ - 规则 CRUD   │
                        │ - 告警记录    │
                        │ - 内置模式库  │
                        │ - 测试接口    │
                        └──────────────┘
```

### 1.3 设计原则

- **零延迟优先**：命令匹配在本地执行，不走网络请求。策略和规则通过定时同步缓存到本地。
- **不阻断个人用户**：未登录团队或团队未启用命令审计时，`send_command()` 行为与现在完全一致。
- **最小侵入**：改动集中在 `send_command()` 入口 + 新增 `CmdAuditEngine` 模块；不改变终端渲染逻辑。
- **可审计**：每次拦截/确认/告警都通过现有审计通道上报团队。

---

## 2. 服务端 API 参考（已实现）

> 以下接口已全部实现并部署到 `https://api.mistlab.dev`。客户端直接对接。

### 2.1 同步配置（客户端核心接口）

```
GET /v1/teams/{team_id}/command-audit/sync
Authorization: Bearer <access_token>
```

**响应 `200`：**
```json
{
  "enabled": true,
  "policy": {
    "team_id": "team_xxx",
    "enabled": true,
    "dangerous_action": "block",
    "sensitive_action": "confirm",
    "unknown_action": "allow",
    "alert_webhook": "",
    "confirm_timeout": 300
  },
  "rules": [
    {
      "id": "rule_xxx",
      "team_id": "team_xxx",
      "name": "Block drop database",
      "pattern": "(?i)(drop|truncate)\\s+(database|table)",
      "match_type": "regex",
      "scope": "command",
      "action": "block",
      "description": "Prevent database destruction",
      "priority": 50,
      "enabled": true,
      "created_by": "u_xxx",
      "created_at": "2026-05-28T00:00:00Z",
      "updated_at": "2026-05-28T00:00:00Z"
    }
  ],
  "builtin_stats": {
    "dangerous": 180,
    "safe": 74,
    "read_dangerous": 71,
    "read_sensitive": 11,
    "read_safe": 92
  },
  "sync_interval_sec": 300
}
```

> **同步策略**：客户端应按 `sync_interval_sec`（默认 300 秒）定时拉取。首次拉取或策略变更时刷新本地引擎。

### 2.2 检测命令（在线 API，可选）

```
POST /v1/teams/{team_id}/command-audit/check
Authorization: Bearer <access_token>
```

**请求体：**
```json
{
  "command": "rm -rf /",
  "scope": "command"
}
```

**响应 `200`：**
```json
{
  "allowed": false,
  "action": "block",
  "matches": [
    {
      "rule_id": "rm_recursive_root",
      "source": "builtin",
      "level": "dangerous",
      "message": "Recursive force delete from root",
      "action": "block"
    }
  ]
}
```

> **注意**：此接口用于 UI 管理面板的「测试沙盒」功能，**不用于实时终端拦截**（实时拦截走本地引擎）。

### 2.3 策略管理

| 方法 | 路径 | 权限 | 说明 |
|------|------|------|------|
| GET | `/v1/teams/{team_id}/command-audit/policy` | viewer+ | 获取策略 |
| PUT | `/v1/teams/{team_id}/command-audit/policy` | admin | 更新策略 |

### 2.4 自定义规则管理

| 方法 | 路径 | 权限 | 说明 |
|------|------|------|------|
| GET | `/v1/teams/{team_id}/command-audit/rules` | viewer+ | 列出规则 |
| POST | `/v1/teams/{team_id}/command-audit/rules` | admin | 创建规则 |
| PUT | `/v1/teams/{team_id}/command-audit/rules/{rule_id}` | admin | 更新规则 |
| DELETE | `/v1/teams/{team_id}/command-audit/rules/{rule_id}` | admin | 删除规则 |

### 2.5 告警记录

| 方法 | 路径 | 权限 | 说明 |
|------|------|------|------|
| POST | `/v1/teams/{team_id}/command-audit/alerts` | viewer+ | 上报告警 |
| GET | `/v1/teams/{team_id}/command-audit/alerts` | viewer+ | 查询告警 |

**上报请求体：**
```json
{
  "command": "rm -rf /",
  "matched_rule": "rm_recursive_root",
  "match_level": "dangerous",
  "action_taken": "blocked"
}
```

### 2.6 内置模式查询

| 方法 | 路径 | 权限 | 说明 |
|------|------|------|------|
| GET | `/v1/command-audit/patterns?category=dangerous` | 登录用户 | 查看内置模式统计 |

### 2.7 测试接口（公开，不需团队上下文）

```
POST /v1/command-audit/test
Authorization: Bearer <access_token>
```

**请求体：**
```json
{
  "command": "rm -rf /",
  "scope": "command",
  "policy": { "enabled": true, "dangerous_action": "block", ... },
  "rules": [ ... ]
}
```

> 此接口接受完整的策略和规则，用于 UI 管理面板的命令测试，不依赖团队上下文。

### 2.8 权限矩阵

| 操作 | viewer | editor | admin |
|------|--------|--------|-------|
| 查看策略 | ✅ | ✅ | ✅ |
| 同步配置 | ✅ | ✅ | ✅ |
| 检测命令 | ✅ | ✅ | ✅ |
| 上报告警 | ✅ | ✅ | ✅ |
| 查看告警 | ✅ | ✅ | ✅ |
| 修改策略 | ❌ | ❌ | ✅ |
| 创建/编辑/删除规则 | ❌ | ❌ | ✅ |

---

## 3. 客户端实现需求

### 3.1 新增模块：`CmdAuditEngine`

**文件位置**：`src/core/cmd_audit.rs`（新文件）

```
src/core/
├── mod.rs                  # 新增 pub mod cmd_audit;
├── cmd_audit.rs            # 新文件：本地命令审计引擎
├── audit.rs                # 现有：审计日志（不变）
├── team/
│   ├── client.rs           # 现有：扩展新增 API 调用方法
│   ├── service.rs          # 现有：扩展新增同步任务
│   └── ...
```

#### 3.1.1 数据结构

```rust
/// 命令审计动作
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CmdAuditAction {
    Block,
    Confirm,
    Alert,
    Allow,
}

/// 命令匹配规则匹配类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchType {
    Regex,
    Prefix,
    Contains,
    Exact,
}

/// 团队命令审计策略
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CmdAuditPolicy {
    pub team_id: String,
    pub enabled: bool,
    pub dangerous_action: CmdAuditAction,
    pub sensitive_action: CmdAuditAction,
    pub unknown_action: CmdAuditAction,
    pub confirm_timeout: u64,  // 秒
}

/// 自定义规则
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CmdAuditRule {
    pub id: String,
    pub name: String,
    pub pattern: String,
    pub match_type: MatchType,
    pub scope: String,          // "command" | "filepath" | "both"
    pub action: CmdAuditAction,
    pub description: String,
    pub priority: i32,
    pub enabled: bool,
    // 预编译正则（内部使用，不序列化）
    #[serde(skip)]
    pub compiled_regex: Option<Regex>,
}

/// 匹配结果
#[derive(Debug, Clone)]
pub struct CmdAuditMatch {
    pub rule_id: String,
    pub source: String,     // "builtin" | "custom"
    pub level: String,      // "dangerous" | "sensitive" | "safe" | "custom"
    pub message: String,
    pub action: CmdAuditAction,
}

/// 检查结果
#[derive(Debug, Clone)]
pub struct CmdAuditResult {
    pub allowed: bool,
    pub action: CmdAuditAction,
    pub matches: Vec<CmdAuditMatch>,
}
```

#### 3.1.2 引擎核心逻辑

```rust
pub struct CmdAuditEngine {
    enabled: bool,
    policy: Option<CmdAuditPolicy>,
    rules: Vec<CmdAuditRule>,           // 按 priority 排序
    builtin_patterns: BuiltinPatterns,  // 从 sync 拿到的内置模式（编译到本地）
    last_sync: Option<Instant>,
    sync_interval: Duration,            // 默认 300s
}
```

**检查流程**（在 `send_command()` 中调用）：

```
check(command) -> CmdAuditResult
    │
    ├── 未启用 / 无策略 → allowed: true
    │
    ├── 1. 自定义 allow 规则（白名单）
    │   └── 匹配 → allowed: true
    │
    ├── 2. 自定义 block/confirm/alert 规则
    │   └── 匹配 → 返回对应 action
    │
    ├── 3. 内置 dangerous 模式
    │   └── 匹配 → 按策略 dangerous_action
    │
    ├── 4. 内置 safe 模式
    │   └── 匹配 → allowed: true, action: safe
    │
    └── 5. 未知命令 → 按策略 unknown_action
```

**匹配实现**：

| match_type | 实现 |
|------------|------|
| `regex` | 预编译 `Regex::new()`，调用 `is_match()` |
| `prefix` | `command.starts_with(&rule.pattern)` |
| `contains` | `command.contains(&rule.pattern)` |
| `exact` | `command == rule.pattern` |

> **内置模式**：服务端已将所有 hardstop-patterns 编译到二进制中（180 dangerous + 74 safe 等）。客户端不需要内置这些模式——客户端在 sync 时只拿 `builtin_stats`（统计数据），实际的 dangerous/safe 匹配通过以下两种方式之一实现：
>
> **方案 A（推荐）**：客户端也内嵌 hardstop-patterns JSON，编译时 `include_str!` 加载。与服务端同步的只是策略和自定义规则。
> **方案 B**：客户端不内置模式，仅依赖自定义规则 + 在线 check API。但此方案有延迟，不适合实时拦截。
>
> **建议采用方案 A**。服务端可提供一个打包好的 JSON 供客户端 include。

#### 3.1.3 内置模式文件

服务端 `internal/cmdaudit/patterns/` 目录下的 JSON 文件：

```
patterns/
├── bash-dangerous.json    # 180 条危险命令模式
├── bash-safe.json         # 74 条安全命令模式
├── read-dangerous.json    # 71 条危险读取路径
├── read-sensitive.json    # 11 条敏感读取路径
└── read-safe.json         # 92 条安全读取路径
```

客户端可通过 API 下载或在构建时复制这些文件。推荐构建时复制，避免网络依赖。

### 3.2 改动点：`Terminal::send_command()`

**当前代码**（`src/ui/terminal.rs` ~L1990）：

```rust
pub fn send_command(&mut self, command: &str) {
    if !self.connected { return; }
    // ... 直接发送到 PTY
}
```

**改造后**：

```rust
pub fn send_command(&mut self, command: &str) -> CommandSendResult {
    if !self.connected { return CommandSendResult::NotConnected; }
    
    // 命令审计检查
    if let Some(result) = self.cmd_audit_check(command) {
        match result.action {
            CmdAuditAction::Block => {
                // 记录审计事件
                self.audit_cmd_blocked(command, &result);
                return CommandSendResult::Blocked(result);
            }
            CmdAuditAction::Confirm => {
                // 返回需要确认的状态，UI 层弹确认框
                return CommandSendResult::NeedsConfirm(command.to_string(), result);
            }
            CmdAuditAction::Alert => {
                // 允许执行但记录告警
                self.audit_cmd_alert(command, &result);
                // 继续发送
            }
            CmdAuditAction::Allow | _ => {
                // 正常发送
            }
        }
    }
    
    // ... 原有发送逻辑 ...
    CommandSendResult::Sent
}

pub enum CommandSendResult {
    Sent,
    NotConnected,
    Blocked(CmdAuditResult),
    NeedsConfirm(String, CmdAuditResult),
}
```

### 3.3 UI 改动

#### 3.3.1 命令拦截弹窗

当 `send_command()` 返回 `NeedsConfirm` 时：

```
┌─────────────────────────────────────────────┐
│ ⚠️ 命令需要确认                              │
├─────────────────────────────────────────────┤
│                                              │
│ 检测到敏感操作：                              │
│                                              │
│ 命令: rm -rf /var/log/*.log                  │
│                                              │
│ 匹配规则: [builtin] rm_recursive_delete      │
│ 等级: dangerous                              │
│ 说明: 递归强制删除可能造成数据丢失             │
│                                              │
│ ┌──────────┐  ┌──────────┐                   │
│ │  确认执行  │  │   取消    │                   │
│ └──────────┘  └──────────┘                   │
└─────────────────────────────────────────────┘
```

**行为**：
- 点击「确认执行」→ 发送命令到 PTY + 记录审计 `action_taken: "confirmed"`
- 点击「取消」→ 不发送 + 记录审计 `action_taken: "blocked"`
- `confirm_timeout` 秒后自动取消（可选实现）

#### 3.3.2 命令被阻止通知

当 `send_command()` 返回 `Blocked` 时：

在终端底部或右上角显示一个 toast 通知（非阻塞，自动消失）：

```
🚫 命令已拦截: rm -rf / — 危险操作已被团队策略阻止
                    [×]
```

#### 3.3.3 片段执行拦截

**改动位置**：`src/ui/app.rs` 中片段执行的调用处。

当用户执行一个片段时，片段的 `command` 字段同样需要通过 `CmdAuditEngine::check()` 检查。如果被拦截，显示同样的弹窗。

### 3.4 策略同步

#### 3.4.1 扩展 `TeamClient`

在 `src/core/team/client.rs` 中新增：

```rust
impl TeamClient {
    pub async fn cmd_audit_sync(&self, team_id: &str) -> Result<CmdAuditSyncResponse, TeamApiError> {
        self.get(&format!("/v1/teams/{}/command-audit/sync", team_id)).await
    }
    
    pub async fn cmd_audit_report_alert(&self, team_id: &str, req: &CmdAuditAlertRequest) -> Result<(), TeamApiError> {
        self.post(&format!("/v1/teams/{}/command-audit/alerts", team_id), req).await
    }
}
```

#### 3.4.2 扩展 `TeamService`

在 `src/core/team/service.rs` 中新增：

```rust
enum TeamJob {
    // ... 现有 variants
    CmdAuditSync {
        api_base: String,
        team_id: String,
    },
}

enum TeamAsyncResult {
    // ... 现有 variants
    CmdAuditSyncOk {
        policy: CmdAuditPolicy,
        rules: Vec<CmdAuditRule>,
    },
}
```

**同步时机**：
1. 登录成功后立即同步
2. 切换团队后同步
3. 定时同步（间隔取 `sync_interval_sec`，默认 300 秒）
4. 网络恢复后同步

#### 3.4.3 本地缓存

策略和规则缓存到 `~/.config/mistterm/cmd_audit_cache.json`（加密存储），离线时使用缓存。

```json
{
  "team_id": "team_xxx",
  "policy": { ... },
  "rules": [ ... ],
  "synced_at": "2026-05-28T12:00:00Z",
  "sync_interval_sec": 300
}
```

### 3.5 审计事件集成

#### 3.5.1 新增审计 category 和 action

在 `src/core/audit.rs` 中扩展：

```rust
pub enum AuditCategory {
    // ... 现有
    CommandAudit,  // 新增
}

// 新增 action（通过 detail 字段区分）
// command_audit.blocked     — 命令被阻止
// command_audit.confirmed   — 命令经确认后执行
// command_audit.alert       — 命令触发告警但仍执行
```

#### 3.5.2 告警上报

每次命令拦截/确认/告警后：

1. **本地审计**：通过现有 `AuditLogger` 记录 JSONL
2. **团队上报**：调用 `POST /v1/teams/{team_id}/command-audit/alerts` 上报

上报应走现有的异步审计通道（批量队列），不要在终端主线程中同步 HTTP 请求。

### 3.6 性能要求

| 指标 | 要求 |
|------|------|
| 单条命令检查延迟 | < 1ms（本地匹配，不涉网） |
| 规则数量上限 | 支持至少 500 条自定义规则 |
| 内置模式库 | 428 条（180+74+71+11+92），全量正则预编译 |
| 内存增量 | < 5MB（预编译正则 + 策略缓存） |
| 同步流量 | 每次 sync 约 10-50KB JSON |

---

## 4. 文件清单与改动范围

### 4.1 新增文件

| 文件 | 说明 |
|------|------|
| `src/core/cmd_audit.rs` | 命令审计引擎（策略管理、规则匹配、内置模式） |
| `src/core/cmd_audit/builtin.rs` | 内置模式加载与匹配（可选，如果模式文件较大） |
| `assets/cmd-audit-patterns/*.json` | 从服务端复制的内置模式 JSON（可选） |

### 4.2 改动文件

| 文件 | 改动内容 |
|------|----------|
| `src/core/mod.rs` | 新增 `pub mod cmd_audit;` |
| `src/core/team/client.rs` | 新增 `cmd_audit_sync()`、`cmd_audit_report_alert()` API 方法 |
| `src/core/team/service.rs` | 新增 `CmdAuditSync` 异步任务 |
| `src/core/team/models.rs` | 新增 `CmdAuditPolicy`、`CmdAuditRule` 等数据结构 |
| `src/core/audit.rs` | 新增 `CommandAudit` category |
| `src/ui/terminal.rs` | `send_command()` 增加审计检查拦截逻辑 |
| `src/ui/app.rs` | 片段执行增加审计检查；处理 `NeedsConfirm` / `Blocked` 事件 |
| `src/ui/preferences_dialog.rs` | 偏好设置中增加命令审计开关（可选） |
| `Cargo.toml` | 新增 `regex` crate（如尚未依赖） |

### 4.3 不改动的文件

| 文件 | 说明 |
|------|------|
| `src/ui/terminal.rs` 渲染逻辑 | 终端渲染与审计无关，不改 |
| `src/ssh/` | SSH 通道层不感知审计 |
| `src/core/session_logger.rs` | 会话回放与审计分离，不改 |

---

## 5. 联调验收场景

| # | 场景 | 通过标准 |
|---|------|----------|
| CA-1 | 策略同步 | 登录团队后，客户端成功拉取策略和规则，本地缓存更新 |
| CA-2 | 定时同步 | 每 5 分钟自动同步一次，策略变更后下次同步生效 |
| CA-3 | 危险命令拦截 | 输入 `rm -rf /`，终端不发送，显示 toast「🚫 命令已拦截」 |
| CA-4 | 敏感命令确认 | 输入匹配 confirm 策略的命令，弹确认框，点确认后执行 |
| CA-5 | 告警放行 | 输入匹配 alert 策略的命令，正常执行，终端底部显示告警通知 |
| CA-6 | 安全命令放行 | 输入 `ls -la`、`git status` 等安全命令，直接执行无拦截 |
| CA-7 | 自定义规则 | 管理员在 Web 端新增规则 `(?i)drop\\s+table` → block，客户端同步后 `drop table users` 被拦截 |
| CA-8 | 白名单规则 | 自定义 allow 规则优先级最高，匹配后跳过所有其他检查 |
| CA-9 | 片段执行拦截 | 执行一个内容为 `rm -rf /tmp/*` 的片段，同样触发拦截 |
| CA-10 | 审计上报 | 被拦截/确认的命令在团队审计日志中可查（`command_audit.blocked` / `command_audit.confirmed`） |
| CA-11 | 告警记录 | 被拦截的命令在 `/v1/teams/{id}/command-audit/alerts` 中可查 |
| CA-12 | 离线使用 | 断网时使用本地缓存的策略和规则，命令拦截正常工作 |
| CA-13 | 未登录无回归 | 未登录团队时，所有命令正常发送，无任何拦截 |
| CA-14 | 未启用无回归 | 团队已登录但 `enabled: false` 时，所有命令正常发送 |
| CA-15 | 多行命令 | 粘贴多行命令时，每行独立检查，只拦截匹配的行 |
| CA-16 | 性能 | 本地命令检查延迟 < 1ms，不感知卡顿 |

---

## 6. 内置模式库详情

服务端使用的 hardstop-patterns 数据（客户端需内嵌或同步）：

### 6.1 Dangerous（180 条）

典型模式示例：

| ID | 模式 | 说明 |
|----|------|------|
| `rm_recursive_root` | `^rm\s+(-[a-zA-Z]*f[a-zA-Z]*\s+)?-[a-zA-Z]*r[a-zA-Z]*\s+/` | 递归删除根目录 |
| `dd_destroy_disk` | `dd\s+if=/dev/zero\s+of=/dev/` | dd 写零到磁盘 |
| `mkfs_on_disk` | `mkfs\.(ext[234]\|xfs\|btrfs\|ntfs)\s+/dev/` | 格式化磁盘 |
| `chmod_777_root` | `chmod\s+(-R\s+)?777\s+/` | 递归 777 根目录 |
| `fork_bomb` | `:()\{\s*:\|:&\s*\}` | Fork 炸弹 |
| `iptables_flush` | `iptables\s+-F` | 清空防火墙规则 |

### 6.2 Safe（74 条）

典型模式：`ls`、`cd`、`cat`（普通文件）、`git status`、`echo`、`pwd`、`whoami` 等。

### 6.3 Read Dangerous（71 条）

典型路径：`/etc/shadow`、`/etc/gshadow`、`/root/.ssh/`、`/proc/*/environ` 等。

### 6.4 Read Sensitive（11 条）

典型路径：`/etc/passwd`、`/etc/hosts`、`/proc/cpuinfo` 等。

### 6.5 Read Safe（92 条）

典型路径：`/var/log/syslog`、`/tmp/`、`/home/` 等。

> **客户端获取模式库的方式**：
> - 在构建时从服务端仓库 `internal/cmdaudit/patterns/` 复制 JSON 文件
> - 或通过 API `GET /v1/command-audit/patterns?category=dangerous` 在运行时下载
> - **推荐**：构建时复制，避免运行时网络依赖

---

## 7. 实现优先级建议

### P0 — 核心功能（必须）

1. `CmdAuditEngine` 核心引擎（策略 + 规则匹配）
2. `send_command()` 拦截改造
3. 策略同步（`/sync` 接口）
4. 命令拦截弹窗（confirm / block toast）
5. 内置 dangerous 模式库内嵌

### P1 — 增强功能（应该）

6. 审计事件上报（本地 JSONL + 团队 alert API）
7. 片段执行拦截
8. 离线缓存（`cmd_audit_cache.json`）
9. 内置 safe 模式库

### P2 — 锦上添花（可选）

10. 偏好设置中的命令审计开关
11. 内置 read-dangerous / read-sensitive 模式
12. 命令审计统计面板（最近拦截、最常触发规则等）
13. 自定义规则本地管理 UI（当前在 Web admin 管理）

---

## 8. 与现有模块的关系

| 现有模块 | 关系 |
|----------|------|
| `AuditLogger` | 命令审计事件作为新 category 写入现有 JSONL；上报走现有 HTTP sink |
| `TeamClient` | 新增命令审计 API 调用方法 |
| `TeamService` | 新增 `CmdAuditSync` 异步任务 |
| `FragmentManager` | 片段执行前经过命令审计检查 |
| `Terminal::send_command()` | 增加拦截逻辑，返回值从 `()` 改为 `CommandSendResult` |
| `session_logger` | 不变，终端回放与命令审计分离 |

---

## 9. 风险与注意事项

| 风险 | 缓解 |
|------|------|
| 正则性能 | 预编译所有正则；规则按 priority 排序，匹配即返回；内置模式在启动时一次性编译 |
| 阻塞 UI | 所有匹配在本地执行（<1ms），不涉及网络请求；同步在后台线程 |
| 命令误拦 | safe 模式优先级高于 dangerous；自定义 allow 规则最高优先级 |
| 多行粘贴 | 每行独立检查，只拦截危险行，安全行正常发送 |
| 中文/特殊字符 | 正则使用 Rust `regex` crate，支持 UTF-8 |
| 缓存过期 | `sync_interval_sec` 可配置；缓存带时间戳，过期后降级为「不拦截」（宁可漏过不可误拦） |

---

**文档维护**：接口契约已冻结。服务端 API 不会再做 breaking change。新增字段向前兼容。
