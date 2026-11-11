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
//!
//! ## 产品开关（2026-11-10）
//!
//! 真实 sshd 上 zsh 新连接/新标签仍可能在提示符处露出钩子函数体（可见性修复未达标）。
//! 在那项工作恢复之前，[`CWD_HOOK_INJECTION_ENABLED`] 默认关闭：连接、重连、新标签
//! 都不会把 `__mist_cwd_report` / `PROMPT_COMMAND` / `precmd` 文本写进交互 PTY，
//! 也不会再为注入跑远端探测（探测与 PTY 读写互斥，最坏阻塞约 15s）。
//! 服务器自己上报的 OSC 7 / 9;9 / 1337 CurrentDir 仍会听，目录继承在那种情况下还能用。
//! 把该常量改回 `true` 即恢复注入；用户偏好 `AppSettings.shell_integration_hook` 仍叠在上面。

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
pub const HOOK_FN: &str = "__mist_cwd_report";

/// 是否允许向交互 PTY 注入 cwd 上报钩子。默认关闭。
///
/// 产品口径（2026-11-10）：先保证线上可用，可见性修复后续再做。改回 `true`
/// 即可恢复探测与注入；不要另加用户可见的新开关。
pub const CWD_HOOK_INJECTION_ENABLED: bool = false;

/// 本次会话是否应注入钩子：产品总闸与用户偏好同时开启才注入。
pub fn cwd_hook_injection_enabled(user_pref: bool) -> bool {
    CWD_HOOK_INJECTION_ENABLED && user_pref
}

/// 是否应启动为注入服务的 shell 探测（独立 exec，最长约 15s，期间 PTY 泵暂停）。
///
/// 总闸关闭时跳过：没有注入就不需要探测，也避免无谓地卡住新连接。
pub fn cwd_hook_probe_enabled() -> bool {
    CWD_HOOK_INJECTION_ENABLED
}

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
        if let Some(last) = raw
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .next_back()
        {
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
    r#"__mist_cwd_report() { printf '\033]7;file://%s%s\033\\' "${HOSTNAME:-$(uname -n)}" "$PWD"; }"#,
    r#"; case "${PROMPT_COMMAND-}" in *__mist_cwd_report*) ;; *)"#,
    r#" PROMPT_COMMAND="__mist_cwd_report${PROMPT_COMMAND:+;$PROMPT_COMMAND}" ;; esac"#,
);

/// zsh：`precmd_functions` 是数组，用 `[*]` 拼成字符串做包含判断。
///
/// zsh 没有 `HOSTNAME`，只有 `HOST`。
const ZSH_HOOK: &str = concat!(
    r#"__mist_cwd_report() { printf '\033]7;file://%s%s\033\\' "${HOST:-$(uname -n)}" "$PWD"; }"#,
    r#"; case ":${precmd_functions[*]}:" in *:__mist_cwd_report:*) ;; *)"#,
    r#" precmd_functions+=(__mist_cwd_report) ;; esac"#,
);

/// fish：同名函数重复定义即替换，天然幂等，不需要额外的哨兵判断。
/// `--on-event fish_prompt` 相当于 bash 的 `PROMPT_COMMAND`。
const FISH_HOOK: &str = r#"function __mist_cwd_report --on-event fish_prompt; printf '\033]7;file://%s%s\033\\' (uname -n) $PWD; end"#;

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

/// 无行尾变体之后、行终止之前的允许填充：至多一个行首 `\r`，
/// 加若干个光标移动/擦行控制序列（fish 行编辑器的增量重绘，\x1b[NNN C/D/K 等），
/// 再到 `\r\n`/`\n`/控制序列为止。返回填充长度；None 表示不允许。
fn line_edit_tail_len(buf: &[u8]) -> Option<usize> {
    let mut i = 0usize;
    if i < buf.len() && buf[i] == b'\r' {
        i += 1;
    }
    while i + 1 < buf.len() && buf[i] == 0x1b && buf[i + 1] == b'[' {
        let mut j = i + 2;
        while j < buf.len() && (buf[j].is_ascii_digit() || buf[j] == b';' || buf[j] == b'?') {
            j += 1;
        }
        if j >= buf.len() {
            return None; // 序列被截断，等下一 chunk
        }
        // CSI 最终字节：0x40–0x7E（含 l/h/C/D/K/J/? 等）。
        if !(0x40..=0x7e).contains(&buf[j]) {
            return None;
        }
        j += 1;
        i = j;
    }
    if i < buf.len() && buf[i] == b'\n' {
        Some(i + 1)
    } else if i + 1 < buf.len() && buf[i] == b'\r' && buf[i + 1] == b'\n' {
        Some(i + 2)
    } else if i + 2 < buf.len() && buf[i] == b'\r' && buf[i + 1] == b'\r' && buf[i + 2] == b'\n' {
        // zsh ZLE 重置后的双 CR + LF。
        Some(i + 3)
    } else {
        None
    }
}

