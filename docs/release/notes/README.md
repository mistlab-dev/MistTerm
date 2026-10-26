# 更新说明（可选）

发版时如果这里有 `v<版本>.zh.md` / `v<版本>.en.md`（例如 `v1.2.0.zh.md`），
CI 会把内容写进 `latest.json`，客户端的"发现新版本"窗口里会直接显示。
没有这两个文件也没关系，窗口里会显示指向 GitHub Release 页面的链接。

写法：普通 Markdown，几条要点即可，用户看得懂的大白话。

打包减负（[#127](https://github.com/mistlab-dev/MistTerm/issues/127)，与扩展点 #113 无关）落地后的**第一版**说明里写上这三句，避免用户去下已经不存在的 universal 包，或在桌面包里找 `mist`：

- Linux 桌面 `Mist-linux-x86_64.tar.gz` 只有 `Mist`。命令行用 `mist-cli-linux-x86_64.tar.gz` / `mist-cli-linux-aarch64.tar.gz`，或 `curl -fsSL https://mistlab.dev/install | bash`。目录里若还有旧的 `mist`，先删掉再检查更新，或重新解压。
- Mac 主下载是 `Mist-macos-aarch64.dmg`（Apple Silicon）。Intel 用 `Mist-macos-x86_64.dmg`。不再提供 universal。已经装了 universal 的，先手动换一次对应架构的 dmg，之后才能一键更新。
- 细节和 v1.2.34 的体积表：`docs/release/PACKAGE_SIZE.md`。
