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
tag-release（push main；满足条件才创建 vX.Y.Z，并在该标签上启动下面这条流程）
  → preflight（版本号一致性 + 清单 / 镜像 / 打标签计划自测）
  → platform ×4（Linux、Windows、macOS arm64、macOS x86_64；正式构建会检查公钥）
  + cli-linux ×2（命令行静态版 x86_64 / aarch64：确认是静态程序，六个发行版冒烟测试，打包）
  → manifest（生成 SHA256SUMS + latest.json，不需要任何密钥）
  → sign（environment: release；**无人工审批**；用 MINISIGN_SECRET_KEY 签名，再用仓库公钥验一遍）
  → release（一次性发布全部文件，含 latest.json 和签名）
  → mirror（environment: release；先同步 v<版本>/ 安装包并核对公网大小，再放该目录的 latest.json，
     最后才更新 stable/latest.json 和官网按钮用的 latest/ 别名）
```

- 发布触发有两条，走的是同一个 Build & Test：
  1. **发版 PR 合进 main**（`.github/workflows/tag-release.yml`）。HEAD 的 `Cargo.toml` / `Info.plist` 是同一个正式 `X.Y.Z`（不带预发布后缀），比已有的正式标签都新，并且 `docs/release/notes/vX.Y.Z.zh.md` 和 `.en.md` 都有内容时，workflow 在这个 main 提交上创建附注标签 `vX.Y.Z`，然后启动构建。
  2. **人工推送 `v*` 标签**。标签指向的提交必须已经在 main 上，构建前的检查会拒绝不在 main 上的标签。
- 用 workflow 里的 `GITHUB_TOKEN` 推上去的标签**不会**再触发 `on.push.tags`（GitHub 用这个限制避免 workflow 自己把自己叫醒）。Tag release 打完标签后执行 `gh workflow run build.yml --ref vX.Y.Z`。`workflow_dispatch` 是这个 token 仍然能启动的事件，而且 ref 是标签，所以签名、Release 和 mirror 都会跑。不需要另外的 PAT。
- 已有标签不会被移动或删除。同一个版本之后的 main 提交不会再次打标签。标签已经指在这个提交上时，重跑 Tag release（或在 Actions 里手动跑它）会再启动一次构建，用来补上「标签打上了但 dispatch 失败」的情况。构建本身失败则重跑 Build & Test。
- 在 main 上手动 Run workflow 跑 Build & Test 仍然只生成清单做检查，不签名、不发布、不同步。
- 标签上的 sign → release → mirror **自动跑完**，无需在 Actions 里点批准。`release` 环境没有 Required reviewers。
- mirror 默认会跑。只有仓库变量 `MIST_MIRROR_ENABLED` **精确等于** `false` 时才跳过。不要再把它设成必须等于 `true`：v1.3.14 和 v1.3.15 的 mirror job 就是因为不等于 `true` 被整段跳过。
- 缺 `MIST_DEPLOY_KEY`、安装包不齐、公网大小对不上、或 stable 在同步途中被更新的版本抢走时，mirror job **失败**。GitHub Release 已经发出，但这次 Actions 运行是红的，方便补跑。旧实现在密钥为空时只打 notice 并以成功结束。
- 重跑同一个标签是幂等的：公网文件大小已经一致的安装包不会重传。已经公开、且大小和这次构建不一致的 `v<版本>/` 文件**不会被覆盖**，job 会失败并停下，避免把已发布的版本目录改掉。公网 `stable/latest.json` 比这个标签新时，只补这个标签自己目录里缺的文件，**不会**把 stable 或官网 `latest/` 退回旧版。官网 `latest/` 上的 `SHA256SUMS` 和 `latest.json` **每次都会重写**：校验和文件在版本号位数相同时常常一样大（例如 1.3.15 和 1.3.16 都是 1036 字节），只比大小会误判为已是当前版本并跳过上传。
- manifest job **始终**写入镜像优先的安装包地址，不看 `MIST_MIRROR_ENABLED`。
- 签名和镜像用的私钥都只写到临时文件、用完即删，从不打印。镜像 job 通过环境变量读密钥，不把密钥写进 `if` 脚本。
- `Release mirror plan`（`.github/workflows/release-mirror-plan.yml`）在改到同步脚本或 `build.yml` 的 PR 上跑计划自测，不连生产机。
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
   在 `release` 环境（不是只放在仓库级、也不要只放在没指定 environment 的 workflow 里）添加 secret：
   - `MIST_DEPLOY_KEY`：生产机 `mist-deploy` 的现有专用私钥。公钥仍由 `command=` 限制成只能 `rsync` 到 `mistterm*` 目录。不要换钥匙，不要把私钥写进仓库或日志。
   可选仓库变量 `MIST_MIRROR_HOST`：不设时用 workflow 里现有的镜像主机。`MIST_MIRROR_ENABLED=false` 是紧急开关。
   nginx 不要让 SPA 回退把缺失文件变成首页 HTML；不存在的路径应返回 404。

## 6. 发版步骤

1. 改 `Cargo.toml` 和 仓库根目录的 `Info.plist` 的版本号（`python3 scripts/check-version-consistency.py` 检查）。
2. **写** `docs/release/notes/v<版本>.zh.md` / `.en.md`（人话短句；规则见 `docs/release/notes/README.md`）。
   CI 会同时写入 `latest.json` notes，并嵌进 GitHub Release body 开头（`## 更新内容` / `## What's New`），
   供 mistlab.dev 下载页「更新日志」读取；不要只靠自动 `## What's Changed`。
