//! #413 后继方案：exec 通道**带外**查询远端交互 shell 的当前目录。
//!
//! 旧方案（#410 / #413）靠向交互 PTY 注入 `__mist_cwd_report` 钩子让 shell 主动发
//! OSC 7；真实 sshd + zsh 上 PTY 的 ECHO 会把整行钩子函数体弹回给用户，精确回显
//! 抑制在 ZLE 折行重绘碎片面前收敛不了（#413 评论里 12 用例归一化判定全 LEAK）。
//! 产品在 box 上点查 Fail，#414 已把注入总闸默认关闭。
//!
//! 本模块走第三条通道：探测用的 SSH **exec 通道本身无 TTY**，输出不经用户终端。
//! 在该通道里遍历 `/proc/<pid>/`，找到「本次 SSH 会话的交互 shell」，直接
//! `readlink /proc/<pid>/cwd`。全程**不向用户 PTY 写入任何字节**，因此不存在回显、
//! 折行、ZLE 碎片、可见性泄漏这些问题；也不依赖服务器装 shell integration。
//!
//! ## 精确锁定「本次会话」
//!
//! 同一 UID 可能同时开多条 SSH 连接，光看 `tty != "?"` 会命中别人的 shell。
//! 实测（loopback sshd + paramiko，2026-11-12）发现：exec 进程与交互 shell 由**同一个
//! sshd 子进程** fork 出来，即二者 `$PPID` 相同；另一条 SSH 会话的 shell 挂在另一个
//! sshd 下。因此筛选键为：
//!
//! 1. `/proc/<pid>/stat` 的 `tty_nr != 0`（排除 exec 通道自己无 TTY 的 shell）；
//! 2. `ppid == $PPID`，或 `/proc/<pid>/environ` 里 `SSH_CONNECTION` 与本次一致
//!    （兼容 sshd 把 exec 与 PTY 分给不同子进程的布局）；
//! 3. `comm` 属于已知 shell 名单；
//! 4. 多个候选（嵌套子 shell / tmux）时取**最大 pid**，即最近 fork 的活跃 shell
//!    ——实测 `bash` 子 shell 里 `cd` 后能实时跟随。
//!
//! `stat` 的 `comm` 字段可能含空格/括号，用 `${s##*) }` 从最后一个 `) ` 之后开始切，
//! 避免按固定列号解析踩坑。命令整体包一层 `sh -c`，保证登录 shell 是 fish/csh
//! 时也按 POSIX 语法执行；脚本内只用双引号，不含单引号。
//!
//! ## 边界（诚实）
//!
//! - 只能读同 UID 进程的 cwd；root 交互 shell 普通用户读不到。MistTerm 本来就按登录
//!   用户自己的会话查，天然成立。
//! - 容器 / `hidepid` / `/proc` 被裁剪时 `readlink` 失败 → 输出无 `cwd=` 行，调用方
//!   保持原有 `shell_cwd`，降级链照旧（OSC 7 优先 → 带外兜底 → home）。
//! - 嵌套 tmux/screen 的 cwd 拿不到（tty 过滤命中外层），与旧注入方案同一盲区。
//! - 同 tty 多 shell 取最新 pid 是启发式，刚 `exit` 子 shell 的瞬间可能短暂不一致。

use super::shell_integration::RemoteIdentity;

/// 带外查询命令，交给 [`crate::ssh::SshSessionHandle::enqueue_remote_exec`]。
///
/// 输出三行（顺序无关，`cwd=` 仅在命中时出现）：
/// `user=<id -un>` / `host=<nodename>` / `cwd=<绝对路径>`。
/// 身份顺带补全 #414 关闭注入探测后 `remote_identity` 一直为空的缺口，
/// 让跨会话目录继承的身份判定重新可用——仍然零 PTY 写入。
pub const OOB_CWD_COMMAND: &str = r##"sh -c 'me=$$; parent=$PPID; peer="${SSH_CONNECTION:-}"; hit=0; chosen=""; for p in /proc/[0-9]*/; do pid=${p%/}; pid=${pid##*/}; [ "$pid" = "$me" ] && continue; c=$(cat "$p/comm" 2>/dev/null) || continue; case "$c" in bash|zsh|fish|sh|dash|ash|ksh|mksh|tcsh|csh|busybox|pwsh) ;; *) continue ;; esac; s=$(cat "$p/stat" 2>/dev/null) || continue; r=${s##*) }; set -- $r; [ "$5" != "0" ] || continue; if [ "$2" = "$parent" ]; then :; elif [ -n "$peer" ] && tr "\000" "\012" < "$p/environ" 2>/dev/null | grep -qF "SSH_CONNECTION=$peer"; then :; else continue; fi; [ "$pid" -gt "$hit" ] || continue; d=$(readlink "$p/cwd" 2>/dev/null) || continue; case "$d" in /*) ;; *) continue ;; esac; hit=$pid; chosen=$d; done; printf "user=%s\nhost=%s\n" "$(id -un)" "${HOSTNAME:-$(uname -n 2>/dev/null)}"; [ -n "$chosen" ] && printf "cwd=%s\n" "$chosen"'"##;

