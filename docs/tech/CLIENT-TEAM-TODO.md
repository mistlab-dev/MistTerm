# 客户端团队功能对接 TODO

> 更新：2026-05-24  
> 对接服务端 API 文档见：[TEAM-PLATFORM-DEV-PLAN.md](./TEAM-PLATFORM-DEV-PLAN.md) **附录 A**  
> 默认 API：`https://api.mistlab.dev`（站点 `mistlab.dev`）

---

## 一、设置与账号（P0）

### 1.1 api_base 配置
- [x] `app_settings.team.api_base`（`TeamSettings`）
- [x] 为空时隐藏团队入口（片段侧栏个人/团队切换、云端同步团队区等）
- [x] 填写后展示团队相关 UI

### 1.2 登录/注册 UI
- [x] 偏好设置「团队平台」+ 云端同步「团队账户」
- [x] `POST /v1/auth/login`（email / username）
- [x] `POST /v1/auth/register`
- [x] 桌面 OAuth：`GET /v1/oauth/google|github` + 本地 `127.0.0.1` 回调（`OAuthProvider` / `run_browser_oauth`）
- [x] 登录成功后存 access_token + refresh_token

### 1.3 Token 管理
- [x] 密钥链 `MistTerm-Team`
- [x] access 过期前 `POST /v1/auth/refresh`（JWT `exp` 判断）
- [x] 刷新失败清除 token + 本地 session，提示重新登录
- [x] API 401 时强制 refresh 并重试一次（`with_auth_retry`）

### 1.4 用户信息
- [x] 登录后 `GET /v1/me`（登录流程内）
- [x] UI 展示 display_name、email

---

## 二、团队选择（P0）

### 2.1 获取团队列表
- [x] `GET /v1/teams`，缓存 `team_state.json`

### 2.2 团队切换
- [x] 下拉选择当前团队
- [x] 持久化 `current_team_id`
- [x] 单团队自动选中

---

## 三、团队片段同步（P0）

### 3.1 增量同步
- [x] `POST /v1/teams/{team_id}/fragments:sync`
- [x] cursor / limit / upsert / deleted_ids
- [x] 与个人 `fragments.json` 分离（`team_fragments_cache.json`）

### 3.2 片段列表展示
- [x] 命令片段侧栏「个人 / 团队」
- [x] 团队片段标签 `@团队名`
- [x] 按 role 控制新建/编辑/删除按钮

### 3.3 创建团队片段
- [x] `POST /v1/teams/{team_id}/fragments` + 新建弹窗

### 3.4 编辑团队片段
- [x] `PUT /v1/fragments/{id}` + revision + 编辑弹窗

### 3.5 删除团队片段
- [x] `DELETE /v1/fragments/{id}`（admin）

### 3.6 冲突解决（409）
- [x] 409 弹窗：以服务端为准 / 保留本地 / 合并 / 取消

### 3.7 定时同步
- [x] `CloudSyncSettings.frequency_minutes` 后台 sync
- [x] 失败写入 `team_state.last_error`，手动重试

---

## 四、审计上报（P1）

### 4.1 团队审计上报
- [x] 登录后 HTTP sink → `{api_base}/v1/audit/events`
- [x] 请求体含 `team_id`、标准 event 字段

### 4.2 离线队列
- [x] flush 失败时事件回队（内存队列，溢出丢弃最旧）

### 4.3 埋点补全
- [x] `fragment.insert` / `fragment.execute`
- [x] `fragment.create` / `fragment.update` / `fragment.delete`
- [x] `fragment.sync_pull`
- [x] `session.connect` / `session.disconnect`
- [x] `team.login`
- [ ] `team.token_refresh`（刷新成功未单独打点，可后续补）

---

## 五、UI 调整（P1）

### 5.1 团队入口
- [x] 偏好设置 + 云端同步面板（未配置或未登录不显示团队区）
- [ ] 系统菜单独立「团队」项（可用偏好/云端同步代替）

### 5.2 同步状态
- [x] 最近同步时间、错误、手动同步

### 5.3 成员信息
- [x] `GET /v1/teams/{team_id}` 拉取团队描述展示
- [ ] 成员列表 UI（需服务端成员列表 API 或扩展 team 详情）

---

## 六、边界情况处理

### 6.1 未登录兼容
- [x] 个人功能不受影响
- [x] `fragments.json` 旧格式 serde default

### 6.2 多团队
- [x] 切换 team_id，审计 `team_id` 随当前团队更新

### 6.3 Token 过期
- [x] 401 → refresh → 失败 logout，不阻断本地 SSH/终端

---

## 实现文件索引

| 模块 | 路径 |
|------|------|
| 核心 API | `src/core/team/` |
| 设置 | `src/core/app_settings.rs` → `team` |
| 团队 UI | `src/ui/team_ui.rs` |
| 片段 CRUD/冲突 | `src/ui/team_fragment_dialog.rs` |
| 片段侧栏 | `src/ui/app.rs`（`FragmentListScope`） |
| 云端同步 | `src/ui/cloud_sync_panel.rs` |
| 审计 | `src/core/audit.rs`（team events body + 失败回队） |

---

## 七、服务端配合清单（桌面 OAuth / 团队登录）

> 给 `mist-team-server` / 运维：以下未满足时，**网页可登录但 MistTerm 无法 Google/GitHub 登录**。

