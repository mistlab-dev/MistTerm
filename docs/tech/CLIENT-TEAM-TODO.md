# 客户端团队功能

> **更新**：2026-05-25  
> 服务端 API 契约：[TEAM-PLATFORM-API.md](./TEAM-PLATFORM-API.md)  
> 整体方案与服务端职责：[TEAM-PLATFORM-DEV-PLAN.md](./TEAM-PLATFORM-DEV-PLAN.md)  
> 数据/审计加密：[security.md](./security.md)  
> **API Base**：`https://api.mistlab.dev`

本文只跟踪**客户端侧**的实现状态、剩余待办，与服务端/运维需要的协议清单。「怎么用」和「字段含义」请去集成指南，不在这里复述。

---

## 一、当前状态

### 1.1 已落地能力

| 能力 | 入口 | 备注 |
|------|------|------|
| 账号 | 偏好设置 → 团队平台；云端同步 → 团队账户 | 邮箱/用户名密码 + Google/GitHub OAuth；access/refresh 自动续约；401 → refresh + retry；登录后 `GET /v1/me` |
| 团队列表 + 切换 | 同上 | `GET /v1/teams` 缓存到 `team_state.json`；下拉切换；单团队自动选中 |
| 登录后一键同步 | 自动 | `GET /v1/team/sync` → 写 `sync_entries`；404 降级为空；401 走 refresh |
| Vault 自动配置 | 偏好设置 → Vault | `auth_type` token / approle；`kv_mount` → `default_mount`；用户手动改后 `team_auto_apply=false` 不再自动覆盖；提示「来自团队 xxx」 |
| 团队服务器 | 左侧栏「团队服务器」分组 | 按 `sort_order` 排序；点击连接；`vault_credential_path` → `SecretBackend::VaultKv`，否则走本地凭证 |
| 团队片段 | 命令片段侧栏 / 团队 | 增量同步、CRUD、409 冲突解决（服务端 / 保留本地 / 合并 / 取消）、按 `CloudSyncSettings.frequency_minutes` 定时同步、按 role 控制按钮 |
| 审计上报 | 后台线程 | `fragment.*` / `shell.connect / exec` / `file.scp.*` / `team.login` / `team.token_refresh` / `config.vault_*`；HTTP sink → `POST /v1/audit/events`；离线持久化 `audit/pending-team-events.jsonl` |
| 本地加密 | 自动 | `device_key` (AES-256-GCM) 加密 `settings.json`、`sessions.json`、`fragments.json`、`credentials.json`、`team_tokens.json`、`team_state.json`、`team_fragments_cache.json`（详见 [security.md](./security.md)） |
| 旁路防呆 | 自动 | OAuth 旧版钥匙串首次启动迁入 `team_tokens.json` 后删除；模态打开时不绘右 dock Foreground 避免双 × |

### 1.2 待办

#### P1（功能性优化，可在现有结构上补）
- [ ] 审计 batch 策略：30 s 或满 50 条（当前 HTTP sink 默认 20 条 / 2 s，对齐集成指南建议）
- [ ] 审计 `event_id` 改 `evt_{ts}_{random}`（当前 UUID，便于服务端按时间窗去重）

#### P2 / 依赖外部
- [ ] 系统菜单独立「团队」项（目前由偏好设置 + 云端同步面板代替）
- [ ] 成员列表 UI（**依赖**服务端先暴露成员列表 API 或扩展团队详情）
- [ ] 确认 `https://mistlab.dev/oauth/desktop-callback.html` 已部署，且 `?port=` 与授权 URL 拼接正确

---

## 二、模块索引

| 模块 | 路径 |
|------|------|
| 团队核心（auth / client / state / cache / service / sync_config / models） | `src/core/team/` |
| 应用设置（含 `TeamSettings`、`CloudSyncSettings`） | `src/core/app_settings.rs` |
| 团队 UI（登录 / 团队选择 / 同步状态） | `src/ui/team_ui.rs` |
| 团队片段编辑 + 409 冲突解决 | `src/ui/team_fragment_dialog.rs` |
| 命令片段侧栏（个人 / 团队 scope） | `src/ui/app.rs`（`FragmentListScope`） |
| 团队服务器侧栏 | `src/ui/sidebar.rs` |
| 云端同步面板 | `src/ui/cloud_sync_panel.rs` |
| 审计 + 离线队列 | `src/core/audit.rs` |
| `device_key` + AES 加密 | `src/security/{device_key,encrypted_file}.rs` |
| OAuth 桥接（含本机 127.0.0.1 监听） | `src/core/team/auth.rs::run_browser_oauth` |

