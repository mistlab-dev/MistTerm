#!/usr/bin/env bash
# Cloud Agent start: per-boot runtime services for MistTerm. Idempotent; tolerates
# restarts and avoids duplicate processes.
#   - Xvfb :99 : virtual display so the egui/wgpu GUI runs headless
#                (software Vulkan / lavapipe). Run the app with DISPLAY=:99.
#   - sshd     : a local OpenSSH server so the SSH / SFTP / ZMODEM integration
#                tests (and a real GUI SSH session) have something to connect to.
#                Credentials match the test defaults probed by the code.
set -euo pipefail

# --- Virtual display for the GUI ---
if ! pgrep -f "Xvfb :99" >/dev/null 2>&1; then
  Xvfb :99 -screen 0 1600x1000x24 -ac +extension GLX +render -noreset \
    >/tmp/xvfb.log 2>&1 &
fi

# --- Local SSH test server ---
# Integration tests probe 127.0.0.1:22 as root/mistterm123 by default (override
# with MISTTERM_TEST_SSH_*); a mistterm_test/test123456 user matching CI is also
# provided. Tests auto-skip when no sshd is reachable, so this is best-effort.
sudo mkdir -p /run/sshd
sudo ssh-keygen -A >/dev/null 2>&1 || true

echo "root:mistterm123" | sudo chpasswd
if ! id mistterm_test >/dev/null 2>&1; then
  sudo useradd -m -s /bin/bash mistterm_test
fi
echo "mistterm_test:test123456" | sudo chpasswd

sudo sed -i 's/^#\?PasswordAuthentication.*/PasswordAuthentication yes/' /etc/ssh/sshd_config
sudo sed -i 's/^#\?PermitRootLogin.*/PermitRootLogin yes/' /etc/ssh/sshd_config
grep -q '^PermitRootLogin yes' /etc/ssh/sshd_config \
  || echo 'PermitRootLogin yes' | sudo tee -a /etc/ssh/sshd_config >/dev/null

if ! pgrep -x sshd >/dev/null 2>&1; then
  sudo /usr/sbin/sshd
fi
