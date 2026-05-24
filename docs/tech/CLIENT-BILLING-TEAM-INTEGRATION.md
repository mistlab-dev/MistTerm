# MistTerm 客户端付费与团队对接改造方案

> **版本**: 1.0  
> **更新**: 2026-05-22  
> **读者**: MistTerm 客户端开发  
> **前置**: `docs/tech/TEAM-PLATFORM-DEV-PLAN.md`（团队功能设计）

---

## 1. 改造目标

让 MistTerm 客户端连接 mist-team-server，实现：

1. **用户认证**：登录、token 管理、OAuth 支持
2. **团队片段同步**：从服务端拉取/推送团队片段
3. **审计上报**：已有 HTTP sink，只需配置服务端 URL
4. **订阅/试用期**：显示试用期状态、到期提示升级

---

## 2. 改造模块清单

| 序号 | 新建/改造 | 文件 | 说明 |
|------|----------|------|------|
| 1 | 新建 | `src/core/auth.rs` | 登录、token、OAuth |
| 2 | 新建 | `src/core/team_client.rs` | 团队 API 客户端 |
| 3 | 改造 | `src/core/cloud_sync.rs` | 接入真实 API |
| 4 | 改造 | `src/core/fragment.rs` | 区分个人/团队片段 |
| 5 | 改造 | `src/core/audit.rs` | 团队审计上报开关 |
| 6 | 新建 | `src/core/billing.rs` | 订阅状态检查 |
| 7 | 改造 | `src/ui/cloud_sync_panel.rs` | 登录 UI + 同步状态 |
| 8 | 改造 | `src/ui/fragment_library.rs` | 团队片段展示 |
| 9 | 改造 | `src/ui/app.rs` | 试用期提示 UI |
| 10 | 改造 | `src/core/app_settings.rs` | 新增 auth/billing 配置 |

---

## 3. 新模块设计

### 3.1 认证模块 (`auth.rs`)

```rust
/// 用户认证状态
pub struct AuthState {
    /// 是否已登录
    pub logged_in: bool,
    /// 当前用户信息
    pub user: Option<UserInfo>,
    /// Access token（存密钥链，不落盘）
    access_token: Option<String>,
    /// Refresh token
    refresh_token: Option<String>,
    /// Token 过期时间
    token_expires_at: Option<i64>,
    /// 当前团队
    pub current_team: Option<TeamInfo>,
    /// 可用团队列表
    pub teams: Vec<TeamInfo>,
}

pub struct UserInfo {
    pub id: String,
    pub email: String,
    pub display_name: String,
}

pub struct TeamInfo {
    pub id: String,
    pub name: String,
    pub role: String, // viewer, editor, admin
}

/// 认证客户端
pub struct AuthClient {
    api_base: String,
    http: reqwest::blocking::Client,
}

impl AuthClient {
    /// 登录（邮箱/用户名 + 密码）
    pub fn login(&self, email_or_username: &str, password: &str) -> Result<AuthState, AuthError>;
    
    /// OAuth 登录（返回授权 URL，用户在浏览器完成）
    pub fn oauth_url(&self, provider: &str) -> Result<String, AuthError>;
    
    /// OAuth 回调处理（接收 code，换取 token）
    pub fn oauth_callback(&self, provider: &str, code: &str) -> Result<AuthState, AuthError>;
    
    /// 刷新 token
    pub fn refresh_token(&self, refresh_token: &str) -> Result<(String, String, i64), AuthError>;
    
    /// 获取用户信息 + 团队列表
    pub fn get_me(&self, access_token: &str) -> Result<(UserInfo, Vec<TeamInfo>), AuthError>;
    
    /// 切换当前团队
    pub fn switch_team(&mut state: &mut AuthState, team_id: &str);
}
```

**API 调用：**