/// fish 行编辑器会把命令行拆成多个"载荷片段 + \r + 光标前移 CSI"重绘碎片，
/// 最后再打一份完整逐字回显。这个函数从 `buf[pos..]` 匹配**整套碎片序列 + 最终重绘**：
/// 碎片内容必须严格拼接等于载荷本体，分隔符只允许 `\r` + 单个光标前移 CSI。
/// 匹配成功返回总消费长度（含最终重绘及其行尾）；不确定则返回 None。
/// 仅当载荷不含 `\r`/`\x1b`（我们的钩子都是单行 printf，满足）时才启用。
fn fish_fragment_match(body: &[u8], buf: &[u8]) -> Option<usize> {
    if body.contains(&b'\r') || body.contains(&0x1b) {
        return None;
    }
    // 碎片分隔符：可选一个 \r，加一至多个 CSI 光标移动（\x1b[NNNC/D/K…）。
    // fish 重绘前会用纯 CSI（如 \x1b[117D）回退光标，所以 \r 是可选的。
    fn sep(mut rest: &[u8]) -> Option<(usize, &[u8])> {
        let mut c = 0usize;
        if rest.first() == Some(&b'\r') {
            c = 1;
            rest = &rest[1..];
        }
        let mut moves = 0usize;
        while rest.len() >= 2 && rest[0] == 0x1b && rest[1] == b'[' {
            let mut j = 2usize;
            while j < rest.len() && (rest[j].is_ascii_digit() || rest[j] == b';' || rest[j] == b'?')
            {
                j += 1;
            }
            if j >= rest.len() {
                return None; // 序列被截断，等下一 chunk
            }
            if !(0x40..=0x7e).contains(&rest[j]) {
                return None;
            }
            j += 1;
            c += j;
            rest = &rest[j..];
            moves += 1;
        }
        if moves > 0 {
            Some((c, rest))
        } else {
            None
        }
    }
    let mut consumed = 0usize;
    let mut frag = 0usize;
    let mut rest = buf;
    loop {
        let n = rest.len().min(body.len() - frag);
        let mut k = 0usize;
        while k < n && rest[k] == body[frag + k] {
            k += 1;
        }
        if k == 0 {
            return None;
        }
        frag += k;
        consumed += k;
        rest = &rest[k..];
        if frag == body.len() {
            // 碎片拼完了。后面：可选分隔符 + 可选最终完整重绘 + 行尾。
            if let Some((c, r)) = sep(rest) {
                consumed += c;
                rest = r;
            }
            if rest.starts_with(body) {
                consumed += body.len();
                rest = &rest[body.len()..];
            }
            return Some(consumed + line_edit_tail_len(rest)?);
        }
        let (c, r) = sep(rest)?;
        consumed += c;
        rest = r;
    }
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
    /// 暂存候选之后、暂存结果未定（回显或走偏）之前到达的字节。
    /// 无论候选最终被剥离还是放行，这些字节都排在候选后面输出，保证顺序。
    deferred: Vec<Vec<u8>>,
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
            deferred: Vec::new(),
            deadline: None,
        }
    }

    pub fn is_active(&self) -> bool {
        self.deadline.is_some()
    }

    /// 开始/追加一个抑制窗口：`payload` 是实际写入 PTY 的字节。
    /// 首个载荷重置截止时钟；同帧追加（hook + cd）按顺序排队。
    pub fn begin(&mut self, payload: &[u8]) {
        if payload.is_empty() {
            return;
        }
        if self.queue.is_empty() && self.pending.is_empty() && self.deferred.is_empty() {
            self.deadline = Some(std::time::Instant::now() + ECHO_SUPPRESS_WINDOW);
        }
        self.queue.push(payload.to_vec());
    }

    /// 超时检查：到点关闭窗口，交回所有暂存字节（残余必须可见）。
    /// 返回 Some(flushed) 表示刚超时，调用方必须把 flushed 喂进 VTE/日志路径。
    pub fn tick(&mut self, now: std::time::Instant) -> Option<Vec<u8>> {
        match self.deadline {
            Some(d) if now >= d => {
                let mut flushed = std::mem::take(&mut self.pending);
                for d_part in std::mem::take(&mut self.deferred) {
                    flushed.extend_from_slice(&d_part);
                }
                self.queue.clear();
                self.deadline = None;
                Some(flushed)
            }
            _ => None,
        }
    }

    pub fn reset(&mut self) {
        self.queue.clear();
        self.pending.clear();
        self.deferred.clear();
        self.deadline = None;
    }

    /// 过滤一段 PTY 输出：剥离注入载荷的完整回显，返回剩余应进 VTE 的字节。
    /// 窗口未激活时原样返回输入。任何走偏都立即放行暂存字节——绝不吞用户输出。
    /// 完成匹配后剩余字节会继续走匹配循环（可能是下一项回显、OSC 7、提示符）。
    pub fn filter(&mut self, data: &[u8], now: std::time::Instant) -> Vec<u8> {
        self.filter_inner(data, now, false)
    }

    /// `fresh=true`：递归扫描已暂存候选之后的余量。不把 self.pending 拼进扫描缓冲，
    /// 也不做「无新证据 → 走偏」判定（那是候选自己的状态，不属于余量）。
    fn filter_inner(&mut self, data: &[u8], now: std::time::Instant, fresh: bool) -> Vec<u8> {
        if !self.is_active() {
            return data.to_vec();
        }
        if self.tick(now).is_some() {
            // 刚超时：窗口已关，本次数据原样放行（flush 交给 tick 调用方）。
            return data.to_vec();
        }
        let mut out: Vec<u8> = Vec::with_capacity(data.len());
        // 把本次数据接到暂存候选后面，统一走完整匹配循环。
        let old_pending = if fresh {
            Vec::new()
        } else {
            std::mem::take(&mut self.pending)
        };
        let mut buf = old_pending.clone();
        for d_part in std::mem::take(&mut self.deferred) {
            buf.extend_from_slice(&d_part);
        }
        buf.extend_from_slice(data);
        let mut pos = 0usize;
        while pos < buf.len() {
            let Some(expected) = self.queue.first() else {
                break;
            };
            // 完整匹配某个变体（长变体优先）。
            let mut matched = None;
            for v in echo_variants(expected) {
                if buf[pos..].starts_with(&v) {
                    matched = Some(v.len());
                    break;
                }
            }
            // 兕底：无行尾变体（zsh ZLE / fish 行编辑器吞掉 \n 的回显）。载荷本体
            // 逐字回显后跟增量重绘控制序列，最后以 \r\n / \n 收尾才允许剥离；
            // 后面直接接可打印文本（用户敲的内容）时不能剥。
            if matched.is_none() && expected.last() == Some(&b'\n') {
                let body = &expected[..expected.len() - 1];
                if buf[pos..].starts_with(body) {
                    let rest = &buf[pos + body.len()..];
                    if let Some(tail) = line_edit_tail_len(rest) {
                        // 连同行编辑填充与行终止一起剥离：那是回显行的收尾。
                        matched = Some(body.len() + tail);
                    }
                }
            }
            // fish 行编辑器：碎片化重绘 + 最终完整重绘。整个序列必须精确拼接
            // 等于载荷本体才允许剥离；缓冲不足以判定时等下一个 chunk。
            if matched.is_none() && expected.last() == Some(&b'\n') {
                let body = &expected[..expected.len() - 1];
                if let Some(total) = fish_fragment_match(body, &buf[pos..]) {
                    if pos + total <= buf.len() {
                        matched = Some(total);
                    }
                }
            }
            if let Some(mlen) = matched {
                pos += mlen;
                self.queue.remove(0);
                if self.queue.is_empty() {
                    // 队列空了：剩余字节（提示符、OSC、用户输出）原样放行。
                    out.extend_from_slice(&buf[pos..]);
                    self.deadline = None;
                    return out;
                }
                continue;
            }
            // 没有完整匹配：若 buf[pos..] 仍是某变体的前缀，暂存等下一 chunk。
            let variants = echo_variants(expected);
            let max_prefix = variants
                .iter()
                .map(|v| {
                    let n = (buf.len() - pos).min(v.len());
                    let mut k = 0usize;
                    while k < n && v[k] == buf[pos + k] {
                        k += 1;
                    }
                    k
                })
                .max()
                .unwrap_or(0);
            if max_prefix > 0 {
                let remainder = &buf[pos + max_prefix..];
                // 钩子正文已齐、行尾/行编辑填充可能被截断：必须等下一 chunk。
                let body_waiting_tail = expected.last() == Some(&b'\n')
                    && max_prefix == expected.len() - 1
                    && !remainder.is_empty();
                // 行编辑碎片尚未结束（下一字节是 \r 或 CSI）时也必须等，不能当走偏。
                let maybe_line_edit =
                    !remainder.is_empty() && (remainder[0] == b'\r' || remainder[0] == 0x1b);
                if remainder.is_empty() || body_waiting_tail || maybe_line_edit {
                    // 有希望构成回显续体：只暂存匹配前缀本身。
                    // 若新前缀没有比已暂存的更长（没有新证据），说明候选已走偏。
                    if max_prefix <= old_pending.len() && !old_pending.is_empty() {
                        out.extend_from_slice(&old_pending);
                        pos += old_pending.len();
                        continue;
                    }
                    self.pending = buf[pos..pos + max_prefix].to_vec();
                    if !remainder.is_empty() {
                        self.deferred.push(remainder.to_vec());
                    }
                    return out;
                }
                // 同一 chunk 里前缀后面的字节已经对不上任何变体（例如 MOTD
                // 的 "from" 对上 fish 钩子 "function" 的首字母）：立刻走偏并继续扫，
                // 否则会把后面的钩子回显、OSC、提示符一起挂到下一轮，超时后整段露出。
                if max_prefix <= old_pending.len() && !old_pending.is_empty() {
                    out.extend_from_slice(&old_pending);
                    pos += old_pending.len();
                    continue;
                }
                out.extend_from_slice(&buf[pos..pos + max_prefix]);
                pos += max_prefix;
                continue;
            }
            // 走偏：这个字节与任何载荷都无关（MOTD/Last login 等），放行后继续扫。
            // 注意不能在这里放弃队列——载荷回显可能出现在后面的位置。
            out.push(buf[pos]);
            pos += 1;
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
        assert!(parse_probe_output("shell=/bin/bash\nhost=box\n")
            .identity
            .is_none());
        assert!(parse_probe_output("shell=/bin/bash\nuser=root\n")
            .identity
            .is_none());
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
    fn cwd_hook_injection_defaults_off() {
        // 产品默认：不注入、不探测。改 CWD_HOOK_INJECTION_ENABLED 时请同步改这组断言。
        assert!(
            !CWD_HOOK_INJECTION_ENABLED,
            "injection must stay off until the hook-visibility work is re-enabled"
        );
        assert!(!cwd_hook_injection_enabled(true));
        assert!(!cwd_hook_injection_enabled(false));
        assert!(!cwd_hook_probe_enabled());
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
        assert!(
            cmd.contains("/etc/passwd"),
            "优先读本地 passwd，不依赖可能卡网络的 getent"
        );
        assert!(cmd.contains("${SHELL:-}"), "读不到时回落 $SHELL");
        assert!(
            cmd.contains(r#"${HOSTNAME:-$(uname -n"#),
            "主机名要和钩子上报的取值同源，否则身份永远比不上"
        );
        assert!(
            cmd.contains("user=") && cmd.contains("host="),
            "要一并带出身份"
        );
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
            assert!(
                out.is_empty(),
                "回显应被精确剥掉，got {:?}",
                String::from_utf8_lossy(&out)
            );
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
        fn split_chunk_completed_with_osc_and_prompt_suffix() {
            // review 复现用例：第一块是回显前缀，第二块在同一 chunk 里带着
            // OSC 7 + 提示符后缀。完成匹配后必须只剥回显本体，后缀原样放行。
            let mut s = EchoSuppressor::new();
            s.begin(b"hook-line\n");
            let out_a = s.filter(b"hook-", now());
            assert!(out_a.is_empty());
            let out_b = s.filter(b"line\r\n\x1b]7;file://h/home\x1b\\prompt$ ", now());
            assert_eq!(
                String::from_utf8_lossy(&out_b),
                "\x1b]7;file://h/home\x1b\\prompt$ "
            );
            assert!(!s.is_active());
        }

        #[test]
        fn split_chunk_completed_then_cd_echo_follows() {
            // review 复现用例：hook 前缀跨块，完成块里还带着同帧 cd 的回显。
            let mut s = EchoSuppressor::new();
            s.begin(b"hook-line\n");
            s.begin(b"cd -- '/tmp'\r");
            let out_a = s.filter(b"hook-", now());
            assert!(out_a.is_empty());
            let out_b = s.filter(b"line\r\ncd -- '/tmp'\r\n", now());
            assert!(
                out_b.is_empty(),
                "两段回显都要剥掉，got {:?}",
                String::from_utf8_lossy(&out_b)
            );
            assert!(!s.is_active());
        }

        #[test]
        fn passes_leading_output_then_strips_hook() {
            let mut s = EchoSuppressor::new();
            s.begin(b"hook-line\n");
            let stream = b"Last login: Mon Nov 10 from 1.2.3.4\r\nhook-line\r\nprompt$ ";
            let out = s.filter(&stream[..], now());
            assert_eq!(
                out,
                b"Last login: Mon Nov 10 from 1.2.3.4\r\nprompt$ ".to_vec()
            );
        }

        #[test]
        fn motd_from_does_not_stall_fish_function_hook() {
            // fish 钩子以 function 开头；MOTD 的 "from" 只共享首字母。
            // 同一 chunk 必须立刻走偏并继续剥后面的真回显，不能把 OSC/提示符挂到下一轮。
            let hook = injection_command(ShellKind::Fish).unwrap();
            let mut payload = hook.as_bytes().to_vec();
            payload.push(b'\n');
            let mut s = EchoSuppressor::new();
            s.begin(&payload);
            let mut stream = b"Last login: Mon Nov 10 from 1.2.3.4\r\n".to_vec();
            stream.extend_from_slice(&payload[..payload.len() - 1]);
            stream.extend_from_slice(b"\r\n\x1b]7;file://h/tmp\x1b\\prompt$ ");
            let out = s.filter(&stream, now());
            let text = String::from_utf8_lossy(&out);
            assert!(
                text.contains("Last login: Mon Nov 10 from 1.2.3.4"),
                "MOTD 必须放行，got {text:?}"
            );
            assert!(
                text.contains("]7;file://h/tmp") && text.contains("prompt$ "),
                "OSC/提示符必须在同一轮放行，got {text:?}"
            );
            assert!(!text.contains(HOOK_FN), "钩子回显必须剥掉，got {text:?}");
            assert!(!s.is_active());
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
            assert!(
                out.is_empty(),
                "两段回显都要剥掉，got {:?}",
                String::from_utf8_lossy(&out)
            );
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
            // 第一块是载荷完整前缀（含可能的回显起始），先暂存——流式契约。
            let out0 = s.filter(b"__mist_cwd_report", now());
            assert!(out0.is_empty());
            // 第二块走偏（不是行终止/行编辑填充）：暂存字节 + 新数据全部放行。
            let out1 = s.filter(b"X and user output\r\n", now());
            assert_eq!(out1, b"__mist_cwd_reportX and user output\r\n".to_vec());
        }

        #[test]
        fn zsh_zle_swallows_newline_echo_still_stripped() {
            // zsh ZLE: payload 逐字回显但吞掉行尾 \n，后面跟 ZLE 重置序列与 \r\r\n。
            let mut s = EchoSuppressor::new();
            s.begin(b"__mist_cwd_report() { ... }; esac\n");
            let stream = b"__mist_cwd_report() { ... }; esac\x1b[?2004l\r\r\nprompt# ";
            let out = s.filter(&stream[..], now());
            assert_eq!(String::from_utf8_lossy(&out), "prompt# ");
        }

        #[test]
        fn no_newline_variant_not_stripped_before_printable() {
            // 无行尾变体不能在可打印字符前误剥——那是用户敲的文本。
            let mut s = EchoSuppressor::new();
            s.begin(b"__mist_cwd_report\n");
            let out0 = s.filter(b"__mist_cwd_report", now());
            assert!(out0.is_empty(), "完整载荷前缀先暂存");
            let out1 = s.filter(b"X and user output\r\n", now());
            assert_eq!(out1, b"__mist_cwd_reportX and user output\r\n".to_vec());
        }

        #[test]
        fn fish_line_editor_tail_stripped() {
            // fish 行编辑器：payload 回显 + \r + 光标前移序列 + \r\n。
            let mut s = EchoSuppressor::new();
            s.begin(b"function __mist_cwd_report --on-event fish_prompt; end\n");
            let stream = b"function __mist_cwd_report --on-event fish_prompt; end\r\x1b[63C\r\n\x1b]7;file://h/tmp\x1b\\prompt# ";
            let out = s.filter(&stream[..], now());
            assert_eq!(
                String::from_utf8_lossy(&out),
                "\x1b]7;file://h/tmp\x1b\\prompt# "
            );
        }

        #[test]
        fn fish_tail_with_printable_after_not_stripped() {
            // 行编辑填充后面若不是行终止而是可打印文本，不能剥。
            let mut s = EchoSuppressor::new();
            s.begin(b"function hook; end\n");
            let out0 = s.filter(b"function hook; end", now());
            // \r + CSI 后面不是行终止而是用户文本：不许剥。候选前缀先暂存，余量
            // 先行扫描放行；下一轮无新证据 → 走偏放行（保留队列）。
            let out1 = s.filter(b"\r\x1b[20Cuser text", now());
            let out2 = s.filter(b"", now());
            let mut all = out0.clone();
            all.extend(out1);
            all.extend(out2);
            assert_eq!(all, b"function hook; end\r\x1b[20Cuser text".to_vec());
        }

        #[test]
        fn fish_fragmented_redraw_stripped() {
            // fish 行编辑器碎片重绘：内容拼接等于载荷本体 + 分隔符 + 最终完整重绘。
            let mut s = EchoSuppressor::new();
            s.begin(b"function hook; end\n");
            let stream = b"function \r\x1b[9Chook; \r\x1b[15Cend\r\x1b[18Cfunction hook; end\r\x1b[18C\r\n\x1b]7;file://h/tmp\x1b\\prompt# ";
            let out = s.filter(&stream[..], now());
            assert_eq!(
                String::from_utf8_lossy(&out),
                "\x1b]7;file://h/tmp\x1b\\prompt# "
            );
        }

        #[test]
        fn fish_fragments_with_wrong_content_not_stripped() {
            // 碎片拼接不等于载荷本体：不能剥。
            let mut s = EchoSuppressor::new();
            s.begin(b"function hook; end\n");
            let stream =
                b"function \r\x1b[9CWRONG; end\r\x1b[18Cfunction hook; end\r\x1b[18C\r\nprompt# ";
            let out = s.filter(&stream[..], now());
            // 碎片内容对不上：碎片放行；但后面的完整逐字重绘就是真实回显，仍要剥。
            let out2 = s.filter(b"", now());
            let mut all = out.clone();
            all.extend(out2);
            assert_eq!(
                String::from_utf8_lossy(&all),
                "function \r\x1b[9CWRONG; end\r\x1b[18Cprompt# "
            );
        }

        #[test]
        fn fish_fragment_incomplete_waits_for_next_chunk() {
            // 碎片未拼完且缓冲截止：暂存等待，不误放行也不误剥。
            let mut s = EchoSuppressor::new();
            s.begin(b"function hook; end\n");
            let out0 = s.filter(b"function \r\x1b[9Chook; ", now());
            let out1 = s.filter(
                b"\r\x1b[15Cend\r\x1b[18Cfunction hook; end\r\x1b[18C\r\nprompt# ",
                now(),
            );
            // 碎片跨 chunk 拼接：可见输出只有 OSC 7 与提示符。
            let mut all = out0.clone();
            all.extend(out1);
            assert_eq!(String::from_utf8_lossy(&all), "prompt# ");
        }

        #[test]
        fn timeout_flushes_staged_pending_visible() {
            // review 复现用例：超时不能吞掉已暂存的候选前缀。
            let mut s = EchoSuppressor::new();
            s.begin(b"hook-line\n");
            let t0 = Instant::now();
            let out0 = s.filter(b"hook-li", t0);
            assert!(out0.is_empty());
            // 超时：tick 必须交回暂存字节，残余全部可见。
            let later = t0 + ECHO_SUPPRESS_WINDOW + std::time::Duration::from_millis(10);
            let flushed = s.tick(later);
            assert_eq!(
                flushed.as_ref().map(|v| v.as_slice()),
                Some(b"hook-li".as_slice())
            );
            // 超时后再来的数据原样放行。
            let out1 = s.filter(b"ne is visible\r\n", later);
            assert_eq!(out1, b"ne is visible\r\n".to_vec());
            assert!(!s.is_active());
        }

        #[test]
        fn timeout_with_empty_pending_flushes_nothing() {
            let mut s = EchoSuppressor::new();
            s.begin(b"hook-line\n");
            let later =
                Instant::now() + ECHO_SUPPRESS_WINDOW + std::time::Duration::from_millis(10);
            let flushed = s.tick(later);
            assert!(flushed.is_some());
            assert!(flushed.unwrap().is_empty());
            assert!(!s.is_active());
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
