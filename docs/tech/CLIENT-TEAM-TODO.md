# 客户端团队功能对接 TODO

> 更新：2026-05-24  
> 对接服务端 API 文档见：[TEAM-PLATFORM-DEV-PLAN.md](./TEAM-PLATFORM-DEV-PLAN.md) **附录 A**

---

## 一、设置与账号（P0）

### 1.1 api_base 配置
- [ ] `app_settings.rs` 或 `preferences_dialog.rs` 添加 `team_api_base: String` 配置项
- [ ] 为空时隐藏所有团队入口（现有行为不变）
- [ ] 填写后展示团队相关 UI

### 1.2 登录/注册 UI
- [ ] 新增登录/注册对话框（或嵌入 preferences）
- [ ] 调用 `POST /v1/auth/login`，支持 email 或 username 登录
- [ ] 调用 `POST /v1/auth/register` 注册
- [ ] 登录成功后存 access_token + refresh_token

### 1.3 Token 管理
- [ ] access_token 存系统密钥链（macOS Keychain / Windows Credential Manager / Linux secret-service）
- [ ] refresh_token 同上
- [ ] access 过期前自动调用 `POST /v1/auth/refresh` 刷新
- [ ] 刷新失败 → 清除 token，提示重新登录
- [ ] JWT 解码判断过期（不用额外请求）

### 1.4 用户信息
- [ ] 登录后调 `GET /v1/me` 获取用户信息
- [ ] 在设置/UI 中展示当前登录用户名、邮箱

---

## 二、团队选择（P0）

### 2.1 获取团队列表
- [ ] 调用 `GET /v1/teams`，返回 `{ "teams": [{ "team": {...}, "role": "editor" }] }`
- [ ] 缓存到本地

### 2.2 团队切换
- [ ] UI：下拉选择当前团队
- [ ] 记住上次选择的 `team_id`
- [ ] 只有一个团队时自动选中

---

## 三、团队片段同步（P0）

### 3.1 增量同步
- [ ] 调用 `POST /v1/teams/{team_id}/fragments:sync`
- [ ] 请求体：`{ "cursor": "上次保存的", "limit": 500 }`
- [ ] 首次传空 cursor 拉全量
- [ ] 响应：`{ "cursor", "fragments", "deleted_ids", "server_time" }`
- [ ] 本地保存 cursor，下次用
- [ ] `fragments` 按 id upsert 到本地缓存
- [ ] `deleted_ids` 从本地缓存移除
- [ ] 区分 `scope: "team"` vs 个人片段（本地 `fragments.json`）

### 3.2 片段列表展示
- [ ] FragmentPanel 区分「个人 / 团队」两个 tab 或分组
- [ ] 团队片段标记来源团队名
- [ ] 根据 role 控制操作按钮：viewer 只读、editor 可编辑、admin 可删除

### 3.3 创建团队片段
- [ ] 调用 `POST /v1/teams/{team_id}/fragments`
- [ ] 请求体：`{ "title", "command", "category?", "tags?", "variables?" }`
- [ ] `tags` 和 `variables` 传 JSON 字符串（如 `"[]"` `"{}"`）
- [ ] 权限：editor+

### 3.4 编辑团队片段
- [ ] 调用 `PUT /v1/fragments/{id}`
- [ ] **必须带 `revision` 字段**（当前持有的版本号）
- [ ] 成功后本地更新缓存

### 3.5 删除团队片段
- [ ] 调用 `DELETE /v1/fragments/{id}`
- [ ] 软删除，下次 sync 会在 `deleted_ids` 里
- [ ] 权限：admin

### 3.6 冲突解决（409）
- [ ] 编辑时收到 `409` 响应，body 含 `server_version`
- [ ] 弹窗让用户选择：以服务端为准 / 保留本地 / 合并 / 取消
- [ ] 选择后重新提交

### 3.7 定时同步
- [ ] 复用 `CloudSyncSettings.frequency_minutes`
- [ ] 后台线程定时调用 `fragments:sync`
- [ ] 失败时展示错误信息，自动重试