| 操作 | 端点 | 请求体 |
|------|------|--------|
| 登录 | `POST /v1/auth/login` | `{email_or_username, password}` |
| 注册 | `POST /v1/auth/register` | `{email, username, password, display_name}` |
| OAuth URL | `GET /v1/oauth/{provider}` | — |
| OAuth 回调 | `GET /v1/oauth/{provider}/callback?code=...` | — |
| Token 刷新 | `POST /v1/auth/refresh` | `{refresh_token}` |
| 用户信息 | `GET /v1/auth/me` | Bearer token |

**Token 存储：**
- `access_token` 存系统密钥链（macOS Keychain、Windows Credential Manager、Linux secret-service）
- `refresh_token` 同上
- 不落盘到 JSON 文件

---

### 3.2 团队客户端 (`team_client.rs`)

```rust
/// 团队 API 客户端
pub struct TeamClient {
    api_base: String,
    http: reqwest::blocking::Client,
}

impl TeamClient {
    /// 片段同步（增量拉取）
    pub fn sync_fragments(
        &self,
        team_id: &str,
        cursor: Option<&str>,
        limit: usize,
        token: &str,
    ) -> Result<SyncResult, ApiError>;
    
    /// 创建团队片段
    pub fn create_fragment(
        &self,
        team_id: &str,
        fragment: &FragmentStats,
        token: &str,
    ) -> Result<FragmentStats, ApiError>;
    
    /// 更新团队片段
    pub fn update_fragment(
        &self,
        fragment_id: &str,
        fragment: &FragmentStats,
        revision: u64,
        token: &str,
    ) -> Result<FragmentStats, ApiError>;
    
    /// 删除团队片段
    pub fn delete_fragment(&self, fragment_id: &str, token: &str) -> Result<(), ApiError>;
    
    /// 上报审计事件
    pub fn upload_audit_events(
        &self,
        events: &[AuditEvent],
        token: &str,
    ) -> Result<UploadResult, ApiError>;
    
    /// 获取订阅信息
    pub fn get_plan_info(&self, token: &str) -> Result<PlanInfo, ApiError>;
}

pub struct SyncResult {
    pub cursor: String,
    pub fragments: Vec<FragmentStats>,
    pub deleted_ids: Vec<String>,
}

pub struct PlanInfo {
    pub plan: String,        // free, pro, team
    pub status: String,      // active, trialing, trial_expired
    pub is_trial: bool,
    pub trial_days_left: i32,
    pub limits: PlanLimits,
}

pub struct PlanLimits {
    pub max_teams: i32,
    pub max_fragments: i32,
    pub max_members: i32,
    pub audit_enabled: bool,
}
```

**API 调用：**

| 操作 | 端点 | 说明 |
|------|------|------|
| 片段同步 | `POST /v1/teams/{team_id}/fragments:sync` | 增量拉取 |
| 创建片段 | `POST /v1/teams/{team_id}/fragments` | 新建 |
| 更新片段 | `PUT /v1/fragments/{id}` | 带 revision |
| 删除片段 | `DELETE /v1/fragments/{id}` | 软删 |
| 审计上报 | `POST /v1/audit/events` | 批量 |
| 订阅信息 | `GET /billing/plan` | 试用期检查 |

---

### 3.3 订阅/试用期模块 (`billing.rs`)

```rust
/// 订阅状态（全局缓存，登录后更新）
pub struct BillingState {
    pub plan: String,
    pub status: String,
    pub is_trial: bool,
    pub trial_days_left: i32,
    pub limits: PlanLimits,
    pub last_checked: i64,
}

impl BillingState {
    /// 从服务端获取
    pub fn fetch(client: &TeamClient, token: &str) -> Result<Self, ApiError>;
    
    /// 是否需要显示升级提示
    pub fn should_show_upgrade_prompt(&self) -> bool;
    
    /// 生成提示文案
    pub fn prompt_text(&self) -> String;
}
```

---

## 4. 现有模块改造

### 4.1 `cloud_sync.rs` 改造

**现状**：只有本地导出/导入包，无真实 API。

**改造**：