/// 一次带外查询的结果。`cwd` 为 `None` 表示没查到（/proc 受限、无匹配 shell 等）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OobCwdResult {
    pub cwd: Option<String>,
    pub identity: Option<RemoteIdentity>,
}

/// 解析 exec 通道输出。容忍诊断噪音插队；只接受绝对路径，避免把报错文本当目录。
pub fn parse_oob_cwd_output(raw: &str) -> OobCwdResult {
    let mut cwd = None;
    let mut user = String::new();
    let mut host = String::new();
    for line in raw.lines().map(str::trim) {
        if let Some(v) = line.strip_prefix("cwd=") {
            let v = v.trim();
            if v.starts_with('/') && v.chars().all(|c| !c.is_control()) {
                cwd = Some(v.to_string());
            }
        } else if let Some(v) = line.strip_prefix("user=") {
            user = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("host=") {
            host = v.trim().to_string();
        }
    }
    let identity = if user.is_empty() || host.is_empty() {
        None
    } else {
        Some(RemoteIdentity { host, user })
    };
    OobCwdResult { cwd, identity }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_is_single_line_and_shell_agnostic() {
        let cmd = OOB_CWD_COMMAND;
        assert!(!cmd.contains('\n'), "exec 命令必须单行");
        assert!(cmd.starts_with("sh -c '"), "包 sh -c 保证非 POSIX 登录 shell 也能跑");
        assert!(
            !cmd["sh -c '".len()..cmd.len() - 1].contains('\''),
            "脚本体内不得出现单引号"
        );
        assert!(cmd.contains("/proc/"), "核心是读 /proc");
        assert!(cmd.contains("SSH_CONNECTION"), "要靠它区分同 UID 的其它会话");
        assert!(cmd.contains("readlink"), "cwd 来自 readlink");
        assert!(cmd.contains("cwd="), "输出带 cwd= 行");
        assert!(cmd.contains("user="), "顺带补全身份");
    }

    #[test]
    fn parses_full_result() {
        let r = parse_oob_cwd_output("user=deploy\nhost=box\ncwd=/srv/app\n");
        assert_eq!(r.cwd.as_deref(), Some("/srv/app"));
        assert_eq!(
            r.identity,
            Some(RemoteIdentity {
                host: "box".into(),
                user: "deploy".into(),
            })
        );
    }

    #[test]
    fn parses_path_with_spaces() {
        let r = parse_oob_cwd_output("user=tian\nhost=box\ncwd=/tmp/mist oob d2\n");
        assert_eq!(r.cwd.as_deref(), Some("/tmp/mist oob d2"));
    }

    #[test]
    fn tolerates_diagnostic_noise_and_reordered_lines() {
        let r = parse_oob_cwd_output(
            "sh: warning: setlocale\ncwd=/home/u/proj\nhost=box\nawk: not found\nuser=u\n",
        );
        assert_eq!(r.cwd.as_deref(), Some("/home/u/proj"));
        assert_eq!(r.identity.unwrap().user, "u");
    }

    #[test]
    fn missing_cwd_still_yields_identity() {
        let r = parse_oob_cwd_output("user=u\nhost=box\n");
        assert!(r.cwd.is_none(), "没命中就别硬给目录");
        assert_eq!(r.identity.unwrap().host, "box");
    }

    #[test]
    fn rejects_non_absolute_and_control_paths() {
        assert!(parse_oob_cwd_output("cwd=relative/path\n").cwd.is_none());
        assert!(parse_oob_cwd_output("cwd=/tmp/\x1b[2J\n").cwd.is_none());
        assert!(parse_oob_cwd_output("cwd=\n").cwd.is_none());
    }

    #[test]
    fn empty_or_partial_identity() {
        assert_eq!(parse_oob_cwd_output(""), OobCwdResult::default());
        assert!(parse_oob_cwd_output("user=u\n").identity.is_none());
        assert!(parse_oob_cwd_output("host=box\n").identity.is_none());
    }
}
