# MistTerm 团队平台

> **维护**：2026-05-30 · 字段与路径以 `src/core/team/client.rs`、`src/core/market/client.rs` 为准  
> **结构**：§一 产品与需求 · §二 集成参考（`team/sync` 等） · **§三 服务端接口清单** · §四 客户端索引 · 附录 A API 契约

| 章节 | 读者 | 内容 |
|------|------|------|
| §一 | 产品 / 架构 | 背景、职责划分、需求条目 |
| §二 | 客户端开发 | `GET /v1/team/sync`、Vault/团队服务器映射 |
| **§三** | **后端 / 运维** | **MistTerm 依赖的全部接口、状态、契约、验收** |
| §四 | 客户端开发 | 模块路径、OAuth 部署、联调脚本 |
| 附录 A | 联调 | 认证 / 片段 CRUD / 审计 ingest 请求响应细节 |

命令审计（拦截/策略）：见 [COMMAND-AUDIT.md](./COMMAND-AUDIT.md)（服务端已实现，本文不重复）。

---

## 一、需求与方案

# 团队片段、命令审计与 AI 辅助 — 需求与设计

---

## 1. 背景与目标

### 1.1 要解决的问题

| 需求 | 说明 |
|------|------|
| 团队命令片段共享 | 运维/开发组共用命令模板，减少重复录入，统一高危操作口径 |
| 命令审计 | 谁在何时对哪台主机执行/插入了什么操作，可查询、可导出 |
| AI 辅助 | 用户在客户端配置 **OpenAI 兼容 API**（地址 + Key + 模型），用于生成命令、解读输出、分析数据等；**不经团队服务端** |
| 个人用户不受影响 | 不登录、不用团队能力时，与当前 MistTerm 单机体验一致 |

### 1.2 设计原则

- 团队能力通过 **HTTPS API** 提供；**Git / 文件夹同步包** 不作为正式团队方案（仅个人备份）。
- 团队能力作为**一体交付**：认证、片段同步、审计上报在同一套契约下联调；**AI 为客户端独立能力**，与团队服务端解耦。
- 客户端已有 `FragmentManager`、`AuditLogger`，在现有模型上演进，避免推倒重来。

### 1.3 非目标（当前阶段）

评论墙、审批流、公开模板市场、强制全员注册账号才能用终端、团队统一 AI 网关/配额、服务端代调用大模型或采集终端全文用于训练。

---

## 2. 用户与使用模式

### 2.1 个人用户（默认）

| 场景 | 是否依赖团队服务端 | 设计说明 |
|------|-------------------|----------|
| SSH、会话、终端 | 否 | 与现网一致 |
| 个人命令片段 | 否 | 本地 `fragments.json`，本地增删改 |
| 个人审计 | 否 | 本地 JSONL；可选自行对接 SIEM |
| 跨设备迁移 | 否 | 设置内个人导出/导入包 |
| 团队片段 / 团队审计 | 是 | 片段同步 + **审计上报**（查询在服务端/管理端，桌面端不提供） |
| AI 辅助 | 否 | 设置中配置 OpenAI 兼容 `base_url` + API Key；请求直达用户所选接口 |

未配置团队服务地址、未登录时：**不出现团队入口，不阻断任何现有功能**。

### 2.2 团队用户（可选叠加）

```text
个人能力（始终可用）          团队能力（登录后）
────────────────────          ──────────────────
终端 + 会话 + 个人片段         ＋ 查看/使用团队片段
本地审计（可选）               ＋ 按权限编辑团队片段
个人导出包（可选）             ＋ 审计上报（可选；团队侧查询在服务端）

（AI 辅助与是否登录团队无关，均在客户端配置 OpenAI 兼容接口。）
```

---

## 3. 职责划分（客户端 vs 服务端）

### 3.1 系统边界

```text
┌─────────────────────────────┐         HTTPS / JSON          ┌─────────────────────────────┐
│      MistTerm 客户端         │ ◄──────────────────────────► │        团队服务端            │
│  【交互、采集、缓存、上报、AI】  │                             │  【权威数据、鉴权】           │
└─────────────────────────────┘                             └─────────────────────────────┘
```

### 3.2 能力归属总表

| 能力域 | 客户端负责 | 服务端负责 | 不经服务端（个人路径） |
|--------|------------|------------|----------------------|
| SSH / 终端 | 会话、输入、渲染、本地回放 | — | 全部在客户端 |
| 个人片段 | 本地 CRUD、`fragments.json` | — | 全部在客户端 |
| 团队片段 | 缓存、同步触发、冲突 UI、调用 API | 权威存储、revision、RBAC、增量 sync | — |
| 个人审计 | 本地 JSONL、可选 Syslog/HTTP 外推 | — | 全部在客户端 |
| 团队审计 | 埋点、脱敏、批量上报、离线队列 | 接收、幂等、存储与查询 | — |
| AI 辅助 | 配置 OpenAI 兼容接口、组 prompt、调用、脱敏、展示、用户确认 | —（团队服务端不提供 AI API） | 用户自配接口，与团队登录无关 |
| 账号 | 登录 UI、token 存密钥链、refresh 调度 | 认证、团队/角色、token 签发 | — |

**原则：**

- **谁离用户近，谁采集上下文**（终端选区、监控快照、自然语言意图均在客户端组装）。
- **谁存团队数据，谁做权威校验**（片段、团队审计在服务端；AI 配置与请求均在客户端）。
- 服务端**不**拉取 SSH 会话流、**不**代替用户在终端执行命令。

### 3.3 数据权威与同步方向

| 数据 | 权威方 | 客户端角色 | 服务端角色 |
|------|--------|------------|------------|
| 个人片段 | 客户端 | 读写 `fragments.json` | 不存储 |
| 团队片段 | 服务端 | 只读缓存 + 上报变更 | CRUD + `fragments:sync` 下发 |
| 个人审计 | 客户端 | 写本地 JSONL | 不存储（除非用户上报到团队） |
| 团队审计 | 服务端 | 上报 `AuditEvent`（ingest only） | 存储与查询（AUD-2，管理端/Web） |
| AI 配置 | 客户端 | 读写本地设置；API Key 存系统密钥链 | 不存储 |
| AI 请求/响应 | 用户 ↔ 模型供应商 | 组装请求、脱敏、预览、解析结果；可选本机历史 | 不参与 |

### 3.4 AI 场景（仅客户端）

客户端按场景组装 prompt，统一通过用户配置的 **OpenAI 兼容** `POST {base_url}/chat/completions` 调用；`scenario` 用于选择系统提示与解析响应，**不上报团队服务端**。

| scenario | 用途 | 关键 context 字段 |
|----------|------|-------------------|
| `command_generate` | 自然语言 → 可执行命令 | `user_intent` |
| `command_suggest` | 输入前缀补全 | `user_input` |
| `error_explain` | 报错解读 | `terminal_excerpt`, `last_command?` |
| `output_summarize` | 日志/输出摘要 | `terminal_excerpt` |
| `data_analyze` | 表格/指标分析 | `terminal_excerpt?`, `structured_data?`, `analysis_goal` |
| `fragment_draft` | 片段起草 | `user_intent`, `reference_fragment_ids?` |
| `fragment_recommend` | 片段推荐 | `hint_ids`（无命令明文） |

---

## 4. 功能需求 — 服务端

> **本章范围**：团队平台的 HTTP API、RBAC、权威数据。  
> **本章不包含**：终端 UI、SSH、AI 与大模型调用（见第 5 章 5.5）。

### 4.0 服务端职责摘要

