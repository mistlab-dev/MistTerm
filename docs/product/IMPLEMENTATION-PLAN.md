# MistTerm 规格对齐实施计划

> 更新：2026-05-29  
> 对照：`FUNCTIONAL_SPEC.md`、`COMMAND-AUDIT.md`、`CLIENT-GAP-ANALYSIS.md`

## 状态总览

| 阶段 | 项 | 状态 |
|------|-----|------|
| **P1** | SSH ProxyJump 多跳 | ✅ 已完成 |
| **P1** | 会话 proxy_jump / 编辑 UI | ✅ 已完成 |
| **P2** | 云端同步（团队配置/偏好勾选） | ✅ 已完成 |
| **P2** | 团队成员入口（Win/Linux） | ✅ 已完成 |
| **P2** | Git 同步入口 + sessions 脱敏导出 | ✅ 已完成 |
| **P2** | FUNCTIONAL_SPEC §1.4/§5/跳板说明 | ✅ 已完成 |
| **P3** | 命令审计引擎 + 发送拦截 | ✅ 已完成 |
| **P3** | 审计策略 sync + 本地加密缓存 | ✅ 已完成 |
| **P3** | 审计告警 POST + read 内置模式 | ✅ 已完成 |
| **P3** | Git pull 合并保留本地密码 | ✅ 已完成 |
| **P3** | ProxyCommand | ✅ 已完成 |
| **P3** | SSH 主机密钥 known_hosts | ✅ 已完成 |
| **P3** | 本地端口转发 (-L) | ✅ 已完成 |
| **P3** | 片段使用统计面板 | ✅ 已完成（个人库 Top5） |
| **P3** | 片段 market 来源筛选 | ✅ 已完成 |
| **P4** | Tab 终端分屏（MVP：左右/上下 2 窗、合并、Alt+←→ 焦点） | ✅ MVP |
| **P4** | 远程端口转发 (-R) | ✅ 已完成 |
| **P4** | Dynamic 端口转发 (-D / SOCKS5) | ✅ 已完成 |
| **P4** | 片段分析大盘（本地 + 可选 API） | ✅ 已完成 |
| **P4** | 分屏增强（≤4 窗格、关单窗格、窄屏合并） | ✅ 已完成 |
| **P4** | 片段市场 catalog | ✅ 客户端已对接；服务端见 [SERVER-API-BACKEND.md](../tech/SERVER-API-BACKEND.md) §2 |
| **运维** | OAuth 白名单 / members API 等 | ⬜ 服务端，非客户端 |

## 本批交付范围（P3 收尾）

1. **Git pull**：`sessions.json` 按 id 合并，`<encrypted_local>` 不覆盖本机已加密密码  
2. **ProxyCommand**：`%h/%p/%r/%u` 展开，子进程 stdio 桥接（与 OpenSSH 语义一致）  
3. **known_hosts**：`~/.config/mistterm/known_hosts`，首次信任、变更拒绝  
4. **本地端口转发**：会话字段 `local_forwards`，连接成功后 `channel_forward_listen`  
5. **片段统计**：侧栏展示 Top 使用片段（本地 `usage_count`）  
6. **market 标签**：片段列表增加「市场」筛选

## 后续（P4+）

- 分析：效率报告 **PDF**、**服务端全团队**成员/区间统计 API  
- 市场：服务端 catalog 部署联调、`install` 计数  
- 分屏：窗格拖放改布局树（当前为交换会话内容）  
- 其它：Block 输出等（见 `docs/archive/CLIENT-GAP-ANALYSIS.md`）

**已完成（2026-05-30）**：**批量多机执行**（工具菜单；独立 SSH exec；个人会话 + 团队服务器；并行度 1–16；命令审计拦截）；智能片段推荐 + 效率报告 Markdown；凭证面板 `ssh-keygen`；分屏快捷键 ⇧D/⇧U。

**已完成（2026-05-29）**：树形分屏≤8 + 标题拖放换位；执行日志区间增量 + 成员对比；市场 `cursor` 加载更多。

**文档**：`FUNCTIONAL_SPEC.md` v1.1 已补充 P4 行为说明。
