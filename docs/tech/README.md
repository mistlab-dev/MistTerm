# MistTerm 技术文档

> **最后更新**：2026-05-25  
> **维护**：技术团队

---

## 文档导航

### 架构与实现

| 文档 | 描述 |
|---|---|
| [ARCHITECTURE.md](./ARCHITECTURE.md) | 系统架构、分层设计、数据流 |
| [MODULE-DESIGN.md](./MODULE-DESIGN.md) | 模块详细设计与接口说明 |
| [IMPLEMENTATION-GUIDE.md](./IMPLEMENTATION-GUIDE.md) | 实现指南（最厚的一份） |
| [API.md](./API.md) | 各层 API 接口文档 |
| [TECH-STACK.md](./TECH-STACK.md) | 技术选型与依赖管理 |
| [TECHNICAL-ASSESSMENT.md](./TECHNICAL-ASSESSMENT.md) | 技术可行性评估（最终版） |

### 平台能力

| 文档 | 描述 |
|---|---|
| [SECURITY.md](./SECURITY.md) | 本地配置加密 + 审计策略 |
| [COMMAND-AUDIT.md](./COMMAND-AUDIT.md) | 命令审计客户端集成（策略同步、实时拦截、告警上报） |
| [TERMINAL-BEHAVIOR.md](./TERMINAL-BEHAVIOR.md) | 终端 / VT / ANSI 行为约定 |
| [ZMODEM.md](./ZMODEM.md) | ZMODEM 实现 + `rz -bye` 兼容性排障 + 兜底方案 |
| [AI-INTERACTION-DESIGN.md](./AI-INTERACTION-DESIGN.md) | 右侧 AI 面板交互设计 |

### 团队平台

| 文档 | 描述 |
|---|---|
| [TEAM-PLATFORM-DEV-PLAN.md](./TEAM-PLATFORM-DEV-PLAN.md) | 团队平台需求与设计（含服务端职责） |
| [TEAM-PLATFORM-API.md](./TEAM-PLATFORM-API.md) | 客户端 ↔ 服务端 API 契约 |
| [SERVER-API-BACKEND.md](./SERVER-API-BACKEND.md) | **后端实现清单**（市场 catalog、片段分析、待补齐项） |
| [CLIENT-TEAM-TODO.md](./CLIENT-TEAM-TODO.md) | 客户端团队功能清单 + **服务端/运维配合**对照表 |

### 运维与质量

| 文档 | 描述 |
|---|---|
| [DEPLOYMENT.md](./DEPLOYMENT.md) | 编译、打包、发布 |
| [TESTING.md](./TESTING.md) | 单元 / 集成 / 性能测试方案 |
| [SMOKE.md](./SMOKE.md) | 多平台手工冒烟清单 |
| [CROSS_PLATFORM_QA.md](./CROSS_PLATFORM_QA.md) | 跨平台 UI 验收清单 |

---

## 快速开始

```bash
# 1. 安装系统依赖（macOS）
brew install libssh2 pkg-config

# 2. 克隆并构建
git clone https://github.com/c-wind/MistTerm.git
cd MistTerm
cargo build --release --bin Mist

# 3. 运行测试
cargo test
```

详细步骤见 [`DEPLOYMENT.md`](./DEPLOYMENT.md)；测试方案见 [`TESTING.md`](./TESTING.md)。

---

## 历史归档

以下评估 / 改造方案的核心结论已沉淀到当前文档，原稿见 [`docs/archive/`](../archive/)：

- `FEASIBILITY-ANALYSIS.md` → 被 [`TECHNICAL-ASSESSMENT.md`](./TECHNICAL-ASSESSMENT.md) 取代
- `CLIENT-BILLING-TEAM-INTEGRATION.md` → 改造任务已落地，剩余事项见 [`CLIENT-TEAM-TODO.md`](./CLIENT-TEAM-TODO.md)
- `CLIENT-GAP-ANALYSIS.md` → 差距分析结论已用于 P0/P1 规划
- `team-collaboration.md` → 被 [`TEAM-PLATFORM-DEV-PLAN.md`](./TEAM-PLATFORM-DEV-PLAN.md) 覆盖

---

## 相关文档

- 顶层入口：[`docs/README.md`](../README.md)
- 产品文档：[`docs/product/`](../product/)
- 安装：[`docs/INSTALL.md`](../INSTALL.md)

---

## 贡献规范

- 一份能力 → 一个权威文档；新增前先看是否能扩写既有文档
- 文件名使用英文（kebab-case 或 UPPER-KEBAB-CASE 与同目录风格保持一致）
- 历史稿放 `docs/archive/`，**不要直接删除**
- 文档前置 `> **更新**：YYYY-MM-DD` 元信息保持更新
