//! SSH 窗格启动状态机：探测 → 短窗口等 OSC → 仅未见目录上报才注入 → 自动 cd → Ready。
//!
//! 本模块不依赖 UI，可单测。注入通道的论证见 [`InjectPlan`]：
//! - 不改 SSH 登录命令 / `--rcfile` / ForceCommand 包装（会改变
//!   `SSH_ORIGINAL_COMMAND`，破坏 git-shell、internal-sftp 等 ForceCommand）。
//! - 不把窗口内 `stty -echo` / 关内核 ECHO 当主修复（提示符后行编辑器往往已关
//!   内核回显，可见的是折行重绘；关 ECHO 还可能进历史、恢复失败伤密码回显）。
//! - 必须注入时：先把**远端 PTY 列宽**临时加到钩子长度以上（不改可见 VTE 网格），
//!   等 SIGWINCH 落定后再发钩子，行编辑器不再插入 `\r<` 折行标记；
//!   [`super::shell_integration::EchoSuppressor`] 剥线性回显作兜底；
//!   [`HookEchoGuard`] 再兜一层折行重绘碎片。三者都不是「清屏/关能力」。
//! - 本会话已经见到 OSC 7 / 9;9 / 1337 CurrentDir 则完全不注入。

use std::time::{Duration, Instant};

use super::shell_integration::{injection_command, ShellKind, HOOK_FN};

/// 探测完成且提示符已出现后，再等这么久看本会话是否已有 cwd OSC。
pub const OSC_WAIT: Duration = Duration::from_millis(400);
/// 远端 PTY 加宽后等待 SIGWINCH 被行编辑器消化。
pub const WIDEN_SETTLE: Duration = Duration::from_millis(80);
/// 整段启动（探测 + 等待 + 注入）的上限；到点进入 Degraded 并原样放行按键。
pub const STARTUP_BUDGET: Duration = Duration::from_secs(8);
/// 启动期独立按键队列上限。超过则中止启动并立刻放行，禁止丢键。
pub const KEY_QUEUE_CAP: usize = 4096;
/// `SshSessionHandle::resize_pty` 的列宽上限，加宽不能超过这个值。
pub const INJECT_PTY_COLS_MAX: u16 = 512;
/// 钩子正文两侧额外留白，避免刚好顶格。
pub const INJECT_PTY_COLS_MARGIN: u16 = 64;

/// 启动阶段。未 Ready / Degraded 时用户按键进独立短队列。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupPhase {
    /// 未连接或本机 shell，不走注入。
    Idle,
    /// 已发起探测，同时听 OSC 与提示符。
    Probing,
    /// 探测已完成，短窗口等待本会话 cwd OSC。
    WaitingForReport,
    /// 已决定注入，等待远端 PTY 加宽落定。
    Widening,
    /// 钩子（及同帧自动 cd）已发出，等待回显剥尽。
    AwaitingEcho,
    /// 钩子已处理或跳过，可以发自动 cd。
    StartingCd,
    /// 启动完成，按键原样放行。
    Ready,
    /// 探测失败 / 超时 / 用户 Ctrl+C 逃生。按键放行，不注入。
    Degraded,
}

/// `poll` 交给 UI 执行的动作。一次只返回一个，UI 做完后再 `poll`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartupAction {
    None,
    /// 只改远端 PTY 列宽，不要改可见 VTE 网格。
    WidenPty {
        cols: u16,
    },
    /// 把钩子写入 PTY（调用方负责补换行并打开回显抑制）。
    InjectHook {
        command: String,
    },
    /// 可以发自动 cd 了（若有）。
    SendStartupCd,
    /// 把远端 PTY 恢复为窗格的真实列宽。
    RestorePtySize,
    /// 把队列里的用户按键原样写入 PTY（禁止丢弃）。
    FlushKeys {
        bytes: Vec<u8>,
    },
}

/// 一次必须注入时的通道计划，便于测试与 PR 说明对齐。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectPlan {
    pub command: String,
    pub widen_cols: u16,
}

pub fn inject_pty_cols(hook_len: usize, current_cols: u16) -> u16 {
    let need = (hook_len as u32)
        .saturating_add(INJECT_PTY_COLS_MARGIN as u32)
        .min(INJECT_PTY_COLS_MAX as u32) as u16;
    current_cols.max(need).min(INJECT_PTY_COLS_MAX)
}

/// 提示符启发式，只作为状态机的一个输入，不再单独决定注入。
pub fn looks_like_prompt(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    t.starts_with("PS ")
        || t.ends_with('$')
        || t.ends_with('#')
        || t.ends_with('%')
        || t.ends_with('>')
}

/// 从输出字节流里识别 OSC 7 / 9;9 / 1337 CurrentDir（允许跨 chunk）。
#[derive(Debug, Default)]
pub struct CwdOscScanner {
    pending: Vec<u8>,
}

impl CwdOscScanner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.pending.clear();
    }

    /// 喂一段 PTY 输出。返回本段是否出现过目录上报 OSC。
    pub fn feed(&mut self, data: &[u8]) -> bool {
        if data.is_empty() && self.pending.is_empty() {
            return false;
        }
        self.pending.extend_from_slice(data);
        let found = stream_contains_cwd_osc(&self.pending);
        // 只保留可能构成 OSC 前缀的尾部，避免无限增长。
        // KEEP 必须覆盖「起始 ESC ]7; 之后仍无终止符」的最大安全前缀：
        // 长路径的 OSC 7 首片可以远超 32 字节，若只保留尾 32 字节，
        // 起始 ESC ]7; 被截掉，下一片的终止符到了也认不出完整序列，
        // 会被误判为「本会话未上报」而重复注入。保留自最近一次
        // 未终止 OSC 起始 ESC 起的全部尾部；没有未终止 OSC 才截到 32 字节。
        const KEEP: usize = 32;
        let keep_from = if has_unterminated_osc(&self.pending) {
            // 最近一个未终止 OSC 的起始 ESC；若找不到（理论上不可能），退回 KEEP。
            self.pending
                .iter()
                .rposition(|&b| b == 0x1b)
                .unwrap_or(self.pending.len().saturating_sub(KEEP))
        } else {
            let start = self.pending.len().saturating_sub(KEEP);
            self.pending[start..]
                .iter()
                .rposition(|&b| b == 0x1b)
                .map(|i| start + i)
                .unwrap_or(start)
        };
        self.pending.drain(..keep_from);
        if found {
            // 完整序列已经见过，不必再为它保留长缓冲。
            if !has_unterminated_osc(&self.pending) {
                self.pending.clear();
            }
        }
        found
    }
}

