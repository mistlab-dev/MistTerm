# MistTerm 服务端待办与客户端降级行为

> **读者**：后端 / 运维 · **维护**：与 `src/core/team/client.rs` 同步  
> **完整 API 契约**：见 [TEAM.md](./TEAM.md) §三、附录 A

桌面端团队能力均走 **HTTPS API**。会话/片段/凭证跨设备同步使用团队 API 或「云端同步 → 导出/导入本地包」；**不提供 Git 仓库同步**。

---

## 1. 待实现接口（P1）

以下接口客户端**已实现调用**；服务端返回 **404** 或未部署时，客户端按下文降级，**不阻断 SSH 终端**。

### 1.1 片段执行上报

```
POST /v1/teams/{team_id}/fragments/{fragment_id}/usage
Authorization: Bearer <access_token>
Content-Type: application/json

{ "success": true, "duration_ms": 1200 }
```

| 项 | 说明 |
|----|------|
| 触发时机 | 用户在终端执行**团队片段**成功后，后台异步上报（`TeamService::spawn_report_fragment_usage`） |
| 期望响应 | `200` 或 `204` |
| **404 时客户端** | 视为成功，**静默忽略**（`do_report_fragment_usage`） |
| **其它错误** | 仅 `debug` 日志，不弹窗 |
| **用户可见影响** | `GET …/fragments/analytics` 的 `usage_count` 不递增；团队 Top5 / 成功率偏本机缓存 |
| **服务端建议** | 按 `(team_id, fragment_id, user_id)` 存明细，rollup 到 §1.2 聚合表 |

**验收**：上报 2xx 后，`GET /v1/teams/{team_id}/fragments/analytics` 中对应 `fragment_id` 的 `usage_count` 递增。

---

### 1.2 成员区间统计

```
GET /v1/teams/{team_id}/fragments/analytics/members?since=7d
Authorization: Bearer <access_token>
```

| Query | 取值 |
|-------|------|
| `since` | `7d` · `30d` · `90d`（与客户端 `FragmentAnalyticsTimeRange` 一致；「全部时间」不请求本接口） |

**期望响应 `200`：**

```json
{
  "members": [
    {
      "user_id": "u_xxx",
      "display_name": "Alice",
      "run_count": 120,
      "success_count": 115
    }
  ]
}
```

| 状态 | 客户端行为 | 用户可见 |
|------|------------|----------|
| **404** | `fetch_fragment_member_analytics` → `Ok(None)` | 分析弹窗「团队成员」折叠区显示：**「仅统计本机执行的团队片段（服务端接口未就绪时使用）」** |
| **200 且 members 非空** | 替换成员表，`member_stats_from_server = true` | 提示：**「全团队成员数据（来自服务端分析 API）」** |
| **5xx / 其它** | `debug` 日志，保留本机 `fragment_usage_events.json` 聚合 | 同 404 |

**验收**：多成员各执行团队片段后，`since=7d` 返回各成员 `run_count` 与上报一致。

---

### 1.3 片段聚合（已实现，供联调对照）

```
GET /v1/teams/{team_id}/fragments/analytics
```

| 状态 | 客户端行为 | 用户可见 |
|------|------------|----------|
| **200** | 合并到 `TeamFragmentCache`，`team_api_available = true` | 提示：**「团队数据已与服务端分析 API 合并」** |
| **404** | `Ok(None)`，仅用本机 cache + overlay | 无上述合并提示；区间统计仍可用本机 `fragment_usage_events.json` |

若 §1.1 未实现，本接口可返回静态/同步内嵌统计，但**无法反映其它成员**的执行。

---

## 2. 运维待部署（OAuth）

| 项 | 说明 |
|----|------|
| 桥接页 | `https://mistlab.dev/oauth/desktop-callback.html`（源码：`docs/product/oauth-desktop-callback.html`） |
| 服务端 | `GET /v1/oauth/{google\|github}?redirect_uri=…` 须 **302** 回传 `access_token`/`refresh_token` 或 `code` |
| **404 时客户端** | 启动 OAuth 前探测失败，弹窗：**「团队 API 的 OAuth 接口尚未可用（/v1/oauth/google 返回 404）。请先用邮箱密码登录…」**（`probe_oauth_start`） |
| 回退 | 客户端可回退 `http://127.0.0.1:{port}/callback`；须在 OAuth App 白名单允许 |

详见 [TEAM.md](./TEAM.md) §四 3。

---

## 3. Vault SSH CA（服务端签发，非桌面端待办）

**短期 SSH 证书由 HashiCorp Vault SSH CA 在服务端签发**，不在 MistTerm 客户端实现 CA 逻辑。

| 角色 | 职责 |
|------|------|
| **服务端 / Vault** | 配置 SSH CA、签发短期证书、通过 `GET /v1/team/sync` 下发 Vault 地址与 `vault_credential_path` |
| **桌面端（已实现）** | 从 Vault KV 读取密码/私钥（`SecretBackend::VaultKv`）；连接团队服务器时使用 `vault_credential_path` |

客户端**不会**调用 Vault `sign-ssh-key` 等 CA API；若产品需要证书登录，由运维在连接前注入已签证书，或扩展 `team/sync` 下发证书引用（需另定契约）。

---

## 4. 其它接口与 404 降级（已实现）

| 接口 | 404 时客户端 |
|------|----------------|
| `GET /v1/team/sync` | 空团队列表，不报错 |
| `GET /v1/teams/{id}/members` | 成员弹窗：**「服务端尚未提供成员列表接口」**（若生产仍 404，请对照附录 A.3.4 实现） |
| `GET /v1/market/fragments/catalog` | 提示市场未部署，用本地 `market_fragments_cache.json` |
| `POST /v1/market/fragments/{id}/install` | 静默忽略 |

---

## 5. 源码索引

| 能力 | 路径 |
|------|------|
| HTTP 客户端 | `src/core/team/client.rs` |
| 片段 usage 上报 | `src/core/team/service.rs` · `do_report_fragment_usage` |
| 分析大盘构建 | `src/core/team/service.rs` · `build_fragment_analytics_dashboard` |
| 分析 UI 文案 | `src/ui/fragment_analytics_dialog.rs` |
| OAuth 探测错误 | `src/core/team/oauth.rs` · `probe_oauth_start` |
| 成员列表 404 文案 | `src/ui/team_members_dialog.rs` · `localize_members_error` |

---

## 6. 后端自测清单（P1）

1. `POST …/fragments/{id}/usage` → 2xx → `GET …/analytics` 中 `usage_count` 递增  
2. `GET …/analytics/members?since=7d` → 200 + 多成员 `run_count` 正确  
3. 上述两接口 404 时，桌面端分析弹窗仍可用（本机数据 + 上文提示文案）  
4. OAuth：`GET /v1/oauth/google?redirect_uri=…` 非 404，302 携带 token 或 code  