```rust
pub struct CloudSyncSettings {
    // 现有字段保留...
    
    /// 新增：团队服务地址
    pub api_base: String,
    /// 新增：是否已登录团队
    pub team_logged_in: bool,
    /// 新增：当前团队 ID
    pub current_team_id: Option<String>,
    /// 新增：自动同步间隔（分钟）
    pub auto_sync_interval: u32,
}

impl CloudSyncPanel {
    /// 新增：登录入口 UI
    fn show_login_ui(&mut self, ui: &mut egui::Ui, auth: &mut AuthState);
    
    /// 新增：团队选择 UI
    fn show_team_selector(&mut self, ui: &mut egui::Ui, auth: &mut AuthState);
    
    /// 新增：同步状态 + 错误提示
    fn show_sync_status(&mut self, ui: &mut egui::Ui);
    
    /// 新增：手动同步按钮
    fn sync_now(&mut self, deps: &mut CloudSyncDeps);
}
```

---

### 4.2 `fragment.rs` 改造

**现状**：`FragmentManager` 只管理本地片段。

**改造**：区分个人片段和团队片段。

```rust
pub struct FragmentManager {
    /// 个人片段（本地 fragments.json）
    personal: Vec<FragmentStats>,
    /// 团队片段（缓存，来源服务端）
    team: Vec<FragmentStats>,
    /// 团队片段来源团队 ID
    team_source_id: Option<String>,
    /// ID 映射
    id_map: HashMap<String, usize>,
    /// 片段来源标记（personal / team）
    source_map: HashMap<String, FragmentSource>,
}

pub enum FragmentSource {
    Personal,
    Team { team_id: String, revision: u64 },
}

impl FragmentManager {
    /// 获取所有片段（个人 + 团队合并）
    pub fn get_all(&self) -> Vec<&FragmentStats>;
    
    /// 获取个人片段
    pub fn get_personal(&self) -> &[FragmentStats];
    
    /// 获取团队片段
    pub fn get_team(&self) -> &[FragmentStats];
    
    /// 添加个人片段
    pub fn add_personal(&mut self, ...);
    
    /// 更新团队片段（需要权限检查）
    pub fn update_team_fragment(&mut self, id: &str, ...) -> Result<(), String>;
    
    /// 合并同步结果
    pub fn merge_team_sync(&mut self, result: &SyncResult);
}
```

---

### 4.3 `audit.rs` 改造

**现状**：HTTP sink 已有，只需配置。

**改造**：

```rust
pub struct AuditSettings {
    // 现有字段保留...
    
    /// 新增：是否上报到团队服务端
    pub team_upload_enabled: bool,
    /// 新增：团队服务端 URL（从 auth 模块获取）
    pub team_upload_url: Option<String>,
}

impl AuditLogger {
    /// 新增：团队上报开关检查
    fn should_upload_to_team(&self) -> bool;
    
    /// 新增：团队上报时使用 auth token
    fn upload_to_team(&self, events: &[AuditEvent], token: &str);
}
```

**流程**：
- 登录后，`team_upload_url` 自动设置为 `{api_base}/v1/audit/events`
- `http.bearer_token` 自动设置为 access token
- 用户可在设置中关闭团队上报

---

### 4.4 `app_settings.rs` 改造

```rust
pub struct AppSettings {
    pub vault: VaultSettings,
    pub audit: AuditSettings,
    
    /// 新增：认证配置
    pub auth: AuthSettings,
    /// 新增：订阅配置
    pub billing: BillingSettings,
}

pub struct AuthSettings {
    /// 团队服务地址
    pub api_base: String,
    /// 是否启用团队功能
    pub team_enabled: bool,
    /// 上次登录用户（展示用，token 存密钥链）
    pub last_user_hint: Option<String>,
    /// OAuth 配置（可选）
    pub oauth providers: Vec<String>,
}

pub struct BillingSettings {
    /// 是否显示试用期提示
    pub show_trial_prompt: bool,
    /// 上次提示时间
    pub last_prompt_time: Option<i64>,
}
```