fn has_unterminated_osc(buf: &[u8]) -> bool {
    if let Some(i) = buf.iter().rposition(|&b| b == 0x1b) {
        if i + 1 < buf.len() && buf[i + 1] == b']' {
            return !buf[i + 2..].iter().any(|&b| b == 0x07 || b == 0x1b);
        }
    }
    false
}

pub fn stream_contains_cwd_osc(buf: &[u8]) -> bool {
    let mut i = 0;
    while i + 2 < buf.len() {
        if buf[i] != 0x1b || buf[i + 1] != b']' {
            i += 1;
            continue;
        }
        let rest = &buf[i + 2..];
        let end = rest.iter().position(|&b| b == 0x07).or_else(|| {
            rest.windows(2)
                .position(|w| w == [0x1b, b'\\'])
                .map(|p| p + 1)
        });
        let Some(end) = end else {
            break;
        };
        let payload = &rest[..end];
        if payload.starts_with(b"7;")
            || payload.starts_with(b"9;9;")
            || payload.starts_with(b"1337;CurrentDir=")
        {
            return true;
        }
        i += 2 + end + 1;
    }
    false
}

/// 注入窗口内的折行重绘兜底：按钩子的可打印字节顺序消费，跳过 CSI、
/// `\r`/`\n` 以及 zsh ZLE 风格的行首/行尾 `<>` 折行标记。
/// 不整段吞流；对不上的字节立即放行。
#[derive(Debug, Default)]
pub struct HookEchoGuard {
    /// 每项是「还没消费完的可打印载荷」。
    queue: Vec<Vec<u8>>,
    /// 当前载荷是否已经匹配到第一个可打印字节。未开始时不剥 MOTD 的换行/CSI。
    started: bool,
    deadline: Option<Instant>,
}

impl HookEchoGuard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_active(&self) -> bool {
        self.deadline.is_some()
    }

    pub fn reset(&mut self) {
        self.queue.clear();
        self.started = false;
        self.deadline = None;
    }

    pub fn begin(&mut self, payload: &[u8], now: Instant, window: Duration) {
        let printable = printable_payload(payload);
        if printable.is_empty() {
            return;
        }
        if self.deadline.is_none() {
            self.deadline = Some(now + window);
        }
        self.queue.push(printable);
    }

    pub fn tick(&mut self, now: Instant) {
        if self.deadline.is_some_and(|d| now >= d) {
            self.reset();
        }
    }

    pub fn filter(&mut self, data: &[u8], now: Instant) -> Vec<u8> {
        self.tick(now);
        if !self.is_active() {
            return data.to_vec();
        }
        let mut out = Vec::with_capacity(data.len());
        let mut i = 0;
        let mut just_wrap_edge = false;
        while i < data.len() {
            if self.queue.is_empty() {
                out.extend_from_slice(&data[i..]);
                self.deadline = None;
                break;
            }
            // CSI / ESC 序列：匹配中则剥掉（行编辑器重绘），否则放行。
            // OSC 序列（ESC ]）必须整段放行：目录上报可能恰好落在抑制窗口内，
            // 若在 started 状态把 ESC 当控制字节吞掉，OSC 会被拆碎，
            // 迟到的目录上报丢失 → 幂等/继承判断失效。OSC 不参与折行重绘。
            if data[i] == 0x1b {
                if data.len() > i + 1 && data[i + 1] == b']' {
                    let Some(end) = osc_seq_len(&data[i..]) else {
                        // OSC 被截断：剩余整段挂起，等下一块再处理。
                        out.extend_from_slice(&data[i..]);
                        break;
                    };
                    out.extend_from_slice(&data[i..i + end]);
                    i += end;
                    continue;
                }
                if let Some(n) = escape_len(&data[i..]) {
                    if self.started {
                        i += n;
                        continue;
                    }
                    out.extend_from_slice(&data[i..i + n]);
                    i += n;
                    continue;
                }
            }
            let b = data[i];
            if b == b'\r' || b == b'\n' {
                if self.started {
                    i += 1;
                    just_wrap_edge = true;
                    continue;
                }
                out.push(b);
                i += 1;
                just_wrap_edge = false;
                continue;
            }
            // 仅把紧挨行边界的 <> 当折行标记；载荷里的字符（若有）仍按顺序匹配。
            if self.started && (b == b'<' || b == b'>') {
                let at_edge = just_wrap_edge
                    || (i + 1 < data.len() && (data[i + 1] == b'\r' || data[i + 1] == b'\n'));
                if at_edge {
                    i += 1;
                    just_wrap_edge = false;
                    continue;
                }
            }
            just_wrap_edge = false;
            if b < 0x20 {
                if self.started {
                    i += 1;
                    continue;
                }
                out.push(b);
                i += 1;
                continue;
            }
            let expected = self.queue[0][0];
            if b == expected {
                self.started = true;
                self.queue[0].remove(0);
                if self.queue[0].is_empty() {
                    self.queue.remove(0);
                    self.started = false;
                    if self.queue.is_empty() {
                        self.deadline = None;
                    }
                }
                i += 1;
                continue;
            }
            // 对不上：这个字节与当前载荷无关，放行后继续扫。
            out.push(b);
            i += 1;
        }
        out
    }
}

