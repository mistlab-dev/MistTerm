# MistTerm 自动更新：设计与发版手册

> 适用版本：1.2.0 起。方案原文见内部文档《mistterm-autoupdate-plan》，本文记录**实际实现**。

## 1. 用户看到什么

| 平台 / 安装方式 | 行为 |
| --- | --- |
| Linux 压缩包（`Mist` + `mist` 在同一个可写目录） | 提醒 + 一键更新（下载 → 校验 → 替换 → 提示重启） |
| Linux 命令行静态版（`mist-cli-linux-*.tar.gz` / 官网 `/install`） | `mist update` 一键更新，只换 `mist`（见 [CLI_STATIC.md](CLI_STATIC.md)） |
| Windows 安装版（Inno Setup，目录里有 `unins000.exe`） | 提醒 + "安装并重启"：静默运行新安装程序，装完自动重新打开 |
| Windows 便携版（zip） | 提醒 + 一键更新（同 Linux） |
| macOS | **只提醒**，给出手动更新步骤和下载页（签名/公证/改 Bundle ID 是 P5） |
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

客户端按顺序取清单（`src/core/updater/mod.rs` 的 `STABLE_MANIFEST_URLS`）：

1. `https://mistlab.dev/downloads/mistterm/stable/latest.json`（主；**目前还没部署**，会拿到网站首页 HTML，客户端识别后直接跳过）
2. `https://github.com/mistlab-dev/MistTerm/releases/latest/download/latest.json`（备用）

安装包下载地址顺序写在清单里（`urls` 数组），目前只有 GitHub；镜像开通后 CI 会在 GitHub 后面追加镜像地址
（`--url-order github,mirror`，以后想镜像优先只改这一个参数）。

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
  → platform ×3（构建、打包；正式构建会检查公钥）
  + cli-linux ×2（命令行静态版 x86_64 / aarch64：确认是静态程序，六个发行版冒烟测试，打包）
  → manifest（生成 SHA256SUMS + latest.json，不需要任何密钥）
  → sign（environment: release，需 Tian 批准；用 MINISIGN_SECRET_KEY 签名，再用仓库里的公钥验一遍）
  → release（一次性发布全部文件，含 latest.json 和签名）
  → mirror（镜像上传；仓库变量 MIST_MIRROR_ENABLED != 'true' 时跳过，目前未实现）
```

- 触发条件没变：只有推 `v*` 标签或手动触发才运行；合并 PR **不会**发版。手动触发会跑 manifest 当作检查
  （版本号取 Cargo.toml，产物只留在这次运行里），**不会**进入 sign/release。
- 签名 job 只签两个小文件，私钥写到临时文件、用完即删，从不打印。
- `Update E2E` 工作流（`.github/workflows/update-e2e.yml`）：PR 修改更新相关代码时在 Linux 上跑离线端到端测试；
  Windows 版只能手动触发（勾选 `windows`）。两者都只有只读权限，不发布任何东西。

## 5. Tian 需要做的一次性准备（1.2.0 之前）

**请在自己的电脑上做，不要在共享机器上生成私钥。**

1. ~~安装 minisign，生成两对密钥~~ ✅ 已完成（2026-10-07）。
2. 离线备份两把私钥和密码（例如加密 U 盘 + 密码管理器），备用钥平时**不要**放进 CI。
3. ~~把两个 `.pub` 文件提交进仓库~~ ✅ 已完成。
4. GitHub 仓库 → Settings → Environments → 新建 `release`：Required reviewers 设为自己；
   Deployment branches and tags 只允许 `v*` 标签。
5. 在 `release` 环境里添加 secrets：
   - `MINISIGN_SECRET_KEY`：`mistterm-primary.key` 的完整内容
   - `MINISIGN_PASSWORD`：主钥密码
6. （以后）镜像：在服务器上给 nginx 配好 `/downloads/mistterm/` 目录（**注意别被网站的 SPA 回退规则吃掉**，
   不存在的文件要返回 404 而不是首页），准备只能写该目录的部署密钥，再实现并打开 `mirror` job。

## 6. 发版步骤（准备好之后）

1. 改 `Cargo.toml` 和 仓库根目录的 `Info.plist` 的版本号（`python3 scripts/check-version-consistency.py` 检查）。
2. 可选：写 `docs/release/notes/v<版本>.zh.md` / `.en.md`。
3. 合并到 main，打标签 `v<版本>` 推送。
4. 在 Actions 里批准 `release` 环境的 sign job。
5. 发布后核对：`curl -sL https://github.com/mistlab-dev/MistTerm/releases/latest/download/latest.json`，
   旧版本运行 `mist update --check` 应提示新版本。

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

- macOS 一键更新、改 Bundle ID（`com.mist.term` → 正式 ID）属于 P5，现在只提醒。
- Windows 安装包没有代码签名证书（未购买），SmartScreen 可能提示；静默安装路径未在真实 Windows 上跑过 CI 之外的测试。
- 镜像上传未开通（见第 5 节第 7 条）。
