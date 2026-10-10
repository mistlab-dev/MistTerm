# Mist 产品与技术设计（草案）：受控执行控制面

> 状态：草案・供评审・2026-10-10
> 范围：问题背景、术语、参考客户环境、核心用例（含实现要点）、架构与存量映射、商业原则、分期交付、待决项
> 关联：
>
> [CONVERSATIONAL-TERMINAL.md](./CONVERSATIONAL-TERMINAL.md)
>
> （Agent 循环与确认门闩；本文在此基础上引入控制面作为统一裁决权威）
>
> 详细设计（API / 表结构 / Runner / 工程分解）：
> mist-server `docs/architecture/control-plane-detailed-design.md`



***

## 阅读顺序



1. §1 术语

2. §2 问题与参考客户环境

3. §3 核心用例（行为 + 实现）

4. §4 产品定义与约束

5. §5 商业原则

6. §6 架构与存量映射

7. §7 分期交付

8. §8 待决项

9. §9 UC-1 时序（实现对照）

10. §10 修订记录



***

## 1. 术语

下文术语以本表定义为准；中英文可混用，含义冲突时以定义列为准。

### 1.1 主体与角色



| 术语    | 英文            | 定义                                   |
| ----- | ------------- | ------------------------------------ |
| 用户    | User          | 已认证的自然人账号                            |
| 主体    | Principal     | 可发起 Plan 的身份：User 或 Agent            |
| Agent | Agent         | 非人类主体（脚本、工作流、模型驱动进程等），持有独立 client 凭证 |
| 责任人   | Owner / Admin | 对环境安全与合规负责，配置策略与急停                   |
| 审批人   | Approver      | 处理待审批 Plan 的角色（可为 on-call）           |
| 集成方   | Integrator    | 开发或接入 Agent 的工程师                     |

### 1.2 核心对象



| 术语       | 英文            | 定义                                                             |
| -------- | ------------- | -------------------------------------------------------------- |
| 意图       | Intent        | 自然语言或系统描述的目标（尚未可执行）                                            |
| 计划       | Plan          | 可裁决、可审批的结构化提案（目标、步骤、风险相关元数据）                                   |
| 步骤       | Step          | Plan 内原子动作（执行器类型、目标、命令或 API 调用等）                               |
| 策略       | Policy        | 强制规则集，决定 Plan 的裁决结果                                            |
| 裁决       | Decision      | `allow_auto` / `pending_approval` / `deny`                     |
| 审批       | Approval      | 审批人对 Plan 的明确决定（含收窄目标等）                                        |
| 执行租约     | Lease         | 绑定 Plan（及范围）的短时、窄权限执行许可                                        |
| 执行器      | Effector      | 触达真实资源的适配器；MVP 为 `ssh`                                         |
| 运行实例     | Run           | 持 Lease 的一次执行尝试                                                |
| 证据       | Evidence      | Run 的可核验记录（计划快照、裁决 / 审批、命令、输出摘要等）                              |
| 急停       | Kill Switch   | 禁用某 Principal 或环境：拒绝新 Lease，并取消进行中执行                           |
| 旁路       | Bypass        | 不经控制面仍可变更生产的路径（长期密钥、裸 SSH、未纳入审计的交互式 PTY 等）                     |
| 控制面      | Control Plane | Plan / Policy / Approval / Lease / Evidence / Kill Switch 所在服务 |
| 治理控制台    | Console       | 面向责任人 / 审批人的 Web 管理界面                                          |
| MistTerm | MistTerm      | 桌面 SSH 客户端；过渡期执行器与获客入口，非终局产品定义                                 |

### 1.3 风险等级



| 等级       | 含义                     | 默认策略倾向                |
| -------- | ---------------------- | --------------------- |
| R0 只读    | 不修改系统状态                | 策略允许时可 `allow_auto`   |
| R1 低风险变更 | 可逆或影响范围（blast radius）小 | 通常 `pending_approval` |
| R2 高风险变更 | 难逆或影响数据 / 权限 / 产能      | 必须审批                  |
| R3 禁止    | 策略黑名单                  | `deny`                |

