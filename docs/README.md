# MistTerm 文档中心

> 产品 / 技术文档统一入口  
> **最后更新**：2026-05-25

---

## 目录结构

```
docs/
├── README.md                  # 本文件（文档总入口）
├── INSTALL.md                 # 安装说明（macOS / Linux / Windows）
├── product/                   # 产品文档（功能、UI、原型）
├── tech/                      # 技术文档（架构、模块、API、运维）
├── archive/                   # 历史设计稿（已落地或已被取代）
└── protos/                    # PNG 原型图
```

---

## 产品文档（[`product/`](product/)）

当前权威文档：

| 文档 | 用途 |
|---|---|
| [`product/README.md`](product/README.md) | 功能详细设计（10 章，含主终端、片段、SFTP、监控、团队等） |
| [`product/FUNCTIONAL_SPEC.md`](product/FUNCTIONAL_SPEC.md) | 产品视角的功能规范（背景、目标、边界、异常） |
| [`product/SPECIFICATION_DETAILED.md`](product/SPECIFICATION_DETAILED.md) | 研发视角的视觉规格（颜色、字号、圆角、Token 真源） |
| [`product/UI-GUIDELINES.md`](product/UI-GUIDELINES.md) | 界面设计规范（侧栏、面板、Tab、对话框、底栏） |
| [`product/LAYOUT.md`](product/LAYOUT.md) | 布局真源（egui 区域注册顺序，**改 UI 必读**） |
| [`product/fragments-analytics.md`](product/fragments-analytics.md) | 命令片段 + 分析统计的产品详细设计 |
| [`product/proto-*.html`](product/) | 可交互 HTML 原型 |
| [`protos/*.png`](protos/) | 原型 PNG 截图 |

---

## 技术文档（[`tech/`](tech/)）

| 文档 | 用途 |
|---|---|
| [`tech/README.md`](tech/README.md) | 技术文档导航 |
| [`tech/ARCHITECTURE.md`](tech/ARCHITECTURE.md) | 系统架构、分层、数据流 |
| [`tech/MODULE-DESIGN.md`](tech/MODULE-DESIGN.md) | 模块详细设计与接口 |
| [`tech/IMPLEMENTATION-GUIDE.md`](tech/IMPLEMENTATION-GUIDE.md) | 实现指南（最厚的一份） |
| [`tech/API.md`](tech/API.md) | 各层 API 文档 |
| [`tech/TECH-STACK.md`](tech/TECH-STACK.md) | 技术选型 |
| [`tech/TECHNICAL-ASSESSMENT.md`](tech/TECHNICAL-ASSESSMENT.md) | 技术可行性评估（最终版） |
| [`tech/DEPLOYMENT.md`](tech/DEPLOYMENT.md) | 编译、打包、发布 |
| [`tech/TESTING.md`](tech/TESTING.md) | 单元 / 集成 / 性能测试方案 |
| [`tech/SMOKE.md`](tech/SMOKE.md) | 多平台手工冒烟清单 |
| [`tech/CROSS_PLATFORM_QA.md`](tech/CROSS_PLATFORM_QA.md) | 跨平台 UI 验收清单 |
| [`tech/TERMINAL-BEHAVIOR.md`](tech/TERMINAL-BEHAVIOR.md) | 终端 / VT / ANSI 行为约定 |
| [`tech/ZMODEM.md`](tech/ZMODEM.md) | ZMODEM 实现 + `rz` 兼容性排障 + 兜底方案 |
| [`tech/SECURITY.md`](tech/SECURITY.md) | 本地配置加密 + 审计 |
| [`tech/AI-INTERACTION-DESIGN.md`](tech/AI-INTERACTION-DESIGN.md) | 右侧 AI 面板交互设计 |
| [`tech/TEAM-PLATFORM-DEV-PLAN.md`](tech/TEAM-PLATFORM-DEV-PLAN.md) | 团队平台需求与设计（含服务端职责） |
| [`tech/TEAM-PLATFORM-API.md`](tech/TEAM-PLATFORM-API.md) | 团队平台 API 契约（客户端 ↔ 服务端） |
| [`tech/CLIENT-TEAM-TODO.md`](tech/CLIENT-TEAM-TODO.md) | 客户端团队功能落地清单 + 待办 |

---

## 历史归档（[`archive/`](archive/)）

下列文档已被取代或对应方案已落地，仅作背景参考：

- `MistTerm-2.0-Design.md` / `-Integrated.md` / `-Document.md`：2.0 重构设计三连
- `MistTerm-Efficiency-First.md`：早期"效率优先"理念稿
- `MistTerm-设计文档.md`：v1.1 旧版终端设计
- `P0功能详细设计.md`：13 项 P0/P1/P2 详细设计（已落地）
- `改造设计规范.md` + `改造后原型.html`：UI 改造方案 + 原型（已落地）
- `team-collaboration.md`：团队协同初稿（被 `tech/TEAM-PLATFORM-DEV-PLAN.md` 取代）
- `COMMAND-ANALYTICS.md`：命令分析需求初稿（合并入 `product/fragments-analytics.md`）
- `FEASIBILITY-ANALYSIS.md`：早期可行性分析（被 `tech/TECHNICAL-ASSESSMENT.md` 取代）
- `CLIENT-BILLING-TEAM-INTEGRATION.md`：客户端付费/团队对接改造方案（已落地，剩余事项见 `tech/CLIENT-TEAM-TODO.md`）
- `CLIENT-GAP-ANALYSIS.md`：客户端 vs 竞品差距分析（结论已沉淀）

---

## 安装与冒烟

- 安装：[`INSTALL.md`](INSTALL.md)
- 多平台冒烟：[`tech/SMOKE.md`](tech/SMOKE.md) · `scripts/smoke.sh`

---

## 文档命名约定

- 全部使用英文文件名（kebab-case 或 UPPER-KEBAB-CASE 都可，向已有风格靠拢即可）
- 一份能力一个权威文档，新增前先看是否能扩写已有文档
- 历史稿放 `archive/`，不要直接删除
