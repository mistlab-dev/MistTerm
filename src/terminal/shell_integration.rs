//! #401 方案 B：连接时按远端 shell 类型注入目录上报钩子。
//!
//! 终端只从输出流被动听 `OSC 7` / `OSC 9;9`（见 [`super::sem_osc`]）；服务器装了
//! shell integration 时这条路就够用，没装的（老服务器、精简镜像）则完全收不到，
//! 于是连接成功后主动补一份等价钩子，走 VS Code Remote 同一套思路。
//!
//! 三条约束决定了这里的形态：
//!
//! 1. **不落盘**：只改当前会话的 shell 变量，断开即失效，不写 `~/.bashrc`。
//! 2. **幂等**：重复连接/重连不能叠加钩子，靠函数名当哨兵判断是否已注入。
//! 3. **不破坏用户已有钩子**：`PROMPT_COMMAND` / `precmd_functions` 里原本的东西必须照跑，
//!    所以是"前置拼接"而不是"覆盖赋值"。
//!
//! 注入命令是**单行**。#410：发送后进入精确回显抑制窗口——PTY 的 ECHO 会把
//! 注入的那行原样弹回给用户（这就是产品看到的"露出一行内部命令"），我们在
//! VTE 消化之前按字节流精确剥掉 `expected` 的回显：流式匹配、跨 chunk、
//! `\n`↔`\r\n` 规范化、超时放行。不做清屏、不整段吞流、窗口外一个字节都不碰。
//! probe 本身走独立 exec 通道，输出不经用户终端，用户看不到探测过程。

/// 远端交互 shell 的类型。只覆盖能稳定注入的三种；其余一律 `Unknown` = 不注入。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShellKind {
    Bash,
    Zsh,
    Fish,
    #[default]
    Unknown,
}

/// 远端身份：`host` 与 OSC 7 载荷里的是同一个值（系统 nodename），`user` 是登录名。
///
/// 用途只有一处：判断"源窗格报的目录，搬到这条新连接上还站不站得住"。主机名相同但
/// 登录用户不同时不能搬——那多半会 `cd` 进对方无权访问的目录（#400 那类报错）。
/// 所以这里坚持 `host + user` 一起比，而不是 issue 原本写的只比主机名。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteIdentity {
    pub host: String,
    pub user: String,
}

/// 一次探测的完整结果：shell 类型 + 远端身份（身份可能缺失）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProbeResult {
    pub shell: ShellKind,
    pub identity: Option<RemoteIdentity>,
}

/// 钩子函数名，同时用作"是否已注入"的哨兵。
///
/// 名字带双下划线前缀是 shell 惯例里的私有命名，尽量不与用户函数撞车。
const HOOK_FN: &str = "__mist_cwd_report";

/// 探测命令：跑在独立 exec 通道（无 TTY），输出对用户不可见。
///
/// 一行拿三样东西：登录 shell、登录名、主机名。
///
/// - 登录 shell 取 `/etc/passwd` 里的字段 —— sshd 就是按它启动交互 shell 的，比 `$SHELL`
///   更接近真相（`$SHELL` 在非登录环境里可能为空或仍是旧值）。用 `awk` 读本地文件而不是
///   `getent`：LDAP/NIS 环境下 `getent` 可能卡在网络上，而这条命令跑在 shell 泵线程里、
///   期间 PTY 读写是暂停的，不能赌它多快。读不到再回落 `$SHELL`。
/// - 主机名用 `${HOSTNAME:-$(uname -n)}`，与注入钩子里上报的值保持同一个来源，
///   否则判定时会拿两个拼写不同的名字互相比。
pub fn detect_command() -> &'static str {
    concat!(
        r#"p=$(awk -F: -v u="$(id -un)" '$1==u{print $7; exit}' /etc/passwd 2>/dev/null); "#,
        r#"[ -n "$p" ] || p=${SHELL:-}; "#,
        r#"printf 'shell=%s\nuser=%s\nhost=%s\n' "$p" "$(id -un)" "${HOSTNAME:-$(uname -n 2>/dev/null)}""#
    )
}

