# MistTerm 文档中心

> 产品文档与技术文档统一入口

---

## 📁 目录结构

```
docs/
├── README.md                 # 本文档（文档索引）
├── product/                  # 产品文档
│   ├── README.md            # 产品详细设计文档
│   ├── MistTerm-Efficiency-First.md  # 效率优先版（推荐）
│   ├── MistTerm-2.0-Design*.md      # 历史版本
│   ├── team-collaboration.md        # 团队协同详细设计
│   └── proto-*.html         # HTML 原型（可交互）
├── tech/                     # 技术文档（待创建）
│   ├── architecture.md      # 架构设计
│   ├── api.md              # API 文档
│   └── ...
└── protos/                   # 原型图（PNG）
    ├── terminal-main.png    # 主终端界面
    ├── fragments-create.png # 命令片段创建
    ├── fragments-list.png   # 命令片段列表
    ├── sftp-main.png       # SFTP 文件传输
    ├── credentials-list.png # 凭证管理
    ├── monitor-dashboard.png # 实时监控
    ├── team-manage.png     # 团队管理
    ├── sync-settings.png   # 云端同步
    └── theme-editor.png    # 主题编辑器
```

---

## 📖 产品文档

### 推荐阅读顺序

1. **MistTerm-Efficiency-First.md** ⭐ 推荐
   - 核心理念：一切为了效率
   - 聚焦核心功能，砍掉冗余
   - 适合：快速了解产品价值

2. **README.md**（产品详细设计）
   - 完整的功能设计
   - 详细的交互说明
   - 适合：深入了解产品细节

3. **team-collaboration.md**
   - 团队协同功能详细设计
   - 权限模型、审计日志
   - 适合：团队协作场景

4. **HTML 原型**
   - 可交互的原型页面
   - 直接浏览器打开查看
   - 适合：UI/UX 评审

### 快速访问

| 文档 | 说明 | 链接 |
|-----|------|------|
| 效率优先版 | 核心理念 + 优先级 | [product/MistTerm-Efficiency-First.md](product/MistTerm-Efficiency-First.md) |
| 详细设计 | 完整功能规格 | [product/README.md](product/README.md) |
| 团队协同 | 团队功能详细设计 | [product/team-collaboration.md](product/team-collaboration.md) |
| HTML 原型 | 可交互原型 | [product/proto-terminal-main.html](product/proto-terminal-main.html) |

---

## 🔧 技术文档

技术文档正在整理中，预计包含：

- 系统架构
- 技术选型
- 模块设计
- API 文档
- 数据库设计
- 部署指南

---

## 🖼️ 原型图索引

| 功能 | PNG 原型 | HTML 原型 |
|-----|---------|----------|
| 主终端界面 | [terminal-main.png](protos/terminal-main.png) | [proto-terminal-main.html](product/proto-terminal-main.html) |
| 命令片段创建 | [fragments-create.png](protos/fragments-create.png) | [proto-fragments-create.html](product/proto-fragments-create.html) |
| 命令片段列表 | [fragments-list.png](protos/fragments-list.png) | [proto-fragments-list.html](product/proto-fragments-list.html) |
| SFTP 文件传输 | [sftp-main.png](protos/sftp-main.png) | [proto-sftp-main.html](product/proto-sftp-main.html) |
| 凭证管理 | [credentials-list.png](protos/credentials-list.png) | [proto-credentials-list.html](product/proto-credentials-list.html) |
| 实时监控 | [monitor-dashboard.png](protos/monitor-dashboard.png) | [proto-monitor-dashboard.html](product/proto-monitor-dashboard.html) |
| 团队管理 | [team-manage.png](protos/team-manage.png) | [proto-team-manage.html](product/proto-team-manage.html) |
| 云端同步 | [sync-settings.png](protos/sync-settings.png) | [proto-sync-settings.html](product/proto-sync-settings.html) |
| 主题编辑器 | [theme-editor.png](protos/theme-editor.png) | [proto-theme-editor.html](product/proto-theme-editor.html) |

---

## 📝 文档版本

| 版本 | 日期 | 变更内容 | 作者 |
|-----|------|---------|------|
| 2.0 | 2026-04-24 | 效率优先版，重新整理文档结构 | 产品专家 |
| 1.0 | 2026-04-24 | 初始版本，完整功能设计 | 产品专家 |

---

## 🚀 快速开始

### 查看产品文档

```bash
# 打开效率优先版（推荐）
open docs/product/MistTerm-Efficiency-First.md

# 打开详细设计文档
open docs/product/README.md

# 打开 HTML 原型
open docs/product/proto-terminal-main.html
```

### 开发参考

1. 先阅读 **MistTerm-Efficiency-First.md** 了解核心理念
2. 再阅读 **README.md** 了解详细功能
3. 查看 **HTML 原型** 了解 UI 交互
4. 技术文档待创建后查看架构设计

---

**文档维护**: 产品专家  
**最后更新**: 2026-04-24
