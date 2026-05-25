# 团队平台集成指南

MistTerm 客户端与 MistTeam 服务端（\`api.mistlab.dev\`）的集成方案。

---

## 1. 登录后同步团队配置

OAuth 登录成功拿到 \`access_token\` 后，调用同步接口拉取用户的全部团队配置。

### 请求

\`\`\`
GET /v1/team/sync
Authorization: Bearer <access_token>
\`\`\`

### 响应

\`\`\`json
{
  "teams": [
    {
      "team_id": "t_xxx",
      "team_name": "研发组",
      "role": "editor",
      "vault_config": {
        "id": "tvc_xxx",
        "team_id": "t_xxx",
        "address": "https://vault.example.com:8200",
        "kv_mount": "secret",
        "auth_type": "approle",
        "namespace": ""
      },
      "credential": {
        "role": "editor",
        "approle_role_id": "role-editor-xxx",
        "approle_secret_id": "secret-editor-xxx"
      },
      "servers": [
        {
          "id": "srv_xxx",
          "name": "prod-api-1",
          "host": "10.0.0.1",
          "port": 22,
          "username": "deploy",
          "tags": ["backend", "prod"],
          "vault_credential_path": "",
          "sort_order": 0
        },
        {
          "name": "db-master",
          "host": "10.0.0.2",
          "port": 22,
          "username": "admin",
          "tags": ["db"],
          "vault_credential_path": "secret/data/ssh/db-master",
          "sort_order": 1
        }
      ]
    }
  ]
}
\`\`\`

### 字段说明

| 字段 | 说明 |
|------|------|
| \`role\` | 用户在团队中的角色：\`admin\` / \`editor\` / \`viewer\` |
| \`vault_config\` | 团队 Vault 连接配置，\`null\` 表示未配置 |
| \`credential\` | 当前角色对应的 Vault 凭证，\`null\` 表示该角色无凭证 |
| \`servers\` | 根据角色+用户权限过滤后的可访问服务器列表 |

### 客户端处理

\`\`\`
登录成功
  ↓
GET /v1/team/sync
  ↓
遍历 teams[]:
  ├── vault_config + credential → 自动填充 VaultSettings
  ├── servers[] → 显示在「团队服务器」列表（快速连接）
  └── role → 决定 UI 显示（viewer 只读等）
\`\`\`

---

## 2. Vault 自动配置

### 当前 VaultSettings 结构（\`src/core/vault/mod.rs\`）

\`\`\`rust
pub struct VaultSettings {
    pub enabled: bool,
    pub address: String,
    pub namespace: String,
    pub default_mount: String,  // "secret"
    pub auth: VaultAuthSettings,
    pub tls_skip_verify: bool,
}

pub enum VaultAuthSettings {
    None,
    Token(String),
    AppRole { role_id: String, secret_id: String },
}
\`\`\`

### 映射关系

| 服务端字段 | 客户端字段 |
|-----------|-----------|
| \`vault_config.address\` | \`VaultSettings.address\` |
| \`vault_config.kv_mount\` | \`VaultSettings.default_mount\` |
| \`vault_config.namespace\` | \`VaultSettings.namespace\` |
| \`vault_config.auth_type == "token"\` | \`VaultAuthSettings::Token(credential.vault_token)\` |
| \`vault_config.auth_type == "approle"\` | \`VaultAuthSettings::AppRole { role_id, secret_id }\` |

### 实现建议

在 \`src/core/team/client.rs\` 的 \`TeamClient\` 中加方法：

\`\`\`rust
pub async fn sync_team_config(&self) -> Result<TeamSyncResponse> {
    let resp = self.client
        .get(&format!("{}/v1/team/sync", self.base_url))
        .bearer_auth(&self.token)
        .send()
        .await?;
    // 解析并返回
}

pub fn apply_vault_config(&self, sync: &TeamSyncResponse) {
    for team in &sync.teams {
        if let Some(vc) = &team.vault_config {
            let auth = match vc.auth_type.as_str() {
                "token" => VaultAuthSettings::Token(
                    team.credential.as_ref().map(|c| c.vault_token.clone()).unwrap_or_default()
                ),
                "approle" => VaultAuthSettings::AppRole {
                    role_id: team.credential.as_ref().map(|c| c.approle_role_id.clone()).unwrap_or_default(),
                    secret_id: team.credential.as_ref().map(|c| c.approle_secret_id.clone()).unwrap_or_default(),
                },
                _ => VaultAuthSettings::None,
            };
            // 更新 VaultSettings 并持久化
        }
    }
}
\`\`\`

---

## 3. 团队服务器列表

### 数据结构建议

\`\`\`rust
pub struct TeamServer {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub tags: Vec<String>,
    pub vault_credential_path: String,
    pub sort_order: i32,
}
\`\`\`

### UI 展示

- 在主机列表/连接管理中新增「团队服务器」分组
- 按 \`sort_order\` 排序，按 \`tags\` 分组
- 点击直接发起 SSH 连接
- 如果 \`vault_credential_path\` 非空，连接前从 Vault 读取 SSH 私钥/密码

### 连接流程

\`\`\`
用户点击团队服务器
  ↓
检查 vault_credential_path 是否非空
  ├── 是 → Vault KV 读取凭证 → SSH 连接
  └── 否 → 使用本地 SSH key / 密码 → SSH 连接
\`\`\`

---

## 4. 审计事件上报

用户的 shell 操作、文件传输等行为上报到服务端，管理员可在后台搜索审计。

### 上报接口

\`\`\`
POST /v1/audit/events
Authorization: Bearer <access_token>
Content-Type: application/json
\`\`\`

### 请求体

\`\`\`json
{
  "events": [
    {
      "team_id": "t_xxx",
      "category": "shell",
      "action": "exec",
      "outcome": "success",
      "session_id": "sess_abc123",
      "host": "10.0.0.1",
      "resource": "/home/deploy",
      "detail": "{\"command\": \"ls -la /var/log/app.log\"}"
    },
    {
      "category": "file",
      "action": "scp",
      "outcome": "success",
      "host": "10.0.0.1",
      "detail": "{\"direction\": \"upload\", \"local\": \"/tmp/config.yaml\", \"remote\": \"/etc/app/config.yaml\", \"bytes\": 2048}"
    }
  ]
}
\`\`\`

### 响应

\`\`\`json
{
  "accepted": 2,
  "duplicate": 0
}
\`\`\`

### 事件类型

| category | action | 说明 |
|----------|--------|------|
| \`shell\` | \`exec\` | 执行命令 |
| \`shell\` | \`connect\` | 建立 SSH 会话 |
| \`shell\` | \`disconnect\` | 断开 SSH 会话 |
| \`file\` | \`scp\` | SCP 文件传输 |
| \`file\` | \`sftp\` | SFTP 操作 |
| \`auth\` | \`login\` | 用户登录 |
| \`auth\` | \`sudo\` | sudo 提权 |
| \`session\` | \`start\` | 会话开始（录像） |
| \`session\` | \`end\` | 会话结束 |
| \`config\` | \`vault_read\` | 读取 Vault 密钥 |
| \`config\` | \`vault_write\` | 写入 Vault 密钥 |

### 客户端实现

\`\`\`rust
pub struct AuditEvent {
    pub event_id: String,
    pub user_id: String,
    pub team_id: String,
    pub timestamp: DateTime<Utc>,
    pub category: String,
    pub action: String,
    pub outcome: String,
    pub session_id: String,
    pub host: String,
    pub resource: String,
    pub detail: String,  // JSON string
}
\`\`\`

**上报策略：**
- 本地缓存，每 30 秒或积累 50 条批量上报
- 网络断开时持久化到本地文件，下次启动重试
- \`event_id\` 格式：\`evt_\` + 时间戳 + 随机数，用于去重
- \`user_id\` 和 \`timestamp\` 可省略，服务端会自动补充

---

## 5. 认证相关

### JWT Token

| Token | 有效期 | 存储 |
|-------|--------|------|
| \`access_token\` | 30 分钟 | \`~/.config/mistterm/team_tokens.json\`（AES，密钥为 \`device_key\`） |
| \`refresh_token\` | 7 天 | 同上 |

> 客户端实现见 \`src/core/team/auth.rs\`（\`TeamTokenStore\`）与 \`docs/tech/SECURITY.md\`。Vault AppRole/Token 仍存系统钥匙串；团队 OAuth token **不**走 Keychain。

### Token 刷新

\`\`\`
POST /v1/auth/refresh
Authorization: Bearer <refresh_token>
\`\`\`

\`\`\`json
{
  "access_token": "eyJ...",
  "refresh_token": "eyJ..."
}
\`\`\`

客户端应在 \`access_token\` 过期前（建议提前 5 分钟）自动刷新。

### API Base URL

\`\`\`
生产: https://api.mistlab.dev
\`\`\`

---

## 6. 错误处理

所有 API 返回统一错误格式：

\`\`\`json
{
  "error": "error message"
}
\`\`\`

| HTTP 状态码 | 含义 | 客户端处理 |
|------------|------|-----------|
| 401 | Token 过期/无效 | 刷新 Token 或重新登录 |
| 403 | 权限不足 | 提示用户无权限 |
| 404 | 资源不存在 | 提示或忽略 |
| 429 | 请求频率过高 | 退避重试 |
| 500 | 服务端错误 | 提示稍后重试 |
