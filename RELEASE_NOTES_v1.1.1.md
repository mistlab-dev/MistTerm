# MistTerm v1.1.1 Release Notes

**发布日期**: 2026-08-30
**版本类型**: Patch Release
**对比版本**: v1.1.0 → v1.1.1

---

## 📋 概览

本版本主要聚焦于**单元测试覆盖率大幅扩展**和一项关键的 **ZMODEM 协议 bug 修复**。
累计新增 **272 个单元测试**，测试总数达到 **477 个**（全部通过，0 失败），
同时修复了 ZMODEM 接收管道缓冲区溢出丢弃逻辑错误，避免大文件传输时数据丢失。

---

## 🐛 Bug 修复

### 修复 ZMODEM 接收管道溢出丢弃错误

- **文件**: `src/ssh/zmodem_pty_pipeline.rs`
- **问题描述**: `pull_from_rx` 在缓冲区溢出需要丢弃旧数据时，`stale_len` 的获取时机错误——
  它在 `self.buf.clear()` 之后才读取 `self.buf.len()`（此时值为 0），导致实际丢弃的字节数
  比预期多出 `stale_len` 个，造成接收数据丢失。
- **修复方案**: 在 `clear()` 调用之前捕获 `stale_len`，确保只丢弃超出容量的部分。
- **验证**: 新增严格断言测试（`buf.len() == INCOMING_CAP`），防止回归。

---

## 🧪 单元测试覆盖扩展（+272 个测试）

测试覆盖 6 个批次、24 个核心模块，累计新增代码 **4183 行**。

### Batch 1: 基础模块
| 模块 | 新增测试数 | 覆盖要点 |
|------|-----------|----------|
| `ai_session_meta.rs` | + | 会话元数据结构与默认值 |
| `terminal/style.rs` | + | 终端样式枚举、serde 往返 |
| `ssh/proxy_command.rs` | + | SSH ProxyCommand 解析与参数化 |
| `ssh/known_hosts.rs` | + | known_hosts 文件读写与匹配 |

### Batch 2: 核心配置与工具
| 模块 | 新增测试数 | 覆盖要点 |
|------|-----------|----------|
| `fragment_command.rs` | 24 | `finalize_fragment_command_text` 占位符替换、Rhai 表达式求值、回退链 |
| `cloud_sync.rs` | + | Default 实现、Serde 往返、缺字段回退、`mark_sync_err` |
| `ssh_keygen.rs` | + | 文件已存在错误处理、父目录创建、`.pub` 后缀约定、缺失 `.pub` 诊断 |

### Batch 3: Team / Market / Vault 模型
| 模块 | 新增测试数 | 覆盖要点 |
|------|-----------|----------|
| `team/models.rs` | + | 成员、片段、权限模型 Serde 与默认值 |
| `market/models.rs` | + | 市场片段、标签、分页模型 |
| `vault/mod.rs` | + | Vault 凭据路径解析、后端枚举 |
| `hang_reporter.rs` | + | 看门狗超时与上报状态机 |

### Batch 4: 网络模块纯逻辑层解耦（可测试化重构）
将网络依赖模块的纯逻辑抽取为独立函数，无需真实网络即可单元测试。

| 模块 | 新增测试数 | 覆盖要点 |
|------|-----------|----------|
| `ssh/zmodem_pty_pipeline.rs` | 16 | ZMODEM 状态机、相位检测、缓冲区溢出丢弃修复验证 |
| `vault/hashicorp.rs` | 18 | Vault v1/v2 路径构建、响应解析、KV 数据解码 |
| `team/client.rs` | 14 | URL 归一化、错误码解码、查询参数构建 |
| `market/client.rs` | 11 | 市场查询 URL 构建、响应列表解码、游标分页 |

### Batch 5: Team 内部状态与缓存
| 模块 | 新增测试数 | 覆盖要点 |
|------|-----------|----------|
| `team/settings.rs` | + | 团队设置结构、合并、默认值 |
| `team/state.rs` | + | 团队本地状态、成员/片段缓存状态 |
| `team/cache.rs` | + | 缓存合并、去重、增量刷新 |
| `market/cache.rs` | + | 市场缓存合并、游标更新、去重逻辑 |

### Batch 6: 分析、凭据、会话排序
| 模块 | 新增测试数 | 覆盖要点 |
|------|-----------|----------|
| `session_sort.rs` | 11 | 默认/全部/标签筛选、3 种排序、Copy+Eq、Serde |
| `fragment_analytics.rs` | 18 | 时间范围过滤、Dashboard 聚合、Top N / 最慢 / 最高错误、`with_events`、JSON 导出 |
| `fragment_usage_log.rs` | 22 | 默认、追加、`MAX_EVENTS=8000` 头淘汰、`events_since`、Serde、成员统计回退链、周期统计应用 |
| `credential.rs` | 22 | Category/AuthKind/Backend 枚举标签与 Serde、`from_credential` Vault 擦除 vs 本地保留、密钥解析、标准化、元数据往返、文件系统往返 + 遗留迁移 |
| `team/sync_config.rs` | 8 | Vault v1/v2 路径 trim/空/data 剥离、`apply_sync_response` 角色升级/保持不变/空角色保留/空团队无操作 |

---

## 🔧 内部重构

- **纯逻辑层提取**: 对 `vault/hashicorp.rs`、`team/client.rs`、`market/client.rs`、
  `zmodem_pty_pipeline.rs` 进行了内部 API 解耦——新增 `pub(crate)` 纯函数，
  原有公开方法全部委托给新函数，保持向后兼容不变。
- **类型实现补全**: `session_sort.rs` 为排序配置补齐 `Copy + Eq` 派生，
  便于集合与比较场景使用。

---

## ✅ 测试结果

```
cargo test --lib
= 477 passed
= 0 failed
= 3 ignored
```

---

## 📦 版本号变更

| 文件 | 旧版本 | 新版本 |
|------|--------|--------|
| `Cargo.toml` | 1.1.0 | 1.1.1 |
| `Info.plist` (CFBundleShortVersionString) | 1.1.0 | 1.1.1 |
| `Info.plist` (CFBundleVersion) | 1.1.0 | 1.1.1 |

---

## 🙌 致谢

感谢所有为本次发布贡献测试用例和 bug 报告的开发者。
