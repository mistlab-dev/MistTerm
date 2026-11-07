# MistTerm 接口面

> 类型与函数签名不在这里复制——复制出来的那份一定会过期。本文只回答"哪一层对外承诺了什么、改的时候要注意什么"。

## 1. 查 API 的三种方式

```bash
cargo doc --no-deps --open     # 生成 rustdoc
mist --help                    # CLI 子命令一览
mist <subcommand> --help
```

模块与文件对应关系见 [MODULE-DESIGN.md](./MODULE-DESIGN.md)。

## 2. 对外承诺兼容的接口

### 2.1 `mist` 命令行

已落地的子命令：`ls`、`exec`、`ssh`、`rls`、`get`、`put`、`fwd`、`frag`、`import`、`sop`。参数与输出格式见 [CLI-DESIGN.md](./CLI-DESIGN.md)。改这些子命令的参数或输出，算破坏性变更。

### 2.2 团队服务端 HTTP API

客户端依赖的服务端接口、状态码与契约集中在 [TEAM.md](./TEAM.md) §三 与附录 A。404 降级约定见同文「404 约定」。

### 2.3 本地配置文件

`sessions.json`、`settings.json`、`fragments.json`、`credentials.json` 等都是 `device_key` + AES-256-GCM 信封（`mistterm-aes-v1`），只在应用内读写。外部脚本不要直接解析，格式见 [SECURITY.md](./SECURITY.md)。

### 2.4 自动更新

检查更新走版本清单 + 签名校验，失败时只提示、不自作主张安装。实现与打包细节见 [DEPLOYMENT.md](./DEPLOYMENT.md)。

## 3. 内部模块接口（不承诺兼容）

| 层 | 入口 | 说明 |
|----|------|------|
| SSH 会话 | `ssh::manager::SshManager`、`SshSessionHandle` | 建连、写数据、订阅消息 |
| SSH 客户端 | `ssh::client::SshClient` / `SshConfig` | 单连接参数与生命周期 |
| 终端 | `terminal::alacritty`、`terminal::emulator::Terminal` | 喂字节、取网格 |
| 会话数据 | `core::session::SessionConfig` / `SessionManager` | 已保存会话的增删改查 |
| 片段 | `core::fragment` | 片段模型与变量 |
| 审计 | `core::cmd_audit`、`core::audit` | 本地规则与服务端策略 |
| 扩展 | `capability::ModuleRegistry` | 输出触发、链接识别等模块注册 |

这些是内部实现，重构时可以直接改；改的时候顺手改调用方和本文。

## 4. 错误处理

- 底层错误在 `ssh/user_facing.rs` 转成用户可读的文案，UI 只显示转换后的结果。
- 需要用户决策的错误走对话框（主机密钥变更、认证失败）或 Toast（传输失败）。
- 崩溃与卡死写 `logs/crash-reports/`、`logs/hang-reports/`，下次启动提示一次。
- 团队接口不可用时按 404 降级：不阻断本地终端使用。

## 相关文档

- [模块设计](./MODULE-DESIGN.md)
- [架构](./ARCHITECTURE.md)
- [团队接口](./TEAM.md)
- [安全与加密](./SECURITY.md)