fn printable_payload(payload: &[u8]) -> Vec<u8> {
    payload
        .iter()
        .copied()
        .filter(|&b| b >= 0x20 && b != 0x7f)
        .collect()
}

/// OSC 序列长度：`ESC ]` 起，`BEL` 或 `ESC \` 收；未终止返回 None。
fn osc_seq_len(data: &[u8]) -> Option<usize> {
    if data.len() < 2 || data[0] != 0x1b || data[1] != b']' {
        return None;
    }
    let mut j = 2;
    while j < data.len() {
        if data[j] == 0x07 {
            return Some(j + 1);
        }
        if data[j] == 0x1b {
            if j + 1 < data.len() && data[j + 1] == b'\\' {
                return Some(j + 2);
            }
            // OSC 里出现另一个 ESC 但不是 ST：按新序列开始，终止在前面。
            return Some(j);
        }
        j += 1;
    }
    None
}

fn escape_len(data: &[u8]) -> Option<usize> {
    if data.is_empty() || data[0] != 0x1b {
        return None;
    }
    if data.len() == 1 {
        return Some(1);
    }
    if data[1] == b'[' {
        let mut j = 2;
        while j < data.len() && (data[j].is_ascii_digit() || data[j] == b';' || data[j] == b'?') {
            j += 1;
        }
        if j < data.len() && (0x40..=0x7e).contains(&data[j]) {
            return Some(j + 1);
        }
        return Some(data.len()); // 截断，先整段挂起由下次补；这里消费到末尾以免把 '[' 当正文。
    }
    if data[1] == b']' {
        // OSC 由 filter 里的 osc_seq_len 分支整段放行，不会走到这里。
        return None;
    }
    Some(2)
}

/// 构造窄终端下 zsh ZLE 风格的折行回显（含 `\r<`），供夹具使用。
pub fn simulate_zle_wrap(body: &[u8], cols: usize) -> Vec<u8> {
    let printable = printable_payload(body);
    let cols = cols.max(2);
    let mut out = Vec::new();
    let mut col = 0usize;
    for b in printable.iter().copied() {
        if col == cols {
            out.extend_from_slice(b"\r<");
            col = 1;
        }
        out.push(b);
        col += 1;
    }
    out.extend_from_slice(b"\r\n");
    out
}

/// 可见网格/文本不得出现的内部脚本痕迹。
pub fn visible_text_leaks_hook(text: &str) -> bool {
    text.contains(HOOK_FN)
        || text.contains("PROMPT_COMMAND=\"__mist")
        || text.contains("precmd_functions+=(__mist")
        || text.contains("function __mist_cwd_report")
}

#[derive(Debug)]
pub struct StartupController {
    phase: StartupPhase,
    hook_enabled: bool,
    seen_cwd_osc: bool,
    prompt_seen: bool,
    probe_done: bool,
    shell: Option<ShellKind>,
    cd_pending: bool,
    started_at: Option<Instant>,
    report_deadline: Option<Instant>,
    widen_deadline: Option<Instant>,
    current_cols: u16,
    key_queue: Vec<u8>,
    pending_hook: Option<String>,
    widen_cols: Option<u16>,
    /// 已向 UI 发出过 Widen/Inject/Restore/SendCd，避免重复。
    widen_requested: bool,
    hook_sent: bool,
    restore_requested: bool,
    flushed_on_ready: bool,
}

impl Default for StartupController {
    fn default() -> Self {
        Self::new()
    }
}

impl StartupController {
    pub fn new() -> Self {
        Self {
            phase: StartupPhase::Idle,
            hook_enabled: true,
            seen_cwd_osc: false,
            prompt_seen: false,
            probe_done: false,
            shell: None,
            cd_pending: false,
            started_at: None,
            report_deadline: None,
            widen_deadline: None,
            current_cols: 80,
            key_queue: Vec::new(),
            pending_hook: None,
            widen_cols: None,
            widen_requested: false,
            hook_sent: false,
            restore_requested: false,
            flushed_on_ready: false,
        }
    }

    pub fn phase(&self) -> StartupPhase {
        self.phase
    }

    pub fn hook_enabled(&self) -> bool {
        self.hook_enabled
    }

    /// 供诊断/测试：状态机内部是否持有待注入的钩子文本。
    /// 产品总闸关闭时必须恒为 false，避免「探测结果迟到仍注入」。
    pub fn has_pending_hook(&self) -> bool {
        self.pending_hook.is_some()
    }

    pub fn set_hook_enabled(&mut self, on: bool) {
        self.hook_enabled = on;
    }

    pub fn set_current_cols(&mut self, cols: u16) {
        self.current_cols = cols.max(1);
    }

    pub fn set_cd_pending(&mut self, pending: bool) {
        self.cd_pending = pending;
        if !pending && self.phase == StartupPhase::StartingCd {
            self.phase = StartupPhase::Ready;
        }
    }

    pub fn seen_cwd_osc(&self) -> bool {
        self.seen_cwd_osc
    }

    pub fn is_complete(&self) -> bool {
        matches!(
            self.phase,
            StartupPhase::Ready | StartupPhase::Degraded | StartupPhase::Idle
        )
    }

    pub fn queues_user_input(&self) -> bool {
        !matches!(
            self.phase,
            StartupPhase::Idle | StartupPhase::Ready | StartupPhase::Degraded
        )
    }

    pub fn reset(&mut self) {
        let hook_enabled = self.hook_enabled;
        let cols = self.current_cols;
        *self = Self::new();
        self.hook_enabled = hook_enabled;
        self.current_cols = cols;
    }