---

## 四、审计上报（P1）

### 4.1 团队审计上报
- [ ] 复用现有 `AuditLogger` 的 HTTP sink 通道
- [ ] 已登录团队时，`POST /v1/audit/events` 批量上报
- [ ] 请求体：`{ "events": [{ "event_id", "category", "action", "outcome", ... }] }`
- [ ] `event_id` 幂等（UUID，重复忽略）
- [ ] 服务端自动补全 `user_id`、`ts`

### 4.2 离线队列
- [ ] 断网时事件入本地队列
- [ ] 恢复网络后批量补报
- [ ] 队列溢出时丢弃最旧的

### 4.3 埋点补全
- [ ] 补充以下 action 埋点（与设计文档 4.4.3 对齐）：
  - `fragment.insert` / `fragment.execute` — 片段插入/执行
  - `fragment.create` / `fragment.update` / `fragment.delete` — 片段 CRUD
  - `fragment.sync_pull` — 同步拉取
  - `session.connect` / `session.disconnect` — 会话连接/断开
  - `team.login` / `team.token_refresh` — 团队认证

---

## 五、UI 调整（P1）

### 5.1 团队入口
- [ ] 侧边栏或菜单添加「团队」入口
- [ ] 未配置 `team_api_base` 或未登录时不显示

### 5.2 同步状态
- [ ] 展示最近同步时间
- [ ] 展示同步错误
- [ ] 手动触发同步按钮

### 5.3 成员信息
- [ ] 展示当前团队成员列表（可选，调 `GET /v1/teams/{team_id}` 获取）

---

## 六、边界情况处理

### 6.1 未登录兼容
- [ ] 未登录时所有个人功能正常，无团队入口
- [ ] 旧版 `fragments.json` 无新字段时仍可加载（serde default）

### 6.2 多团队
- [ ] 支持切换当前活跃团队
- [ ] 片段/审计绑定当前 team_id

### 6.3 Token 过期
- [ ] 收到 `401` → 尝试 refresh → 失败则清除登录态，提示重新登录
- [ ] 不阻断任何本地功能

---

## 参考文件

| 文件 | 说明 |
|------|------|
| [TEAM-PLATFORM-DEV-PLAN.md 附录 A](./TEAM-PLATFORM-DEV-PLAN.md) | 服务端 API 完整细节（请求/响应/错误码） |
| [API.md](./API.md) | 通用 API 设计文档 |
| `src/core/cloud_sync.rs` | 现有同步配置结构 |
| `src/ui/cloud_sync_panel.rs` | 现有同步面板（改为团队 API） |
| `src/core/audit.rs` | 现有审计模块（有 HTTP sink，可复用） |
| `src/core/app_settings.rs` | 应用设置（添加 team_api_base） |
| `src/ui/preferences_dialog.rs` | 偏好设置对话框（添加团队配置页） |

---

## 服务端 API 速查

```
POST /v1/auth/register          注册
POST /v1/auth/login             登录 → { access_token, refresh_token, user }
POST /v1/auth/refresh           刷新 → { access_token, refresh_token }
GET  /v1/me                     当前用户
GET  /v1/teams                  我的团队列表 → { teams: [{ team, role }] }
POST /v1/teams                  创建团队
GET  /v1/teams/:team_id         团队详情
POST /v1/teams/:team_id/members 添加成员（admin）
GET  /v1/teams/:team_id/fragments    片段列表
POST /v1/teams/:team_id/fragments    创建片段（editor+）
POST /v1/teams/:team_id/fragments:sync  增量同步（viewer+）
GET  /v1/fragments/:id          片段详情
PUT  /v1/fragments/:id          更新片段（editor+，带 revision）
DELETE /v1/fragments/:id        删除片段（admin）
POST /v1/audit/events           批量审计上报
GET  /v1/teams/:team_id/audit/events  审计查询（admin）
```

> 认证方式：`Authorization: Bearer <access_token>`
> 错误格式：`{ "error": "具体信息" }`