风险由 **环境标签 + 命令 / API 特征 + Principal 授权范围** 判定，**不以模型自评结果为准**。可复用 `cmd_audit` / 命令分类作为特征输入。



***

## 2. 问题与参考客户环境

### 2.1 问题陈述

运维自动化与 Agent 正在直接连接主机执行命令。组织面临：



1. 长期凭证进入脚本 / Agent 后难以收回与追责；

2. 即时通讯中的口头批准无法形成可导出证据；

3. 缺少对非人类主体的急停与执行租约机制；

4. 现有「团队 SSH 客户端」能力无法单独覆盖上述控制需求。

目标产品形态：**在 Agent（或自动化）与生产资源之间，强制走 Plan → Policy → Lease → Effector → Evidence 这条路径，并逐步消除绕过它的旁路。**

### 2.2 参考客户环境（示例组织 Org-A）

用于锚定需求与验收口径，是一个示意性的客户画像，不代表真实组织。



| 项    | 取值                                              |
| ---- | ----------------------------------------------- |
| 规模   | 约 6 人产品工程团队                                     |
| 业务   | 垂直 SaaS                                         |
| 资产   | 约 10+ Linux 主机（prod /staging/demo）              |
| 协作   | 飞书 / 钉钉；告警来自 Prometheus 等                       |
| 现状痛点 | 跳板机密钥曾在群文件共享；变更常靠群聊确认；客户追问「谁动过机器」时拿不出结构化证据      |
| 近期变化 | 已有告警触发的自动 SSH 巡检脚本；曾出现只读账号被改回高权限、主机命名相近导致误操作的风险 |

### 2.3 与当前 Mist 能力的关系



| 能力                                  | 现状 | 在控制面中的角色                                  |
| ----------------------------------- | -- | ----------------------------------------- |
| MistTerm + 团队登录 / 短时证书              | 已有 | 人工接入与证书签发基础；证书需演进为任务级 Lease 的落地手段         |
| 命令库 /snippets                       | 已有 | 非主线；可作策略白名单的样本                            |
| `audited_command_*` / 服务端审计         | 部分 | Evidence 的输入之一；**交互式 PTY 手输仍未覆盖，须对外如实说明** |
| AI 多机计划卡 / `planner` / `batch_exec` | 已有 | Plan 草案生成与 SSH 执行内核候选；**裁决权威收归控制面**       |



***

## 3. 核心用例（行为 + 实现）

每个用例包含：触发条件、期望行为、控制流、实现要点、存量复用。



***

### UC-1：Staging 只读巡检（自动放行）

**触发**

Staging 磁盘告警；自动化希望在标签 `env=staging` 的主机上执行只读检查（如 `df` / `du`），并将结果回写告警通道。

**期望行为**



1. Agent 提交 Plan（只读步骤 + 目标选择器）。

2. Policy：`env=staging` 且步骤只读 → `allow_auto`。

3. 签发 Lease；Effector (ssh) 短连接执行。

4. 写入 Evidence；通知通道仅含摘要 + Console 深链。

5. 人工仍可用 MistTerm 并行复核（过渡期允许旁路存在，但不鼓励）。

**控制流**



```
Alert/Cron → Agent Worker
  → POST Plan
  → Control Plane: Policy → allow_auto → Lease
  → Effector(ssh) 执行
  → Evidence + 出站 Webhook 通知
```

**实现要点**



| 项       | 说明                                                            |
| ------- | ------------------------------------------------------------- |
| Plan 生成 | 告警类型可映射固定只读 Step 模板；不强制每次调用 LLM                               |
| 目标选择    | 主机 / 资源标签（团队服务器列表扩展）；与 team server / MistTerm 会话名册同步          |
| 只读判定    | `readonly=true` + 命令特征表双重校验                                   |
| 凭证      | Agent **不持有**长期主机密钥；由 Runner 在 Lease 校验后从密钥库取用或签发任务证书         |
| 复用      | 执行路径对齐 `batch_exec`；UI 计划展示可参考 MistTerm `agent_plan`，裁决以控制面为准 |



***

### UC-2：生产变更须审批（支持收窄目标）

**触发**

