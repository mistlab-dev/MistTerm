# MistTerm 扩展点与内置模块架构（设计稿 v0.1）

> 状态：**草案，供评审**（对应 [#113](https://github.com/mistlab-dev/MistTerm/issues/113)）  
> 视角：研发架构 — 先定边界与演进路径，再动手迁代码  
> 日期：2026-10-25

---

## 0. 一句话结论

MistTerm **需要插件化的是「能力模块」**，不是「用户逛商店装 Skill」。  
短期目标：把已散落的规则引擎 / 导入器 / 通知类能力，收敛到 **同进程、可开关、可测试的内置模块注册表**；  
长期才考虑第三方代码（WASM），且必须另立安全模型。

与 [`CONVERSATIONAL-TERMINAL.md`](../product/CONVERSATIONAL-TERMINAL.md) 对齐：**不做**用户可浏览的 Skill/能力目录；本设计中的「模块」是工程结构，不是产品功能列表。

---

## 1. 问题陈述

### 1.1 现状（症状）

| 现象 | 证据 |
|------|------|
| 巨石宿主 | `ui/app.rs`、`ui/terminal.rs` 同时拥有连接、审计、SFTP、AI、display-link、trigger… |
| 半解耦已出现 | `display_link` / `output_trigger` / `cmd_audit` 各有独立 JSON + 引擎，但生命周期仍手写进 View |
| 回归面膨胀 | 每加一个「输出扫描」类能力，都要改泵路径 + 宿主 poll + Toast 策略 |
| 文档分层过时 | `ARCHITECTURE.md` 仍是 UI / core / ssh / terminal 四层，未描述扩展面 |

### 1.2 要解决什么 / 不解决什么

**要解决**

1. 新特性默认有「该挂哪一层、该实现哪些 hook」的答案  
2. 关闭某模块后，行为可论证「等于从未编译进来」（测试口径）  
3. Kernel 保持薄：SSH/PTY/渲染/会话不被业务规则污染  

**明确不解决（本阶段）**

- 插件商店、热更新第三方 `.dll`、npm 式生态  
- 把 AI Agent 改成可选 Skill 列表（产品禁止）  
- 为插件重写 egui 布局系统  

---

## 2. 目标架构

```
┌─────────────────────────────────────────────────────────────────┐
│                        Host (MistTermApp)                        │
│  窗口壳 · 标签/分屏 · 右 Dock 槽位 · Toast · 偏好总开关            │
└───────────────┬─────────────────────────────┬───────────────────┘
                │ Capability API              │
                ▼                             ▼
┌───────────────────────────┐   ┌─────────────────────────────────┐
│     Kernel（永不插件）      │   │     Module Registry（内置模块）   │
│  · SSH / PTY 泵            │   │  display_link · output_trigger  │
│  · VTE / 渲染 / 选区       │   │  cmd_audit(local) · importers   │
│  · Session 持久化          │   │  （未来）企业规则包 …            │
│  · Auth / Vault 边界       │   │  同进程 · compile-time 注册      │
│  · 重连状态机              │   │  可按 id 禁用                    │
└───────────────────────────┘   └─────────────────────────────────┘
                │
                ▼
        assets/modules/*.json   （规则数据，非代码）
```

### 2.1 三层定义

| 层 | 定义 | 变更频率 | 示例 |
|----|------|----------|------|
| **Kernel** | 没有它终端不能工作 | 极低 | `ssh/`、`terminal/`、`session`、reconnect |
| **Capability API** | Host 暴露给模块的稳定钩子 | 低（改要兼容） | 见 §3 |
| **Modules** | 可开关业务能力 | 高 | display-link、trigger、导入器 |

### 2.2 「插件」在本设计里的含义（刻意收窄）

| 阶段 | 形态 | 谁写代码 | 加载方式 |
|------|------|----------|----------|
| **P0（现在要设计）** | 内置 Rust 模块 + JSON 规则 | 仅官方 | compile-time registry + 运行时 enable |
| **P1（可选）** | 规则包热更新（只数据） | 官方 / 企业配置 | 读目录或团队下发 JSON |
| **P2（以后拍板）** | 第三方逻辑 | 外部 | WASM sandbox（另立安全票） |

**禁止跳步**：未做 P0 注册表之前，不上 P2 动态库。

---

## 3. Capability API（最小钩子集）

原则：**少而稳**。第一版只定 5 个；多了说明 Kernel 又在漏业务。

### 3.1 钩子一览

| ID | 时机 | 模块典型用途 | Host 约束 |
|----|------|--------------|-----------|
| `on_output_line` | PTY 完整行（UTF-8 已解码） | display-link 扫描、trigger、日志标注 | 同步、限时；禁止阻塞 SSH 泵 |
| `on_before_send` | 用户回车 / 片段插入即将写入 PTY | 本地 cmd_audit | 可 `Allow` / `Confirm` / `Block`；服务器审计仍走原通道 |
| `contribute_context_menu` | 终端右键菜单构建 | 「新开连接」、自定义项 | 只贡献条目，不拥有菜单生命周期 |
| `contribute_right_dock` | 右栏槽位注册 | 未来独立面板 | 槽位有上限；与现有 Dock 枚举兼容 |
| `on_session_event` | 连接成功 / 断开 / 重连放弃 | 清理模块状态、注意点 | 事件只读 |

### 3.2 建议的 Rust 形状（示意，非最终 API）

```rust
/// 模块清单里的稳定 id（如 "display-link" / "output-trigger"）
pub type ModuleId = &'static str;

pub trait MistModule: Send + Sync {
    fn id(&self) -> ModuleId;
    fn name_zh(&self) -> &str;
    fn enabled_by_default(&self) -> bool { true }

    fn on_output_line(&self, ctx: &OutputLineCtx) -> Vec<ModuleAction> { vec![] }
    fn on_before_send(&self, ctx: &BeforeSendCtx) -> SendVerdict {
        SendVerdict::Allow
    }
    fn contribute_context_menu(&self, ctx: &MenuCtx) -> Vec<MenuItem> { vec![] }
    // dock / session_event 第二批再加，避免一次改爆
}

pub enum ModuleAction {
    /// 渲染层：某字节区间可点（display-link）
    DecorateLink { start: usize, end: usize, action: LinkAction, text: String },
    /// 宿主通知：后台标签注意点 + 可选 Toast
    Notify { severity: Severity, text: String, attention: bool },
}

pub enum SendVerdict {
    Allow,
    Confirm { reason: String },
    Block { reason: String },
}
```

### 3.3 Host 责任（写进规范，避免模块越权）

1. **泵线程不跑模块**：`on_output_line` 只在 UI/编排线程、对「完整行」调用（与现 trigger 行缓冲一致）。  
2. **模块无 SSH handle**：不能直接 `channel.write`；要发送必须经 Host 已有路径。  
3. **模块无 egui Context 长持**：菜单/面板通过声明式贡献，由 Host 绘制。  
4. **失败隔离**：单模块 panic 或超时 → 记日志并禁用该模块本会话，不影响 Kernel。

---

## 4. 模块注册与配置

### 4.1 Compile-time 注册表（P0）

```rust
inventory::submit! { ... }  // 或手写 once_cell 列表
fn builtin_modules() -> Vec<&'static dyn MistModule> { ... }
```

偏好设置增加一节「扩展模块」：勾选启用（**本机设置**，默认全开官方模块）。  
关闭某模块后：对应 JSON 引擎不加载、钩子不调用；单测断言「关闭 display-link ≈ 空引擎」。

### 4.2 规则数据 vs 模块代码

| 资产 | 位置 | 变更方式 |
|------|------|----------|
| 模块代码 | `src/modules/<id>/`（迁入后） | 发版 |
| 规则 JSON | `assets/<id>-patterns.json` | 发版；P1 可叠加用户/团队覆盖 |

已有资产直接对上：

- `assets/display-link-patterns.json` → 模块 `display-link`  
- `assets/output-trigger-patterns.json` → 模块 `output-trigger`  
- `assets/cmd-audit-patterns/*` → 模块 `cmd-audit-local`（**不**吞服务器 `MIST_AUDIT` 通道）

### 4.3 目录演进（建议，分 PR）

```
src/
  kernel/          # 可选：文档别名；物理上仍 ssh/ terminal/ session
  capability/      # trait + registry + ModuleAction
  modules/
    display_link/
    output_trigger/
    cmd_audit_local/
    session_import/
  ui/              # Host：只调 registry，不内嵌规则
```

迁移动作必须 **行为字节级兼容**（现有单测全绿），禁止「借重构改产品语义」。

---

## 5. 哪些进模块 / 哪些留 Kernel

### 5.1 第一批迁入（证明架构）

| 优先级 | 模块 | 现挂载点 | 迁入收益 |
|--------|------|----------|----------|
| P0-a | `output-trigger` | `terminal` 泵 + `app` poll | 最新、最小；验证 Notify 路径 |
| P0-b | `display-link` | `terminal` 渲染 + 点击/右键 | 验证 DecorateLink + context_menu |
| P0-c | `cmd-audit-local` | `app` 发送门闩 | 验证 `on_before_send` |
| P1 | `session-import` | foreign_import / ssh_config | 验证无钩子的「纯工具模块」 |

### 5.2 永留 Kernel

- SSH 连接 / 通道 / ZMODEM / SFTP 传输状态机  
- VTE、选区、IME、快捷键转发  
- 标签分屏、右 Dock **槽位框架**（槽位内容可来自模块）  
- 自动重连调度、会话加密存储  
- 团队登录与 Token 边界  

### 5.3 灰色地带（评审时拍板）

| 能力 | 倾向 | 理由 |
|------|------|------|
| 排障复盘草稿 | Host 功能（AI Dock） | 强依赖团队 API 与 UI，不像规则引擎 |
| SFTP UI | Kernel 邻域 | 与传输状态机耦合深；子系统报错文案可模块化 |
| Agent / Gate | 独立子系统 | 已有 `core/agent`；不要伪装成「终端输出插件」 |
| 云同步面板 | Host | 账号级，非会话输出 |

---

## 6. 与现有产品红线的划界

```
用户可见「技能商店 / 选一个 Skill 跑」     →  禁止（CONVERSATIONAL-TERMINAL）
工程师可见「模块注册表 / 可禁用 display-link」 →  允许（本设计）
企业下发「只读 JSON 规则包」               →  P1 允许（仍无第三方代码）
第三方 .so 注入终端进程                    →  P2 前禁止
```

一句话：**模块是架构单元；Skill 目录是产品表面——两者不是一回事。**

---

## 7. 演进路线（建议）

| 阶段 | 交付 | 退出标准 |
|------|------|----------|
| **R0 设计评审** | 本文 + #113 拍板 | 钩子集合与「永不插件」清单签字 |
| **R1 脚手架** | `capability` crate 区 + registry + 偏好开关 UI（可先隐藏） | 空模块可注册；开关测通 |
| **R2 迁 trigger** | output-trigger 只经 `on_output_line` | 行为与 #112 一致；关闭模块无 Toast |
| **R3 迁 display-link** | 装饰 + 右键贡献走 API | #74 验收项不回退 |
| **R4 迁本地审计** | `on_before_send` | 与服务器审计双通道用例仍过 |
| **R5 文档** | 更新 `ARCHITECTURE.md` 引用本文 | 新人按图能找到该加代码的位置 |

**并行约束**：R1–R4 期间不接「插件市场」需求；有人提第三方脚本，一律指向 P2 安全票。

---

## 8. 风险与对策

| 风险 | 对策 |
|------|------|
| 钩子过多变成第二套回调地狱 | 硬顶 5 个；新钩子必须附「哪个模块卡住了」案例 |
| 模块在输出热路径拖垮帧率 | 行级扫描预算（已有 4KiB/行、命中封顶）；禁止模块内正则编译 |
| 「可开关」导致客服排障矩阵爆炸 | 默认全开；开关先藏在高级设置或仅环境变量 `MISTTERM_DISABLE_MODULES=` |
| 借模块化偷偷改产品行为 | 每个迁移 PR 必须带「关闭模块前后」对比测 |
| 与 AI Agent 边界模糊 | Agent 不注册为 MistModule；保持 `core/agent` |

---

## 9. 待评审决议（碰头用）

请在评审时明确勾选：

1. **P0 是否只做同进程内置模块**（本文推荐：是）  
2. **第一批钩子是否就这 5 个**（可先落地 2 个：`on_output_line` + `contribute_context_menu`）  
3. **偏好里是否暴露模块开关**（推荐：R1 用环境变量，R4 后再做 UI）  
4. **`src/modules/` 物理拆分是否本季度做**（推荐：R2 起按模块迁，不先大搬目录）  
5. **P2 WASM 是否立项**（推荐：不立，直到有明确企业第三方需求）  

---

## 10. 参考

- 现状分层：[`ARCHITECTURE.md`](./ARCHITECTURE.md)  
- 产品红线：[`CONVERSATIONAL-TERMINAL.md`](../product/CONVERSATIONAL-TERMINAL.md)  
- 已解耦样例：`core/display_link.rs`、`core/output_trigger.rs`、`core/cmd_audit.rs`  
- 规划票：MistTerm #113  
