# 测试

## 1. 三层测试

| 层 | 位置 | 现状 |
|----|------|------|
| 单元测试 | 各模块源文件内的 `#[cfg(test)]` | 129 处：`ui` 41、`core` 48、`ssh` 24、`terminal` 12、`cli` 4 |
| 集成测试 | `tests/*.rs` | 33 个文件，见 §3 |
| 手动验证 | GUI 实机 | 见 §5 |

## 2. 跑测试

本机（4GB 内存）不做全量构建，链接会 OOM——见 [AGENTS.md](../../AGENTS.md)。

```bash
# 单元测试（限制并行度）
CARGO_BUILD_JOBS=2 cargo test --lib -- --test-threads=2

# 单个集成测试文件
CARGO_BUILD_JOBS=2 cargo test --test fragment_test

# 只看失败的
cargo test --lib -- --test-threads=2 2>&1 | grep -A 20 "FAILED"
```

Windows 下把 `CARGO_BUILD_JOBS=2` 换成 `$env:CARGO_BUILD_JOBS=2`。完整构建与全量测试交给 GitHub Actions。

## 3. `tests/` 清单

| 域 | 文件 |
|----|------|
| AI | `ai_chat_store_test`、`ai_client_test`、`ai_live_fallback_test`、`ai_settings_test` |
| 会话与凭证 | `session_ssh_agent_test`、`credential_test`、`secret_resolver_test`、`reconnect_test`、`app_settings_test` |
| SSH 与传输 | `ssh_transfer_three_modes`、`sftp_integration_test`、`port_forward_test`、`ssh_config_importer_test`、`upload_policy_test` |
| 终端与键位 | `terminal_behavior_test`、`terminal_focus_test`、`keyboard_shortcuts_integration_test` |
| 片段 | `fragment_test`、`fragment_expr_test`、`fragment_analytics_test`、`fragment_recommendations_test`、`fragment_usage_log_test` |
| 审计 | `audit_test`、`cmd_audit_test` |
| 监控 | `monitor_integration_test`、`monitor_snapshot_test` |
| 团队与市场 | `team_api_test`、`market_client_test` |
| ZMODEM | `zmodem_integration_test` |
| 其它 | `client_todo_acceptance_test`、`gui_v113_mock_test`、`command_history_test`、`paste_history_test` |

## 4. 集成测试怎么跑起来

- `sftp_integration_test`、`zmodem_integration_test`、`monitor_integration_test` 需要一个真实可连的 SSH 服务器。本地起一个临时 OpenSSH 实例最省事（22 端口或另开端口都行）。
- `ssh_transfer_three_modes` 覆盖三种传输模式，跑之前确认模式参数。
- 依赖外部服务的用例用环境变量做开关，缺环境时跳过而不是硬失败——CI 上没有这些服务。

## 5. 手动验证清单

新功能上线前，至少走一遍这些路径：

1. 新建会话 → 连接 → 断开 → 重连（目录应保留）。
2. 连接失败：错误卡片上的「关闭标签 / 编辑主机 / 重试」三个按钮都点一次。
3. 复制标签、关闭标签、Ctrl+Shift+W 关闭。
4. 文件面板：双栏拖拽上传 / 下载，传输队列顺序执行。
5. 监控面板：刷新、切换指标。
6. 片段插入、发送序列重放、粘贴历史。
7. 命令块跳转（Ctrl+↑/↓）与块复制。
8. 结果提示是否为 Toast，而不是常驻底栏。

## 6. 覆盖率

```bash
cargo install cargo-tarpaulin
cargo tarpaulin --out Html --skip-clean
```

覆盖率是参考值，不设硬门槛；`ssh` 与 `terminal` 的解析分支优先补。

## 7. 内存安全

```bash
cargo +nightly miri test --lib
```

Miri 只在纯 Rust 逻辑上跑；涉及 FIDO2、系统托盘、窗口这些平台调用时跳过。

## 8. CI

push / PR 触发 `Build & Test`，产物与发版流程见 [DEPLOYMENT.md](./DEPLOYMENT.md)。本机不出包。