    /// 交互壳已起来，开始探测。重连必须再走一遍。
    pub fn on_shell_started(&mut self, now: Instant) {
        let cd_pending = self.cd_pending;
        self.reset();
        self.cd_pending = cd_pending;
        self.phase = StartupPhase::Probing;
        self.started_at = Some(now);
    }

    pub fn on_cursor_line(&mut self, line: &str) {
        if looks_like_prompt(line) {
            self.prompt_seen = true;
            self.maybe_arm_report_deadline(Instant::now());
        }
    }

    pub fn on_cursor_line_at(&mut self, line: &str, now: Instant) {
        if looks_like_prompt(line) {
            self.prompt_seen = true;
            self.maybe_arm_report_deadline(now);
        }
    }

    pub fn on_cwd_osc(&mut self) {
        self.seen_cwd_osc = true;
    }

    pub fn on_probe(&mut self, shell: ShellKind, now: Instant) {
        self.probe_done = true;
        self.shell = Some(shell);
        if self.hook_enabled {
            if let Some(cmd) = injection_command(shell) {
                self.pending_hook = Some(cmd.to_string());
            }
        }
        if self.phase == StartupPhase::Probing {
            self.phase = StartupPhase::WaitingForReport;
        }
        self.maybe_arm_report_deadline(now);
    }

    pub fn on_probe_failed(&mut self, now: Instant) {
        self.probe_done = true;
        self.shell = Some(ShellKind::Unknown);
        self.pending_hook = None;
        if self.phase == StartupPhase::Probing {
            self.phase = StartupPhase::WaitingForReport;
        }
        self.maybe_arm_report_deadline(now);
    }

    fn maybe_arm_report_deadline(&mut self, now: Instant) {
        if self.report_deadline.is_some() {
            return;
        }
        if self.probe_done && self.prompt_seen && self.phase == StartupPhase::WaitingForReport {
            self.report_deadline = Some(now + OSC_WAIT);
        }
    }

    /// 用户按键。返回需要立刻写入 PTY 的字节（逃生或已完成时原样）。
    /// 队列将满时把已有队列连同本次一并交出并进入 Degraded，禁止丢键。
    pub fn push_user_input(&mut self, data: &[u8]) -> Option<Vec<u8>> {
        if data.is_empty() {
            return None;
        }
        if !self.queues_user_input() {
            return Some(data.to_vec());
        }
        if data.contains(&0x03) {
            let mut out = std::mem::take(&mut self.key_queue);
            out.extend_from_slice(data);
            self.abort(StartupPhase::Degraded);
            return Some(out);
        }
        if self.key_queue.len().saturating_add(data.len()) > KEY_QUEUE_CAP {
            let mut out = std::mem::take(&mut self.key_queue);
            out.extend_from_slice(data);
            self.abort(StartupPhase::Degraded);
            return Some(out);
        }
        self.key_queue.extend_from_slice(data);
        None
    }

