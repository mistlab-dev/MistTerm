# 安装包体积（#127）

桌面功能和命令行功能都不砍。变小的是**用户下载的那一个文件**：Linux 桌面包不再塞进第二份 `mist`，macOS 主下载不再是 universal。

体积专项与 [#113](https://github.com/mistlab-dev/MistTerm/issues/113) 的扩展点无关。

## Linux：v1.2.34 实测（改打包之前）

发布文件来自 GitHub Release `v1.2.34`（2026-10-25）。桌面包用 `tar -tzvf` 列出成员；「去掉 mist 之后」是把同一批已解压文件按发布脚本的 gzip 重新打包，用来估计压缩后少掉多少，不是另一套构建。

| 成员 | 未压缩 |
| --- | --- |
| `Mist-linux-x86_64/Mist`（GUI，glibc） | 59,784,008（约 57.0 MiB） |
| `Mist-linux-x86_64/mist`（CLI，同一台 Ubuntu 24.04 上的 glibc 构建） | 60,578,392（约 57.8 MiB） |
| LICENSE / README / INSTALL | 约 53 KiB |
| 未压缩合计 | 120,415,532 |

两个 ELF 的 SHA-256 不同（不是同一份文件复制两次）。`mist` 这一份就占未压缩体积的一半。

| 压缩包 | 字节 |
| --- | --- |
| 发布的 `Mist-linux-x86_64.tar.gz`（含 `Mist` + `mist`） | 51,680,170 |
| 同一内容去掉 `mist` 再打包 | 25,587,801 |
| 只打包那份 glibc `mist` | 25,840,801 |
| 发布的 `mist-cli-linux-x86_64.tar.gz`（静态 musl，另一套构建） | 25,546,577 |
| 发布的 `mist-cli-linux-aarch64.tar.gz` | 25,117,473 |

静态 `mist-cli-*` 和桌面包里的 glibc `mist` **不是同一个二进制**（musl、`--no-default-features`、OpenSSL 打进程序里；桌面 CLI 跟 GUI 一起用系统 glibc，还带着默认的 `file-dialogs`）。重叠的是产品：桌面用户为了 GUI 不得不再下一整份命令行。命令行应只从 `mist-cli-linux-*.tar.gz` 或 `curl -fsSL https://mistlab.dev/install | bash` 安装。

从下一版起，`Mist-linux-x86_64.tar.gz` 只含 `Mist`。`scripts/measure-linux-gui-archive.py` 会在 CI 里拒绝再出现名为 `mist` 的成员。清单键仍是 `linux-x86_64`，文件名不变，所以 checksum / minisig 的生成方式不变。

### 已经装过带 `mist` 的桌面包

1.2.34 及更早的更新器若发现目录里有 `mist`，会要求新压缩包里也有 `mist`。下一版的桌面包没有这个文件，**就地一键更新会失败一次**（旧程序还在，不会装坏）。处理办法：

- 删掉桌面目录里的旧 `mist`，再用界面里的检查更新（运行的是 `Mist`）；或
- 重新解开新的 `Mist-linux-x86_64.tar.gz`。

命令行改用静态包，之后 `mist update` 只换 `mist`。新桌面程序只替换 `Mist`，旁边留下的旧 `mist` 不会被要求、也不会被盖掉。

## macOS：v1.2.34 实测（universal 主下载）

| 资产 | 字节 | 角色 |
| --- | --- | --- |
| `Mist-macos-universal.dmg` | 51,425,510 | 当时的主下载（两个架构 lipo 在一起） |
| `Mist-macos-universal-update.tar.gz` | 47,610,346 | 一键更新，只有 universal 的 `Mist.app` |
| `Mist-macos-universal.tar.gz` | 142,321,137 | 手动包：`Mist.app` + 裸 `Mist` + `mist-cli`，三份都是 universal |

发布脚本对 `aarch64-apple-darwin` 和 `x86_64-apple-darwin` 各编一次再 `lipo`。主下载因此总是带上用户用不到的那一架构。

从下一版起不再发布 universal：

| 主下载 | 可选 |
| --- | --- |
| `Mist-macos-aarch64.dmg`（签名并公证） | `Mist-macos-x86_64.dmg` |
| `Mist-macos-aarch64.tar.gz` | `Mist-macos-x86_64.tar.gz` |
| 一键更新 `Mist-macos-aarch64-update.tar.gz`，清单键 `macos-aarch64` | `Mist-macos-x86_64-update.tar.gz`，清单键 `macos-x86_64` |

完整 tar 仍带裸 `Mist` 和 `mist-cli`（Mac 没有单独的静态 CLI 包）。DMG 和一键更新包只有 `Mist.app`。

已经装上的 universal 客户端只查清单键 `macos-universal`。新清单没有这个键，所以要**手动装一次**对应架构的 dmg；之后的一键更新按当前二进制的架构走 `macos-aarch64` 或 `macos-x86_64`。

## 复现测量

```sh
python3 scripts/measure-linux-gui-archive.py --self-test
python3 scripts/measure-linux-gui-archive.py path/to/Mist-linux-x86_64.tar.gz
```