| 做 | 不做 |
|----|------|
| 用户/团队/角色与 token | 终端渲染、命令执行 |
| 团队片段权威库与 sync | 个人 `fragments.json` 托管 |
| 审计 ingest 与团队查询 | 代替客户端采集终端全文、AI 代理 |
| OpenAPI、健康检查、限流 | 客户端 UI、AI 配置与 OpenAI 兼容调用 |

### 4.1 认证与团队上下文

| 编号 | 需求 | 验收要点 |
|------|------|----------|
| AUTH-1 | 用户可登录并获取访问凭证 | 支持企业 SSO（OIDC）或等价方式；内测可用 API Key |
| AUTH-2 | 访问凭证可刷新 | 短期 access + 长期 refresh；过期返回 401 |
| AUTH-3 | 用户可获知所属团队及角色 | `GET /me` 返回团队列表与 role |
| AUTH-4 | 请求可携带当前团队上下文 | 团队级 API 按 `team_id` 隔离数据 |

**`GET /me` 响应示例：**

```json
{
  "user_id": "u_1",
  "display_name": "张三",
  "teams": [
    { "id": "team_ops", "name": "运维组", "role": "editor" }
  ]
}
```

### 4.2 团队命令片段

#### 4.2.1 数据要求（逻辑模型）

与客户端 `FragmentStats` 对齐，团队片段额外约定：

| 字段 | 说明 |
|------|------|
| `id` | 全局唯一 |
| `team_id` | 所属团队 |
| `scope` | 团队场景固定为 `team`（个人片段继续走本地，不经团队片段 API） |
| `title`, `command`, `category`, `tags`, `variables` | 与客户端一致 |
| `revision` | 单调递增，用于乐观锁 |
| `status` | 默认 `published`；支持 `draft` / `archived`（团队可选启用草稿流） |
| `created_by`, `updated_by`, `updated_at` | 审计与增量同步 |
| 删除 | 软删除；同步时下发已删 id 列表 |

#### 4.2.2 接口需求

| 编号 | 方法 | 路径 | 权限 | 行为 |
|------|------|------|------|------|
| FRAG-1 | GET | `/v1/teams/{team_id}/fragments` | viewer+ | 列表，支持增量参数 |
| FRAG-2 | GET | `/v1/fragments/{id}` | viewer+ | 单条详情 |
| FRAG-3 | POST | `/v1/teams/{team_id}/fragments` | editor+ | 创建 |
| FRAG-4 | PUT | `/v1/fragments/{id}` | editor+ | 更新；带 revision，过期返回 409 |
| FRAG-5 | DELETE | `/v1/fragments/{id}` | admin | 删除（软删） |
| FRAG-6 | POST | `/v1/teams/{team_id}/fragments:sync` | viewer+ | **推荐**批量增量同步 |

**增量同步 `fragments:sync` 请求：**

```json
{
  "cursor": "客户端上次游标",
  "limit": 500
}
```

**响应：**

```json
{
  "cursor": "新游标",
  "fragments": [],
  "deleted_ids": [],
  "server_time": "2026-05-20T12:00:00Z"
}
```

**冲突（FRAG-4）**：revision 不一致时 **409**，响应体含服务端当前片段，供客户端选择覆盖/保留/合并。

### 4.3 权限（RBAC）

| 角色 | 读片段 | 写片段 | 删片段 | 查团队审计（管理端） |
|------|--------|--------|--------|----------------------|
| viewer | ✓ | | | — |
| editor | ✓ | ✓ | | — |
| admin | ✓ | ✓ | ✓ | ✓（服务端/Web，非 MistTerm 桌面端） |

### 4.4 命令审计

#### 4.4.1 事件模型（与客户端一致）

客户端 `AuditEvent` 字段约定保持不变，例如：

```json
{
  "ts": "2026-05-20T12:00:00.000Z",
  "event_id": "uuid",
  "actor": { "os_user": "", "hostname": "", "app_version": "" },
  "category": "fragment",
  "action": "fragment.execute",
  "outcome": "success",
  "session_id": "可选",
  "host": "可选",
  "resource": "可选",
  "detail": {}
}
```

#### 4.4.2 接口需求

| 编号 | 方法 | 路径 | 行为 |
|------|------|------|------|
| AUD-1 | POST | `/v1/audit/events` | 批量接收；`event_id` 幂等（重复忽略） |
| AUD-2 | GET | `/v1/teams/{team_id}/audit/events` | 分页查询；**管理端/Web**，MistTerm 桌面端不调用 |

**上报请求示例：**

```json
{
  "events": [ { /* AuditEvent */ } ]
}
```

**响应：** `202` + `{ "accepted": n, "duplicate": m }`

#### 4.4.3 约定的 action

| category | action |
|----------|--------|
| command | `command.submit` |
| fragment | `fragment.insert`, `fragment.execute`, `fragment.create`, `fragment.update`, `fragment.delete`, `fragment.sync_pull` |
| session | `session.connect`, `session.disconnect` |
| auth | `team.login`, `team.token_refresh` |
| ai | `ai.invoke`, `ai.suggestion_accept`（可选写本地审计；不上报团队除非用户开启且团队策略允许记录元数据） |

#### 4.4.4 脱敏与保留（团队策略）

| 策略项 | 说明 |
|--------|------|
| 命令预览长度 | 如最多 120 字符，不全文入库 |
| 可选 hash | `detail.command_hash` 代替明文 |
| 保留期 | 团队级配置，如默认 90 天 |

客户端通过现有 `AuditSettings` 上报；个人用户可关闭上报，仅保留本地审计。

### 4.5 非功能需求（服务端）

| 编号 | 需求 |
|------|------|
| NFR-1 | 全接口 HTTPS |
| NFR-2 | 健康检查 `GET /health` |
| NFR-3 | 按用户/团队限流，审计上报单独配额 |
| NFR-4 | 服务端自身管理操作可追溯（谁改了哪条团队片段） |
| NFR-5 | 提供联调/测试环境与固定测试账号（viewer / editor / admin） |

### 4.6 错误码约定

| HTTP | 含义 | 客户端处理 |
|------|------|------------|
| 401 | 未登录或 token 失效 | 提示重新登录 |
| 403 | 无权限 | 提示联系管理员 |
| 409 | 片段版本冲突 | 冲突解决 UI |
| 422 | 参数校验失败 | 展示服务端 message |

---

## 5. 功能需求 — 客户端（MistTerm）

> **本章范围**：终端、本地数据、团队 API 调用方、**用户自配 OpenAI 兼容 AI**。  
> **本章不包含**：团队片段权威库、服务端 RBAC 判定、团队侧 AI 代理接口。

### 5.0 客户端职责摘要

| 做 | 不做 |
|----|------|
| SSH、终端 UI、监控面板展示 | 托管团队片段权威数据 |
| 个人片段/审计本地读写 | 代替服务端做团队 RBAC 最终裁决 |
| 调用第 4 章 API、本地缓存与冲突 UI | 托管或代发用户的大模型请求 |
| 右侧 AI 面板、OpenAI 兼容调用、脱敏、「用到终端」 | 未点击「用到终端」即向 SSH 发命令 |
| — | 默认上传 `session_logger` 全文 |

### 5.1 设置与账号

- 可配置团队服务根地址 `api_base`（为空则隐藏团队功能）。
- 登录态与 refresh；凭证存系统密钥链。
- 可选择当前团队；展示同步状态与最近错误。

### 5.2 命令片段 UI

- 片段列表区分 **个人 / 团队** 来源。
- 团队片段：按 role 控制新建、编辑、删除；只读时仅可执行/插入。
- 支持 **手动同步** 与 **定时同步**；失败自动重试并展示最近错误。
- revision 冲突时弹窗：以服务端的为准 / 保留本地草稿 / 合并编辑 / 取消。

