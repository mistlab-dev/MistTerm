# MistTerm 依赖的服务端 API（后端实现清单）

> **更新**：2026-05-29  
> **读者**：`api.mistlab.dev` 后端 / 运维  
> **客户端状态**：MistTerm 已对接下列接口；**404 时客户端会降级**（本地缓存 / 本地聚合），但功能不完整。  
> **API Base**：`https://api.mistlab.dev`（与客户端 `TeamSettings.api_base` 一致，可配置）

本文只描述**需要服务端实现或补齐**的契约，与客户端 `src/core/team/client.rs`、`src/core/market/client.rs` 字段对齐。基线团队 API（登录、sync、片段 CRUD、审计 POST）见 [TEAM-PLATFORM-API.md](./TEAM-PLATFORM-API.md)；命令审计全量 API 见 [COMMAND-AUDIT.md](./COMMAND-AUDIT.md)。

---

## 1. 通用约定

| 项 | 说明 |
|----|------|
| 鉴权 | 除注明「可选」外，均需 `Authorization: Bearer <access_token>` |
| Content-Type | `application/json` |
| 错误体 | 建议 `{ "error": "human readable" }` 或 `{ "message": "..." }`（客户端会解析其一） |
| 未实现 | 客户端对 **404** 视为「接口未部署」，不弹致命错误，走本地回退 |
| 分页 | 片段 sync 使用 `cursor`；市场 catalog 使用 `cursor`（可选，客户端当前主要用 `limit`） |

---

## 2. P4 新增：片段市场（Market）

客户端：`src/core/market/`；UI 为命令片段侧栏 **「市场」** scope。

### 2.1 拉取目录

```
GET /v1/market/fragments/catalog
Authorization: Bearer <access_token>   # 可选：未登录也可浏览公开目录（若产品允许）
```

**Query（均可选）**

| 参数 | 类型 | 说明 |
|------|------|------|
| `category` | string | 分类筛选，空=全部 |
| `search` | string | 标题/命令/描述模糊搜索 |
| `limit` | int | 客户端默认 `200` |

**响应 `200`**

```json
{
  "catalog_version": "2026-05-29T12:00:00Z",
  "cursor": "",
  "fragments": [
    {
      "id": "mkt_abc123",
      "title": "查看磁盘",
      "command": "df -h",
      "category": "ops",
      "tags": "[\"linux\",\"disk\"]",
      "variables": "[]",
      "description": "常用磁盘检查",
      "author": "mistlab",
      "revision": 3,
      "install_count": 1280,
      "updated_at": "2026-05-20T08:00:00Z"
    }
  ]
}
```

**字段说明**

| 字段 | 必填 | 说明 |
|------|------|------|
| `id` | 是 | 市场片段全局 ID，客户端安装后写入个人库标签 `mkt:{id}` |
| `title` / `command` | 是 | 与团队片段一致 |
| `category` | 否 | 空时客户端展示为 `market` |
| `tags` | 否 | **JSON 字符串数组**（与团队片段相同，不是原生 JSON 数组） |
| `variables` | 否 | **JSON 字符串**，元素结构同团队片段变量 |
| `description` / `author` | 否 | 展示用 |
| `revision` | 否 | 版本号，默认 `0` |
| `install_count` | 否 | 安装次数统计，默认 `0` |
| `updated_at` | 否 | ISO8601 字符串 |
| `catalog_version` | 否 | 目录版本标识，便于客户端缓存失效 |
| `cursor` | 否 | 下一页游标（Query）；空表示无更多；客户端「加载更多」会带上次响应的 `cursor` |

**客户端行为**

- 成功：写入 `~/.config/mistterm/market_fragments_cache.json`（AES 加密）
- **404**：提示「市场接口未部署」，继续显示本地缓存 + 个人库中带 `market` 标签的片段
- 其他错误：显示错误文案，仍保留旧缓存

### 2.2 安装计数（可选）

```
POST /v1/market/fragments/{fragment_id}/install
Authorization: Bearer <access_token>   # 可选
Content-Type: application/json

{}
```

