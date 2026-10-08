# 让 AI 助手（Codex、Claude Code）用 mist 查服务器

Codex、Claude Code 这类在你电脑上运行的 AI 助手，可以通过 `mist exec` 在你保存过的服务器上执行命令，帮你看日志、查进程、看磁盘。

- 用的是 MistTerm 里已经保存的会话，**不用把服务器密码告诉 AI**。
- 只读的命令直接执行；会改动服务器的命令 mist 不会执行，要你同意后才执行。
- 每条命令（执行的、拦下的、你同意的）都记下来，和桌面版记在同一个地方。

## 一、准备

1. 安装 MistTerm，确认终端里能运行 `mist`：

   ```
   mist --version
   mist ls
   ```

   Windows 上命令也是 `mist`（安装目录里的 `mist.cmd`）。提示找不到时，把 MistTerm 的安装目录加进 PATH，或写完整路径。

2. 在 MistTerm 里保存要让 AI 看的服务器，起个好认的名字，例如 `测试机-01`。先自己试一下：

   ```
   mist exec 测试机-01 -- "df -h"
   ```

   能看到磁盘情况就可以了。**建议先只给 AI 用测试机。**

## 二、mist 怎么把关

AI 助手运行 `mist exec` 时，mist 会先看这条命令：

| 命令 | mist 怎么做 | 退出码 |
| --- | --- | --- |
| 只读的：看日志、看进程、看磁盘、`curl` 健康检查等，例如 `df -h`、`journalctl -u nginx \| grep error`、`ps aux \| head` | 直接执行 | 远端命令的退出码 |
| 会改动服务器的：`rm`、`systemctl restart`、写文件（`>`、`sed -i`）、`curl -X POST` 等 | **不执行**，告诉 AI「需要人确认」 | 76 |
| 看不出是否只读的（例如 `mysql -e …`） | 和会改动的一样，**不执行** | 76 |
| 团队命令策略要求确认的 | **不执行**，要你确认 | 76 |
| 团队命令策略禁止的 | 不执行，加 `--yes` 也不执行 | 77 |

要执行会改动服务器的命令，必须写成 `mist exec --yes 目标 -- 命令`（`--yes` 紧跟在 `exec` 后面，写在别处会报错）。下面第三、四步会把 AI 助手设成：**只要命令里有 `mist exec --yes`，每次都先问你**。

自己在终端里直接用 `mist exec` 时，遇到会改动的命令 mist 会当场问你「确认执行吗？」，输入 `y` 回车才执行。

团队命令策略用的是桌面版最近一次同步下来的设置，所以至少要用桌面版登录过一次团队。

## 三、设置 Claude Code

在项目目录的 `.claude/settings.json`（只给自己用就放 `.claude/settings.local.json`）里加上：

```json
{
  "permissions": {
    "allow": [
      "Bash(mist ls *)",
      "Bash(mist ls)",
      "Bash(mist exec *)",
      "Bash(mist rls *)"
    ],
    "ask": [
      "Bash(mist exec --yes *)",
      "Bash(mist exec -y *)",
      "Bash(mist frag run --yes *)",
      "Bash(mist frag run -y *)"
    ]
  }
}
```

Claude Code 先看 `ask` 再看 `allow`，所以 `mist exec --yes …` 每次都会先问你，其它 `mist exec` 直接运行（会改动的命令 mist 自己会拦下）。

`mist put`（上传文件）、`mist ssh`、`mist fwd` 没有放进 `allow`，用到时 Claude Code 会照常问你。

## 四、设置 Codex

新建 `~/.codex/rules/mist.rules`：

```python
# 只读查询直接运行（会改动的命令 mist 自己会拦下）
prefix_rule(
    pattern = ["mist", ["ls", "exec", "rls"]],
    decision = "allow",
    justification = "mist 自己会拦下会改动服务器的命令",
    match = ["mist ls", "mist exec 测试机-01 -- df -h"],
    not_match = ["mist put a.txt 测试机-01:/tmp/", "mist ssh 测试机-01"],
)
# 确认执行：每次都先问你
prefix_rule(
    pattern = ["mist", "exec", ["--yes", "-y"]],
    decision = "prompt",
    justification = "这条命令会改动服务器，需要你同意",
    match = ["mist exec --yes 测试机-01 -- systemctl restart nginx"],
)
prefix_rule(
    pattern = ["mist", "frag", "run", ["--yes", "-y"]],
    decision = "prompt",
    justification = "这条命令会改动服务器，需要你同意",
)
```

Codex 同时匹配多条时按最严的来，所以 `mist exec --yes …` 一定会先问你。`allow` 的命令在 Codex 的沙盒外运行，这样 mist 才能连上服务器。

可以这样检查规则：

```
codex execpolicy check --pretty --rules ~/.codex/rules/mist.rules -- mist exec --yes 测试机-01 -- rm /tmp/x
```

输出里 `"decision": "prompt"` 就对了。

## 五、告诉 AI 怎么用

把下面这段放进项目的 `AGENTS.md`（Codex）或 `CLAUDE.md`（Claude Code）：

```markdown
## 查服务器

用 MistTerm 的 mist 命令，不要直接 ssh，也不要问我要密码。

- 看有哪些服务器：`mist ls`（要结构化结果用 `mist ls --json`）
- 执行命令：`mist exec <会话名> -- "<命令>"`，整条命令放在一对引号里（管道、分号会交给服务器上的 shell）。
  要结构化结果用 `mist exec --json <会话名> -- "<命令>"`。
- 先用只读命令排查：日志、进程、磁盘、端口、curl 健康检查。
- 如果 mist 退出码是 76（提示「需要人确认」）：不要自己加 --yes。先把要执行的命令和原因告诉我，
  我同意后再用 `mist exec --yes <会话名> -- "<命令>"` 执行。
- 退出码 77 表示团队策略禁止，换别的办法或问我。
```

然后就可以直接说：「看看 测试机-01 的磁盘还剩多少」。AI 会运行 `mist exec 测试机-01 -- "df -h"` 并把结果整理给你。

## 六、在哪里看记录

每条命令都会记下来：

- **审计日志**（和桌面版同一份）：执行的记为 `command.submit`，你同意的另记 `command.confirmed`（写明是 `--yes` 还是终端里输入的 `y`），拦下的记为 `command.needs_confirm` / `command.blocked`，在终端里取消的记为 `command.cancelled`。都带 `"source": "cli"`。
  - Linux：`~/.config/mistterm/audit/audit-日期.jsonl`
  - Windows：`%APPDATA%\mistterm\audit\`
  - macOS：`~/Library/Application Support/mistterm/audit/`
  - 登录了团队并开启了审计上报时，桌面版下次运行会一起上报。
- **执行历史**（命令、目标、退出码、输出摘要）：`~/.mist/logs/exec-history.jsonl`。桌面 AI 助手分析失败原因时也会看这里。

## 常见问题

**AI 说命令「看不出是否只读」被拦了，但其实是只读的？**
mist 只认识常见的只读写法，认不出的一律当作要确认。你看过觉得没问题，同意 AI 加 `--yes` 执行就行。

**能不能让 AI 一直不问？**
不建议。会改动服务器的命令每次都应该由人看一眼。

**`mist exec` 不在终端里运行时（被 AI、脚本调用）为什么不弹确认？**
没有终端可问，所以直接拒绝并说明原因，由 AI 来问你。脚本里确实要执行会改动的命令时，写成 `mist exec --yes …`。