Agent 提议在 `prod-web-01`、`prod-web-02` 删除超过 7 天的应用日志（非只读）。

**期望行为**



1. Policy：`env=prod` 且非只读 → `pending_approval`。

2. 审批人在 Console 查看 Plan：Intent、目标、完整命令、策略命中原因。

3. 审批人可将目标收窄为仅 `prod-web-01` 后批准。

4. 签发 Lease → 执行 → Evidence。

5. 可再批准其余主机或提交新 Plan。

6. 支持导出 Evidence Bundle（提出者、审批者、命令、时间、输出摘要 / 指针）。

7. 若用户绕过控制面使用交互式 SSH：记为旁路；Console 需逐步暴露 "有哪些未受控操作"（分期完善）。

**控制流**



```
Agent → POST Plan
Control Plane → pending_approval → 通知 Approver（深链；IM 不得作为唯一批准通道）
Approver → approve(scope' ) → Lease
Runner → SSH 执行 → Evidence
```

**Plan 示例（字段级）**



```
{
  "id": "pln_xxx",
  "team_id": "tm_...",
  "principal": { "type": "agent", "id": "agt_disk" },
  "intent": "清理生产 Web 应用日志以释放磁盘",
  "environment": "prod",
  "steps": [
    {
      "id": "s1",
      "effector": "ssh",
      "targets": ["host:prod-web-01"],
      "command": "find /var/log/app -type f -name '*.log' -mtime +7 -delete",
      "readonly": false
    }
  ],
  "decision": {
    "status": "allow",
    "approver_user_id": "u_...",
    "policy_hits": ["prod_requires_approval", "destructive_fs"]
  }
}
```

**Lease 约束（最小集）**

绑定 `plan_id` / 允许的 `step` 与 `targets`；TTL；最大并行与步数；过期或用尽即失效。

**复用**

命令风险特征 ← `cmd_audit` / `classify_command`；MistTerm「确认执行」演进为提交 / 服从控制面 Plan（feature flag）。



***

### UC-3：Principal 急停

**触发**

Agent 因错误目标列表或标签误标，对多台主机提交变更类 Plan；责任人需立即停止该 Agent。

**期望行为**



1. Console 对 Principal 执行 Kill Switch。

2. 撤销未过期 Lease；拒绝新的 submit/execute（`denied.kill_switch`）。

3. Runner 取消进行中连接。

4. 记录操作者与时间戳。

5. 恢复时需显式重新 enable；不自动恢复旧 Lease。

**说明**

破坏性命令一旦执行即无法回滚，所以 R2 高风险变更默认必须审批，并支持审批时收窄目标，把急停兜底作为最后手段。

**实现要点**

每次执行前校验 Lease；Runner 维护 `lease_id → session` 映射，以便急停时取消进行中的会话。



***

### UC-4：附加客户形态（验收变体）



| 形态                | 差异需求                         | 实现影响                      |
| ----------------- | ---------------------------- | ------------------------- |
| 单人 / 极小规模（少量 VPS） | 流程同 UC-1/2，审批可为移动端打开 Console | 同一 API；示例 Worker 可本机 cron |
| 多客户外包运维           | Evidence 按客户隔离导出             | 租户 / 客户维度过滤；非新执行器         |



***

## 4. 产品定义与约束

### 4.1 定义

Mist 提供**受控执行控制面**：Agent 与自动化必须通过 Plan → Policy → Lease → Effector → Evidence 访问约定资源；责任人可配置策略、审批与急停。

MistTerm 保留为人工排障客户端与过渡执行路径，**不作为公司终局产品定义**。

### 4.2 对外表述（可选）



* 中文：Agent 可操作生产环境，但不得绕过许可、审批与证据链。

* 英文：Agents may operate production. They must not own it.

### 4.3 非目标



* 不做通用运维聊天机器人门面

* 不做 "无审批的生产自愈" 作为主叙事

* 不以终端主题 / 命令市场为主线

* 不把 IM 一键回复当作唯一审批依据

* 不宣称已覆盖全部交互式 PTY 输入审计（当前未覆盖）

### 4.4 设计约束



1. 无 Plan 不得对生产路径执行（沙箱除外）。