---

## 5. UI 改造

### 5.1 登录/注册 UI

在 `CloudSyncPanel` 或新建 `AuthPanel` 中添加：

```
┌─────────────────────────────────────┐
│ 🔐 登录 MistTerm 团队服务            │
├─────────────────────────────────────┤
│ 邮箱/用户名: [___________________]   │
│ 密码:       [___________________]   │
│                                     │
│ [登录]  [注册]                       │
│                                     │
│ ─── 或使用 OAuth ───                 │
│ [Google] [GitHub]                   │
│                                     │
│ ☑ 记住登录                          │
└─────────────────────────────────────┘
```

登录成功后：
- 显示用户名 + 当前团队
- 团队选择下拉框
- 同步状态（已同步 / 待同步 / 错误）

---

### 5.2 片段库 UI 改造

在 `FragmentLibrary` 中添加来源区分：

```
┌─────────────────────────────────────┐
│ 📦 命令片段库                        │
├─────────────────────────────────────┤
│ [个人] [团队] [全部]                 │
│                                     │
│ 搜索: [___________________] [🔍]    │
│                                     │
│ ┌─ 系统监控 (个人) ────────────────┐│
│ │ 磁盘使用    df -h    12次 85%成功 ││
│ │ 内存使用    free -h   8次 100%成功││
│ └─────────────────────────────────┘│
│                                     │
│ ┌─ Docker (团队: 运维组) ──────────┐│
│ │ 查看容器    docker ps -a   👁️ 只读││
│ │ 容器日志    docker logs -f  ✏️ 可编辑││
│ └─────────────────────────────────┘│
│                                     │
│ [新建个人片段] [新建团队片段]         │
│                                     │
│ [同步团队片段] ↻ 上次同步: 10分钟前   │
└─────────────────────────────────────┘
```

**权限标识**：
- 团队片段根据 role 显示图标：
  - `viewer`: 👁️ 只读
  - `editor`: ✏️ 可编辑
  - `admin`: 🔧 可删除

---

### 5.3 试用期提示 UI

在主窗口顶部显示提示条：

```
┌──────────────────────────────────────────────────┐
│ 🎉 Pro 试用期剩余 15 天 | [升级付费] [稍后提醒]    │
└──────────────────────────────────────────────────┘
```

或到期时：

```
┌──────────────────────────────────────────────────┐
│ ⚠️ 试用期已结束，已降级为 Free | [升级 Pro]       │
└──────────────────────────────────────────────────┘
```

---

## 6. 数据流

### 6.1 登录流程

```
用户输入邮箱/密码
    ↓
AuthClient.login()
    ↓
POST /v1/auth/login
    ↓
返回 {access_token, refresh_token, expires_in}
    ↓
存 token 到密钥链
    ↓
AuthClient.get_me()
    ↓
GET /v1/auth/me
    ↓
返回 {user, teams}
    ↓
更新 AuthState
    ↓
TeamClient.get_plan_info()
    ↓
GET /billing/plan
    ↓
返回 {plan, is_trial, trial_days_left}
    ↓
更新 BillingState
    ↓
UI 显示用户名 + 团队 + 试用期状态
```

---

### 6.2 片段同步流程

```
用户点击「同步团队片段」
    ↓
TeamClient.sync_fragments(team_id, cursor, limit)
    ↓
POST /v1/teams/{team_id}/fragments:sync
    ↓
返回 {cursor, fragments, deleted_ids}
    ↓
FragmentManager.merge_team_sync(result)
    ↓
更新本地缓存
    ↓
保存 cursor 到 CloudSyncSettings.last_sync_cursor
    ↓
UI 刷新片段列表
```

---

### 6.3 审计上报流程

```
AuditLogger 收集事件
    ↓
批量队列达到阈值或定时器触发
    ↓
检查 AuditSettings.team_upload_enabled
    ↓
如果启用：
    TeamClient.upload_audit_events(events, token)
    ↓
    POST /v1/audit/events
    ↓
    返回 {accepted, duplicate}
    ↓
    清空已上报队列
如果禁用：
    只写本地 JSONL
```

