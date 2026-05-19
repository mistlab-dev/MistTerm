# 安全与审计

## 机密存储

| 方式 | 位置 | 说明 |
|------|------|------|
| 本地凭证库 | `~/.config/mistterm/credentials.json` | AES-GCM + 设备派生密钥 |
| 会话密码 | `sessions.json` | 同上 |
| HashiCorp Vault | 偏好设置配置 | KV v2 优先；引用仅存 mount/path/field |
| 会话 Vault 引用 | 新建/编辑会话对话框 | 与凭证面板相同的 mount/path/field 表单 |
| Vault 认证 | OS 钥匙串 | Token 或 AppRole `role_id`/`secret_id` |

## 审计日志（与终端回放分离）

- **目录**：`~/.config/mistterm/audit/audit-YYYY-MM-DD.jsonl`
- **应用内查看**：偏好设置 → 安全审计 → **查看审计日志…**（按日筛选、搜索 action/类别）
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

## 威胁模型（简要）

- 本地 `device_key` 绑定设备指纹，非 OS 钥匙串级隔离
- 审计文件可被本机用户篡改，无内置哈希链；高合规场景请依赖远程 sink
- Vault TLS 校验默认开启；仅开发环境可勾选跳过验证
