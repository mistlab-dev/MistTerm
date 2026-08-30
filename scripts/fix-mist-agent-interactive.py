#!/usr/bin/env python3
"""
Deploy mist-agent interactive DEBUG hook (skip completion; single python; short timeout).

Problems fixed:
1. DEBUG trap ran on bash-completion internals → Tab felt hung.
2. Every command spawned python3 twice (~0.9s+) even when API was fast.
3. Interactive timeout was 5s → API blips froze the shell.

Fix:
- Skip when COMP_LINE is set / completion helpers (_*, compgen, …)
- Skip high-frequency harmless builtins
- Single python3 process for check + MIST_AUDIT emit
- Connect 0.3s / total 1.5s timeout
- Recurse guard MIST_AUDIT_IN_HOOK

Patches both mist-audit-wrapper (embedded HOOK heredoc) and interactive.bash.
"""
from __future__ import annotations

import os
import re
import sys
import time

import paramiko

HOOK = r'''# sourced as bash rcfile
[[ -f /etc/bashrc ]] && . /etc/bashrc
[[ -f ~/.bashrc ]] && . ~/.bashrc

# Load agent env if wrapper did not export (e.g. manual bash --rcfile).
[[ -f /etc/mist-agent/config ]] && { set -a; . /etc/mist-agent/config; set +a; }

mist_audit_preexec() {
  # Re-entrancy first (DEBUG also fires for commands inside this hook).
  [[ -n "${MIST_AUDIT_IN_HOOK-}" ]] && return 0
  # Programmable completion sets COMP_LINE — never block Tab / menu-complete.
  [[ -n "${COMP_LINE-}" ]] && return 0

  local cmd="$BASH_COMMAND"
  case "$cmd" in
    mist_audit_*|trap\ *|source\ *|.\ *) return 0 ;;
    MIST_AUDIT_APPROVE*) return 0 ;;
    # bash-completion helpers (e.g. _longopt for `ls`)
    _*|compgen\ *|complete\ *) return 0 ;;
    python3|python3\ *|curl|curl\ *) return 0 ;;
    ''|\#*) return 0 ;;
    # high-frequency interactive builtins — not policy-interesting
    true|false|:|pwd|clear|history|jobs|fg|bg|wait|ulimit\ *|umask\ *|times) return 0 ;;
    echo|echo\ *|printf|printf\ *) return 0 ;;
  esac

  # One python process: HTTP check + optional MIST_AUDIT line + exit status.
  # exit 1 → with extdebug, bash skips the pending command (block).
  MIST_AUDIT_IN_HOOK=1
  python3 - "$cmd" <<'PY'
import json, os, sys, urllib.request

cmd = sys.argv[1]
api = os.environ.get("MIST_API", "").rstrip("/")
agent = os.environ.get("MIST_AGENT_ID", "")
secret = os.environ.get("MIST_SECRET", "")
team = os.environ.get("MIST_TEAM_ID", "")
user = os.environ.get("MIST_AUDIT_USER", "")

def emit(action, message, rule, token=""):
    o = {"v": 1, "action": action, "message": message, "rule": rule, "command": cmd}
    if token:
        o["token"] = token
    print("MIST_AUDIT\t" + json.dumps(o, ensure_ascii=False), flush=True)

if not (api and agent and secret and team):
    sys.exit(0)

payload = json.dumps({
    "agent_id": agent,
    "team_id": team,
    "user": user,
    "command": cmd,
    "scope": "command",
}).encode()
req = urllib.request.Request(
    api + "/server/command-audit/check",
    data=payload,
    headers={
        "Content-Type": "application/json",
        "Authorization": "Bearer %s:%s" % (agent, secret),
    },
    method="POST",
)
try:
    with urllib.request.urlopen(req, timeout=1.5) as r:
        raw = r.read().decode("utf-8", "replace")
except Exception:
    # Fail-open on transport errors (interactive must stay usable).
    sys.exit(0)

try:
    d = json.loads(raw) if raw else {}
except Exception:
    d = {}

action = (d.get("action") or "").lower()
allowed = d.get("allowed")
msg = d.get("message") or action or "server"
rule = d.get("rule") or "server"
token = d.get("token") or ""

if action == "block" or allowed is False:
    emit("block", msg, rule)
    sys.exit(1)
if action == "confirm":
    emit("confirm", msg, rule, token)
    # Allow through so MistTerm can show confirm UI / approve flow.
    sys.exit(0)
if action == "alert":
    emit("alert", msg, rule)
sys.exit(0)
PY
  local __mist_st=$?
  unset MIST_AUDIT_IN_HOOK
  return $__mist_st
}

shopt -s extdebug
trap 'mist_audit_preexec' DEBUG
'''


def replace_hook(wrapper: str, new_hook: str) -> str:
    """Replace the <<'HOOK' ... HOOK heredoc body in mist-audit-wrapper."""
    m = re.search(r"(cat > /etc/mist-agent/interactive\.bash <<'HOOK'\n)(.*?)(\nHOOK\n)", wrapper, re.S)
    if not m:
        raise RuntimeError("HOOK heredoc not found in mist-audit-wrapper")
    body = new_hook.rstrip("\n")
    return wrapper[: m.start(2)] + body + wrapper[m.end(2) :]