/// `/bin/bash`、`/usr/local/bin/zsh` → 对应类型；`sh`/`dash`/`tcsh`/空 → `Unknown`。
pub fn kind_from_path(path: &str) -> ShellKind {
    let name = path.trim().rsplit('/').next().unwrap_or("").trim();
    match name {
        "bash" => ShellKind::Bash,
        "zsh" => ShellKind::Zsh,
        "fish" => ShellKind::Fish,
        _ => ShellKind::Unknown,
    }
}

/// 解析探测命令的输出（`shell=` / `user=` / `host=` 三行，顺序无关）。
///
/// 兼容只有一行路径的老格式：那种情况按最后一条非空行当 shell 路径。
pub fn parse_probe_output(raw: &str) -> ProbeResult {
    let mut shell_path = String::new();
    let mut user = String::new();
    let mut host = String::new();
    for line in raw.lines().map(str::trim) {
        if let Some(v) = line.strip_prefix("shell=") {
            shell_path = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("user=") {
            user = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("host=") {
            host = v.trim().to_string();
        }
    }
    if shell_path.is_empty() {
        // exec 通道里可能先冒出一行诊断噪音（awk: not found），取最后一条非空行最稳。
        if let Some(last) = raw.lines().map(str::trim).filter(|l| !l.is_empty()).next_back() {
            shell_path = last.to_string();
        }
    }
    let identity = if user.is_empty() || host.is_empty() {
        None
    } else {
        Some(RemoteIdentity { host, user })
    };
    ProbeResult {
        shell: kind_from_path(&shell_path),
        identity,
    }
}

/// 旧签名保留：只要 shell 类型。
pub fn kind_from_probe_output(raw: &str) -> ShellKind {
    parse_probe_output(raw).shell
}

/// 该类型对应的注入命令（单行，调用方负责补 `\n` 触发执行）。
///
/// `None` = 这个 shell 我们不认识，宁可不注入也不猜——注入错命令会把用户 shell 弄坏。
pub fn injection_command(kind: ShellKind) -> Option<&'static str> {
    match kind {
        ShellKind::Bash => Some(BASH_HOOK),
        ShellKind::Zsh => Some(ZSH_HOOK),
        ShellKind::Fish => Some(FISH_HOOK),
        ShellKind::Unknown => None,
    }
}

/// bash：定义钩子函数 + 前置拼进 `PROMPT_COMMAND`。
///
/// - `${HOSTNAME:-$(hostname)}`：bash 通常有 `HOSTNAME`，没有就起一次子进程兜底。
/// - `case ... in *__mist_cwd_report*)`：已注入过就什么都不做（幂等）。
/// - `${PROMPT_COMMAND:+;$PROMPT_COMMAND}`：原值非空时用分号接在后面，原钩子照跑。
///   注意 bash 5.1+ 允许 `PROMPT_COMMAND` 是数组，这种极端用法下拼接只作用于首元素，
///   属于已知取舍——数组写法太少见，为它加分支不值得。
/// - `printf '\033]7;...\033\\'`：`ESC ]` 起、`ESC \`（ST）收。
const BASH_HOOK: &str = concat!(
    r#"__mist_cwd_report() { printf '\033]7;file://%s%s\033\\' "${HOSTNAME:-$(hostname 2>/dev/null)}" "$PWD"; }"#,
    r#"; case "${PROMPT_COMMAND-}" in *__mist_cwd_report*) ;; *)"#,
    r#" PROMPT_COMMAND="__mist_cwd_report${PROMPT_COMMAND:+;$PROMPT_COMMAND}" ;; esac"#,
);

/// zsh：`precmd_functions` 是数组，用 `[*]` 拼成字符串做包含判断。
///
/// zsh 没有 `HOSTNAME`，只有 `HOST`。
const ZSH_HOOK: &str = concat!(
    r#"__mist_cwd_report() { printf '\033]7;file://%s%s\033\\' "${HOST:-$(hostname 2>/dev/null)}" "$PWD"; }"#,
    r#"; case ":${precmd_functions[*]}:" in *:__mist_cwd_report:*) ;; *)"#,
    r#" precmd_functions+=(__mist_cwd_report) ;; esac"#,
);

/// fish：同名函数重复定义即替换，天然幂等，不需要额外的哨兵判断。
/// `--on-event fish_prompt` 相当于 bash 的 `PROMPT_COMMAND`。
const FISH_HOOK: &str =
    r#"function __mist_cwd_report --on-event fish_prompt; printf '\033]7;file://%s%s\033\\' (hostname 2>/dev/null) $PWD; end"#;

/// 精确回显抑制窗口的时长：超过后停止过滤，残余回显保持可见（宁可露一行
/// 也不吞用户输出）。覆盖慢速链路下 shell 回显 + 执行的开销。
pub const ECHO_SUPPRESS_WINDOW: std::time::Duration = std::time::Duration::from_millis(1500);

/// `expected` 末尾如果是 `\n`，额外允许 `\r\n` 结尾；末尾是 `\r` 则额外允许
/// `\r\n`（PTY 回显会把行尾的 CR 展开成 CRLF）。
fn echo_variants(expected: &[u8]) -> Vec<Vec<u8>> {
    let mut variants = Vec::new();
    match expected.last() {
        Some(&b'\n') => {
            let mut crlf = expected.to_vec();
            let n = crlf.len();
            crlf[n - 1] = b'\r';
            crlf.push(b'\n');
            // 长变体优先：能用 CRLF 吃掉就别留下半个行尾。
            variants.push(crlf);
            variants.push(expected.to_vec());
        }
        Some(&b'\r') => {
            let mut crlf = expected.to_vec();
            crlf.push(b'\n');
            variants.push(crlf);
            variants.push(expected.to_vec());
        }
        _ => variants.push(expected.to_vec()),
    }
    variants
}

/// #410：注入后的精确回显抑制状态机。
///
/// 发送钩子（及同帧的自动 `cd`）后进入抑制窗口：后续 PTY 输出里，
/// **只有**完整出现的注入载荷回显（允许 `\n`↔`\r\n` 规范化）会被剥掉，
/// 其余字节（MOTD、Last login、报错、OSC 7、用户输出）原样放行。
/// 超时后窗口关闭，残余保持可见；不做清屏、不宽泛吞流。
///
/// 实现用"暂存候选 + 完整匹配才剥离"：跨 chunk 的部分匹配先暂存在
/// `pending` 里，一旦走偏立即把暂存字节原样放行——绝不吞用户输出，
/// 代价只是走偏时那一次多余的暂存。
#[derive(Debug)]
pub struct EchoSuppressor {
    /// 依次待匹配的精确载荷（hook 在前、同帧自动 cd 在后）。
    queue: Vec<Vec<u8>>,
    /// 已暂存的候选回显字节（可能是某个载荷前缀的一部分）。
    pending: Vec<u8>,
    /// 窗口截止时刻；`None` 表示窗口已关。
    deadline: Option<std::time::Instant>,
}

impl Default for EchoSuppressor {
    fn default() -> Self {
        Self::new()
    }
}

impl EchoSuppressor {
    pub fn new() -> Self {
        Self {
            queue: Vec::new(),
            pending: Vec::new(),
            deadline: None,
        }
    }

    pub fn is_active(&self) -> bool {
        self.deadline.is_some()
    }

    /// 兼容别名。
    pub fn active(&self) -> bool {
        self.is_active()
    }

    /// 开始/追加一个抑制窗口：`payload` 是实际写入 PTY 的字节。
    /// 首个载荷重置截止时钟；同帧追加（hook + cd）按顺序排队。
    pub fn begin(&mut self, payload: &[u8]) {
        if payload.is_empty() {
            return;
        }
        if self.queue.is_empty() && self.pending.is_empty() {
            self.deadline = Some(std::time::Instant::now() + ECHO_SUPPRESS_WINDOW);
        }
        self.queue.push(payload.to_vec());
    }

    /// 超时检查：到点关闭窗口，暂存与残余全部放行（可见）。
    pub fn tick(&mut self, now: std::time::Instant) -> bool {
        match self.deadline {
            Some(d) if now >= d => {
                self.reset();
                true
            }
            _ => false,
        }
    }

    pub fn reset(&mut self) {
        self.queue.clear();
        self.pending.clear();
        self.deadline = None;
    }

    /// 暂存字节与首个载荷的所有变体不再有任何前缀关系时，返回 true。
    fn pending_is_stale(&self) -> bool {
        let Some(expected) = self.queue.first() else {
            return true;
        };
        !echo_variants(expected)
            .iter()
            .any(|v| v.starts_with(&self.pending))
    }

    /// 暂存字节已完整构成首个载荷的某个变体。返回 Some(剥离起点)：
    /// 匹配结束后从该位置起的字节要放行。只允许多余字节是行尾重复的 `\r`。
    fn pending_completed(&self) -> Option<usize> {
        let Some(expected) = self.queue.first() else {
            return None;
        };
        for v in echo_variants(expected) {
            if self.pending.starts_with(&v) {
                let tail = &self.pending[v.len()..];
                if tail.iter().all(|&b| b == b'\r') {
                    return Some(v.len());
                }
            }
        }
        None
    }

    /// 过滤一段 PTY 输出：剥离注入载荷的完整回显，返回剩余应进 VTE 的字节。
    /// 窗口未激活时原样返回输入。任何走偏都立即放行暂存字节——绝不吞用户输出。
    pub fn filter(&mut self, data: &[u8], now: std::time::Instant) -> Vec<u8> {
        if !self.is_active() {
            return data.to_vec();
        }
        if self.tick(now) {
            return data.to_vec();
        }
        let mut out: Vec<u8> = Vec::with_capacity(data.len());
        // 先处理上次剩下的暂存候选。
        if !self.pending.is_empty() {
            self.pending.extend_from_slice(data);
            if let Some(keep_from) = self.pending_completed() {
                // 完整匹配：剥离载荷本体（含可放行的行尾 `\r`），多余字节放行。
                out.extend_from_slice(&self.pending[keep_from..]);
                self.pending.clear();
                self.queue.remove(0);
                if self.queue.is_empty() {
                    self.deadline = None;
                }
            } else if self.pending_is_stale() {
                // 走偏：暂存字节是真实输出，原样放行并放弃当前载荷。
                out.append(&mut self.pending);
                self.queue.remove(0);
                if self.queue.is_empty() {
                    self.deadline = None;
                }
            }
            return out;
        }
        // 正常路径：逐字节推进候选窗口，只暂存有希望构成载荷回显的前缀。
        let mut i = 0usize;
        while i < data.len() {
            let Some(expected) = self.queue.first() else {
                break;
            };
            // 尝试从 i 开始直接完整匹配某个变体。
            let mut matched_len = None;
            for v in echo_variants(expected) {
                if data[i..].starts_with(&v) {
                    matched_len = Some(v.len());
                    break;
                }
            }
            if let Some(mlen) = matched_len {
                i += mlen;
                self.queue.remove(0);
                if self.queue.is_empty() {
                    // 窗口结束：剩余字节（提示符、用户输出）必须原样放行。
                    out.extend_from_slice(&data[i..]);
                    self.pending.clear();
                    self.deadline = None;
                    return out;
                }
                continue;
            }
            // 没有完整匹配：暂存从 i 开始的、仍是某个变体前缀的最长候选。
            let variants = echo_variants(expected);
            let max_prefix = variants
                .iter()
                .map(|v| {
                    let n = (data.len() - i).min(v.len());
                    let mut k = 0usize;
                    while k < n && v[k] == data[i + k] {
                        k += 1;
                    }
                    k
                })
                .max()
                .unwrap_or(0);
            if max_prefix == 0 {
                // 这个字节与任何载荷都无关：放行。
                out.push(data[i]);
                i += 1;
                continue;
            }
            // 暂存候选，等下一个 chunk 再判。剩余字节若是候选之外的输出也先放行？
            // 不——候选之后的字节归属未定（可能是回显续体），先暂存起来一起等。
            self.pending.extend_from_slice(&data[i..]);
            i = data.len();
            if self.pending_completed().is_some() {
                let keep_from = self.pending_completed().unwrap();
                out.extend_from_slice(&self.pending[keep_from..]);
                self.pending.clear();
                self.queue.remove(0);
                if self.queue.is_empty() {
                    self.deadline = None;
                }
            } else if self.pending_is_stale() {
                out.append(&mut self.pending);
                self.queue.remove(0);
                if self.queue.is_empty() {
                    self.deadline = None;
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_from_shell_path() {
        assert_eq!(kind_from_path("/bin/bash"), ShellKind::Bash);
        assert_eq!(kind_from_path("/usr/local/bin/zsh"), ShellKind::Zsh);
        assert_eq!(kind_from_path("/usr/bin/fish"), ShellKind::Fish);
        assert_eq!(kind_from_path("bash"), ShellKind::Bash, "裸名字也要认");
        assert_eq!(kind_from_path("/bin/sh"), ShellKind::Unknown);
        assert_eq!(kind_from_path("/bin/dash"), ShellKind::Unknown);
        assert_eq!(kind_from_path(""), ShellKind::Unknown);
    }

    #[test]
    fn probe_output_parsing() {
        assert_eq!(kind_from_probe_output("/bin/bash\n"), ShellKind::Bash);
        assert_eq!(kind_from_probe_output("/usr/bin/zsh"), ShellKind::Zsh);
        assert_eq!(
            kind_from_probe_output("awk: not found\n/bin/bash\n"),
            ShellKind::Bash,
            "取最后一行非空，诊断噪音不影响判定"
        );
        assert_eq!(kind_from_probe_output(""), ShellKind::Unknown);
        assert_eq!(kind_from_probe_output("\n\n"), ShellKind::Unknown);
    }

    #[test]
    fn probe_parses_shell_and_identity() {
        let raw = "shell=/usr/bin/zsh\nuser=deploy\nhost=opencloudos\n";
        let pr = parse_probe_output(raw);
        assert_eq!(pr.shell, ShellKind::Zsh);
        assert_eq!(
            pr.identity,
            Some(RemoteIdentity {
                host: "opencloudos".into(),
                user: "deploy".into()
            })
        );

        // 顺序无关：探针输出被前置诊断噪音插队也照样认。
        let raw = "awk: line 1\nhost=box\nuser=root\nshell=/bin/bash\n";
        let pr = parse_probe_output(raw);
        assert_eq!(pr.shell, ShellKind::Bash);
        assert_eq!(pr.identity.unwrap().user, "root");

        // 身份缺一半就当没有：只有 host 不足以判断该不该继承目录。
        assert!(parse_probe_output("shell=/bin/bash\nhost=box\n").identity.is_none());
        assert!(parse_probe_output("shell=/bin/bash\nuser=root\n").identity.is_none());
        assert!(parse_probe_output("").identity.is_none());
    }

    #[test]
    fn identity_compares_host_and_user() {
        // 同机不同账号不能算同一身份——继承过去会 cd 进无权目录。
        let a = RemoteIdentity {
            host: "box".into(),
            user: "alice".into(),
        };
        let b = RemoteIdentity {
            host: "box".into(),
            user: "bob".into(),
        };
        assert_ne!(a, b);
        assert_eq!(
            a,
            RemoteIdentity {
                host: "box".into(),
                user: "alice".into()
            }
        );
    }

    #[test]
    fn unknown_shell_never_injects() {
        assert!(injection_command(ShellKind::Unknown).is_none());
        assert!(injection_command(ShellKind::Bash).is_some());
        assert!(injection_command(ShellKind::Zsh).is_some());
        assert!(injection_command(ShellKind::Fish).is_some());
    }

    #[test]
    fn hooks_are_single_line() {
        // 注入靠一次 send_input 发送，命令里出现换行会被拆成多条命令执行。
        for k in [ShellKind::Bash, ShellKind::Zsh, ShellKind::Fish] {
            let cmd = injection_command(k).unwrap();
            assert!(!cmd.contains('\n'), "{k:?} 注入命令必须是单行");
            assert!(!cmd.is_empty());
        }
    }

    #[test]
    fn hooks_are_idempotent_and_non_destructive() {
        // 每条钩子都要：定义同名函数（哨兵）+ 检测已存在 + 不覆盖原值。
        let bash = injection_command(ShellKind::Bash).unwrap();
        assert!(bash.contains(HOOK_FN), "哨兵函数名缺失");
        assert!(
            bash.contains("PROMPT_COMMAND:+"),
            "必须前置拼接原 PROMPT_COMMAND，不能覆盖掉用户已有钩子"
        );
        assert!(
            !bash.contains("PROMPT_COMMAND=") || bash.contains("${PROMPT_COMMAND:+"),
            "赋值只能发生在拼接里"
        );

        let zsh = injection_command(ShellKind::Zsh).unwrap();
        assert!(zsh.contains(HOOK_FN));
        assert!(zsh.contains("precmd_functions+="), "zsh 要追加而非替换");
        assert!(zsh.contains("*:__mist_cwd_report:*"), "先检测再追加");

        let fish = injection_command(ShellKind::Fish).unwrap();
        assert!(fish.contains(HOOK_FN));
        assert!(fish.contains("--on-event fish_prompt"));
    }

    #[test]
    fn hooks_emit_osc7_with_st() {
        // 载荷必须以 ST（ESC \）收尾而不是 BEL：BEL 在部分 shell 下会被读成提示音。
        for k in [ShellKind::Bash, ShellKind::Zsh, ShellKind::Fish] {
            let cmd = injection_command(k).unwrap();
            assert!(cmd.contains(r"\033]7;file://"), "{k:?} 应上报 OSC 7");
            assert!(cmd.contains(r"\033\\"), "{k:?} 应以 ST 收尾");
        }
    }

    #[test]
    fn detect_command_reads_login_shell_with_fallback() {
        let cmd = detect_command();
        assert!(cmd.contains("/etc/passwd"), "优先读本地 passwd，不依赖可能卡网络的 getent");
        assert!(cmd.contains("${SHELL:-}"), "读不到时回落 $SHELL");
        assert!(
            cmd.contains(r#"${HOSTNAME:-$(uname -n"#),
            "主机名要和钩子上报的取值同源，否则身份永远比不上"
        );
        assert!(cmd.contains("user=") && cmd.contains("host="), "要一并带出身份");
        assert!(!cmd.contains('\n'), "探测命令也走单行");
    }

    mod suppressor_tests {
        use super::*;
        use std::time::Instant;

        fn now() -> Instant {
            Instant::now()
        }

        #[test]
        fn inactive_passes_through() {
            let mut s = EchoSuppressor::new();
            let out = s.filter(b"Last login: ...\r\nprompt$ ", now());
            assert_eq!(out, b"Last login: ...\r\nprompt$ ".to_vec());
        }

        #[test]
        fn strips_full_echo_in_one_chunk() {
            let mut s = EchoSuppressor::new();
            s.begin(b"__mist_cwd_report() { ... }; PROMPT_COMMAND=...\n");
            let echo = b"__mist_cwd_report() { ... }; PROMPT_COMMAND=...\r\n";
            let out = s.filter(&echo[..], now());
            assert!(out.is_empty(), "回显应被精确剥掉，got {:?}", String::from_utf8_lossy(&out));
            assert!(!s.is_active());
        }

        #[test]
        fn strips_across_split_chunks() {
            let mut s = EchoSuppressor::new();
            let payload = b"__mist_cwd_report() { printf ... }\n";
            s.begin(payload);
            let echo = b"__mist_cwd_report() { printf ... }\r\n";
            let (a, b) = echo.split_at(10);
            let out_a = s.filter(a, now());
            assert!(out_a.is_empty());
            let out_b = s.filter(b, now());
            assert!(out_b.is_empty());
            assert!(!s.is_active());
        }

        #[test]
        fn passes_leading_output_then_strips_hook() {
            let mut s = EchoSuppressor::new();
            s.begin(b"hook-line\n");
            let stream = b"Last login: Mon Nov 10 from 1.2.3.4\r\nhook-line\r\nprompt$ ";
            let out = s.filter(&stream[..], now());
            assert_eq!(out, b"Last login: Mon Nov 10 from 1.2.3.4\r\nprompt$ ".to_vec());
        }

        #[test]
        fn passes_osc_and_following_user_output() {
            let mut s = EchoSuppressor::new();
            s.begin(b"hook-line\n");
            // 回显后 shell 执行钩子产出 OSC 7，再下一个提示符 + 用户命令输出。
            let stream = b"hook-line\r\n\x1b]7;file://h/home\x1b\\prompt$ file1 file2\r\n";
            let out = s.filter(&stream[..], now());
            assert_eq!(
                String::from_utf8_lossy(&out),
                "\x1b]7;file://h/home\x1b\\prompt$ file1 file2\r\n"
            );
        }

        #[test]
        fn queue_handles_hook_then_startup_cd() {
            let mut s = EchoSuppressor::new();
            s.begin(b"__mist_cwd_report() { ... }; PROMPT_COMMAND=...\n");
            s.begin(b"cd -- '/srv/app'\r");
            let stream = b"__mist_cwd_report() { ... }; PROMPT_COMMAND=...\r\ncd -- '/srv/app'\r\n";
            let out = s.filter(&stream[..], now());
            assert!(out.is_empty(), "两段回显都要剥掉，got {:?}", String::from_utf8_lossy(&out));
            assert!(!s.is_active());
        }

        #[test]
        fn mismatch_passes_rest_through() {
            let mut s = EchoSuppressor::new();
            s.begin(b"hook-line\n");
            // 回显根本没出现，来的全是别的东西：一个字节都不能吞。
            let stream = b"Welcome to Ubuntu 22.04\r\nprompt$ ";
            let out = s.filter(&stream[..], now());
            assert_eq!(out, stream.to_vec());
        }

        #[test]
        fn timeout_stops_filtering_residue_visible() {
            let mut s = EchoSuppressor::new();
            s.begin(b"hook-line\n");
            let t0 = Instant::now();
            // 窗口内只回显了前半截。
            let out0 = s.filter(b"hook-li", t0);
            assert!(out0.is_empty());
            // 超时后残余保持可见。
            let later = t0 + ECHO_SUPPRESS_WINDOW + std::time::Duration::from_millis(10);
            let out1 = s.filter(b"ne is visible\r\n", later);
            assert_eq!(out1, b"ne is visible\r\n".to_vec());
            assert!(!s.is_active());
        }

        #[test]
        fn partial_match_then_divergence_passes_through() {
            let mut s = EchoSuppressor::new();
            let payload = b"__mist_cwd_report\n";
            s.begin(payload);
            // 前缀匹配后走偏：暂存字节全部放行（宁可多露一字也不吞用户输出）。
            let stream = b"__mist_cwd_reportX and user output\r\n";
            let out = s.filter(&stream[..], now());
            assert_eq!(out, stream.to_vec());
        }

        #[test]
        fn reset_clears_everything() {
            let mut s = EchoSuppressor::new();
            s.begin(b"hook\n");
            s.reset();
            let out = s.filter(b"hook\r\nanything", now());
            assert_eq!(out, b"hook\r\nanything".to_vec());
            assert!(!s.is_active());
        }

        #[test]
        fn empty_begin_is_noop() {
            let mut s = EchoSuppressor::new();
            s.begin(b"");
            assert!(!s.is_active());
        }
    }
}