3. 合并到 main。Tag release 发现版本比现有正式标签新、版本号一致、中英 notes 都在，就在这个提交上创建 `v<版本>` 并启动 Build & Test。不要再手工打标签，也不要移动已经存在的标签。
4. 若 Tag release 在创建标签之前失败（例如忘了 notes），补一次提交再合进 main。标签会打在第一次满足条件的 main 提交上。若标签已经打上但构建没启动，重跑 Tag release。若构建失败，重跑该标签上的 Build & Test。人工 `git push origin v<版本>` 仍然可以直接启动同一次构建。
5. 等待 Actions 自动完成 sign、GitHub Release 和 mirror（无需人工批准）。mirror 成功后才算镜像同步完成；失败就重跑该 workflow，不要手工先改 `stable/latest.json`。
6. 发布后核对：
   - `curl -sL https://github.com/mistlab-dev/MistTerm/releases/latest/download/latest.json`，
     各平台 `urls` 第一项是 `https://mistlab.dev/downloads/mistterm/v<版本>/…`，第二项是 GitHub。
   - Release body 含 `## 更新内容` 与 `## What's New`（再是 Downloads，再是自动 What’s Changed）。
   - 镜像上的 `stable/latest.json` 已换成这份清单（客户端先读镜像），并且 `v<版本>/` 里的安装包大小和清单一致。
   - 官网下载按钮用的 `https://mistlab.dev/downloads/mistterm/latest/<文件名>` 已是这一版（mirror 在更新 stable 之后才会写这个别名）。
   - 旧版本运行 `mist update --check` 应提示新版本。

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
`https://mistlab.dev/downloads/mistterm/stable/latest.json`。要让镜像跟上刚签过的文件，用 **main 上**
的 `scripts/sync-release-mirror.py` 再同步一次（它会先核对安装包，再换 `v<版本>/` 和 `stable/` 的清单）。
不要重跑旧标签上的 Build & Test：那个 workflow 文件停在打标签时的提交，不会带上后来的镜像脚本。

## 7. 本地测试

```sh
cargo test --release --lib core::updater          # 单元测试
python3 scripts/update-e2e/run_e2e.py             # 离线端到端（构建 1.90.0/1.91.0 两个测试版，起假服务器）
python3 scripts/update-e2e/run_e2e.py --no-gui    # 跳过 Xvfb 下的 GUI 检查
python3 scripts/update-e2e/run_e2e.py --static-cli  # 命令行静态版（需要 cargo-zigbuild + musl target）
python3 scripts/gen-update-manifest.py --self-test
python3 scripts/sync-release-mirror.py --self-test   # 缺密钥 / 半包 / 重试 / stable 顺序，不上传
```

测试专用环境变量（只在 `--features update-test` 构建里生效）：`MIST_UPDATE_MANIFEST_URL`（逗号分隔，可用
`http://127.0.0.1`）、`MIST_UPDATE_HOME`（状态/缓存目录）、`MIST_BUILD_VERSION`、`MIST_UPDATE_TEST_PUBKEY`。

## 8. 已知限制 / 后续

- 改 Bundle ID（`com.mist.term` → 正式 ID）仍未做。macOS 一键更新按单架构更新包进行；universal 安装需要先手动换一次对应架构的 dmg。
- Windows 安装包没有代码签名证书（未购买），SmartScreen 可能提示；静默安装路径未在真实 Windows 上跑过 CI 之外的测试。
- 官网在 `mistlab-dev/mist-website`。下载卡片的版本号和安装包链接以镜像上的 `downloads/mistterm/latest/latest.json` 为准（mirror job 在更新 stable 之后会写这个别名和清单）。GitHub Releases API 只用来补下载次数和更新说明。因此发版后一般不必再给官网开「对齐版本号」的 PR；页面里的静态文案只是无 JS 时的最后回退。macOS 清单项是自动更新用的 `*-update.tar.gz`，页面卡片仍应指向 `.dmg`。
- 给已发布版本重签 `latest.json` 的 patch workflow 仍然只改 GitHub Release。镜像要跟着变时，在能读到 `release` 环境 `MIST_DEPLOY_KEY` 的地方跑 main 上的 `scripts/sync-release-mirror.py`。重跑旧标签的 Actions 不会用到后来合进 main 的脚本。