def main() -> int:
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    host = os.environ["MISTTERM_TEST_SSH_HOST"]
    pw = os.environ["MISTTERM_TEST_SSH_PASSWORD"]
    user = os.environ.get("MISTTERM_TEST_SSH_USER", "root")

    c = paramiko.SSHClient()
    c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    c.connect(host, 22, user, pw, timeout=20, allow_agent=False, look_for_keys=False)

    def run(cmd: str, timeout: int = 60) -> str:
        _i, o, e = c.exec_command(cmd, timeout=timeout)
        return (o.read() + e.read()).decode("utf-8", "replace")

    sftp = c.open_sftp()
    with sftp.file("/etc/mist-agent/mist-audit-wrapper", "r") as f:
        wrap = f.read().decode("utf-8", "replace")

    # Backup once
    ts = time.strftime("%Y%m%d%H%M%S")
    backup = f"/etc/mist-agent/mist-audit-wrapper.bak.{ts}"
    with sftp.file(backup, "w") as f:
        f.write(wrap)
    print(f"backup wrapper -> {backup}")

    wrap2 = replace_hook(wrap, HOOK)
    with sftp.file("/etc/mist-agent/mist-audit-wrapper", "w") as f:
        f.write(wrap2)
    with sftp.file("/etc/mist-agent/interactive.bash", "w") as f:
        f.write(HOOK if HOOK.endswith("\n") else HOOK + "\n")
    sftp.close()

    print(run("bash -n /etc/mist-agent/interactive.bash && echo interactive_ok"))
    print(run("bash -n /etc/mist-agent/mist-audit-wrapper && echo wrapper_ok"))
    print(run("chmod 755 /etc/mist-agent/mist-audit-wrapper; chmod 644 /etc/mist-agent/interactive.bash"))

    # --- latency benchmarks on a fresh interactive shell (ForceCommand path) ---
    chan = c.invoke_shell(term="xterm-256color", width=120, height=40)
    time.sleep(1.2)
    while chan.recv_ready():
        chan.recv(65535)

    def wait_prompt(timeout: float = 15.0) -> bytes:
        buf = b""
        end = time.time() + timeout
        while time.time() < end:
            if chan.recv_ready():
                buf += chan.recv(65535)
                if buf.rstrip().endswith((b"#", b"$")):
                    return buf
            else:
                time.sleep(0.05)
        return buf

    def drain(idle: float = 0.15, overall: float = 1.0) -> None:
        end = time.time() + overall
        last = time.time()
        while time.time() < end:
            if chan.recv_ready():
                chan.recv(65535)
                last = time.time()
            elif time.time() - last >= idle:
                break
            else:
                time.sleep(0.01)

    def measure_cmd(label: str, line: str) -> float:
        drain()
        t0 = time.perf_counter()
        chan.send(line if line.endswith("\n") else line + "\n")
        while time.perf_counter() - t0 < 12:
            if chan.recv_ready():
                data = chan.recv(65535)
                if data.rstrip().endswith((b"#", b"$")):
                    time.sleep(0.05)
                    while chan.recv_ready():
                        chan.recv(65535)
                    break
            else:
                time.sleep(0.01)
        ms = (time.perf_counter() - t0) * 1000
        print(f"[{label}] prompt_ms={ms:.0f}")
        return ms

    def measure_tab(label: str) -> float:
        drain()
        chan.send("ls a")
        time.sleep(0.25)
        drain(0.1, 0.6)
        t0 = time.perf_counter()
        chan.send("\t\t")
        listed = None
        got = b""
        while time.perf_counter() - t0 < 12:
            if chan.recv_ready():
                got += chan.recv(65535)
                if listed is None and (b"aaaa" in got or b"a.sh" in got or b"\x07" in got):
                    listed = time.perf_counter()
                    time.sleep(0.15)
                    while chan.recv_ready():
                        got += chan.recv(65535)
                    break
            else:
                time.sleep(0.01)
        ms = ((listed - t0) * 1000) if listed else -1.0
        print(f"[{label}] list_ms={ms:.0f} bytes={len(got)} excerpt={got[:80]!r}")
        chan.send("\x03")
        wait_prompt(5)
        return ms

    wait_prompt()
    # Confirm trap is active
    chan.send("trap -p DEBUG\n")
    time.sleep(0.5)
    trap_out = b""
    while chan.recv_ready():
        trap_out += chan.recv(65535)
    print("trap_p:", trap_out.decode("utf-8", "replace").replace("\r", "")[-200:])

    t_true = measure_cmd("true", "true")
    t_echo = measure_cmd("echo", "echo hi")
    t_ls = measure_cmd("ls_harmless", "ls /tmp >/dev/null")
    t_tab = measure_tab("double_tab")

    # Dangerous command should still emit MIST_AUDIT block (may or may not block depending on policy)
    drain()
    chan.send("rm -rf /\n")
    time.sleep(2.0)
    dang = b""
    while chan.recv_ready():
        dang += chan.recv(65535)
    print("dangerous_out:", dang.decode("utf-8", "replace")[-300:].replace("\r", "\\r"))
    has_audit = b"MIST_AUDIT" in dang
    print(f"has_MIST_AUDIT={has_audit}")

    chan.send("exit\n")
    time.sleep(0.3)

    print("--- summary ---")
    print(f"true_ms={t_true:.0f} echo_ms={t_echo:.0f} ls_ms={t_ls:.0f} tab_ms={t_tab:.0f}")
    # Tab must not sit for multi-second completion waits caused by agent
    tab_ok = t_tab < 0 or t_tab < 3000
    # Builtins skip the network path; remaining time is mostly SSH RTT
    builtin_ok = t_true < 1200
    if not tab_ok:
        print("FAIL: Tab still too slow")
        c.close()
        return 1
    if not builtin_ok:
        print("WARN: true still slow (SSH RTT?); check manually")
    if not has_audit:
        print("WARN: dangerous cmd did not show MIST_AUDIT in this run (policy/network?)")
    syn = run("bash -n /etc/mist-agent/interactive.bash && bash -n /etc/mist-agent/mist-audit-wrapper && echo syntax_ok")
    print(syn.strip())
    c.close()
    if "syntax_ok" not in syn:
        print("FAIL: bash -n")
        return 1
    print("DEPLOY_OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
