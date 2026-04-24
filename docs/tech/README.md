# MistTerm 技术文档

> **版本**: 1.0  
> **最后更新**: 2026-04-24  
> **维护**: 技术团队

---

## 📚 文档导航

### 核心文档

| 文档 | 描述 | 状态 |
|-----|------|-----|
| [架构设计](./ARCHITECTURE.md) | 系统架构、分层设计、数据流 | ✅ 已完成 |
| [技术栈](./TECH-STACK.md) | 技术选型、依赖库、版本管理 | ✅ 已完成 |
| [模块设计](./MODULE-DESIGN.md) | 模块详细设计、接口说明 | ✅ 已完成 |
| [API 文档](./API.md) | 各层 API 接口文档 | ✅ 已完成 |
| [部署指南](./DEPLOYMENT.md) | 编译、打包、部署流程 | ✅ 已完成 |
| [测试方案](./TESTING.md) | 单元测试、集成测试、性能测试 | ✅ 已完成 |

### 待创建文档

| 文档 | 描述 | 优先级 |
|-----|------|-------|
| 数据库设计 | 数据存储设计 | 中 |
| 安全设计 | 安全架构、加密方案 | 高 |
| 性能优化 | 性能调优指南 | 中 |
| 故障排查 | 常见问题解决方案 | 高 |
| 开发指南 | 新手开发入门 | 中 |

---

## 🏗️ 快速开始

### 1. 了解架构

如果你是第一次接触 MistTerm，建议按以下顺序阅读：

```
1. 架构设计 (ARCHITECTURE.md)
   └─► 了解整体架构和分层

2. 技术栈 (TECH-STACK.md)
   └─► 了解使用的技术和工具

3. 模块设计 (MODULE-DESIGN.md)
   └─► 了解各模块的详细设计

4. API 文档 (API.md)
   └─► 了解各模块的接口
```

### 2. 开发环境

```bash
# 1. 安装依赖
brew install libssh2 pkg-config  # macOS
# 或
sudo apt install libssh2-1-dev   # Ubuntu

# 2. 克隆代码
git clone https://github.com/your-org/MistTerm.git
cd MistTerm

# 3. 编译运行
cargo run
```

详细步骤见 [部署指南](./DEPLOYMENT.md)。

### 3. 运行测试

```bash
# 运行所有测试
cargo test

# 查看测试覆盖率
cargo tarpaulin --out Html
```

详细步骤见 [测试方案](./TESTING.md)。

---

## 📁 文档结构

```
docs/tech/
├── README.md              # 本文档（索引）
├── ARCHITECTURE.md        # 架构设计
├── TECH-STACK.md          # 技术栈
├── MODULE-DESIGN.md       # 模块设计
├── API.md                 # API 文档
├── DEPLOYMENT.md          # 部署指南
├── TESTING.md             # 测试方案
├── database.md            # 数据库设计（待创建）
├── security.md            # 安全设计（待创建）
├── performance.md         # 性能优化（待创建）
└── troubleshooting.md     # 故障排查（待创建）
```

---

## 🔗 相关文档

- [产品文档](../product/README.md) - 产品功能和交互设计
- [设计文档](../product/MistTerm-2.0-Design-Document.md) - 产品重构设计
- [原型图](../protos/) - UI 原型图

---

## 📝 贡献指南

### 文档更新

1. 修改对应的文档文件
2. 更新文档版本和日期
3. 提交 Pull Request

### 文档规范

- 使用 Markdown 格式
- 中文描述，代码/术语用英文
- 包含示例代码
- 更新版本号

### 版本管理

| 版本 | 日期 | 更新内容 |
|-----|------|---------|
| 1.0 | 2026-04-24 | 初始版本，完成核心文档 |

---

## ❓ 常见问题

### Q: 从哪里开始？

A: 建议按以下顺序：
1. 先看 [架构设计](./ARCHITECTURE.md) 了解整体
2. 再看 [技术栈](./TECH-STACK.md) 了解技术选型
3. 然后看 [部署指南](./DEPLOYMENT.md) 搭建环境

### Q: 如何贡献文档？

A: 
1. Fork 仓库
2. 修改文档
3. 提交 Pull Request
4. 等待 Review

### Q: 文档过时了怎么办？

A: 请提交 Issue 或 PR，我们会及时更新。

---

## 📞 联系方式

- **问题反馈**: 提交 GitHub Issue
- **文档建议**: 提交 GitHub PR
- **技术讨论**: 提交 GitHub Discussion

---

**文档维护**: 技术团队  
**最后更新**: 2026-04-24  
**状态**: 持续维护中