### 5.3 审计

- 本地 JSONL 仅作写入缓冲与离线队列，**不提供查看/检索 UI**。
- 已登录团队时，批量调用 `POST /v1/audit/events`；断网时本地队列暂存，恢复后补报。
- 补全片段 CRUD、同步、执行等埋点（与 4.4.3 对齐）。
- **不**实现团队审计查询（AUD-2）；管理员在服务端或 Web 控制台查看。

### 5.4 与个人路径兼容

- 旧版 `fragments.json` 无新字段时仍可加载（serde default）。
- 未登录时不调用团队 API。

### 5.5 AI 辅助（用户配置 · OpenAI 兼容）

AI 为**纯客户端能力**：用户在设置中填写接口信息即可使用，**不依赖团队服务端**，与是否登录团队无关。

#### 5.5.1 配置项（本地）

| 字段 | 说明 | 默认 / 示例 |
|------|------|-------------|
| `enabled` | 是否启用 AI | `false` |
| `base_url` | API 根路径，需兼容 OpenAI | `https://api.openai.com/v1` |
| `api_key` | 密钥，存系统密钥链，不出现在日志 | 用户填写 |
| `model` | 模型 id | `gpt-4o-mini` |
| `timeout_secs` | 请求超时 | 如 `60` |
| `max_tokens` | 单次回复上限（可选） | 如 `2048` |

**兼容范围（验收以 OpenAI Chat Completions 为准）：**

- 官方 OpenAI、`/v1/chat/completions`
- 国内/自建网关（DeepSeek、Moonshot、硅基流动等）只要路径与请求体兼容
- 本地 LM Studio、Ollama OpenAI 兼容模式等（用户自行填写 `base_url`）

**设置页能力：**

- 「测试连接」：发送最小 `chat/completions` 请求，展示成功或 HTTP/鉴权错误。
- 明确提示：流量与费用归用户所选服务商，MistTerm 不托管模型。

#### 5.5.2 调用约定（客户端 → 用户配置的 API）

统一使用 OpenAI 风格 **Chat Completions**：

```http
POST {base_url}/chat/completions
Authorization: Bearer {api_key}
Content-Type: application/json
```

```json
{
  "model": "gpt-4o-mini",
  "messages": [
    { "role": "system", "content": "（按 scenario 注入，要求 JSON 或 Markdown 结构）" },
    { "role": "user", "content": "（用户意图 + 脱敏后的 excerpt / structured_data）" }
  ],
  "temperature": 0.2
}
```

客户端从 `choices[0].message.content` 解析结果；若模型返回 JSON，按 3.4 场景字段解析；解析失败则原样展示并允许用户复制。

**可选增强（实现自定，非硬依赖）：**

- `response_format: { "type": "json_object" }`（仅当接口支持时）
- 流式 `stream: true` 用于长回答展示（可选，非验收必需）

#### 5.5.3 交互（UI）

> **见** [AI-INTERACTION-DESIGN.md](./AI-INTERACTION-DESIGN.md)（v0.2）：右侧 **AI 面板** 统一生成/解析；终端可选「发送到 AI」附带选区；回复中命令点击 **「用到终端」** 写入左侧当前会话并发送。

实现层仍可按 §3.4 的 `scenario` 区分 prompt，但**不**再做多入口、预览弹窗、`#` 输入模式。

#### 5.5.4 与片段 / 审计

- 保存团队片段仍走第 4 章 FRAG API（与 AI 配置无关）。  
- 可选本地审计 `ai.invoke`、用户点击「用到终端」时 `ai.suggestion_accept`；**默认不向团队上报 AI 请求体**。

---

## 6. 联调验收

团队功能启用后，客户端与服务端应满足下列端到端场景（编号对应上文需求条目）：

| # | 场景 | 通过标准 |
|---|------|----------|
| V-1 | 团队片段共享 | 用户 A 创建团队片段，用户 B 同步后可见并可执行 |
| V-2 | 片段冲突 | 并发修改触发 409 时，客户端可完成冲突解决并再次同步 |
| V-3 | 命令审计上报 | 执行片段后，服务端团队审计可查到 `fragment.execute`（桌面端无查询 UI） |
| V-4 | 审计离线 | 断网期间事件入本地队列，恢复网络后补报且不重复（`event_id` 幂等） |
| V-5 | 个人无回归 | 未登录用户正常使用个人片段与 SSH，无团队入口阻断 |
| V-6 | AI 配置 | 填写 OpenAI 兼容 `base_url` + Key + model，「测试连接」成功 |
| V-7 | AI 用到终端 | 面板生成命令后点击「用到终端」，左侧当前会话执行 |
| V-8 | AI 解析 | 终端选区「发送到 AI」后，面板内得到解读 |
| V-9 | AI 与团队解耦 | 未登录团队时 AI 面板仍可用 |

**服务端最低交付**：AUTH-1～4、FRAG-1～6、AUD-1～2、NFR-1～5、4.6 错误码约定、OpenAPI（或等价物）、测试环境与三组角色账号（**不含 AI 接口**）。

**客户端最低交付**：5.1～5.4；5.5 在 `enabled` 时满足 OpenAI 兼容调用与 V-6～V-8；团队能力可关，默认不影响个人路径。

---

## 7. 服务端交付清单（给对接方）

1. **接口文档**（OpenAPI 等）：auth、fragments、sync、audit（不含 AI）。  
2. **测试环境** URL。  
3. **测试账号**：至少 viewer、editor、admin 各一，同一团队。  
4. **变更流程**：字段或语义变更提前通知客户端，并升文档版本。  

---

## 8. 与现有 MistTerm 能力的关系

| 现有能力 | 定位 |
|----------|------|
| `fragments.json` | 个人片段，长期保留 |
| `cloud_sync_panel` 导出/导入包 | 个人备份与换机，非团队同步 |
| `AuditLogger` 本地文件 | 保留；团队上报为可选通道 |
| `session_logger` | 终端回放，与审计分离，不上报终端全文 |
| 产品愿景中的 AI 辅助 | 仅客户端 5.5（OpenAI 兼容自配）；场景见 3.4 |
| `monitor_panel` 指标 | 客户端采集为 `structured_data`，供 `data_analyze` |

---

## 9. 风险与依赖

| 风险 | 缓解 |
|------|------|
| 服务端晚于客户端 | 团队功能开关默认关；可用 Mock 联调 |
| 审计量过大 | 批量上报、限流；预览截断 |
| 客户仅要 SIEM | 客户端保留 Syslog/HTTP 直推，团队 ingest 可选 |
| 个人用户误绑团队 | UI 与配置默认个人；团队需显式登录 |
| AI 误传敏感输出 | 发送前 excerpt 预览；客户端脱敏 |
| 兼容接口行为不一致 | 以 OpenAI `chat/completions` 为基准；测试连接 + 解析失败时降级为纯文本展示 |

---

**文档维护**：接口契约冻结后，更新文首版本号并标注「契约已冻结」。

---

## 附录 A：服务端实现细节（客户端对接参考）

> 本附录记录 `mist-team-server` 的**实际实现**，供客户端开发直接参照。与正文如有冲突，以本附录为准。

### A.1 Base URL 与通用约定