2. Decision 仅由 Policy 引擎给出。

3. Agent 不持有长期主机密钥（万能钥匙）；执行凭证由 Lease + Runner 按任务签发。

4. Approval 落库；IM 仅通知 + 深链。

5. 稳定错误码：`pending.approval.required`、`denied.policy.*`、`denied.kill_switch`、`denied.lease.expired`。

6. 旁路须可观测（分期）。

7. 覆盖缺口对外如实披露。

### 4.5 界面职责



| 界面        | 用户          | 职责                                                            |
| --------- | ----------- | ------------------------------------------------------------- |
| Console   | 责任人 / 审批人   | Agents、Policies、Approvals、Runs、Kill Switch、导出                 |
| API / MCP | 集成方 / Agent | `submit_plan` → `await_decision` → `execute` → `get_evidence` |
| MistTerm  | 人工排障        | 交互式 SSH；AI / 批量路径逐步接入控制面                                      |



***

## 5. 商业原则



1. MistTerm 与早期试用保持低摩擦（开源客户端、控制面可长周期试用）。

2. 收费时机：在生产接入、对外证据导出、多客户隔离、私有化与支撑等责任增大的节点再考虑收费；个案协商，早期不发布刚性功能矩阵价目表。

3. 不以关闭急停 / 基础 Evidence 作为免费限制手段。

4. 模型推理费用默认客户自备 API Key。

5. 计量维度可后置；实现阶段保证环境标签与 Run 量可观测即可。



***

## 6. 架构与存量映射

### 6.1 逻辑架构



```
┌──────────────┐      ┌─────────────────────────────────────┐
│ Agent Worker │      │ Control Plane (mist-server 演进)      │
│ / Integrator │─────►│ Plan · Policy · Approval · Lease     │
└──────────────┘      │ Evidence · Kill Switch               │
                      └──────────────────┬──────────────────┘
┌──────────────┐                         │ Lease 校验后执行
│ MistTerm     │      ┌──────────────────▼──────────────────┐
│ (人工/调试)   │─────►│ Effector Gateway / SSH Runner         │
└──────────────┘      └──────────────────┬──────────────────┘
                                         ▼
                                      目标主机
```

**Runner 部署**



* MVP：与控制面同信任域的托管 Runner。

* 变体：客户内网 Runner（Agent 仅持 Lease；主机凭证不出客户边界）—— 适合外包 / 内网场景。

### 6.2 最小 API



| 操作          | 方法（示意）                       |
| ----------- | ---------------------------- |
| 创建 Plan     | `POST /v1/teams/:tid/plans`  |
| 查询 Plan     | `GET /v1/plans/:id`          |
| 审批          | `POST /v1/plans/:id/approve` |
| 启动 Run      | `POST /v1/plans/:id/runs`    |
| 获取 Evidence | `GET /v1/runs/:id/evidence`  |
| 急停          | `POST /v1/agents/:id/kill`   |

写操作支持 `Idempotency-Key`。

### 6.3 Plan 状态机



```
draft → submitted → allow_auto | pending_approval | denied
pending_approval → allow | denied | allow_narrowed
allow* → running → completed | failed | cancelled
kill(principal) → 后续 submitted → denied.kill_switch
```

客户端本地 Agent 循环（澄清 / 等待确认 / 执行）可保留为草案生成的交互方式；**生产路径以控制面状态为准**。

### 6.4 存量映射



| 组件                     | 映射                                |
| ---------------------- | --------------------------------- |
| `planner` / AI 计划卡     | Plan 草案生成                         |
| `batch_exec`           | Effector (ssh) 执行内核               |
| `cmd_audit` / 命令分类     | Policy 特征输入                       |
| 团队主机列表                 | Plan targets                      |
| 短时证书 / Vault           | Lease 落地时的任务级凭证（阶段 2 增强）          |
| Console / mist-website | 治理控制台                             |
| 现有审计日志                 | 迁移为 Evidence 写入，需对齐 Plan/Lease 字段 |



***

## 7. 分期交付

### 阶段 0 — 对齐

统一术语与 UC 验收口径；mist-server 可建 Plan 领域骨架。