### 7.1 API 必须对外可用

| 项 | 要求 |
|----|------|
| Base URL | 生产固定 `https://api.mistlab.dev`（与 `mistlab.dev/assets/js/api.js` 中 `API_BASE` 一致） |
| 健康检查 | `GET /health` 或等价探活（建议 200） |
| 密码登录 | `POST /v1/auth/login`、`POST /v1/auth/refresh` 可用（桌面密码登录依赖） |
| OAuth 入口 | `GET /v1/oauth/google`、`GET /v1/oauth/github` **不可 404**（2026-05-24 联调曾返回 404） |

### 7.2 OAuth：支持桌面 `redirect_uri`

桌面端**不会**读取网站 `localStorage`；必须在 OAuth 完成后把 token **重定向回客户端**。

**授权入口（浏览器打开）：**

```
GET /v1/oauth/{google|github}?redirect_uri=<url_encoded>
```

客户端当前传参：

| 用途 | `redirect_uri` 值 |
|------|-------------------|
| 授权跳转（主路径） | `https://mistlab.dev/oauth/desktop-callback.html` |
| 本机监听 | `http://127.0.0.1:8765/callback`（端口见 `OAUTH_LOCAL_PORT`，失败时尝试 8766–8770） |

**服务端须：**

1. 校验 `redirect_uri` 在白名单内（见下表）
2. 与 Google/GitHub 完成授权后，**302 到该 `redirect_uri`**，并携带下列之一：
   - **推荐（与现网网页一致）**：`?access_token=...&refresh_token=...`（JWT 与 `POST /v1/auth/login` 响应相同）
   - **或**：`?code=...`（桌面会再请求下方 callback 换 token）

**白名单建议（至少包含）：**

```
https://mistlab.dev/login
https://mistlab.dev/dashboard
https://mistlab.dev/oauth/desktop-callback.html
http://127.0.0.1:8765/callback
http://127.0.0.1:8766/callback
…（8767–8770，与桥接页一致）
```

**换票接口（若回调只带 `code`）：**

```
GET /v1/oauth/{google|github}/callback?code=...&redirect_uri=...
```

响应 JSON 与登录相同：

```json
{
  "access_token": "eyJ...",
  "refresh_token": "eyJ...",
  "user": { "id", "email", "username", "display_name", ... }
}
```

`redirect_uri` 须与授权请求一致，用于校验 state/PKCE。

### 7.3 静态站：部署 OAuth 桥接页（推荐）

仓库已提供：`docs/product/oauth-desktop-callback.html`

**部署到：** `https://mistlab.dev/oauth/desktop-callback.html`

作用：当 Google/GitHub 回调到该页且 URL 带 `access_token` / `refresh_token`（或 `code`）时，用 JS 请求 `http://127.0.0.1:8765/callback?...`，把 token 交给已启动的 MistTerm。

> 若服务端能**直接** 302 到 `http://127.0.0.1:8765/callback?...`，桥接页可省略，但须在 Google Cloud / GitHub OAuth App 中允许 `http://127.0.0.1` 回调。

### 7.4 Google / GitHub 开发者控制台

| 平台 | 配置 |
|------|------|
| Google OAuth | Authorized redirect URIs 包含服务端 callback **及**（若走桥接）`https://mistlab.dev/oauth/desktop-callback.html` |
| GitHub OAuth App | Authorization callback URL 同上 |

服务端对外的 callback 一般为 `https://api.mistlab.dev/v1/oauth/google/callback`（以实际实现为准）。

### 7.5 CORS / 网络

| 项 | 说明 |
|----|------|
| CORS | 文档已写允许 `https://mistlab.dev`；桥接页 fetch 本机为 `no-cors`，不依赖 API CORS |
| 本机端口 | 文档 A.1 已写 `http://localhost:8765`；客户端绑定 `127.0.0.1:8765` |
| 防火墙 | 用户本机须能监听 127.0.0.1:8765（一般无问题） |

### 7.6 网页登录 vs 桌面登录（产品约定）

| 场景 | 行为 |
|------|------|
| 用户在 **mistlab.dev** 点 Google 登录 | 仅网站 session / `localStorage`，**不会**自动登录 MistTerm |
| 用户在 **MistTerm** 点 Google/GitHub | 必须走 §7.2 的 `redirect_uri` 回传 token |
| 仅密码 | 桌面 `POST /v1/auth/login`，与网页账号体系相同 |

### 7.7 联调验收（OAuth）

1. `curl -sI "https://api.mistlab.dev/v1/oauth/google?redirect_uri=http%3A%2F%2F127.0.0.1%3A8765%2Fcallback"` → **302** 到 Google（非 404）
2. MistTerm 点 Google → 浏览器授权 → 本机出现「登录成功」页 → 终端显示已登录
3. `GET /v1/me`、`GET /v1/teams` 正常
4. GitHub 流程同上

---

## 部署服务端后建议联调顺序

1. `GET https://api.mistlab.dev/health`（若有）
2. 注册 → 登录 → `GET /v1/me` → `GET /v1/teams`
3. `fragments:sync` 全量 + 增量
4. 创建/编辑片段 → 故意双端编辑测 409
5. `POST /v1/audit/events` 看 accepted/duplicate
6. 等 access 过期或改短 JWT 测 refresh / 401 重登