| 项目 | 说明 |
|------|------|
| Base URL | 配置项 `api_base`，如 `https://api.mistlab.dev` 或 `http://localhost:8080` |
| 协议 | HTTPS（生产）；HTTP（开发） |
| 认证 | 除 `/health`、`/v1/auth/*`、`/v1/oauth/*`、`/v1/billing/webhook` 外，所有接口需 `Authorization: Bearer <access_token>` |
| Content-Type | `application/json` |
| CORS | 已配置，允许 `https://mistlab.dev` 和 `http://localhost:8765` |
| 限流 | 默认启用，120 req/min/IP |

### A.2 认证（Auth）

#### A.2.1 注册

```
POST /v1/auth/register
```

**请求体：**
```json
{
  "email": "user@example.com",
  "username": "zhangsan",
  "display_name": "张三",       // 可选，缺省用 username
  "password": "mypassword"      // ≥6 字符
}
```

**响应 `201`：**
```json
{
  "user": {
    "id": "u_abc123def456",
    "email": "user@example.com",
    "username": "zhangsan",
    "display_name": "张三",
    "email_verified": false,
    "created_at": "2026-05-24T00:00:00Z",
    "updated_at": "2026-05-24T00:00:00Z"
  },
  "message": "Account created! Enjoy 30 days Pro trial. Please verify your email."
}
```

**错误：**
- `400` — 参数校验失败
- `409` — email 或 username 已存在

> **注意**：注册不返回 token，需另行调用 `/v1/auth/login`。

#### A.2.2 登录

```
POST /v1/auth/login
```

**请求体（二选一）：**
```json
{
  "email": "user@example.com",   // email 和 username 至少填一个
  "password": "mypassword"
}
```
或
```json
{
  "username": "zhangsan",
  "password": "mypassword"
}
```

**响应 `200`：**
```json
{
  "access_token": "eyJhbG...",
  "refresh_token": "eyJhbG...",
  "user": {
    "id": "u_abc123def456",
    "email": "user@example.com",
    "username": "zhangsan",
    "display_name": "张三",
    "email_verified": true,
    "created_at": "2026-05-24T00:00:00Z",
    "updated_at": "2026-05-24T00:00:00Z"
  }
}
```

**错误：** `401` — 凭证无效

> **JWT 有效期**：access 30 分钟，refresh 7 天（配置项 `jwt.access_duration_min` / `jwt.refresh_duration_min`）。

#### A.2.3 刷新 Token

```
POST /v1/auth/refresh
```

**请求体：**
```json
{
  "refresh_token": "eyJhbG..."
}
```

**响应 `200`：**
```json
{
  "access_token": "eyJhbG...（新的）",
  "refresh_token": "eyJhbG...（新的）"
}
```

**错误：** `401` — refresh token 无效或过期

#### A.2.4 邮箱验证

```
GET /v1/auth/verify-email?token=xxx
```

无需认证。开发模式下验证链接打印在服务端日志中。

#### A.2.5 获取当前用户

```
GET /v1/me
Authorization: Bearer <access_token>
```

**响应 `200`：** 返回 User 对象（同登录响应中的 `user` 字段）。

> **注意**：当前 `/v1/me` 只返回 user 本身，**不返回 teams 列表**。如需获取团队列表，调用 `GET /v1/teams`。

#### A.2.6 OAuth 登录

```
GET /v1/oauth/google      → 重定向到 Google 授权页
GET /v1/oauth/github       → 重定向到 GitHub 授权页
GET /v1/oauth/google/callback?code=xxx
GET /v1/oauth/github/callback?code=xxx
```

Callback 成功后返回与登录相同的 `TokenResponse` 结构。

> **注意**：这是浏览器跳转流程。MistTerm 桌面端实现为：打开  
> `GET /v1/oauth/{google|github}?redirect_uri=http://127.0.0.1:{port}/callback`，  
> 用户在系统浏览器完成授权后，服务端将浏览器重定向到上述 `redirect_uri`，并携带 `code`（或 `access_token`+`refresh_token`）；桌面端再调用  
> `GET /v1/oauth/{provider}/callback?code=...&redirect_uri=...` 换取 `TokenResponse`（若回调已直接带 token 则省略）。  
> 服务端须将 `http://127.0.0.1:*` 类 redirect 列入 OAuth 白名单。

### A.3 团队（Team）

#### A.3.1 创建团队

```
POST /v1/teams
Authorization: Bearer <access_token>
```

**请求体：**
```json
{
  "name": "运维组",
  "description": "运维团队"     // 可选
}
```

**响应 `201`：**
```json
{
  "id": "team_abc123def456",
  "name": "运维组",
  "description": "运维团队",
  "created_at": "2026-05-24T00:00:00Z",
  "updated_at": "2026-05-24T00:00:00Z"
}
```

> 创建者自动成为该团队的 `admin`。

#### A.3.2 获取我的团队列表

```
GET /v1/teams
Authorization: Bearer <access_token>
```

**响应 `200`：**
```json
{
  "teams": [
    {
      "team": {
        "id": "team_abc123def456",
        "name": "运维组",
        "description": "运维团队",
        "created_at": "2026-05-24T00:00:00Z",
        "updated_at": "2026-05-24T00:00:00Z"
      },
      "role": "admin"
    }
  ]
}
```

> **客户端应缓存此列表**，用于切换当前团队上下文。role 值为 `viewer` / `editor` / `admin`。

#### A.3.3 获取团队详情

```
GET /v1/teams/{team_id}
Authorization: Bearer <access_token>
```

**响应 `200`：** Team 对象。需为该团队成员（viewer+）。

#### A.3.4 团队成员列表

> **服务端 ✅** · **客户端 ✅**（`team_members_dialog.rs`）

```
GET /v1/teams/{team_id}/members
Authorization: Bearer <access_token>
```

**响应 `200`：**
```json
{
  "members": [
    {
      "user_id": "u_abc123",
      "email": "user@example.com",
      "username": "alice",
      "display_name": "Alice",
      "role": "editor"
    }
  ]
}
```

权限：viewer+。404 时客户端提示「接口未就绪」。

#### A.3.5 添加团队成员（服务端 ✅ · admin）

```
POST /v1/teams/{team_id}/members
Authorization: Bearer <access_token>
```

**请求体：**
```json
{
  "user_id": "u_xyz789",
  "role": "editor"
}
```

> 需要 admin 权限。`role` 取值 `viewer` / `editor` / `admin`。

### A.4 团队片段（Fragment）

#### A.4.1 片段数据模型

```json
{
  "id": "frag_abc123def456",
  "team_id": "team_abc123def456",
  "title": "查看磁盘",
  "command": "df -h",
  "category": "disk",            // 可选
  "tags": "[\"disk\", \"system\"]",  // JSON 字符串，默认 "[]"
  "variables": "{\"path\": \"/\"}",  // JSON 字符串，默认 "{}"
  "scope": "team",              // 固定为 "team"
  "status": "published",         // published / draft / archived
  "revision": 1,                // 单调递增，乐观锁
  "created_by": "u_abc123def456",
  "updated_by": "u_abc123def456",
  "created_at": "2026-05-24T00:00:00Z",
  "updated_at": "2026-05-24T00:00:00Z"
}
```

> **重要**：`tags` 和 `variables` 在服务端存储为 JSON 字符串，客户端序列化/反序列化时需注意。

#### A.4.2 列表查询

```
GET /v1/teams/{team_id}/fragments?limit=100&offset=0
Authorization: Bearer <access_token>
```

**权限**：viewer+

**响应 `200`：**
```json
{
  "fragments": [ { /* Fragment */ } ]
}
```

#### A.4.3 创建片段

```
POST /v1/teams/{team_id}/fragments
Authorization: Bearer <access_token>
```

