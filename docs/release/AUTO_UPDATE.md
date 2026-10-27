# MistTerm 自动更新：设计与发版手册

> 适用版本：1.2.0 起。方案原文见内部文档《mistterm-autoupdate-plan》，本文记录**实际实现**。

## 1. 用户看到什么

| 平台 / 安装方式 | 行为 |
| --- | --- |
| Linux 桌面压缩包（目录里的 `Mist`；不再附带 `mist`） | 提醒 + 一键更新，只替换 `Mist`。终端里也可以 `Mist update` |
| Linux 命令行静态版（`mist-cli-linux-*.tar.gz` / 官网 `/install`） | `mist update` 一键更新，只换 `mist`（见 [CLI_STATIC.md](CLI_STATIC.md)） |
| Windows 安装版（Inno Setup，目录里有 `unins000.exe`） | 提醒 + "安装并重启"：静默运行新安装程序，装完自动重新打开 |
| Windows 便携版（zip） | 提醒 + 一键更新（替换 `Mist.exe`、`mist-cli.exe`、`mist.cmd`） |
| macOS 单架构 `.app`（arm64 或 x86_64） | 一键更新对应架构的 `Mist-macos-<arch>-update.tar.gz`（已签名）。已安装的 universal 包要先手动换一次单架构 dmg |
| 包管理器 / 源码构建 / 无写权限目录 / glibc 太旧 | 只提醒，并说明原因和该怎么做（例如 `sudo mist update`） |

- 默认：**自动检查开、自动下载开、从不自动重启**。发现新版先在后台下好；安装仍需用户点按钮，装失败再引导打开下载页。偏好设置 → 通用 里可关。
- 环境变量 `MIST_DISABLE_UPDATE_CHECK=1`：彻底关闭（GUI 和 CLI 都不联网检查）。
- CLI：`mist update --check`（有新版退出码 10）、`mist update`（= `mist self-update`）、`mist update --rollback`。
  CLI 不会在其它命令里主动提示更新。
- 只有 stable 通道；清单格式已为 beta 预留 `channel` 字段。

## 2. 发布物

每个 `v*` 标签的 GitHub Release 额外包含：

| 文件 | 说明 |
| --- | --- |
| `latest.json` | 更新清单：版本、发布时间、各平台文件名/大小/SHA-256/下载地址、更新说明 |
| `latest.json.minisig` | 清单的 minisign 签名 |
| `SHA256SUMS` / `SHA256SUMS.minisig` | 所有文件的校验和及其签名（给手动下载的用户核对用） |
| `mist-cli-linux-x86_64.tar.gz` / `mist-cli-linux-aarch64.tar.gz` | 命令行静态版（不依赖 glibc），清单键名 `linux-x86_64-cli` / `linux-aarch64-cli` |
| `Mist-macos-aarch64-update.tar.gz` | Apple Silicon 一键更新：只含已签名的 `Mist-macos-aarch64/Mist.app`。清单键 `macos-aarch64`。主下载是同名 `.dmg`（不是这个更新包，也不是 universal） |
| `Mist-macos-x86_64-update.tar.gz` | Intel Mac 一键更新，清单键 `macos-x86_64`。对应 `.dmg` 是可选下载 |
| `Mist-linux-x86_64.tar.gz` | Linux 桌面，清单键 `linux-x86_64`。只含 `Mist`，不含 `mist` |

不再发布 `Mist-macos-universal.dmg` / `.tar.gz` / `-update.tar.gz`，清单里也没有 `macos-universal`。文件名和清单键一一对应，`SHA256SUMS` 与 `latest.json.minisig` 的签法不变。体积和一次手动切换见 [PACKAGE_SIZE.md](PACKAGE_SIZE.md)。

客户端按顺序取清单（`src/core/updater/mod.rs` 的 `STABLE_MANIFEST_URLS`）：

1. `https://mistlab.dev/downloads/mistterm/stable/latest.json`（主）
2. `https://github.com/mistlab-dev/MistTerm/releases/latest/download/latest.json`（备用）

安装包下载地址写在清单的 `urls` 数组里，顺序是镜像优先、GitHub 备用
（`--mirror-base https://mistlab.dev/downloads/mistterm/v<版本>`，`--url-order mirror,github`）：

1. `https://mistlab.dev/downloads/mistterm/v<版本>/<文件名>`
2. `https://github.com/mistlab-dev/MistTerm/releases/download/v<版本>/<文件名>`

客户端按数组顺序尝试，前一个失败再试下一个。镜像文件在服务器 `/var/www/mistlab/downloads/mistterm/`。

## 3. 安全设计（简述）

- **先验签，后解析**：清单签名用内置公钥校验通过后才会解析 JSON；签名的"可信注释"必须正好是
  `mistterm <channel> <version> <pub_date>`，防止拿旧清单冒充新清单。
