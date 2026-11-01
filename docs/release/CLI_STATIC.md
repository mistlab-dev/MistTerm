# 命令行 `mist` 的 Linux 静态版

> 1.2.1 起提供。桌面版 `Mist` 的系统要求不变（Linux 仍需 glibc 2.39+）。

## 为什么

以前桌面版压缩包 `Mist-linux-x86_64.tar.gz` 里还有一份和桌面版一起在 Ubuntu 24.04 上编译的 `mist`，
需要 glibc 2.39，还会连带要求 GTK3、OpenSSL 3 这些库。服务器上一般没有这些，CentOS 7、Rocky 8/9、
Ubuntu 20.04/22.04、Debian 12 上都跑不起来，ARM 服务器更是没有可用的包。
这份 glibc `mist` 和下面的静态包不是同一个文件，但用户下桌面包时会再下一整份命令行。
从打包减负（[#127](https://github.com/mistlab-dev/MistTerm/issues/127)）起，桌面包只留 GUI `Mist`，
命令行只走这里的静态包。体积数字见 [PACKAGE_SIZE.md](PACKAGE_SIZE.md)。

静态版用 musl 编译，OpenSSL、libssh2、zlib 都编进程序里，不依赖系统的 C 库和任何共享库，
x86_64 和 ARM64（aarch64）各出一个包。

## 发布物

| 文件 | 内容 | 适用 |
| --- | --- | --- |
| `mist-cli-linux-x86_64.tar.gz` | `mist-cli-linux-x86_64/{mist, LICENSE, THIRD_PARTY_LICENSES.txt, README.txt}` | 任意 x86_64 Linux（内核 3.2+） |
| `mist-cli-linux-aarch64.tar.gz` | `mist-cli-linux-aarch64/{…同上}` | 任意 ARM64 Linux |

两个包都会写进 `SHA256SUMS` 和 `latest.json`（键名 `linux-x86_64-cli`、`linux-aarch64-cli`，没有 `min_glibc`）。
官网 `curl -fsSL https://mistlab.dev/install | bash` 在 Linux 上按 `uname -m` 选这两个包。
只对**还带** `mist` 的老桌面包（1.2.34 及更早）才能退回 `Mist-linux-x86_64.tar.gz` 里的 `mist`；
新桌面包里没有这个文件。

## 自动更新

- 静态版 `mist update` 只认 `linux-<arch>-cli` 条目，只替换 `mist` 本身；同一个目录里即使放着桌面版 `Mist`，也不会碰它。
- 清单里没有静态包时（比如老版本的清单），静态版只提示有新版本、不会去装桌面版的包。
- 桌面版 `Mist` 只认 `linux-x86_64`，一键更新只替换 `Mist`。老客户端会忽略新增的两个静态包条目。
  目录里若还留着 1.2.34 及更早打进去的 glibc `mist`，那一版更新器会因为新包里没有 `mist` 而拒绝就地更新；删掉它或重新解压桌面包即可。见 [PACKAGE_SIZE.md](PACKAGE_SIZE.md)。

## 怎么构建

```sh
rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl
pip install ziglang cargo-zigbuild        # zig 负责交叉编译 C 代码和链接
cargo zigbuild --release --bin mist --no-default-features --features vendored-openssl \
  --target x86_64-unknown-linux-musl      # 或 aarch64-unknown-linux-musl
```

`--no-default-features` 关掉的是桌面版的系统文件选择框（`file-dialogs`，Linux 上依赖 GTK3）；
桌面版 `Mist` 声明了 `required-features = ["file-dialogs"]`，不会被误编成没有文件选择框的版本。

## 怎么验证

```sh
# 在 centos:7 / rockylinux:8 / rockylinux:9 / ubuntu:20.04 / ubuntu:22.04 / debian:12 里
# 跑 mist --version、mist ls、mist exec（连脚本自己起的一次性 sshd，只执行只读命令）
scripts/cli-static-smoke.sh target/x86_64-unknown-linux-musl/release/mist amd64
scripts/cli-static-smoke.sh target/aarch64-unknown-linux-musl/release/mist arm64   # 需要 qemu binfmt

# 自动更新离线端到端（只换 mist、不碰桌面版、不装桌面版的包）
python3 scripts/update-e2e/run_e2e.py --static-cli
```

CI：`build.yml` 的 `cli-linux` job 对两种架构都做“确认是静态程序 + 六个发行版冒烟测试”，再打包；
`manifest` job 检查两个包都在 `SHA256SUMS` 和 `latest.json` 里。`update-e2e.yml` 的 `linux-static-cli` job 跑上面的端到端测试。