**请求体：**
```json
{
  "title": "查看磁盘",
  "command": "df -h",
  "category": "disk",           // 可选
  "tags": "[\"disk\"]",          // 可选，默认 []
  "variables": "{}"             // 可选，默认 {}
}
```

**权限**：editor+

**响应 `201`：** 完整 Fragment 对象（含服务端生成的 `id`、`revision: 1`、`scope: "team"`、`status: "published"` 等）。

#### A.4.4 获取单条片段

```
GET /v1/fragments/{id}
Authorization: Bearer <access_token>
```

**响应 `200`：** Fragment 对象。

#### A.4.5 更新片段

```
PUT /v1/fragments/{id}
Authorization: Bearer <access_token>
```

**请求体：**
```json
{
  "title": "查看磁盘使用",
  "command": "df -h && du -sh /*",
  "category": "disk",
  "tags": "[\"disk\", \"du\"]",
  "variables": "{}",
  "status": "published",
  "revision": 1              // 必填：客户端当前持有的 revision
}
```

**权限**：editor+

**响应 `200`：** 更新后的 Fragment 对象（`revision` 已递增）。

**冲突 `409`：**
```json
{
  "error": "revision conflict",
  "server_version": { /* 服务端当前 Fragment */ }
}
```
> 客户端需实现冲突解决 UI：以服务端为准 / 保留本地 / 合并 / 取消。

#### A.4.6 删除片段（软删）

```
DELETE /v1/fragments/{id}
Authorization: Bearer <access_token>
```

**权限**：admin

**响应 `200`：**
```json
{ "message": "deleted" }
```

> 软删除，后续 sync 会返回 `deleted_ids` 列表。

#### A.4.7 增量同步（推荐）

```
POST /v1/teams/{team_id}/fragments:sync
Authorization: Bearer <access_token>
```

**请求体：**
```json
{
  "cursor": "上次 sync 返回的 cursor，首次传空字符串",
  "limit": 500               // 可选，默认 500
}
```

**权限**：viewer+

**响应 `200`：**
```json
{
  "cursor": "新 cursor（客户端保存用于下次请求）",
  "fragments": [ { /* 新增或变更的 Fragment */ } ],
  "deleted_ids": [ "frag_xxx", "frag_yyy" ],
  "server_time": "2026-05-24T12:00:00Z"
}
```

> **客户端同步逻辑建议**：
> 1. 首次同步传空 cursor，拿到全量
> 2. 保存返回的 cursor
> 3. 下次同步用保存的 cursor 拿增量
> 4. `fragments` 数组合并到本地缓存（按 id upsert）
> 5. `deleted_ids` 从本地缓存移除
> 6. 新 cursor 替换旧 cursor

### A.5 命令审计（Audit）

#### A.5.1 批量上报

```
POST /v1/audit/events
Authorization: Bearer <access_token>
```

**请求体：**
```json
{
  "events": [
    {
      "event_id": "evt_001",           // 幂等键，重复忽略
      "user_id": "",                    // 可选，为空时服务端自动填当前用户
      "team_id": "team_xxx",            // 可选
      "ts": "2026-05-24T12:00:00Z",      // 可选，为空时服务端填当前时间
      "category": "fragment",
      "action": "fragment.execute",
      "outcome": "success",
      "session_id": "sess_001",          // 可选
      "host": "192.168.1.100",          // 可选
      "resource": "frag_abc123",         // 可选
      "detail": "{\"command\": \"df -h\"}"  // JSON 字符串，可选
    }
  ]
}
```

**响应 `202`：**
```json
{
  "accepted": 5,
  "duplicate": 1
}
```

> `event_id` 幂等：相同的 `event_id` 重复上报会被忽略（计入 `duplicate`）。
> 服务端会自动补全缺失的 `user_id`、`ts`、`event_id`。

#### A.5.2 查询审计（管理端 · 非桌面客户端）

> MistTerm **只上报**（A.5.1），**不调用**本接口。供 Web 管理端或 SIEM 对接。

```
GET /v1/teams/{team_id}/audit/events?category=fragment&action=fragment.execute&user_id=u_xxx&from=2026-05-01T00:00:00Z&to=2026-05-24T00:00:00Z&limit=100&offset=0
Authorization: Bearer <access_token>
```

**权限**：admin

**查询参数（均可选）：**
| 参数 | 说明 |
|------|------|
| `category` | 按分类过滤 |
| `action` | 按动作过滤 |
| `user_id` | 按用户过滤 |
| `from` | 起始时间 RFC3339 |
| `to` | 结束时间 RFC3339 |
| `limit` | 分页大小，默认 100 |
| `offset` | 偏移量 |

**响应 `200`：**
```json
{
  "events": [ { /* AuditEvent */ } ]
}
```

### A.6 订阅与付费（Billing）

> 客户端目前**无需对接**此部分。官网 (`mist-website`) 直接处理 Stripe Checkout 跳转。

| 接口 | 方法 | 认证 |
|------|------|------|
| `/v1/billing/plan` | GET | Bearer |
| `/v1/billing/checkout` | POST | Bearer |
| `/v1/billing/portal` | POST | Bearer |
| `/v1/billing/webhook` | POST | 无（Stripe 回调） |

### A.7 错误响应格式

所有错误响应统一为：
```json
{
  "error": "具体错误信息"
}
```

部分接口有额外字段（如 409 冲突带 `server_version`）。

### A.8 健康检查

```
GET /health
```

无需认证。
```json
{
  "status": "ok",
  "service": "mist-team-server",
  "time": "2026-05-24T12:00:00Z"
}
```

### A.9 ID 生成规则

| 实体 | 前缀 | 示例 |
|------|------|------|
| 用户 | `u_` | `u_abc123def456` |
| 团队 | `team_` | `team_abc123def456` |
| 片段 | `frag_` | `frag_abc123def456` |
| 审计事件 | `evt_` | 服务端自动生成（如果客户端未提供） |

---

> 客户端对上述基线 API 已全部对接；源码索引见 §四。

---

## 二、集成参考

MistTerm 与 `api.mistlab.dev` 的**增量集成说明**。认证、片段 CRUD 等细节见 **附录 A**；**全部服务端接口**见 **§三**。

| 主题 | 状态 | 文档 |
|------|------|------|
| 全部接口总览 | — | **§三 0** |
| 基线 API（登录、sync、片段、审计 POST） | 服务端 ✅ | 附录 A · §三 |
| 命令审计 sync/alerts | 服务端 ✅ | §三 8 · [COMMAND-AUDIT.md](./COMMAND-AUDIT.md) |
| 成员列表 | 服务端 ✅ | 附录 A.3.4 |
| OAuth redirect 白名单 | 服务端 ✅ | §四 3.1 |
| OAuth 桥接页 | 运维 🟠 | §四 3.2 |
| 片段 usage / 成员 analytics | 服务端 🔴 | §三 5.2–5.3 |

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

## 4. 其他基线能力（不重复展开）

| 能力 | 契约 | 客户端 |
|------|------|--------|
| 审计 ingest | 附录 A.5 · 50 条/30s · `evt_{ms}_{hex}` 去重 | `src/core/audit.rs` |
| 认证 / refresh / OAuth | 附录 A.2 | `src/core/team/auth.rs`、`oauth.rs` |
| 成员列表 | 附录 A.3.4 | `team_members_dialog.rs` |
| 错误格式 | 附录 A.7 | 401→refresh；404→降级或提示 |

桌面 OAuth 部署与白名单：见 **§四 3.2–3.3**。

---

## 三、服务端接口清单

> **API Base**：`https://api.mistlab.dev`（与 `TeamSettings.api_base` 一致，可配置）

