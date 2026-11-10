# AGENTS.md — MistTerm 仓库工作约定

## 本机构建约束（重要）

**开发机（本机，4GB 内存）禁止跑 `cargo build` / `cargo test` 全量构建** —— 链接阶段会 OOM（ld killed）。

- 验证一律走 GitHub Actions（push / PR 自动触发）
- 本机只允许小范围快速检查：`cargo check`（配合 `CARGO_BUILD_JOBS=2`）
- 跑测试需 `CARGO_BUILD_JOBS=2 cargo test --lib -- --test-threads=2`；确需全量链接时先加临时 swap，用完必须 swapoff + 清理
- 此约束原记录于仓库根 `DO_NOT_BUILD_HERE.txt`（#301 P0 已移入本文件）

## 与其他环境的分工

- **生产机 / 发版机**：main 上合入更高的正式版本后，Tag release 会打 `v*` 标签并启动 Build & Test。不在开发机上出包，也不要手工移动已有标签。
- **代理（OpenClaw）**：改代码、提交、push、开 PR 均可。发版是把版本号和中英 notes 合进 main；不要自行推送或覆盖 `v*` 标签。