---

## 三、服务端 / 运维配合清单

> 以下未满足时，**网页可登录但 MistTerm 无法 OAuth 登录或团队同步失败**。

### 3.1 接口必须对外可用

| 项 | 要求 |
|----|------|
| Base URL | 生产固定 `https://api.mistlab.dev`（与 `mistlab.dev/assets/js/api.js` 中 `API_BASE` 一致） |
| 健康检查 | `GET /health` 或等价探活（建议 200） |
| 密码登录 | `POST /v1/auth/login`、`POST /v1/auth/refresh` |
| OAuth 入口 | `GET /v1/oauth/google`、`GET /v1/oauth/github`（**不可 404**） |
| 团队同步 | `GET /v1/team/sync`（详见 [集成指南 §1](./TEAM-PLATFORM-API.md)） |
| 审计入口 | `POST /v1/audit/events`（详见 [集成指南 §4](./TEAM-PLATFORM-API.md)） |

### 3.2 OAuth：支持桌面 `redirect_uri`

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

### 3.3 OAuth 桥接页

仓库已提供：`docs/product/oauth-desktop-callback.html`，部署到 `https://mistlab.dev/oauth/desktop-callback.html`。

工作机制：当 Google / GitHub 回调到该页且 URL 带 `access_token` / `refresh_token`（或 `code`）时，从 `?port=` 读取本机端口，`fetch http://127.0.0.1:{port}/callback?...` 交给 MistTerm；成功后约 0.8 s 自动 `window.close()`。

> 若服务端能**直接** 302 到 `http://127.0.0.1:{port}/callback?...`，桥接页可省略，但须在 Google Cloud / GitHub OAuth App 中允许 `http://127.0.0.1` 回调。

### 3.4 OAuth 应用配置

| 平台 | 配置 |
|------|------|
| Google OAuth | Authorized redirect URIs 包含服务端 callback **及**（若走桥接）`https://mistlab.dev/oauth/desktop-callback.html` |
| GitHub OAuth App | Authorization callback URL 同上 |

服务端对外 callback 一般为 `https://api.mistlab.dev/v1/oauth/google/callback`（以实际为准）。

### 3.5 CORS / 网络

| 项 | 说明 |
|----|------|
| CORS | 允许 `https://mistlab.dev`；桥接页 fetch 本机用 `no-cors`，不依赖 API CORS |
| 本机端口 | 客户端动态绑定 `127.0.0.1:0`，URL 中以 `?port=` 透传 |
| 防火墙 | 用户本机须能监听 127.0.0.1（一般无问题） |

### 3.6 网页登录 vs 桌面登录（产品约定）

| 场景 | 行为 |
|------|------|
| 用户在 mistlab.dev 点 Google 登录 | 仅网站 session / `localStorage`，**不会**自动登录 MistTerm |
| 用户在 MistTerm 点 Google / GitHub | 必须走 §3.2 的 `redirect_uri` 回传 token |
| 仅密码 | 桌面 `POST /v1/auth/login`，与网页账号体系相同 |

---

## 四、联调验收脚本

服务端就绪后按顺序跑通即可：

1. `curl -sI "https://api.mistlab.dev/health"` → 200
2. `curl -sI "https://api.mistlab.dev/v1/oauth/google?redirect_uri=http%3A%2F%2F127.0.0.1%3A8765%2Fcallback"` → **302** 到 Google（非 404）
3. MistTerm 点 Google → 浏览器授权 → 本机出现「登录成功」页 → 终端显示已登录
4. `GET /v1/me`、`GET /v1/teams` 正常
5. `GET /v1/team/sync` 返回至少 1 个 team + servers
6. Vault 自动填入；连接团队服务器 → 走 Vault 路径或本地凭证两条路径都通
7. `fragments:sync` 全量 + 增量；故意双端编辑测 409 弹窗
8. 执行命令、SCP → `POST /v1/audit/events` 后台可查（`accepted` / `duplicate`）
9. 断网操作 → 恢复网络后 `pending-team-events.jsonl` 自动 flush
10. 等 access 过期或缩短 JWT 测 refresh / 401 重登