本节是 **MistTerm 桌面端依赖的全部服务端接口**：已实现与待补齐均列在同一表内，并给出请求/响应契约与 404 降级行为。字段以 `src/core/team/client.rs`、`src/core/market/client.rs` 为准。

**图例**：✅ 已对接且生产须可用 · 🔴 客户端已对接、服务端待实现 · 🟡 建议增强 · 🟠 运维部署 · ➖ 非桌面客户端（管理端）

---

### 0. 总览

| # | 方法 | 路径 | 状态 | 404/失败时客户端行为 | 详细契约 |
|---|------|------|------|----------------------|----------|
| 1 | GET | `/health` | ✅ | — | 附录 A.8 |
| 2 | POST | `/v1/auth/register` | ✅ | 显示注册错误 | 附录 A.2.1 |
| 3 | POST | `/v1/auth/login` | ✅ | 登录失败提示 | 附录 A.2.2 |
| 4 | POST | `/v1/auth/refresh` | ✅ | 401 → 重新登录 | 附录 A.2.3 |
| 5 | GET | `/v1/oauth/{google\|github}` | ✅ | OAuth 不可用提示 | 附录 A.2.6 · §四 3 |
| 6 | GET | `/v1/oauth/{provider}/callback` | ✅ | 登录卡住 | 附录 A.2.6 |
| 7 | GET | `/v1/me` | ✅ | — | 附录 A.2.5 |
| 8 | GET | `/v1/teams` | ✅ | — | 附录 A.3.2 |
| 9 | GET | `/v1/teams/{team_id}` | ✅ | — | 附录 A.3.3 |
| 10 | GET | `/v1/teams/{team_id}/members` | ✅ | 成员弹窗提示未就绪 | 附录 A.3.4 |
| 11 | GET | `/v1/team/sync` | ✅ | 同步条目为空 | **§二 1** |
| 12 | POST | `/v1/teams/{team_id}/fragments:sync` | ✅ | 片段同步失败 | 附录 A.4.7 · §3.4 |
| 13 | POST | `/v1/teams/{team_id}/fragments` | ✅ | 创建失败 | 附录 A.4.3 |
| 14 | PUT | `/v1/fragments/{id}` | ✅ | 409 冲突 UI | 附录 A.4.5 |
| 15 | DELETE | `/v1/fragments/{id}` | ✅ | 删除失败提示 | 附录 A.4.6 |
| 16 | GET | `/v1/teams/{team_id}/fragments/analytics` | ✅ | 大盘仅用本机聚合 | **§3 5.1** |
| 17 | GET | `/v1/teams/{team_id}/fragments/analytics/members?since={N}d` | 🔴 | 成员表仅本机数据 | **§3 5.2** |
| 18 | POST | `/v1/teams/{team_id}/fragments/{fragment_id}/usage` | 🔴 | 静默忽略 | **§3 5.3** |
| 19 | GET | `/v1/market/fragments/catalog` | ✅ | 用本地缓存 + 已安装片段 | **§3 6.1** |
| 20 | POST | `/v1/market/fragments/{id}/install` | ✅ 可选 | 静默忽略 | **§3 6.2** |
| 21 | POST | `/v1/audit/events` | ✅ | 积压 `pending-team-events.jsonl` | **§3 7** |
| 22 | GET | `/v1/teams/{team_id}/command-audit/sync` | ✅ | 仅用本地/内置规则 | **§3 8** · COMMAND-AUDIT |
| 23 | POST | `/v1/teams/{team_id}/command-audit/alerts` | ✅ | debug 日志 | **§3 8** |
| 24 | GET | `/v1/teams/{team_id}/audit/events` | ➖ 管理端 | 桌面端不调用 | **§3 9** |
| — | — | OAuth 桥接页 `mistlab.dev/oauth/desktop-callback.html` | 🟠 | 回退 `127.0.0.1` 回调 | **§四 3** |

**404 约定**：除登录、片段 CRUD 等关键路径外，分析/市场/usage 等接口 404 视为「未部署」，走本地回退，不阻断终端。

---

### 1. 通用约定

| 项 | 说明 |
|----|------|
| 鉴权 | 除 `/health`、`/v1/auth/*`、`/v1/oauth/*` 外，均需 `Authorization: Bearer <access_token>` |
| Content-Type | `application/json` |
| 错误体 | `{ "error": "..." }` 或 `{ "message": "..." }` |
| 片段 tags/variables | **JSON 字符串**（非原生数组），与附录 A.4.1 一致 |
| 分页 | 片段 sync 用 `cursor`；市场 catalog 可选 `cursor` |

---

### 2. 认证与 OAuth（✅）

| 接口 | 说明 |
|------|------|
| `POST /v1/auth/register` · `login` · `refresh` | 邮箱/用户名密码；refresh 轮换 access |
| `GET /v1/oauth/{google\|github}?redirect_uri=…` | 浏览器授权；须白名单 + **302** 回传 token 或 `code` |
| `GET /v1/oauth/{provider}/callback?code=…&redirect_uri=…` | code 模式换票 |

契约：**附录 A.2**。桌面 OAuth 白名单与桥接页：**§四 3**。

---

### 3. 团队、成员、一键同步（✅）

| 接口 | 权限 | 客户端 |
|------|------|--------|
| `GET /v1/me` | Bearer | 登录后展示用户 |
| `GET /v1/teams` | Bearer | 团队列表与 role |
| `GET /v1/teams/{team_id}/members` | viewer+ | 「团队 → 团队成员」 |
| `GET /v1/team/sync` | Bearer | Vault + 团队服务器 + role |

`GET /v1/team/sync` 响应字段与 Vault 映射见 **§二 1–2**。成员响应见附录 A.3.4。

---

### 4. 团队片段 CRUD / sync（✅）

| 接口 | 权限 | 说明 |
|------|------|------|
| `POST …/fragments:sync` | viewer+ | 增量；body `{ "cursor": "", "limit": 500 }` |
| `POST …/fragments` | editor+ | 创建 |
| `PUT /v1/fragments/{id}` | editor+ | 带 `revision`；409 返回 `server_version` |
| `DELETE /v1/fragments/{id}` | admin | 软删，sync 下发 `deleted_ids` |

契约：**附录 A.4**。

#### 4.1 🟡 sync 内嵌统计（建议）

`fragments:sync` 的 `fragments[]` 可携带：

| 字段 | 类型 | 默认 |
|------|------|------|
| `usage_count` | u32 | 0 |
| `success_count` | u32 | 0 |
| `total_time_ms` | u64 | 0 |
| `last_used_at` | i64? | null |

客户端模型 `TeamFragment`（`src/core/team/models.rs`）。

---

### 5. 片段分析（Analytics）

#### 5.1 ✅ 片段聚合 `GET …/fragments/analytics`

```
GET /v1/teams/{team_id}/fragments/analytics
Authorization: Bearer <access_token>
```

权限：viewer+。**响应 `200`：**

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

404 → 客户端 `team_api_available = false`，大盘合并本机 `TeamFragmentCache` + overlay。

#### 5.2 🔴 成员区间 `GET …/fragments/analytics/members`

```
GET /v1/teams/{team_id}/fragments/analytics/members?since=7d
```

`since`：`7d` | `30d` | `90d`（与 `FragmentAnalyticsTimeRange` 一致；全部时间时不请求）。

**响应 `200`：**

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

404 → 仅汇总本机 `fragment_usage_events.json`；UI 标注「仅本机数据」。

#### 5.3 🔴 执行上报 `POST …/fragments/{fragment_id}/usage`

团队片段执行成功后**异步**上报（`TeamClient::report_fragment_usage`）：

