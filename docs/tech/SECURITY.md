# 安全与审计

## 本地配置加密（统一策略）

所有落盘的用户配置与敏感数据均使用 **`device_key`** 作为 AES-256-GCM 密钥，文件外层格式为 **`mistterm-aes-v1`**（见 `src/security/encrypted_file.rs`）。

| 文件 | 路径 | 说明 |
|------|------|------|
| 应用设置 | `~/.config/mistterm/settings.json` | 主题、团队 API、AI 配置等 |
| 命令历史 | `command_history.json` | 执行过的命令记录 |
| 粘贴历史 | `paste_history.json` | 最近成功送入 PTY 的粘贴。尽力过滤、非完备：私钥块、常见令牌前缀、疑似密钥的环境变量赋值、密码/口令提示不入库；只扫描入库前缀 |
| 个人片段 | `fragments.json` | 片段库 |
| 会话列表 | `sessions.json` | SSH 会话（密码在加密信封内明文 JSON，不再逐字段 AES） |
| 凭证库 | `credentials.json` | 服务器账号等（同上） |
| 团队状态 | `team_state.json` | 当前团队/用户 |
| 团队片段缓存 | `team_fragments_cache.json` | 云端片段本地副本 |
| 团队 Token | `team_tokens.json` | OAuth access/refresh |

**`device_key` 来源**（`src/security/device_key.rs`）：

- macOS：`IOPlatformUUID`
- 其他平台：`OS:USER:HOSTNAME` 指纹经 SHA-256 派生

换机、重装或指纹变化后，旧的 **`device_key` 文件无法解密**。跨设备要靠下面的账号同步或同步口令导出包，不能靠把 `device_key` 拷走。

首次启动或升级时，仍为**明文 JSON** 的旧文件会在加载时自动迁移为 `mistterm-aes-v1`。凭证库旧版 HKDF+盐（v2）与纯数组格式亦会自动迁移。

## 其他机密存储

| 方式 | 位置 | 说明 |
|------|------|------|
| HashiCorp Vault | 偏好设置配置 | KV v2；引用仅存 mount/path/field |
| 会话/凭证 Vault 引用 | 对话框 | 不落盘明文，仅存引用 |
| Vault 认证 | OS 钥匙串（可选） | Token 或 AppRole；**与团队 OAuth 无关** |

团队 OAuth **不使用**系统钥匙串；历史 `MistTerm-Team` 钥匙串条目会在首次读取时迁入 `team_tokens.json` 后删除。

## 审计日志（与终端回放分离）

- **目录**：`~/.config/mistterm/audit/audit-YYYY-MM-DD.jsonl`（本地写入缓冲；**应用内不提供查看 UI**）
- **团队上报**：登录团队后自动 `POST /v1/audit/events`（50 条/30s）；离线队列 `pending-team-events.jsonl`
- **会话回放**：`~/.config/mistterm/logs/`（`session_logger`，可含完整终端输出）

审计事件为 JSON 行，字段包括：

- `ts`（RFC3339）、`event_id`（UUID）
- `actor`：`os_user`、`hostname`、`app_version`
- `category`：`auth` | `session` | `credential` | `vault` | `config` | `fragment` | `command`
- `action`：如 `connect.start`、`vault.secret.read`、`command.submit`
- `outcome`：`success` | `failure` | `denied`
- `session_id`、`host`、`resource`（可选）
- `detail`：JSON 扩展字段（**不得**含密码、PEM、token）

### SIEM 对接

1. **HTTP**：偏好中配置 URL + Bearer；后台线程批量 POST `{"events":[...]}`
2. **Syslog**：UDP 或 TCP，RFC5424 风格前缀 + JSON 正文

### Vault 最低权限示例（KV v2）

```hcl
path "secret/data/ssh/*" {
  capabilities = ["create", "read", "update", "list"]
}
path "secret/metadata/ssh/*" {
  capabilities = ["list"]
}
```

## 跨设备同步（账号主机列表 + 可选凭证）

登录现有 mistlab / 团队账号后，客户端调用：

| 方法 | 路径 | 云端保存的内容 |
|------|------|----------------|
| `GET` / `PUT` | `/v1/sync/hosts` | 不透明主机 JSON（名称、host、port、用户、分组、认证提示、密钥**路径**） |
| `GET` / `PUT` | `/v1/sync/credentials` | 仅 `ciphertext` 与 `kdf_hint` |

- 鉴权复用团队 JWT。未登录不调用这些接口，本机 SSH 与只保存本地会话时相同。
- 版本用响应头 `ETag`，写回带 `If-Match`。`409` 带回服务端副本后再合并；`413` 表示正文过大，本机数据不删。
- **主机元数据默认同步**（可在「团队 → 云端同步」关掉）。正文里没有密码，也不会读取磁盘上的私钥文件。
- **凭证同步默认关闭。** 用户设置同步口令后，客户端用 Argon2id 从口令派生密钥，再包一层随机数据密钥（AES-256-GCM）。可选恢复码包同一把数据密钥。服务器看不到口令、恢复码或数据密钥。
- **`device_key` 不是跨设备解封密钥。** 它只加密本机的 `sync_key.json`（口令与恢复码）和同步游标。换机必须重新输入口令或恢复码。
- 口令错误、没有恢复码、信封损坏都会给出明确提示，并且不会用失败结果覆盖云端副本。
- 同一主机 id：`updated_at` 较新的一方胜出。时间戳相同且内容不同时保留冲突副本（名称后缀 ` (conflict)`），不静默丢改。凭证字段时间戳相同且不同时，本机和云端各自保留，不互相覆盖。
- 导出包里的 `hosts.json` 同样不含密码。勾选凭证时写入 `credentials.envelope.json`（同一套口令信封），便于另一平台导入。

偏好设置里的「同步与数据」只跳转到云端同步面板，口令、冲突和导出不放在偏好正文。

## 威胁模型（简要）

- 本地 `device_key` 绑定设备指纹，非 OS 钥匙串级隔离；本机同用户可读配置文件
- 审计文件可被本机用户篡改，无内置哈希链；高合规场景请依赖远程 sink
- Vault TLS 校验默认开启；仅开发环境可勾选跳过验证