**响应**

- `200` / `204`：成功
- **404**：客户端忽略（视为未部署统计）
- 其他 4xx/5xx：客户端忽略，不影响「添加到个人库」

**服务端建议**：对 `fragment_id` 做 `install_count` 原子 +1；可记录 `user_id` 防刷（非 MVP 必需）。

---

## 3. P4 新增：团队片段分析（Analytics）

客户端：`GET` 后合并个人库 + 团队缓存，弹窗「分析大盘」。本地执行会通过 `usage_overlay` 叠加到团队片段统计。

### 3.1 聚合接口

```
GET /v1/teams/{team_id}/fragments/analytics
Authorization: Bearer <access_token>
```

**权限**：建议 `viewer+`（与读团队片段一致）

**响应 `200`**

```json
{
  "fragments": [
    {
      "fragment_id": "frag_xxx",
      "usage_count": 42,
      "success_count": 40,
      "total_time_ms": 120000,
      "last_used_at": 1716969600
    }
  ]
}
```

| 字段 | 说明 |
|------|------|
| `fragment_id` | 团队片段 ID（与 sync 返回的 `TeamFragment.id` 一致） |
| `usage_count` | 总执行次数 |
| `success_count` | 成功次数（客户端用其算成功率） |
| `total_time_ms` | 累计耗时毫秒 |
| `last_used_at` | Unix 秒时间戳，可选 |

**客户端行为**

- **404 / 未实现**：`team_api_available = false`，仅用本地 `TeamFragmentCache` + 本机 overlay 聚合
- 成功：与服务端返回行按 `fragment_id` 合并到 `FragmentStats` 再算 Top5 / 慢命令 / 高错误率

### 3.2 片段 sync 内嵌统计（推荐一并返回）

客户端模型 `TeamFragment` 已支持下列字段（`POST .../fragments:sync` 的 `fragments[]` 内）：

| 字段 | 类型 | 默认 | 说明 |
|------|------|------|------|
| `usage_count` | u32 | 0 | 团队维度执行次数 |
| `success_count` | u32 | 0 | 成功次数 |
| `total_time_ms` | u64 | 0 | 累计耗时 |
| `last_used_at` | i64? | null | Unix 秒 |

若已实现 §3.1，建议在 sync 中带相同统计，减少单独拉 analytics 的频率；analytics 接口用于**跨成员聚合**或**大盘专用查询**。

### 3.3 团队成员区间统计（可选，客户端本机已用事件日志降级）

```
GET /v1/teams/{team_id}/fragments/analytics/members?since=7d
Authorization: Bearer <access_token>
```

**响应示例**

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

未实现时客户端仅汇总**本机** `fragment_usage_events.json` 中的团队执行记录。

### 3.4 统计上报（后续可选，当前客户端未调用）

客户端目前在本地记录执行（`record_fragment_execution`），团队维度依赖 sync/analytics 拉取。若需实时上报，可后续增加例如：

```
POST /v1/teams/{team_id}/fragments/{fragment_id}/usage
{ "success": true, "duration_ms": 1200 }
```

**非阻塞**：未实现不影响现有客户端。

---

## 4. 命令审计（已实现 — 客户端已对接）

服务端据 [COMMAND-AUDIT.md](./COMMAND-AUDIT.md) **已部署**。客户端实际调用：

| 方法 | 路径 | 用途 |
|------|------|------|
| GET | `/v1/teams/{team_id}/command-audit/sync` | 策略 + 规则本地缓存 |
| POST | `/v1/teams/{team_id}/command-audit/alerts` | 拦截/确认/告警上报 |

**告警请求体（客户端 `CmdAuditAlertRequest`）**

```json
{
  "command": "rm -rf /",
  "matched_rule": "rm_recursive_root",
  "match_level": "dangerous",
  "action_taken": "blocked"
}
```

`action_taken` 示例：`blocked` | `confirmed` | `alerted`。

---

## 5. 基线团队 API — 待补齐项（客户端已对接）