- **两把公钥**：主钥 + 备用钥（`resources/update/minisign-{primary,backup}.pub`），主钥泄露/丢失时可用备用钥签发过渡版本。
- **防降级 / 防冻结**：记录见过的最高版本和发布时间，更旧的清单一律忽略。
- **安装包校验**：边下载边算 SHA-256，与签名清单里的值不一致就丢弃，换下一个地址。
- **只允许 HTTPS**；每一跳重定向都检查；拒绝 HTML 响应和超大响应。
- **替换前先试运行**：新程序 `--version` 跑通才替换；旧版本放进 `.mist-update-backup/`，失败自动恢复，用户也可 `mist update --rollback`。
- **同一时间只有一个更新**：`update.lock` 文件锁（GUI 与 CLI 共用）。

### 内置公钥

两把正式公钥已提交（2026-10-07，由 Tian 在自己电脑上生成；私钥加密保存在他那里，从未经过共享机器）：

| 文件 | 用途 | key ID |
| --- | --- | --- |
| `resources/update/minisign-primary.pub` | 日常发版签名（私钥放 CI `release` 环境） | `2F850D3521ADC099` |
| `resources/update/minisign-backup.pub` | 离线备用，**不进 CI**；主钥泄露/丢失时签发换钥版本 | `E3C0CEA51587625C` |

- 单元测试 `core::updater::keys::tests::embedded_production_keys_are_valid` 会核对格式、key ID，并确认两把不同；换钥时要同步改测试里的期望值。
- 防呆仍在：正式发版构建（推 `v*` 标签，CI 设置 `MIST_DIST_CHANNEL=github-release`）里，`build.rs` 发现公钥文件
  是占位符 / 格式不对 / 两把相同，或开启了 `update-test` 特性，会**直接构建失败**。
- 测试密钥只在 `--features update-test` 构建里额外生效（`MIST_UPDATE_TEST_PUBKEY`，端到端测试每次临时生成），正式构建禁止开启该特性。

**换钥流程（主钥泄露或丢失时）**：生成新主钥 → 更新 `minisign-primary.pub` 和上面的测试 → 用**备用钥**签发这个换钥版本的
`latest.json`（老客户端信任备用钥，所以能验证并升级）→ 把 CI 的 `MINISIGN_SECRET_KEY` / `MINISIGN_PASSWORD` 换成新主钥。
注意：CI 的 sign job 默认用 `minisign-primary.pub` 验证，签换钥版本时需临时改成备用钥的公钥文件。

## 4. CI 流程（`.github/workflows/build.yml`）

```
preflight（版本号一致性 + 清单脚本自测）
  → platform ×4（Linux、Windows、macOS arm64、macOS x86_64；正式构建会检查公钥）
  + cli-linux ×2（命令行静态版 x86_64 / aarch64：确认是静态程序，六个发行版冒烟测试，打包）
  → manifest（生成 SHA256SUMS + latest.json，不需要任何密钥）
  → sign（environment: release；**无人工审批**；用 MINISIGN_SECRET_KEY 签名，再用仓库公钥验一遍）
  → release（一次性发布全部文件，含 latest.json 和签名）
  → mirror（rsync 仍未实现；`MIST_MIRROR_ENABLED != 'true'` 时跳过。设为 `true` 也只打印 notice 并成功结束，不把 tag 发版标红）
```

- 触发条件没变：只有推 `v*` 标签或手动触发才运行；合并 PR **不会**发版。手动触发会跑 manifest 当作检查
  （版本号取 Cargo.toml，产物只留在这次运行里），**不会**进入 sign/release。
- 推 `v*` 标签后 sign → release **自动跑完**，无需在 Actions 里点批准。
- manifest job **始终**写入镜像优先的安装包地址，不看 `MIST_MIRROR_ENABLED` / `MIST_MIRROR_BASE_URL`。
- 签名 job 只签两个小文件，私钥写到临时文件、用完即删，从不打印。
- `Update E2E` 工作流（`.github/workflows/update-e2e.yml`）：PR 修改更新相关代码时在 Linux 上跑离线端到端测试；
  Windows 版只能手动触发（勾选 `windows`）。两者都只有只读权限，不发布任何东西。

## 5. 一次性准备（已完成项可跳过）

**请在自己的电脑上做，不要在共享机器上生成私钥。**

1. ~~安装 minisign，生成两对密钥~~ ✅ 已完成（2026-10-07）。
2. 离线备份两把私钥和密码（例如加密 U 盘 + 密码管理器），备用钥平时**不要**放进 CI。
3. ~~把两个 `.pub` 文件提交进仓库~~ ✅ 已完成。
4. GitHub 仓库 → Settings → Environments → `release`：存放签名 secrets；**不要**再开 Required reviewers
   （推 `v*` 即自动签名发布）。Deployment branches/tags 可限制为 `v*`。