```
POST /v1/teams/{team_id}/fragments/{fragment_id}/usage
Content-Type: application/json

{ "success": true, "duration_ms": 1200 }
```

| 响应 | 客户端 |
|------|--------|
| `200` / `204` | 正常 |
| `404` | 静默成功 |
| 其他 | 仅 `debug` 日志 |

服务端建议：与 §5.1 共用聚合表；可按 `(team_id, fragment_id, user_id)` 存明细再 rollup。

---

### 6. 片段市场（Market）

客户端：`src/core/market/`；侧栏 **「市场」** scope。

#### 6.1 ✅ 目录 `GET /v1/market/fragments/catalog`

Bearer **可选**（未登录也可浏览公开目录，若产品允许）。

| Query | 说明 |
|-------|------|
| `category` | 分类，空=全部 |
| `search` | 标题/命令/描述模糊 |
| `limit` | 默认 `200` |
| `cursor` | 分页；「加载更多」带上次响应的 `cursor` |

**响应 `200`：**

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

404 → 提示「市场接口未部署」，继续显示本地缓存。成功写入 `market_fragments_cache.json`（AES）。

#### 6.2 ✅ 安装计数 `POST …/market/fragments/{id}/install`（可选）

```json
{}
```

`200`/`204` 成功；404 客户端忽略，不影响「添加到个人库」。

---

### 7. 审计 ingest（✅ · 客户端只上报）

桌面端**不提供**审计查询 UI；仅后台批量上报。

```
POST /v1/audit/events
Authorization: Bearer <access_token>
Content-Type: application/json
```

**上报策略**：最多 **50 条 / 30 秒**；`event_id` 格式 `evt_{unix_ms}_{8位hex}`；断网写入 `audit/pending-team-events.jsonl`。

**请求体（客户端实际发送）：**

```json
{
  "events": [
    {
      "event_id": "evt_1716969600123_a1b2c3d4",
      "team_id": "team_xxx",
      "ts": "2026-05-24T12:00:00.000Z",
      "category": "fragment",
      "action": "fragment.execute",
      "outcome": "success",
      "session_id": "sess_001",
      "host": "10.0.0.1",
      "resource": "frag_abc",
      "detail": { }
    }
  ]
}
```

**响应 `202`：** `{ "accepted": n, "duplicate": m }`（`event_id` 幂等）。

常见 `category` / `action`：`fragment.*`、`session.connect`、`command.blocked`、`team.login`、`config.vault_*` 等。完整 ingest 字段见附录 A.5.1。

---

### 8. 命令审计（客户端调用）

策略同步与告警上报；完整策略模型见 [COMMAND-AUDIT.md](./COMMAND-AUDIT.md)。

| 方法 | 路径 | 用途 |
|------|------|------|
| GET | `/v1/teams/{team_id}/command-audit/sync` | 拉取 `enabled` / `policy` / `rules` / `sync_interval_sec` |
| POST | `/v1/teams/{team_id}/command-audit/alerts` | 拦截/确认后上报告警 |

**告警请求体（`CmdAuditAlertRequest`）：**

```json
{
  "command": "rm -rf /",
  "matched_rule": "rm_recursive_root",
  "match_level": "dangerous",
  "action_taken": "blocked"
}
```

`action_taken`：`blocked` | `confirmed` | `alerted`。

404 on sync → 客户端仅用内置规则；匹配在本地执行，不走网络。

---

### 9. 管理端：审计查询 AUD-2（➖ 非 MistTerm）

供 Web 管理端 / SIEM；**桌面客户端不调用**。

```
GET /v1/teams/{team_id}/audit/events?category=&action=&user_id=&from=&to=&limit=&offset=
```

权限：admin。契约：§一 4.4.2 · 附录 A.5.2。

---

### 10. 验收清单（后端自测）

**🔴 P1（待实现）**

1. `POST …/fragments/{id}/usage` → 2xx → `GET …/analytics` 中 `usage_count` 递增  
2. `GET …/analytics/members?since=7d` → 200 + 多成员 `run_count` 正确  
3. 上述 404 时客户端分析弹窗仍可用（本机数据）

**✅ 回归**

1. 认证 + OAuth 302（§四 联调 1–4）  
2. `GET /v1/team/sync` 含 servers + vault  
3. `fragments:sync` 全量/增量 + 409 冲突  
4. `GET …/fragments/analytics` · `GET /v1/market/fragments/catalog`  
5. `POST /v1/audit/events` 批量 50 + `duplicate` 去重  
6. `command-audit/sync` + `alerts` 符合 COMMAND-AUDIT  
7. `GET …/members` → 200  

---

### 11. 源码索引

| 能力 | 路径 |
|------|------|
| 团队 HTTP | `src/core/team/client.rs` |
| 团队模型 | `src/core/team/models.rs` |
| 市场 HTTP | `src/core/market/client.rs` |
| 审计上报 | `src/core/audit.rs` |
| 命令审计引擎 | `src/core/cmd_audit.rs` |
| 分析聚合 | `src/core/fragment_analytics.rs` |
| 分析 UI | `src/ui/fragment_analytics_dialog.rs` |

---

## 四、客户端索引

> 服务端待办见 **§三**；API 契约见 **附录 A**。

### 1. 已落地能力

| 能力 | 入口 | 备注 |
|------|------|------|
| 账号 | 偏好设置 → 团队平台；云端同步 → 团队账户 | 邮箱/用户名密码 + Google/GitHub OAuth；OAuth 推荐路径依赖 **运维 🟠** 桥接页（§四 3.2） |
| 团队列表 + 切换 | 同上 | `GET /v1/teams` 缓存到 `team_state.json`；下拉切换；单团队自动选中 |
| 登录后一键同步 | 自动 | `GET /v1/team/sync` → 写 `sync_entries`；404 降级为空；401 走 refresh |
| Vault 自动配置 | 偏好设置 → Vault | `auth_type` token / approle；`kv_mount` → `default_mount`；用户手动改后 `team_auto_apply=false` 不再自动覆盖；提示「来自团队 xxx」 |
| 团队服务器 | 左侧栏「团队服务器」分组 | 按 `sort_order` 排序；点击连接；`vault_credential_path` → `SecretBackend::VaultKv`，否则走本地凭证 |
| 团队片段 | 命令片段侧栏 / 团队 | 增量同步、CRUD、409 冲突解决、定时同步、按 role 控制按钮 |
| 片段分析大盘 | 命令片段 → 分析 | 个人/团队 KPI、Top5、成员对比；`GET .../analytics` + 可选 `.../members`；404 本机回退 |
| 片段市场 | 命令片段侧栏 / 市场 | `GET /v1/market/fragments/catalog`；404 用本地缓存 |
| 命令审计引擎 | 终端 send_command | 本地策略匹配 + 拦截/确认弹窗；sync/alerts 上报 |
| 审计上报 | 后台（无 UI） | `POST /v1/audit/events`（50 条/30s）；离线 `pending-team-events.jsonl` |
| 本地加密 | 自动 | `device_key` (AES-256-GCM) 加密 `settings.json`、`sessions.json`、`fragments.json`、`credentials.json`、`team_tokens.json`、`team_state.json`、`team_fragments_cache.json`（详见 [SECURITY.md](./SECURITY.md)） |
| 旁路防呆 | 自动 | OAuth 旧版钥匙串首次启动迁入 `team_tokens.json` 后删除；模态打开时不绘右 dock Foreground 避免双 × |

### 2. 模块索引