完整契约见 [TEAM-PLATFORM-API.md](./TEAM-PLATFORM-API.md) 与 [CLIENT-TEAM-TODO.md](./CLIENT-TEAM-TODO.md)。

| 优先级 | 方法 | 路径 | 状态 | 客户端未部署时表现 |
|--------|------|------|------|-------------------|
| P2 | GET | `/v1/teams/{team_id}/members` | **待实现** | 成员弹窗提示接口未就绪 |
| P1 | POST | `/v1/audit/events` | 须支持批量 + `evt_*` 去重 | 事件积压本地 `pending-team-events.jsonl` |
| P2 | — | OAuth redirect 白名单 + 302 | 见 CLIENT-TEAM-TODO §3.2 | Google/GitHub 登录失败 |
| 运维 | — | 部署 `oauth-desktop-callback.html` | 见 `docs/product/oauth-desktop-callback.html` | 回退 `127.0.0.1` 回调 |

**成员列表响应**

```json
{
  "members": [
    {
      "user_id": "u_xxx",
      "email": "a@example.com",
      "username": "alice",
      "display_name": "Alice",
      "role": "editor"
    }
  ]
}
```

---

## 6. 已实现基线（供联调索引）

以下接口客户端**已在使用**，新环境部署时需一并可用：

| 分类 | 路径 |
|------|------|
| 认证 | `POST /v1/auth/login`、`POST /v1/auth/register`、`POST /v1/auth/refresh` |
| OAuth | `GET /v1/oauth/{provider}`、`GET /v1/oauth/{provider}/callback` |
| 用户/团队 | `GET /v1/me`、`GET /v1/teams`、`GET /v1/team/sync`、`GET /v1/teams/{id}` |
| 团队片段 | `POST /v1/teams/{id}/fragments:sync`、`POST /v1/teams/{id}/fragments`、`PUT /v1/fragments/{id}`、`DELETE /v1/fragments/{id}` |
| 审计 | `POST /v1/audit/events` |

片段 sync 请求体：

```json
{ "cursor": "", "limit": 500 }
```

---

## 7. 验收清单（后端自测）

### 市场

1. `GET /v1/market/fragments/catalog?limit=10` → 200 + `fragments` 数组  
2. 未登录（无 Bearer）若允许公开目录 → 仍 200  
3. 路由不存在 → 404，客户端显示「未部署」且用缓存  
4. `POST .../install` → 200，`install_count` 递增  

### 片段分析

1. 登录并选择团队 → `GET .../fragments/analytics` → 200 + `fragments`  
2. 404 时客户端分析大盘仍可打开（仅本地数据）  
3. sync 返回的 `TeamFragment` 含 `usage_count` 等字段时，侧栏团队片段排序/展示与大盘一致  

### 回归

1. `POST /v1/audit/events` 批量 50 条，`duplicate` 正确  
2. `GET .../command-audit/sync` 与 `POST .../alerts` 与 COMMAND-AUDIT 文档一致  
3. `GET .../members` 200（若已排期实现）  

---

## 8. 相关文档

| 文档 | 说明 |
|------|------|
| [TEAM-PLATFORM-API.md](./TEAM-PLATFORM-API.md) | 团队平台集成（登录、sync、审计） |
| [COMMAND-AUDIT.md](./COMMAND-AUDIT.md) | 命令审计完整 API（管理端 + sync） |
| [CLIENT-TEAM-TODO.md](./CLIENT-TEAM-TODO.md) | 客户端 vs 服务端配合状态表 |
| [TEAM-PLATFORM-DEV-PLAN.md](./TEAM-PLATFORM-DEV-PLAN.md) | 产品与阶段规划 |

**客户端源码索引**

| 能力 | 路径 |
|------|------|
| 市场 HTTP | `src/core/market/client.rs` |
| 市场模型 | `src/core/market/models.rs` |
| 团队 HTTP | `src/core/team/client.rs` |
| 团队模型 | `src/core/team/models.rs` |
| 分析聚合 | `src/core/fragment_analytics.rs` |