5. 在 `release` 环境里添加 secrets：
   - `MINISIGN_SECRET_KEY`：`mistterm-primary.key` 的完整内容
   - `MINISIGN_PASSWORD`：主钥密码
6. 镜像下载目录已在用（`/var/www/mistlab/downloads/mistterm/`）。清单里的安装包地址已经是镜像在前、GitHub 在后。
   `mirror` job 的 rsync 还没写：把 `MIST_MIRROR_ENABLED` 设为 `true` 不会让发版失败，也不会自动把文件传到服务器。
   发版后仍需把安装包放到该目录，再放 `latest.json` / `.minisig`（先文件、后清单）。
   nginx 不要让 SPA 回退把缺失文件变成首页 HTML；不存在的路径应返回 404。

## 6. 发版步骤

1. 改 `Cargo.toml` 和 仓库根目录的 `Info.plist` 的版本号（`python3 scripts/check-version-consistency.py` 检查）。
2. 可选：写 `docs/release/notes/v<版本>.zh.md` / `.en.md`。
3. 合并到 main，打标签 `v<版本>` 推送。
4. 等待 Actions 自动完成 sign + GitHub Release（无需人工批准）。
5. 发布后核对：`curl -sL https://github.com/mistlab-dev/MistTerm/releases/latest/download/latest.json`，
   各平台 `urls` 第一项是 `https://mistlab.dev/downloads/mistterm/v<版本>/…`，第二项是 GitHub。
   同时确认镜像上的 `stable/latest.json` 已换成这份清单（客户端先读镜像）。
   旧版本运行 `mist update --check` 应提示新版本。

### 给已发布版本补写镜像地址

不改应用版本号。在 Actions 里运行 **Patch update manifest mirror**
（`.github/workflows/patch-update-manifest-mirror.yml`），输入不带 `v` 的版本号（默认 `1.2.38`）：

```sh
gh workflow run patch-update-manifest-mirror.yml --ref main -f version=1.2.38
```

workflow 使用 `release` 环境的 `MINISIGN_SECRET_KEY` / `MINISIGN_PASSWORD`，下载该 tag 上现有的 `latest.json`，
只把每个平台的 `urls` 改成 `[镜像, GitHub]`，用原来的 `pub_date` 重新签名（可信注释仍是
`mistterm stable <版本> <pub_date>`），再以 `gh release upload --clobber` 覆盖上传 `latest.json` 和 `latest.json.minisig`。

若 `release` 环境开了人工审批，这次运行会停在等待批准。若 Deployment branches 只允许 `v*` 标签，
需要把 `main`（或你选择的 ref）加进允许列表，否则读不到签名密钥。

这个 workflow **只更新 GitHub Release**。客户端先请求
`https://mistlab.dev/downloads/mistterm/stable/latest.json`。镜像上的 `stable/latest.json`、
`v<版本>/latest.json` 以及对应的 `.minisig` 要另行换成刚签过的文件，国内客户端才会改从镜像下载安装包。

## 7. 本地测试

```sh
cargo test --release --lib core::updater          # 单元测试
python3 scripts/update-e2e/run_e2e.py             # 离线端到端（构建 1.90.0/1.91.0 两个测试版，起假服务器）
python3 scripts/update-e2e/run_e2e.py --no-gui    # 跳过 Xvfb 下的 GUI 检查
python3 scripts/update-e2e/run_e2e.py --static-cli  # 命令行静态版（需要 cargo-zigbuild + musl target）
python3 scripts/gen-update-manifest.py --self-test
```

测试专用环境变量（只在 `--features update-test` 构建里生效）：`MIST_UPDATE_MANIFEST_URL`（逗号分隔，可用
`http://127.0.0.1`）、`MIST_UPDATE_HOME`（状态/缓存目录）、`MIST_BUILD_VERSION`、`MIST_UPDATE_TEST_PUBKEY`。

## 8. 已知限制 / 后续

- 改 Bundle ID（`com.mist.term` → 正式 ID）仍未做。macOS 一键更新按单架构更新包进行；universal 安装需要先手动换一次对应架构的 dmg。
- Windows 安装包没有代码签名证书（未购买），SmartScreen 可能提示；静默安装路径未在真实 Windows 上跑过 CI 之外的测试。
- 镜像 rsync 仍未接入 CI（见第 5 节第 6 条）。新发版的清单已经是镜像优先；已发布的版本用上面的 patch workflow 重签 GitHub Release 上的 `latest.json`，再把同一对文件同步到 mistlab。
