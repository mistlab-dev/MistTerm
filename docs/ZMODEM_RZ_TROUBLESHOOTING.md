# ZMODEM -> rz 排障结论（2026-05）

## 背景

场景是 MistTerm 内置 `zmodem2` 发送（本机 -> 远端 `rz`）在部分主机上出现：

- 首包发出后无 `ZRPOS`；
- 或远端 `rz` 退出后，协议字节被 shell 当命令执行（`command not found`）；
- 或 `rz` 长时间停在等待状态，看起来“卡住”。

本结论基于多轮日志、TX dump 与远端 `strace` 结果整理。

## 已确认事实

1. **链路不是“没发出去”**  
   远端 `strace` 明确显示 `rz` 的 `read(0, ...)` 收到了我们发出的 ZMODEM 字节。

2. **失败可以发生在协议解析阶段**  
   某些包形态下，`rz` 会记录 `no.name/ZMODEM: got error` 并 `exit_group(128)`。

3. **也可能进入 fallback（非 ZMODEM 主路径）**  
   在另一些情况下，`rz` 会周期性输出单字节 `C`（X/YMODEM 风格等待），导致会话僵住。

4. **`command not found` 不是根因，是后果**  
   这是 `rz` 已退出后，后续协议字节落到 shell 的结果。

## 关键定位证据

- `strace` 中出现：
  - `read(0, "**\30A...1111.txt...") = ...`（收到发送流）
  - `sendto(... "no.name/ZMODEM: got error", ...)`
  - `exit_group(128)`
- 会话僵住时出现：
  - 反复 `write(1, "C", 1)`（fallback 信号）
- 本地日志出现：
  - `text='**010004...: command not found'`（说明已回 shell）

## 本次落地的稳定性策略

1. **发送侧默认不回 sender ZRINIT**

- 变量：`MISTTERM_ZMODEM_SENDER_REPLY_ZRINIT`
- 默认：`false`
- 仅当显式设置 `1/true` 时开启。

2. **保留 ZFILE 编码可切换能力**

- 变量：`MISTTERM_ZMODEM_ZFILE_BIN32`
- 默认：`false`（优先 ZBIN16）
- 用于对照特定远端实现兼容性。

3. **关闭“静默期主动重发+轮换”的激进策略**

- 避免在同一会话里快速切换多种线材形态，降低对端进入 fallback 的概率。

4. **增强失败可观测性**

- 检测到 shell 回退标记（`command not found` / prompt）时快速失败；
- `feed 后无待发帧`日志增加可读文本片段 `text='...'`；
- 错误信息携带首段回显，减少盲查成本。

5. **检测远端 fallback 迹象**

- 握手期若收到连续 `C`（可夹杂 CR/LF），判定远端已转入 X/YMODEM 等待并失败返回。

## 推荐操作方式

1. 远端先执行：

```bash
rz -y
```

2. 在 MistTerm 里选择上传文件。

3. 若失败，优先看错误提示是否包含：

- `远端已回到 shell...`（说明 rz 已退出）
- `已切换到 X/YMODEM（收到连续 'C'）`（说明进入 fallback）

## 建议排障流程（可复用）

1. 开启本地发送 dump（可选）：

```bash
MISTTERM_ZMODEM_DUMP_TX=1
```

2. 远端用 `strace` 抓 `rz`：

```bash
strace -ff -tt -s 256 -o /tmp/rz.trace rz -y 2>/tmp/rz.err
```

3. 复现后查看：

```bash
rg -n "read\\(0|write\\(1|write\\(2|TIMEOUT|exit_group|got error|SIG" /tmp/rz.trace*
sed -n '1,200p' /tmp/rz.err
```

4. 判断分支：

- `read(0, ...)`能看到协议字节 -> 不是链路丢包；
- 若 `got error` + `exit_group(128)` -> 协议解析失败；
- 若反复 `write(1, "C", 1)` -> fallback 卡住。

## 当前结论

在本次修正后，`rz -y` 已可成功上传。  
后续若遇到新主机兼容问题，优先复用本文件流程，并保留变更开关进行 A/B 对照。