| 模块 | 路径 |
|------|------|
| 团队核心（auth / client / state / cache / service / sync_config / models） | `src/core/team/` |
| 应用设置（含 `TeamSettings`、`CloudSyncSettings`） | `src/core/app_settings.rs` |
| 团队 UI（登录 / 团队选择 / 同步状态） | `src/ui/team_ui.rs` |
| 团队成员列表弹窗 | `src/ui/team_members_dialog.rs` |
| macOS「团队」菜单 | `src/platform/macos_menu.rs` |
| OAuth 桥接验收脚本 | `scripts/verify-oauth-bridge.sh` |
| 团队片段编辑 + 409 冲突解决 | `src/ui/team_fragment_dialog.rs` |
| 命令片段侧栏（个人 / 团队 scope） | `src/ui/app.rs`（`FragmentListScope`） |
| 团队服务器侧栏 | `src/ui/sidebar.rs` |
| 云端同步面板 | `src/ui/cloud_sync_panel.rs` |
| 审计 + 离线队列 | `src/core/audit.rs` |
| `device_key` + AES 加密 | `src/security/{device_key,encrypted_file}.rs` |
| 片段分析 UI | `src/ui/fragment_analytics_dialog.rs` |
| 片段市场 | `src/core/market/` |
| 命令审计引擎 | `src/core/cmd_audit.rs` |
| OAuth 桥接（含本机 127.0.0.1 监听） | `src/core/team/auth.rs::run_browser_oauth` |

---

### 3. OAuth 部署（运维）

基线 API 可用性见 **§三** 状态总览。以下仅列桌面 OAuth 专项。

#### 3.1 桌面 `redirect_uri`（服务端 ✅ + 运维 🟠）

**角色**：服务端 ✅（白名单、302、换票）；运维 🟠（桥接页部署、Google/GitHub OAuth App）。客户端 ✅ 已实现桥接探测与 `127.0.0.1` 回退。

桌面端**不会**读取网站 `localStorage`；必须在 OAuth 完成后把 token **重定向回客户端**。

**授权入口（浏览器打开）：**

```
GET /v1/oauth/{google|github}?redirect_uri=<url_encoded>
```

**客户端使用的 `redirect_uri`：**

| 用途 | 值 |
|------|----|
| 授权跳转（主路径） | `https://mistlab.dev/oauth/desktop-callback.html` |
| 本机监听 | `http://127.0.0.1:{动态端口}/callback`（`127.0.0.1:0` 绑定；桥接页 URL 带 `?port=`） |

**服务端须：**

1. 校验 `redirect_uri` 在白名单内（见下表）
2. 与 Google / GitHub 完成授权后，**302 到该 `redirect_uri`** 并携带下列之一：
   - 推荐（与现网网页一致）：`?access_token=...&refresh_token=...`（JWT 与 `POST /v1/auth/login` 响应相同）
   - 或：`?code=...`（桌面会再请求 callback 换 token）
3. Query 必须用 `&` 拼接：`?port=54020&access_token=...`（禁止 `?port=54020?access_token=...`，否则桥接页端口解析失败）

**白名单建议（至少包含）：**

```
https://mistlab.dev/login
https://mistlab.dev/dashboard
https://mistlab.dev/oauth/desktop-callback.html
https://mistlab.dev/oauth/desktop-callback.html?port={n}
http://127.0.0.1:*/callback
```

**换票接口（若回调只带 `code`）：**

```
GET /v1/oauth/{google|github}/callback?code=...&redirect_uri=...
```

响应 JSON 与登录相同（含 `access_token` / `refresh_token` / `user`）。`redirect_uri` 须与授权请求一致，用于校验 state / PKCE。

#### 3.2 OAuth 桥接页（运维 🟠 · 推荐）

**角色**：运维 🟠 必须部署；服务端 ✅ 已将下列 URL 加入 OAuth `redirect_uri` 白名单。客户端 ✅ 在桥接页不可达时回退本机回调。

仓库已提供：`docs/product/oauth-desktop-callback.html`，部署到 `https://mistlab.dev/oauth/desktop-callback.html`。

**运维验收**（仓库脚本，不依赖 MistTerm 运行）：

```bash
./scripts/verify-oauth-bridge.sh
# 或: curl -sI https://mistlab.dev/oauth/desktop-callback.html  # 期望 HTTP 200
```

工作机制：当 Google / GitHub 回调到该页且 URL 带 `access_token` / `refresh_token`（或 `code`）时，从 `?port=` 读取本机端口，`fetch http://127.0.0.1:{port}/callback?...` 交给 MistTerm；成功后约 0.8 s 自动 `window.close()`。

> 若服务端能**直接** 302 到 `http://127.0.0.1:{port}/callback?...`，桥接页可省略，但须在 Google Cloud / GitHub OAuth App 中允许 `http://127.0.0.1` 回调。

#### 3.3 OAuth 应用配置

| 平台 | 配置 |
|------|------|
| Google OAuth | Authorized redirect URIs 包含服务端 callback **及**（若走桥接）`https://mistlab.dev/oauth/desktop-callback.html` |
| GitHub OAuth App | Authorization callback URL 同上 |

服务端对外 callback 一般为 `https://api.mistlab.dev/v1/oauth/google/callback`（以实际为准）。

#### 3.4 CORS / 网络

| 项 | 说明 |
|----|------|
| CORS | 允许 `https://mistlab.dev`；桥接页 fetch 本机用 `no-cors`，不依赖 API CORS |
| 本机端口 | 客户端动态绑定 `127.0.0.1:0`，URL 中以 `?port=` 透传 |
| 防火墙 | 用户本机须能监听 127.0.0.1（一般无问题） |

#### 3.5 网页登录 vs 桌面登录

| 场景 | 行为 |
|------|------|
| 用户在 mistlab.dev 点 Google 登录 | 仅网站 session / `localStorage`，**不会**自动登录 MistTerm |
| 用户在 MistTerm 点 Google / GitHub | 必须走上文 3.1 的 `redirect_uri` 回传 token |
| 仅密码 | 桌面 `POST /v1/auth/login`，与网页账号体系相同 |

---

### 4. 联调验收脚本

**前提**：**§三** 中 🔴 / 🟠 项已就绪。

1. `curl -sI "https://api.mistlab.dev/health"` → 200
2. `./scripts/verify-oauth-bridge.sh` → OK（**运维 🟠** 桥接页）
3. `curl -sI "https://api.mistlab.dev/v1/oauth/google?redirect_uri=http%3A%2F%2F127.0.0.1%3A8765%2Fcallback"` → **302** 到 Google（非 404）
4. MistTerm 点 Google → 浏览器授权 → 本机出现「登录成功」页 → 终端显示已登录
5. `GET /v1/me`、`GET /v1/teams` 正常
6. `GET /v1/team/sync` 返回至少 1 个 team + servers
7. Vault 自动填入；连接团队服务器 → 走 Vault 路径或本地凭证两条路径都通
8. `fragments:sync` 全量 + 增量；故意双端编辑测 409 弹窗
9. 执行命令、SCP → 服务端 `POST /v1/audit/events` 可查 `accepted` / `duplicate`（桌面端无审计查看界面）
10. 断网操作 → 恢复网络后 `pending-team-events.jsonl` 自动 flush
11. 等 access 过期或缩短 JWT 测 refresh / 401 重登
12. `GET /v1/teams/{current_team_id}/members` → 200；「团队 → 团队成员」有列表
13. 执行团队片段 → `POST .../fragments/{id}/usage` 2xx（**§三 P1**）；`GET .../analytics/members?since=7d` 返回全团队成员统计
14. 打开分析大盘 → 团队 KPI 与成员表正常（服务端未部署时仍可用本机数据）