### 阶段 1 — UC-1 + UC-2 闭环（约 6–8 周）

**验收**



* Staging 只读 Plan 可自动执行并产生 Evidence

* 生产非只读 Plan 必须经 Console 审批，支持收窄 targets

* 可导出 Evidence Bundle

* 可对 Agent Principal 执行急停

**交付**

Plan/Policy/Lease/Run/Evidence 存储；Policy v0；SSH Runner；Console 审批箱与导出；示例 Agent Worker；MistTerm feature flag 提交控制面。

**不做**

完整旁路计量、MCP 全量、多云 Effector、IM 内批准。

### 阶段 2 — 非人类主体与唯一出口可演示

Agent 注册与 client 凭证；任务级证书；MCP 薄封装；内网 Runner 文档；旁路消除操作手册。

**验收**：禁用凭证后，约定主机上该 Agent 变更成功次数为 0。

### 阶段 3 — 策略可维护性与旁路指标

策略配置体验增强；多客户导出；可选第二 Effector；未受控比例仪表盘。



***

## 8. 已决 / 后置项

工程向决议见 mist-server `docs/architecture/control-plane-detailed-design.md` §21（E1–E6）。

| ID  | 议题               | 状态 | 决议 |
| --- | ---------------- | ---- | ----------------------- |
| Q1  | 对外主叙事切换时机        | 后置 | 阶段 1 验收后再切 |
| Q2  | MistTerm 路线图定位   | 已决 | 兼容与获客，非主线排序键 |
| Q3  | 交互式人工输入是否纳入 Plan | 已决 | 阶段 1 仅 Agent / 批量；旁路须可见 |
| Q4  | IM 是否可作为批准通道     | 已决 | 否；仅通知 + 深链 |
| Q5  | Runner 默认部署      | 已决 | MVP 托管；文档提供内网变体 |
| Q6  | 首个 Agent 形态      | 已决 | 官方示例巡检 Worker |
| Q7  | MistDocs         | 已决 | 移出主叙事（不作为主推） |
| Q8  | 商业化              | 后置 | 见 §5；不提前发布硬价目表 |
| Q9  | 控制面代码位置          | 已决 | 优先 mist-server 演进 |
| Q10 | 无控制面离线执行         | 已决 | 仅开发 / 演示退化 |
| Q11 | 审批 / Kill 角色     | 已决 | **仅 Admin**；Editor+ 建 Plan |
| Q12 | 阶段 1 执行出口       | 已决 | **一律服务端 Runner** |
| Q13 | 服务端 AI tools     | 已决 | **阶段 1 末强制走控制面** |

### 排期判断



1. 是否强化 Agent 受控执行，而非仅优化人工终端体验？

2. 是否落入 Plan / Policy / Lease / Evidence 之一？

3. 若无 Agent 主体，该项是否仍必要？



***

## 9. UC-1 时序（实现对照）



```
Agent          Control Plane           Runner              Hosts
  |-- Plan --->|                       |                   |
  |            |-- allow_auto+Lease -->|                   |
  |-- Run ---->|-- validate Lease ---->|-- ssh exec ------>|
  |            |<-- output ------------|<------------------|
  |            |-- Evidence            |                   |
  |<- status --|                       |                   |
```

UC-2 在 Plan 后插入 Approval；Lease 仅在批准后签发。

UC-3 在任意时刻 Kill → Lease 失效 + Runner cancel。



***

## 10. 修订记录



| 日期         | 说明                              |
| ---------- | ------------------------------- |
| 2026-10-10 | 多轮草案迭代                          |
| 2026-10-10 | 改为专业术语；用例表述替代「故事」体；场景与实现合写      |
| 2026-10-10 | 通读修订：修正直译腔与表述不通顺处（术语、逻辑衔接、冗余表达） |
| 2026-10-10 | 关联详细设计：mist-server `docs/architecture/control-plane-detailed-design.md` |
| 2026-10-10 | 锁定 Q11–Q13 / E1–E6：Admin 审批；服务端 Runner；阶段 1 末 AI tools 收口 |



***

**下一步**：按详细设计 §19 拆工程里程碑并开分支实现。