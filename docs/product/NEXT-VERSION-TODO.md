# MistTerm 下一版待办（概念稿 v4 对齐）

> 依据：产品方向（管得住 · 帮得上 · 不做手机终端）+ [`concept-desktop-calm.html`](concept-desktop-calm.html) v4。  
> 原则：**不大改现有客户端壳**；增量做「审计拦截 ↔ 团队知识」交汇。  
> 日期：2026-08-29 · 基线 release：v1.0.21

---

## 约束

- [x] UI：保留现有布局语言；只加必要面板/toast/入口，不做全站换肤
- [x] 不做移动端 SSH 终端
- [x] AI 先检索团队知识，再模型兜底；答案必须标明来源（拦截建议：团队/个人片段标签）
- [x] 被策略拦截时，不教「绕过」，只推「允许的替代」

---

## Now — 可演示闭环

### A. 治理收尾（客户端）

- [ ] MistTerm 登录团队后，连 agent 主机：人工确认 **block toast**（「服务器策略」前缀）
- [ ] 人工确认 **confirm 弹窗**（「放行并发送」）与黄横幅（agent 离线）
- [ ] （可选）SSH 压测：大数据块 partial prefix 不丢数据

### B. 知识优先（增量）

- [x] 片段侧栏搜索提示支持「我们怎么…」心智（placeholder）
- [x] 命中结果带来源标签（Team snippet / Personal snippet；拦截 Toast 展示）
- [x] 一键 **用到当前终端**（拦截建议主按钮 → 复用 `begin_fragment_insert`）
- [ ] 一键 **沉底**（存为个人/团队片段；后续补 Toast 次要动作或侧栏）

### C. 双支柱交汇（概念稿主场景）

- [x] 服务器 / 本地 **block** 后：有命中则 Action Toast 推 **合规替代片段**
- [x] 无命中时：保持原 Error Toast（不硬塞模型）
- [x] 文案与本地审计区分保持一致（「本地检查」vs「服务器策略」）
- [ ] 无命中时短提示「暂无团队替代」+ 可选 Model fallback（标明非团队知识）— 未做，避免空话打扰

---

## Next — 加深飞轮

### 治理

- [ ] Agent 安装向导（文档/客户端引导，仍非大改壳）
- [ ] 审计时间线（本会话或近期 block/confirm/allow 只读列表）

### 知识

- [ ] 失败输出 / 成功路径 → **入库候选**（需确认后写入，禁止静默入库）
- [ ] 引用 MistDocs / 团队文档段落（带来源锚点）
- [ ] 拦截后推荐与当前主机/环境标签对齐（有则过滤，无则全局）
- [ ] 「问：我们怎么…」独立入口 / 语义检索

---

## Later — 组织能力

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
- 既有客户端闭环：`docs/tech/CLIENT-TODO.md`（§1–§7 已完成；§5 GUI 人工点看仍待）
- 审计：`docs/tech/COMMAND-AUDIT.md`、`docs/tech/SERVER-SIDE-AUDIT.md`
- 实现：`suggest_compliant_after_block`（`fragment_recommendations.rs`）+ `ToastAction::InsertSuggestedSnippet`