---

## 7. 配置文件示例

`settings.json` 新字段：

```json
{
  "vault": { ... },
  "audit": {
    "enabled": true,
    "team_upload_enabled": true,
    "http": {
      "enabled": false
    }
  },
  "auth": {
    "api_base": "https://api.mistlab.dev",
    "team_enabled": true,
    "last_user_hint": "zhang@example.com"
  },
  "billing": {
    "show_trial_prompt": true
  }
}
```

---

## 8. 错误处理

| 错误 | HTTP | UI 处理 |
|------|------|---------|
| 未登录 | 401 | 弹登录窗口 |
| 无权限 | 403 | 提示「需要 editor/admin 权限」 |
| 片段冲突 | 409 | 弹冲突解决窗口 |
| 网络错误 | — | 显示错误信息 + 重试按钮 |
| 试用期到期 | — | 顶部提示条 + 升级按钮 |

---

## 9. 实现优先级

| 优先级 | 模块 | 说明 |
|--------|------|------|
| P0 | auth.rs | 必须先有登录才能调其他 API |
| P0 | team_client.rs | 片段同步核心 |
| P1 | fragment.rs 改造 | 区分个人/团队 |
| P1 | cloud_sync_panel.rs UI | 登录 + 同步入口 |
| P2 | audit.rs 改造 | 团队上报开关 |
| P2 | billing.rs | 试用期提示 |
| P3 | fragment_library.rs UI | 团队片段展示优化 |

---

## 10. 与服务端 API 对应表

| 客户端方法 | 服务端端点 | 状态 |
|------------|-----------|------|
| `AuthClient.login` | `POST /v1/auth/login` | ✅ 已实现 |
| `AuthClient.register` | `POST /v1/auth/register` | ✅ 已实现 |
| `AuthClient.oauth_url` | `GET /v1/oauth/{provider}` | ✅ 已实现 |
| `AuthClient.oauth_callback` | `GET /v1/oauth/{provider}/callback` | ✅ 已实现 |
| `AuthClient.refresh_token` | `POST /v1/auth/refresh` | ✅ 已实现 |
| `AuthClient.get_me` | `GET /v1/auth/me` | ✅ 已实现 |
| `TeamClient.sync_fragments` | `POST /v1/teams/{id}/fragments:sync` | ✅ 已实现 |
| `TeamClient.create_fragment` | `POST /v1/teams/{id}/fragments` | ✅ 已实现 |
| `TeamClient.update_fragment` | `PUT /v1/fragments/{id}` | ✅ 已实现 |
| `TeamClient.delete_fragment` | `DELETE /v1/fragments/{id}` | ✅ 已实现 |
| `TeamClient.upload_audit_events` | `POST /v1/audit/events` | ✅ 已实现 |
| `TeamClient.get_plan_info` | `GET /billing/plan` | ✅ 已实现 |
| `BillingClient.checkout` | `POST /billing/checkout` | ✅ 已实现 |

---

## 11. 测试账号

服务端已提供测试账号，用于联调：

| 角色 | 权限 | 用途 |
|------|------|------|
| viewer | 只读片段 | 测试同步、只读 UI |
| editor | 读+写片段 | 测试创建/更新 |
| admin | 读+写+删 | 测试删除、权限 UI |

---

## 12. 风险与依赖

| 风险 | 缓解 |
|------|------|
| OAuth 需在浏览器完成 | 弹窗口提示用户去浏览器，完成后粘贴 code 或轮询 |
| Token 过期处理 | 自动 refresh；401 时提示重新登录 |
| 团队片段与个人冲突 | 用 `source` 字段区分；UI 标签清晰 |
| 试用期提示频繁 | 设置中可关闭；最多每天提示一次 |

---

**文档维护**：客户端实现完成后，更新本文「状态」列。