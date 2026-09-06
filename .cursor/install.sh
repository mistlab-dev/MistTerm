#!/usr/bin/env bash
# Cloud Agent install: idempotent bootstrap for MistTerm (Rust GUI SSH terminal).
# Runs once to create the environment baseline; safe to re-run.
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive
export RUSTUP_HOME="${RUSTUP_HOME:-/usr/local/rustup}"
export CARGO_HOME="${CARGO_HOME:-/usr/local/cargo}"

# 1) System dependencies.
#    - build toolchain + linkers: build-essential, pkg-config, mold
#    - SSH / crypto: libssl-dev, libssh2-1-dev
#    - egui/wgpu GUI stack: libx11, libxkbcommon, libxcb-*, libfontconfig1, libgtk-3, libdbus-1
#    - headless rendering: xvfb + software Vulkan (mesa-vulkan-drivers / lavapipe) + GL DRI
#    - integration test helpers: openssh-server, lrzsz (rz/sz for ZMODEM tests)
sudo apt-get update -qq
sudo apt-get install -y --no-install-recommends \
  build-essential pkg-config mold \
  libssl-dev libssh2-1-dev \
  libx11-dev libxkbcommon-dev \
  libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
  libfontconfig1-dev libgtk-3-dev libdbus-1-dev \
  openssh-server lrzsz \
  xvfb mesa-vulkan-drivers libgl1-mesa-dri

# 2) Rust toolchain. The dependency tree pulls crates requiring edition2024
#    (Rust >= 1.85), newer than some base images ship, so pin the current stable.
rustup toolchain install stable --profile minimal -c clippy -c rustfmt
rustup default stable

# 3) Optional CJK font for the Simplified-Chinese UI. It is vendored in-repo, so
#    only fetch when missing (keeps install offline-friendly).
if [ ! -f assets/fonts/NotoSansSC-Regular.otf ]; then
  bash scripts/fetch-cjk-font.sh || true
fi

# 4) Build the app: warms the dependency cache and produces a runnable release
#    binary (target/release/Mist). mold keeps link time low.
export RUSTFLAGS="-C link-arg=-fuse-ld=mold"
export CARGO_INCREMENTAL=0
cargo build --release --bin Mist