    pub fn take_queued_keys(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.key_queue)
    }

    pub fn has_queued_keys(&self) -> bool {
        !self.key_queue.is_empty()
    }

    /// 回显抑制窗口已关闭（成功剥完或超时）。
    pub fn on_echo_finished(&mut self) {
        if self.phase == StartupPhase::AwaitingEcho {
            self.phase = if self.cd_pending {
                StartupPhase::StartingCd
            } else {
                StartupPhase::Ready
            };
        }
    }

    fn abort(&mut self, phase: StartupPhase) {
        self.phase = phase;
        self.pending_hook = None;
        self.report_deadline = None;
        self.widen_deadline = None;
    }

    pub fn inject_plan(&self) -> Option<InjectPlan> {
        let command = self.pending_hook.clone()?;
        let cols = inject_pty_cols(command.len() + 1, self.current_cols);
        Some(InjectPlan {
            command,
            widen_cols: cols,
        })
    }

    fn should_inject(&self) -> bool {
        self.hook_enabled && !self.seen_cwd_osc && self.pending_hook.is_some() && !self.hook_sent
    }

    pub fn next_deadline(&self, now: Instant) -> Option<Duration> {
        let mut soonest: Option<Instant> = None;
        for t in [
            self.report_deadline,
            self.widen_deadline,
            self.started_at.map(|t| t + STARTUP_BUDGET),
        ] {
            if let Some(d) = t {
                if d > now {
                    soonest = Some(soonest.map_or(d, |s| s.min(d)));
                }
            }
        }
        // `abort(Degraded)` 会清掉 report/widen deadline，而 STARTUP_BUDGET 已过；
        // 若此时还有 cd / 恢复列宽 / 排队按键没发，必须给 UI 一个短唤醒，
        // 否则「连上后空等、只按 deadline 再 poll」时这些动作永远发不出去。
        if soonest.is_none() && self.phase == StartupPhase::Degraded {
            if self.cd_pending
                || (self.widen_requested && !self.restore_requested)
                || (!self.flushed_on_ready && !self.key_queue.is_empty())
            {
                return Some(Duration::from_millis(50));
            }
        }
        soonest.map(|d| d.saturating_duration_since(now))
    }

    pub fn poll(&mut self, now: Instant) -> StartupAction {
        if matches!(self.phase, StartupPhase::Idle) {
            return StartupAction::None;
        }
        if let Some(start) = self.started_at {
            if now >= start + STARTUP_BUDGET
                && !matches!(self.phase, StartupPhase::Ready | StartupPhase::Degraded)
            {
                self.abort(StartupPhase::Degraded);
                let bytes = self.take_queued_keys();
                if !bytes.is_empty() {
                    // 先交还按键；`advance_startup` 的循环下一拍会再 poll，
                    // 由下方 Degraded 臂发 cd；`next_deadline` 也会给短唤醒兜底。
                    return StartupAction::FlushKeys { bytes };
                }
                // 空队列：不要提前 return None，让控制流落入下面 match 的
                // Degraded 臂，同一次 poll 就把 RestorePtySize / SendStartupCd 发出去。
            }
        }

        // 已完成：把还没冲出去的按键交给 UI。
        if matches!(self.phase, StartupPhase::Ready | StartupPhase::Degraded)
            && !self.flushed_on_ready
            && !self.key_queue.is_empty()
        {
            self.flushed_on_ready = true;
            return StartupAction::FlushKeys {
                bytes: self.take_queued_keys(),
            };
        }

        match self.phase {
            StartupPhase::Idle => StartupAction::None,
            StartupPhase::Probing => StartupAction::None,
            StartupPhase::WaitingForReport => {
                if self.seen_cwd_osc || !self.hook_enabled || self.pending_hook.is_none() {
                    // 不用等满窗口：已经确定不会注入。
                    if self.prompt_seen && self.probe_done {
                        return self.enter_after_skip();
                    }
                    return StartupAction::None;
                }
                let Some(deadline) = self.report_deadline else {
                    return StartupAction::None;
                };
                if now < deadline {
                    return StartupAction::None;
                }
                if self.should_inject() {
                    self.phase = StartupPhase::Widening;
                    if let Some(plan) = self.inject_plan() {
                        self.widen_cols = Some(plan.widen_cols);
                    }
                    self.widen_deadline = Some(now + WIDEN_SETTLE);
                    self.widen_requested = true;
                    let cols = self.widen_cols.unwrap_or(INJECT_PTY_COLS_MAX);
                    StartupAction::WidenPty { cols }
                } else {
                    self.enter_after_skip()
                }
            }
            StartupPhase::Widening => {
                if !self.widen_requested {
                    self.widen_requested = true;
                    self.widen_deadline = Some(now + WIDEN_SETTLE);
                    let cols = self.widen_cols.unwrap_or(INJECT_PTY_COLS_MAX);
                    return StartupAction::WidenPty { cols };
                }
                if self.widen_deadline.is_some_and(|d| now < d) {
                    return StartupAction::None;
                }
                if let Some(cmd) = self.pending_hook.clone() {
                    self.hook_sent = true;
                    self.phase = StartupPhase::AwaitingEcho;
                    StartupAction::InjectHook { command: cmd }
                } else {
                    self.enter_after_skip()
                }
            }
            StartupPhase::AwaitingEcho => {
                // 同帧自动 cd：钩子已发出后立刻让 UI 把 cd 追加进抑制队列，
                // 仍处在加宽窗口内，避免 cd 行在窄终端折行。
                // 是否真正发出由 UI 在成功/放弃后调用 `set_cd_pending(false)`；
                // 身份未齐时保持 pending，下次 poll 再试。
                if self.cd_pending {
                    return StartupAction::SendStartupCd;
                }
                StartupAction::None
            }
            StartupPhase::StartingCd => {
                // 先尽量把自动 cd 发掉，再恢复列宽，避免 cd 行在窄终端折行。
                if self.cd_pending {
                    return StartupAction::SendStartupCd;
                }
                if !self.restore_requested && self.widen_requested {
                    self.restore_requested = true;
                    return StartupAction::RestorePtySize;
                }
                self.phase = StartupPhase::Ready;
                if !self.flushed_on_ready && !self.key_queue.is_empty() {
                    self.flushed_on_ready = true;
                    return StartupAction::FlushKeys {
                        bytes: self.take_queued_keys(),
                    };
                }
                StartupAction::None
            }
            StartupPhase::Ready | StartupPhase::Degraded => {
                if self.widen_requested && !self.restore_requested {
                    self.restore_requested = true;
                    return StartupAction::RestorePtySize;
                }
                // Degraded（超时 / Ctrl+C / 队列满）时目录继承不能被跳过：
                // 自动 cd 只是把用户带回会话目录，与钩子可见性无关；
                // 启动失败时更要照常发（身份已探到才有 pending），
                // 且此时按键已放行，不再有折行抑制需求。
                if self.phase == StartupPhase::Degraded && self.cd_pending {
                    // 不提前清旗：UI 的 `maybe_send_startup_cd` 在身份未齐或发送
                    // 失败时会保留 pending_startup_cd，成功后才 set_cd_pending(false)。
                    // 这里清掉会导致身份迟到时这次继承永久丢失（与 StartingCd
                    // / AwaitingEcho 两臂保持一致的语义）。
                    return StartupAction::SendStartupCd;
                }
                StartupAction::None
            }
        }
    }

    fn enter_after_skip(&mut self) -> StartupAction {
        self.pending_hook = None;
        self.phase = if self.cd_pending {
            StartupPhase::StartingCd
        } else {
            StartupPhase::Ready
        };
        if self.phase == StartupPhase::StartingCd {
            StartupAction::SendStartupCd
        } else if self.phase == StartupPhase::Ready
            && !self.flushed_on_ready
            && !self.key_queue.is_empty()
        {
            self.flushed_on_ready = true;
            StartupAction::FlushKeys {
                bytes: self.take_queued_keys(),
            }
        } else {
            StartupAction::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::shell_integration::{
        injection_command, EchoSuppressor, ECHO_SUPPRESS_WINDOW,
    };
    use crate::terminal::Terminal;

    fn t0() -> Instant {
        Instant::now()
    }

    #[test]
    fn unknown_or_disabled_or_seen_osc_never_injects() {
        let now = t0();
        let mut c = StartupController::new();
        c.on_shell_started(now);
        c.on_probe(ShellKind::Unknown, now);
        c.on_cursor_line_at("user@host:~$ ", now);
        assert_eq!(c.poll(now + OSC_WAIT), StartupAction::None);
        // unknown：没有 pending hook，应跳到 Ready
        let _ = c.poll(now + OSC_WAIT);
        assert!(!c.should_inject());
        assert!(c.pending_hook.is_none());

        let mut c = StartupController::new();
        c.set_hook_enabled(false);
        c.on_shell_started(now);
        c.on_probe(ShellKind::Bash, now);
        c.on_cursor_line_at("user@host:~$ ", now);
        c.poll(now);
        assert!(c.pending_hook.is_none());
        assert_ne!(
            c.poll(now),
            StartupAction::InjectHook {
                command: String::new()
            }
        );
        assert!(matches!(
            c.phase(),
            StartupPhase::Ready | StartupPhase::StartingCd
        ));

        let mut c = StartupController::new();
        c.on_shell_started(now);
        c.on_cwd_osc();
        c.on_probe(ShellKind::Bash, now);
        c.on_cursor_line_at("user@host:~$ ", now);
        let act = c.poll(now);
        match act {
            StartupAction::InjectHook { .. } | StartupAction::WidenPty { .. } => {
                panic!("已有 OSC 还要注入: {act:?}")
            }
            _ => {}
        }
        assert!(!c.should_inject());
    }

    #[test]
    fn waits_for_osc_before_inject() {
        let now = t0();
        let mut c = StartupController::new();
        c.on_shell_started(now);
        c.on_probe(ShellKind::Bash, now);
        c.on_cursor_line_at("host$ ", now);
        assert_eq!(c.poll(now), StartupAction::None, "窗口未到不应注入");
        // 窗口内来了 OSC：取消注入。
        c.on_cwd_osc();
        let act = c.poll(now + OSC_WAIT);
        match act {
            StartupAction::WidenPty { .. } | StartupAction::InjectHook { .. } => panic!("{act:?}"),
            _ => {}
        }
    }

    #[test]
    fn injects_after_wait_when_no_osc() {
        let now = t0();
        let mut c = StartupController::new();
        c.set_current_cols(40);
        c.on_shell_started(now);
        c.on_probe(ShellKind::Zsh, now);
        c.on_cursor_line_at("host% ", now);
        assert_eq!(c.poll(now), StartupAction::None);
        let act = c.poll(now + OSC_WAIT);
        match act {
            StartupAction::WidenPty { cols } => {
                let hook = injection_command(ShellKind::Zsh).unwrap();
                assert!(
                    cols as usize >= hook.len(),
                    "加宽必须超过钩子长度，got {cols}"
                );
                assert!(cols <= INJECT_PTY_COLS_MAX);
            }
            other => panic!("expected widen, got {other:?}"),
        }
        assert_eq!(c.poll(now + OSC_WAIT), StartupAction::None, "加宽未落定");
        let act = c.poll(now + OSC_WAIT + WIDEN_SETTLE);
        match act {
            StartupAction::InjectHook { command } => {
                assert!(command.contains(HOOK_FN));
            }
            other => panic!("expected inject, got {other:?}"),
        }
    }

    #[test]
    fn keys_queued_then_flushed_in_order_ctrl_c_escapes() {
        let now = t0();
        let mut c = StartupController::new();
        c.on_shell_started(now);
        assert!(c.queues_user_input());
        assert!(c.push_user_input(b"ls").is_none());
        assert!(c.push_user_input(b" -l").is_none());
        let flushed = c.push_user_input(&[0x03]).expect("ctrl-c 应立即放行");
        assert_eq!(flushed, b"ls -l\x03".to_vec());
        assert_eq!(c.phase(), StartupPhase::Degraded);
        assert!(!c.queues_user_input());
        // 之后的按键不再排队。
        assert_eq!(c.push_user_input(b"x"), Some(b"x".to_vec()));
    }

    #[test]
    fn queue_overflow_flushes_instead_of_dropping() {
        let now = t0();
        let mut c = StartupController::new();
        c.on_shell_started(now);
        let chunk = vec![b'a'; KEY_QUEUE_CAP];
        assert!(c.push_user_input(&chunk).is_none());
        let flushed = c.push_user_input(b"Z").expect("满了必须放行");
        assert_eq!(flushed.len(), KEY_QUEUE_CAP + 1);
        assert_eq!(*flushed.last().unwrap(), b'Z');
        assert_eq!(c.phase(), StartupPhase::Degraded);
    }

    #[test]
    fn overall_timeout_degrades_and_flushes() {
        let now = t0();
        let mut c = StartupController::new();
        c.on_shell_started(now);
        c.push_user_input(b"pwd");
        let act = c.poll(now + STARTUP_BUDGET);
        match act {
            StartupAction::FlushKeys { bytes } => assert_eq!(bytes, b"pwd".to_vec()),
            other => panic!("{other:?}"),
        }
        assert_eq!(c.phase(), StartupPhase::Degraded);
    }

    #[test]
    fn reconnect_resets_and_requires_full_path() {
        let now = t0();
        let mut c = StartupController::new();
        c.on_shell_started(now);
        c.on_cwd_osc();
        c.on_probe(ShellKind::Bash, now);
        c.on_cursor_line_at("$ ", now);
        let _ = c.poll(now);
        assert!(c.seen_cwd_osc());
        c.on_shell_started(now + Duration::from_secs(1));
        assert_eq!(c.phase(), StartupPhase::Probing);
        assert!(!c.seen_cwd_osc());
        assert!(c.queues_user_input());
    }

    #[test]
    fn hook_and_cd_same_frame_action_order() {
        let now = t0();
        let mut c = StartupController::new();
        c.set_cd_pending(true);
        c.set_current_cols(80);
        c.on_shell_started(now);
        c.on_probe(ShellKind::Fish, now);
        c.on_cursor_line_at("you@host> ", now);
        let _ = c.poll(now + OSC_WAIT); // widen
        let act = c.poll(now + OSC_WAIT + WIDEN_SETTLE);
        assert!(matches!(act, StartupAction::InjectHook { .. }));
        let act = c.poll(now + OSC_WAIT + WIDEN_SETTLE);
        assert_eq!(act, StartupAction::SendStartupCd, "同帧追加 cd");
        c.set_cd_pending(false);
        c.on_echo_finished();
        let act = c.poll(now + OSC_WAIT + WIDEN_SETTLE);
        assert_eq!(act, StartupAction::RestorePtySize);
    }

    #[test]
    fn looks_like_prompt_examples() {
        assert!(looks_like_prompt("user@host:~$ "));
        assert!(looks_like_prompt("root#"));
        assert!(looks_like_prompt("host% "));
        assert!(looks_like_prompt("PS C:\\Users>"));
        assert!(!looks_like_prompt(""));
        assert!(!looks_like_prompt("Last login: Mon Nov 10"));
    }

    #[test]
    fn scanner_keeps_unterminated_osc_across_long_first_chunk() {
        // 回归（review 软注 1）：OSC 7 长路径首片 >32 字节且无终止符时，
        // 不能把起始 ESC ]7; 截掉，否则第二片终止符到来也认不出，
        // 会被误判为「本会话未上报」而重复注入。
        let osc = b"\x1b]7;file://very-long-hostname-aaaaaaaaaaaaaaaaaaaaaa/long/path\x1b\\";
        let (first, second) = osc.split_at(40);
        let mut s = CwdOscScanner::new();
        assert!(!s.feed(b"user@host$ "), "首片无终止符不算上报");
        assert!(!s.feed(first));
        assert!(s.feed(second), "长路径 OSC 跨 chunk 必须能被识别");
        // 下一 chunk 不应再误判（pending 已清）
        assert!(!s.feed(b"prompt$ "));
    }

    #[test]
    fn guard_passes_osc_intact_even_mid_match() {
        // 回归（review 软注 2）：guard 匹配中到达的 OSC 必须整段放行，
        // 否则迟到的目录上报被拆碎，幂等/继承判断失效。
        let osc = b"\x1b]7;file://h/tmp\x1b\\prompt$ ";
        let mut g = HookEchoGuard::new();
        g.begin(b"__mist_cwd_report() { printf", Instant::now(), ECHO_SUPPRESS_WINDOW);
        let out = g.filter(osc, Instant::now());
        assert_eq!(String::from_utf8_lossy(&out), String::from_utf8_lossy(osc));
    }

    #[test]
    fn degraded_still_sends_startup_cd() {
        // 回归（review 软注 3）：超时 / Ctrl+C / 队列满进入 Degraded 后，
        // 目录继承（自动 cd）不能被跳过。
        let now = t0();
        let mut c = StartupController::new();
        c.set_cd_pending(true);
        c.on_shell_started(now);
        let act = c.poll(now + STARTUP_BUDGET);
        assert_eq!(c.phase(), StartupPhase::Degraded);
        match act {
            StartupAction::SendStartupCd => {}
            other => panic!("Degraded 后应先发自动 cd，got {other:?}"),
        }
        c.set_cd_pending(false);
        let act = c.poll(now + STARTUP_BUDGET + Duration::from_millis(50));
        assert!(matches!(act, StartupAction::RestorePtySize | StartupAction::None));
    }

    #[test]
    fn timeout_with_queued_keys_flushes_then_sends_cd() {
        // 回归（review 阻塞项）：超时且队列非空时先 FlushKeys，
        // 下一拍仍必须发出 SendStartupCd；next_deadline 也要能把 UI 唤醒。
        let now = t0();
        let mut c = StartupController::new();
        c.set_cd_pending(true);
        c.on_shell_started(now);
        c.push_user_input(b"pwd");
        let act = c.poll(now + STARTUP_BUDGET);
        match act {
            StartupAction::FlushKeys { bytes } => assert_eq!(bytes, b"pwd".to_vec()),
            other => panic!("应先交还排队按键, got {other:?}"),
        }
        assert_eq!(c.phase(), StartupPhase::Degraded);
        // abort 清掉了 report/widen deadline，STARTUP_BUDGET 已过；
        // 仍有 cd_pending，必须给一个短唤醒，否则空闲连接上不再 poll。
        assert!(
            c.next_deadline(now + STARTUP_BUDGET).is_some(),
            "Degraded 且有 pending 动作时必须还能唤醒 UI"
        );
        let act = c.poll(now + STARTUP_BUDGET + Duration::from_millis(50));
        assert_eq!(act, StartupAction::SendStartupCd);
    }

    #[test]
    fn degraded_does_not_clear_cd_flag_before_ui_sends() {
        // 身份未齐时 maybe_send_startup_cd 会保留 pending_startup_cd；
        // 控制器不能在发动作时提前清 cd_pending，否则这次继承永久丢失。
        let now = t0();
        let mut c = StartupController::new();
        c.set_cd_pending(true);
        c.on_shell_started(now);
        assert_eq!(c.poll(now + STARTUP_BUDGET), StartupAction::SendStartupCd);
        assert!(c.cd_pending, "cd flag must survive until UI confirms send");
        assert!(c.next_deadline(now + STARTUP_BUDGET).is_some());
    }

    #[test]
    fn cwd_osc_scanner_detects_all_flavors_and_splits() {
        let mut s = CwdOscScanner::new();
        assert!(s.feed(b"\x1b]7;file://h/tmp\x1b\\prompt$ "));
        let mut s = CwdOscScanner::new();
        assert!(s.feed(b"\x1b]9;9;/srv/app\x07"));
        let mut s = CwdOscScanner::new();
        assert!(s.feed(b"\x1b]1337;CurrentDir=/opt\x07"));
        let mut s = CwdOscScanner::new();
        assert!(!s.feed(b"\x1b]7;file://h/tm"));
        assert!(s.feed(b"p\x1b\\"));
        let mut s = CwdOscScanner::new();
        assert!(!s.feed(b"\x1b]0;title\x07Last login\r\n"));
    }

    #[test]
    fn inject_cols_exceeds_hook_and_respects_max() {
        let hook = injection_command(ShellKind::Bash).unwrap();
        let cols = inject_pty_cols(hook.len() + 1, 40);
        assert!(cols as usize > hook.len());
        assert_eq!(inject_pty_cols(80, 200), 200);
        assert_eq!(inject_pty_cols(9000, 40), INJECT_PTY_COLS_MAX);
    }

    fn grid_of(cols: usize, rows: usize, chunks: &[&[u8]]) -> String {
        let mut term = Terminal::new(cols, rows);
        for c in chunks {
            term.feed(c);
        }
        let mut text = term.get_formatted_output();
        text.push('\n');
        text.push_str(&term.tail_plain_text(rows * 2));
        text
    }

    fn assert_clean(label: &str, text: &str) {
        assert!(
            !visible_text_leaks_hook(text),
            "{label} leaked hook in visible text:\n{text}"
        );
    }

    #[test]
    fn narrow_and_split_grids_hide_linear_and_wrapped_hook_echo() {
        for cols in [80usize, 40, 20] {
            for kind in [ShellKind::Bash, ShellKind::Zsh, ShellKind::Fish] {
                let hook = injection_command(kind).unwrap();
                let mut payload = hook.as_bytes().to_vec();
                payload.push(b'\n');
                let osc_prompt = b"\x1b]7;file://h/tmp\x1b\\user@host:~$ ";
                let login = b"Last login: Mon Nov 10 from 1.2.3.4\r\nuser@host:~$ ";

                // 线性回显（加宽 PTY 后的形态）+ EchoSuppressor。
                let mut sup = EchoSuppressor::new();
                sup.begin(&payload);
                let mut stream = login.to_vec();
                stream.extend_from_slice(&payload[..payload.len() - 1]);
                stream.extend_from_slice(b"\r\n");
                stream.extend_from_slice(osc_prompt);
                let filtered = sup.filter(&stream, Instant::now());
                let text = grid_of(cols, 12, &[&filtered]);
                assert_clean(&format!("{kind:?} linear cols={cols}"), &text);
                assert!(
                    stream_contains_cwd_osc(&filtered),
                    "{kind:?} OSC 7 must pass through, filtered={:?}",
                    String::from_utf8_lossy(&filtered)
                );
                assert!(
                    text.contains("Last login") || filtered.windows(10).any(|w| w == b"Last login"),
                    "MOTD must remain"
                );

                // 同帧 hook + cd。
                let cd = b"cd -- '/srv/app'\r";
                let mut sup = EchoSuppressor::new();
                sup.begin(&payload);
                sup.begin(cd);
                let mut stream = payload[..payload.len() - 1].to_vec();
                stream.extend_from_slice(b"\r\n");
                stream.extend_from_slice(b"cd -- '/srv/app'\r\n");
                stream.extend_from_slice(osc_prompt);
                let filtered = sup.filter(&stream, Instant::now());
                let text = grid_of(cols, 12, &[&filtered]);
                assert_clean(&format!("{kind:?} hook+cd cols={cols}"), &text);

                // 带 `\r<` 的 ZLE 折行重绘：EchoSuppressor 不够时由 HookEchoGuard 兜住。
                let wrapped = simulate_zle_wrap(&payload, cols);
                let mut guard = HookEchoGuard::new();
                guard.begin(&payload, Instant::now(), ECHO_SUPPRESS_WINDOW);
                let mut stream = login.to_vec();
                stream.extend_from_slice(&wrapped);
                stream.extend_from_slice(osc_prompt);
                let filtered = guard.filter(&stream, Instant::now());
                let text = grid_of(cols, 16, &[&filtered]);
                assert_clean(&format!("{kind:?} zle-wrap cols={cols}"), &text);
                assert!(
                    stream_contains_cwd_osc(&filtered),
                    "{kind:?} wrap path must keep OSC 7"
                );
            }
        }
    }

    #[test]
    fn hook_echo_guard_passes_motd_errors_and_user_output() {
        let hook = injection_command(ShellKind::Bash).unwrap();
        let mut payload = hook.as_bytes().to_vec();
        payload.push(b'\n');
        let mut guard = HookEchoGuard::new();
        guard.begin(&payload, Instant::now(), ECHO_SUPPRESS_WINDOW);
        let out = guard.filter(
            b"Welcome to Ubuntu 22.04\r\nPermission denied\r\n",
            Instant::now(),
        );
        assert_eq!(
            out,
            b"Welcome to Ubuntu 22.04\r\nPermission denied\r\n".to_vec()
        );
        // 窗口关闭后用户输出原样放行。
        guard.reset();
        let out = guard.filter(b"ls\r\nfile1 file2\r\n", Instant::now());
        assert_eq!(out, b"ls\r\nfile1 file2\r\n".to_vec());
    }

    #[test]
    fn hook_echo_guard_does_not_strip_user_text_that_merely_starts_similar() {
        let mut guard = HookEchoGuard::new();
        guard.begin(b"__mist_cwd_report\n", Instant::now(), ECHO_SUPPRESS_WINDOW);
        let out = guard.filter(b"__mist_cwd_reportX user\r\n", Instant::now());
        // 前缀匹配会先吞掉 sentinel，随后 X 对不上应放行。这是可接受的：
        // 启动窗口内用户按键被排队，不会同时敲出这个函数名。
        // 这里只断言不会把后面的用户文本整段吞掉。
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains('X') && s.contains("user"), "got {s:?}");
    }
}
