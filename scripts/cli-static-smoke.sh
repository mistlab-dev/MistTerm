#!/usr/bin/env bash
# 在 Docker 里把静态版 `mist` 放到新老 Linux 发行版上跑一遍：
#   mist --version、mist ls（空 / 导入后）、mist exec（已保存会话 / 临时 user@host）。
# 连接目标是脚本自己起的一次性 sshd 容器，只执行只读命令，不连任何真实服务器。
#
# 用法：
#   scripts/cli-static-smoke.sh <mist 二进制> <amd64|arm64> [镜像 ...]
#   默认镜像：centos:7 rockylinux:8 rockylinux:9 ubuntu:20.04 ubuntu:22.04 debian:12
#   arm64 需要先注册 qemu binfmt（CI 用 docker/setup-qemu-action）。
# 环境变量：
#   DOCKER=docker        需要时可设为 "sudo docker"
#   EXPECT_VERSION=x.y.z 要求 `mist --version` 输出这个版本
set -euo pipefail

BIN="$(readlink -f "${1:?usage: $0 <mist-binary> <amd64|arm64> [images...]}")"
ARCH="${2:?usage: $0 <mist-binary> <amd64|arm64> [images...]}"
shift 2
IMAGES=("$@")
[ ${#IMAGES[@]} -gt 0 ] || IMAGES=(centos:7 rockylinux:8 rockylinux:9 ubuntu:20.04 ubuntu:22.04 debian:12)
DOCKER="${DOCKER:-docker}"
EXPECT_VERSION="${EXPECT_VERSION:-}"

[ -x "$BIN" ] || { echo "not executable: $BIN" >&2; exit 2; }
case "$ARCH" in amd64|arm64) ;; *) echo "arch must be amd64 or arm64" >&2; exit 2 ;; esac

WORK="$(mktemp -d)"
SSHD="mist-smoke-sshd-$$"
cleanup() {
  $DOCKER rm -f "$SSHD" >/dev/null 2>&1 || true
  rm -rf "$WORK"
}
trap cleanup EXIT

# ---- 一次性 sshd（本机架构），只认这次生成的测试密钥 ----
ssh-keygen -q -t ed25519 -N '' -C mist-smoke -f "$WORK/id_ed25519"
chmod 644 "$WORK/id_ed25519"   # 容器里以 root 读取后再 chmod 600
cat > "$WORK/Dockerfile" <<'DOCKERFILE'
FROM debian:12
RUN apt-get update && apt-get install -y --no-install-recommends openssh-server \
 && rm -rf /var/lib/apt/lists/* && mkdir -p /run/sshd \
 && sed -i 's/^#\?PasswordAuthentication .*/PasswordAuthentication no/' /etc/ssh/sshd_config \
 && useradd -m -s /bin/bash mist_smoke && mkdir -p /home/mist_smoke/.ssh \
 && echo 'hello from mist-smoke-sshd' > /home/mist_smoke/hello.txt
COPY id_ed25519.pub /home/mist_smoke/.ssh/authorized_keys
RUN chown -R mist_smoke:mist_smoke /home/mist_smoke && chmod 700 /home/mist_smoke/.ssh \
 && chmod 600 /home/mist_smoke/.ssh/authorized_keys
CMD ["/usr/sbin/sshd", "-D", "-e"]
DOCKERFILE
$DOCKER build -q --pull -t mist-smoke-sshd "$WORK" >/dev/null
$DOCKER run -d --name "$SSHD" --hostname mist-smoke-sshd mist-smoke-sshd >/dev/null

# ---- 在每个发行版容器里执行的检查（共享 sshd 的网络，连 mist-smoke-sshd:22） ----
cat > "$WORK/t.sh" <<'INNER'
#!/bin/sh
M=/opt/mist/mist
export HOME=/root USER=root
step() { name=$1; shift; echo "\$ $*"; out=$("$@" 2>&1); rc=$?; echo "$out"; echo "[rc=$rc]"; eval "R_$name=$rc"; eval "O_$name=\$out"; }
echo "### $(. /etc/os-release; echo "$PRETTY_NAME") arch=$(uname -m)"
step version $M --version
step ls_empty $M ls
mkdir -p /root/.ssh && cp /keys/id_ed25519 /root/.ssh/id_ed25519 && chmod 700 /root/.ssh && chmod 600 /root/.ssh/id_ed25519
printf 'Host smoke\n  HostName mist-smoke-sshd\n  User mist_smoke\n  IdentityFile /root/.ssh/id_ed25519\n' > /root/.ssh/config
step import $M import-ssh-config
step ls $M ls
step exec $M exec smoke -- cat hello.txt
step exec_adhoc $M exec mist_smoke@mist-smoke-sshd -- id -un
ok() { [ "$1" = 0 ] && echo ok || echo FAIL; }
v=$(ok "$R_version"); case "$O_version" in "mist ${EXPECT_VERSION}"*) ;; *) v=FAIL ;; esac
l=$(ok "$R_ls"); echo "$O_ls" | grep -q 'smoke' || l=FAIL
[ "$R_ls_empty" = 0 ] || l=FAIL
[ "$R_import" = 0 ] || l=FAIL
e=$(ok "$R_exec"); [ "$O_exec" = "hello from mist-smoke-sshd" ] || e=FAIL
a=$(ok "$R_exec_adhoc"); [ "$O_exec_adhoc" = "mist_smoke" ] || a=FAIL
echo "RESULT version=$v ls=$l exec=$e exec_adhoc=$a"
INNER

fail=0
printf '\n| image | arch | --version | ls | exec (saved) | exec (user@host) |\n|---|---|---|---|---|---|\n' > "$WORK/summary.md"
for img in "${IMAGES[@]}"; do
  echo "::group::$img ($ARCH)"
  set +e
  out="$($DOCKER run --rm --pull always --platform "linux/$ARCH" --network "container:$SSHD" \
      -e EXPECT_VERSION="$EXPECT_VERSION" \
      -v "$BIN:/opt/mist/mist:ro" -v "$WORK:/keys:ro" "$img" /bin/sh /keys/t.sh 2>&1)"
  set -e
  echo "$out"
  echo "::endgroup::"
  res="$(printf '%s\n' "$out" | grep '^RESULT ' | tail -1)"
  if [ -z "$res" ]; then
    res="RESULT version=FAIL ls=FAIL exec=FAIL exec_adhoc=FAIL"
  fi
  get() { printf '%s\n' "$res" | sed -n "s/.* $1=\([A-Za-z]*\).*/\1/p"; }
  printf '| %s | %s | %s | %s | %s | %s |\n' "$img" "$ARCH" "$(get version)" "$(get ls)" "$(get exec)" "$(get exec_adhoc)" >> "$WORK/summary.md"
  case "$res" in *FAIL*) fail=1 ;; esac
done
cat "$WORK/summary.md"
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then cat "$WORK/summary.md" >> "$GITHUB_STEP_SUMMARY"; fi
exit "$fail"
