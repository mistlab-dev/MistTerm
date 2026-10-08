# 浏览器里点 ssh:// 链接，用 MistTerm 打开

网页、内部 Wiki、监控告警里常有 `ssh://root@10.0.0.5:22` 这样的链接。设置好以后，点一下就会打开 MistTerm 并连上。

支持 Windows 和 Linux。macOS 以后再做。

## 怎么设置

**Windows 安装版**：安装时勾选「Open ssh:// links in the browser with MistTerm」（默认是勾上的）。自动更新不会改动这一项。

**Windows 便携版、Linux**：打开「设置 → 连接」，在「浏览器里的 ssh:// 链接」一行点「用 MistTerm 打开」。显示「已用 MistTerm 打开」就好了。

也可以在命令行里运行（适合批量部署）：

```
Mist --register-ssh-url
```

成功时输出 `ssh:// links will open in Mist`，失败时退出码不是 0。

只改当前用户自己的设置，不需要管理员权限。

## 点了链接以后

- **已经保存过的主机**（主机和端口一样；链接里写了用户名时，用户名也要一样）：直接连接，有好几个时用最近连过的那个。
- **没保存过的主机**：打开「新建会话」，主机、端口、用户名已经填好。填上密码或选私钥，点「保存并连接」。
- 链接里如果带了密码（`ssh://user:密码@host`），MistTerm 不会使用，会提示你。密码不该放在链接里。
- 链接写错了（端口不对、主机名有空格等），会提示打不开，不会去连接。

每次点链接都会打开一个新的 MistTerm 窗口。

## 常见问题

**Windows 上点了还是别的程序打开？**
你以前在「Windows 设置 → 应用 → 默认应用」里给 ssh 选过别的程序，Windows 不允许其他程序替你改。到那里搜 `ssh`，改成 MistTerm。MistTerm 设置里那一行会写出现在是哪个程序。

**Linux 上点了没反应？**
先在终端里试 `xdg-open ssh://你的用户名@主机`。能打开说明设置没问题，是浏览器第一次问你「要不要打开外部程序」时选了不允许，在浏览器设置里改回来就行。

**MistTerm 换了位置（比如便携版挪了目录）？**
设置里那一行会变回「用 MistTerm 打开」，再点一次就行。

**卸载**：Windows 安装版卸载时会一起去掉。Linux 删掉 `~/.local/share/applications/mistterm-ssh.desktop` 即可。
