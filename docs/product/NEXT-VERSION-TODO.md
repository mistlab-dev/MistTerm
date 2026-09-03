# MistTerm 下一版待办（概念稿 v4 对齐）

> 依据：产品方向（管得住 · 帮得上 · 不做手机终端）+ [`concept-desktop-calm.html`](concept-desktop-calm.html) v4。  
> 原则：**不大改现有客户端壳**；增量做「审计拦截 ↔ 团队知识」交汇。  
> 日期：2026-09-03 · 当前 release：v1.1.2 · 下一目标：v1.1.3

---

## 约束

- [x] UI：保留现有布局语言；只加必要面板/toast/入口，不做全站换肤
- [x] 不做移动端 SSH 终端
- [x] AI 先检索团队知识，再模型兜底；答案必须标明来源（拦截建议：团队/个人片段标签）
- [x] 被策略拦截时，不教「绕过」，只推「允许的替代」

---

## v1.1.2 已完成 — 可演示闭环

### A. 治理收尾（客户端）

- [x] MistTerm 登录团队后，连 agent 主机：人工确认 **block toast**（「服务器策略」前缀；标题+正文；PTY CSI 行内标记解析；泵消息后再 poll）
- [x] **confirm 弹窗**（「放行并发送」）与 Agent 离线黄横幅：代码已随 v1.1.2 发布；仍建议在真实 GUI 团队登录环境人工点验
- [x] 大数据块 partial prefix 解析：已有单测覆盖；SSH 压测作为可选回归项

### B. 知识优先（增量）

- [x] 片段侧栏搜索提示支持「我们怎么…」心智（placeholder）
- [x] 命中结果带来源标签（Team snippet / Personal snippet；拦截 Toast 展示）
- [x] 一键 **用到当前终端**（拦截建议主按钮 → 复用 `begin_fragment_insert`）
- [x] 一键 **沉底**（团队建议 → Toast「存到个人库」写入个人片段；个人命中不重复提供）

### C. 双支柱交汇（概念稿主场景）

- [x] 服务器 / 本地 **block** 后：有命中则 Action Toast 推 **合规替代片段**
- [x] 无命中时：保持原 Error Toast（不硬塞模型）
- [x] 文案与本地审计区分保持一致（「本地检查」vs「服务器策略」）
- [ ] 无命中时短提示「暂无团队替代」+ 可选 Model fallback（标明非团队知识）— 未做，避免空话打扰

### D. v1.1 UI 收口（本轮）

- [x] Toast 标题+正文 + 级别底色
- [x] 侧栏收起按钮可点可见
- [x] 主/次按钮两档；暗夜灰钮不透明底 + 纯白字
- [x] 审计事件泵完再 poll，剥离行仍 request_repaint

---

## v1.1.3 — 加深飞轮（下一期）

### P0 治理闭环

- [ ] **审计时间线**：展示本会话及近期 block / confirm / allow，只读、可按主机和结果筛选
- [ ] **Agent 安装向导**：提供文档和客户端入口，明确安装、绑定、心跳检查，不改现有客户端壳

### P1 知识闭环

- [ ] 失败输出 / 成功路径 → **入库候选**（必须用户确认，禁止静默入库）
- [ ] 拦截后推荐按当前主机 / 环境标签过滤，无标签时回退全局
- [ ] 引用 MistDocs / 团队文档段落，并展示来源锚点

### P2 检索体验

- [ ] 「问：我们怎么……」独立入口 / 语义检索；先检索团队知识，再模型兜底并标明来源

---

## v1.2 — 组织能力（后续）

- [ ] 策略可读视图（人能看懂「为什么拦」）
- [ ] 多主机策略包
- [ ] 按主机/环境的知识分层；新人 onboarding 路径
- [ ] 若有手机能力：仅只读状态 / 审批点头——**仍不做交互式终端**

---

## 验收口径（每个 Now 项）

不是「模型答得漂亮」，而是：

1. 能否命中团队已沉淀条目  
2. 能否一键落到当前 SSH 会话  
3. 拦截场景是否同时给出合规替代（有知识时）

---

## 参考

- Canvas：`mistterm-next-direction`（差异化方向）
- 概念稿：`docs/product/concept-desktop-calm.html`（v4）
- 既有客户端闭环：`docs/tech/CLIENT-TODO.md`（§1–§8 已完成；仅保留 GUI 人工点验建议）
- 审计：`docs/tech/COMMAND-AUDIT.md`、`docs/tech/SERVER-SIDE-AUDIT.md`
- 实现：`suggest_compliant_after_block`（`fragment_recommendations.rs`）+ `ToastAction::InsertSuggestedSnippet`
