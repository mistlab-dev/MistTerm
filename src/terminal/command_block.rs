//! OSC 133 / OSC 633 命令块（#160）。
//!
//! ## 解析子集
//!
//! 只认 FinalTerm / iTerm2 / 常见 bash `PROMPT_COMMAND`、zsh `precmd`/`preexec`
//! 使用的语义标记，以及 VS Code shell integration 的同形 OSC 633。
//! 结束符是 BEL (`0x07`) 或 ST (`ESC \`)：
//!
//! | 标记 | 序列 | 含义 |
//! |------|------|------|
//! | A | `OSC 133 ; A` 或 `OSC 633 ; A` | 提示符开始 |
//! | B | `OSC 133 ; B` 或 `OSC 633 ; B` | 提示符结束、命令输入开始 |
//! | C | `OSC 133 ; C` 或 `OSC 633 ; C` | 命令已提交、输出开始 |
//! | D | `OSC 133 ; D [; 退出码]` 或 `OSC 633 ; D [; 退出码]` | 命令结束 |
//!
//! 字母后的附加参数（iTerm `cl=` / `aid=` 等）忽略。`D` 的第一段十进制数记为退出码。
//! 不认 OSC 1337（iTerm 专有属性）、OSC 133 ; P、标题 OSC 0/2。
//!
//! ## 块模型
//!
//! 块坐标是主屏幕网格上的点（alacritty `Line`，随滚入历史而平移）。
//! 复制范围从 B（没有 B 则从 A）到 D，含命令回显和输出，不含下一条提示符。
//! 命令还在跑时，终点是当前光标。
//!
//! 备用屏幕（vim / less / htop）上的标记不建块、不取样。进行中的命令若进入备用屏幕，
//! 只打 `fullscreen`，全屏帧不进 scrollback 区间。
//!
//! 没见过主屏幕上的 A/B/C/D 时不猜测回车或提示符，因此不会把全屏 TUI 收成命令块。
//! #156 的交互式审计可以旁路看到 `D` 的退出码，但 UI 块不以它为边界。

use std::fmt::Write as _;

/// 复制一块时最多保留的字符数。超出则保留开头并注明截断。
pub const COPY_MAX_CHARS: usize = 200_000;
/// 复制一块时最多读取的网格行数，避免超长输出在 UI 线程上铺开整段 scrollback。
pub const COPY_MAX_LINES: usize = 4_000;
/// 内存里保留的块上限（含尚未形成命令的提示符）。更老的从队头丢掉。
const MAX_BLOCKS: usize = 4_096;
/// 未结束的 OSC 缓冲上限。再长就交给 VTE 透传，直到结束符，避免标题类 OSC 把缓冲撑住。
const OSC_HOLD_LIMIT: usize = 512;

/// 网格上的一个点。`column` 对复制终点是开区间。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridPoint {
    pub line: i32,
    pub column: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MarkerKind {
    /// A，提示符开始。
    PromptStart,
    /// B，提示符结束 / 命令开始。
    PromptEnd,
    /// C，输出开始。
    OutputStart,
    /// D，命令结束。
    CommandFinished,
}

/// 一条已看到的命令（或仍在执行的命令）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandBlock {
    id: u64,
    /// A，或第一个打开这块的标记。
    origin: GridPoint,
    /// B。复制优先从这里开始，这样不含提示符。
    command: Option<GridPoint>,
    /// C。
    output: Option<GridPoint>,
    /// 排他终点（D，或下一次 A 补上的收口）。`None` 表示命令还在跑。
    end: Option<GridPoint>,
    /// 见到 `D`。仅 A 被下一次 A 收口的提示符重绘不算一条命令。
    finished: bool,
    exit_code: Option<i32>,
    /// 这条命令在结束前进入过备用屏幕。全屏帧不在本区间里。
    fullscreen: bool,
}

impl CommandBlock {
    pub(crate) fn start_line(&self) -> i32 {
        self.start_point().line
    }

    fn start_point(&self) -> GridPoint {
        self.command.or(self.output).unwrap_or(self.origin)
    }

    /// 复制区间 `[start, end)`。起点低于 `topmost` 时夹到历史顶。行数超过上限时夹终点。
    pub(crate) fn copy_span(
        &self,
        cursor: GridPoint,
        topmost: i32,
    ) -> (GridPoint, GridPoint, bool) {
        let mut start = self.start_point();
        if start.line < topmost {
            start = GridPoint {
                line: topmost,
                column: 0,
            };
        }
        let end = self.end.unwrap_or(cursor);
        let (end, clipped) = clamp_copy_end(start.line, end, COPY_MAX_LINES);
        let clipped = clipped || start.line != self.start_point().line;
        (start, end, clipped)
    }

    /// 能跳、能复制：命令已经提交（见到 C）或 shell 用 `D` 宣布结束。
    /// 只见到 B 的当前提示符还不是一条命令，避免「上一条」落在空输入行上。
    fn is_navigable(&self) -> bool {
        self.output.is_some() || self.finished
    }
}

/// 导航方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavDir {
    Prev,
    Next,
}

/// 上一条 / 下一条的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockNav {
    /// 把这一行滚到视口顶部。
    ScrollTo(i32),
    /// 回到最新输出。
    Bottom,
    /// 本会话主屏幕还没出现过 OSC 133 / 633。
    NoIntegration,
    /// 该方向没有下一块。
    AtEdge,
}

/// 复制当前块的结果。文本已经按 [`COPY_MAX_CHARS`] / 行数上限截断。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CopyBlock {
    Text(String),
    NoIntegration,
    Empty,
}

/// scrollback 上的命令块，以及「当前停在哪一块」。
#[derive(Clone, Debug, Default)]
pub struct CommandBlockIndex {
    blocks: Vec<CommandBlock>,
    next_id: u64,
    /// 主屏幕上见过 A/B/C/D。
    saw_integration: bool,
    /// 导航停住的块。`None` 表示跟着最新输出。
    parked: Option<u64>,
}

impl CommandBlockIndex {
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub fn shell_integration_seen(&self) -> bool {
        self.saw_integration
    }

    pub fn navigable_count(&self) -> usize {
        self.blocks.iter().filter(|b| b.is_navigable()).count()
    }

    pub fn exit_codes(&self) -> Vec<Option<i32>> {
        self.blocks
            .iter()
            .filter(|b| b.is_navigable())
            .map(|b| b.exit_code)
            .collect()
    }

    pub fn fullscreen_flags(&self) -> Vec<bool> {
        self.blocks
            .iter()
            .filter(|b| b.is_navigable())
            .map(|b| b.fullscreen)
            .collect()
    }

    /// 滚轮等用户滚动后不再停在某一块，下一次「上一条」从最新一块数。
    pub fn clear_parked(&mut self) {
        self.parked = None;
    }

    /// 网格整体上移（内容滚入历史）时，所有已记录的行号加上 `delta`（通常为负）。
    pub fn shift_lines(&mut self, delta: i32) {
        if delta == 0 {
            return;
        }
        for block in &mut self.blocks {
            shift_point(&mut block.origin, delta);
            if let Some(point) = &mut block.command {
                shift_point(point, delta);
            }
            if let Some(point) = &mut block.output {
                shift_point(point, delta);
            }
            if let Some(point) = &mut block.end {
                shift_point(point, delta);
            }
        }
    }

    /// 丢掉已经完全滚出历史的块。`topmost` 是当前网格最老的一行。
    pub fn prune(&mut self, topmost: i32) {
        self.blocks.retain(|block| match block.end {
            Some(end) => end.line >= topmost,
            None => true,
        });
        while self.blocks.len() > MAX_BLOCKS {
            self.blocks.remove(0);
        }
        if let Some(id) = self.parked {
            let still = self
                .blocks
                .iter()
                .any(|block| block.id == id && block.is_navigable());
            if !still {
                self.parked = None;
            }
        }
    }

    /// 尺寸变化后对不齐时丢掉坐标，但保留「见过 shell integration」。
    pub fn drop_coordinates(&mut self) {
        self.blocks.clear();
        self.parked = None;
    }

    /// 进行中的命令进入了备用屏幕。
    pub fn mark_fullscreen(&mut self) {
        if let Some(block) = self.open_mut() {
            block.fullscreen = true;
        }
    }

    /// 备用屏幕上的标记：不取样、不新开块。`D` 只记住退出码，等主屏幕上的收口。
    pub fn on_marker_while_alt(&mut self, kind_byte: u8, exit_code: Option<i32>) {
        let Some(kind) = MarkerKind::from_byte(kind_byte) else {
            return;
        };
        let Some(block) = self.open_mut() else {
            return;
        };
        block.fullscreen = true;
        if kind == MarkerKind::CommandFinished {
            if exit_code.is_some() {
                block.exit_code = exit_code;
            }
            block.finished = true;
        }
    }

    /// 主屏幕上的一个标记。`at` 是标记落点（光标，零宽）。
    pub fn on_marker(&mut self, kind_byte: u8, at: GridPoint, exit_code: Option<i32>) {
        let Some(kind) = MarkerKind::from_byte(kind_byte) else {
            return;
        };
        self.saw_integration = true;
        match kind {
            MarkerKind::PromptStart => {
                self.close_open(at, false);
                self.push_open(at);
            }
            MarkerKind::PromptEnd => {
                self.ensure_open(at);
                if let Some(block) = self.open_mut() {
                    if block.command.is_none() {
                        block.command = Some(at);
                    }
                }
            }
            MarkerKind::OutputStart => {
                self.ensure_open(at);
                if let Some(block) = self.open_mut() {
                    if block.output.is_none() {
                        block.output = Some(at);
                    }
                }
            }
            MarkerKind::CommandFinished => {
                let Some(block) = self.open_mut() else {
                    return;
                };
                if block.end.is_none() {
                    block.end = Some(at);
                }
                block.finished = true;
                if exit_code.is_some() {
                    block.exit_code = exit_code;
                }
            }
        }
    }

    pub fn navigate(&mut self, dir: NavDir) -> BlockNav {
        if !self.saw_integration {
            return BlockNav::NoIntegration;
        }
        let ids: Vec<(u64, i32)> = self
            .blocks
            .iter()
            .filter(|block| block.is_navigable())
            .map(|block| (block.id, block.start_point().line))
            .collect();
        if ids.is_empty() {
            return BlockNav::AtEdge;
        }
        let parked_pos = self
            .parked
            .and_then(|id| ids.iter().position(|(block_id, _)| *block_id == id));
        match dir {
            NavDir::Prev => {
                let pos = match parked_pos {
                    Some(index) => index.checked_sub(1),
                    None => Some(ids.len() - 1),
                };
                match pos {
                    Some(index) => {
                        self.parked = Some(ids[index].0);
                        BlockNav::ScrollTo(ids[index].1)
                    }
                    None => BlockNav::AtEdge,
                }
            }
            NavDir::Next => {
                let Some(index) = parked_pos else {
                    return BlockNav::AtEdge;
                };
                if let Some((id, line)) = ids.get(index + 1) {
                    self.parked = Some(*id);
                    BlockNav::ScrollTo(*line)
                } else {
                    self.parked = None;
                    BlockNav::Bottom
                }
            }
        }
    }

    /// 当前要复制的块：导航停住的那块，否则最新一块可导航的命令。
    pub fn copy_target(&self) -> Result<&CommandBlock, CopyBlock> {
        if !self.saw_integration {
            return Err(CopyBlock::NoIntegration);
        }
        if let Some(id) = self.parked {
            if let Some(block) = self
                .blocks
                .iter()
                .find(|block| block.id == id && block.is_navigable())
            {
                return Ok(block);
            }
        }
        self.blocks
            .iter()
            .rev()
            .find(|block| block.is_navigable())
            .ok_or(CopyBlock::Empty)
    }

    fn ensure_open(&mut self, at: GridPoint) {
        if self.open_mut().is_none() {
            self.push_open(at);
        }
    }

    fn push_open(&mut self, at: GridPoint) {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.blocks.push(CommandBlock {
            id,
            origin: at,
            command: None,
            output: None,
            end: None,
            finished: false,
            exit_code: None,
            fullscreen: false,
        });
        self.prune(i32::MIN);
    }

    fn close_open(&mut self, at: GridPoint, finished: bool) {
        let Some(block) = self.open_mut() else {
            return;
        };
        if block.end.is_none() {
            block.end = Some(at);
        }
        if finished {
            block.finished = true;
        }
    }

    fn open_mut(&mut self) -> Option<&mut CommandBlock> {
        let last = self.blocks.last_mut()?;
        if last.end.is_none() {
            Some(last)
        } else {
            None
        }
    }
}

fn shift_point(point: &mut GridPoint, delta: i32) {
    point.line = point.line.saturating_add(delta);
}

impl MarkerKind {
    fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            b'A' => Some(Self::PromptStart),
            b'B' => Some(Self::PromptEnd),
            b'C' => Some(Self::OutputStart),
            b'D' => Some(Self::CommandFinished),
            _ => None,
        }
    }
}

/// 复制文本的有界截断。超长时保留开头，避免一次把整段输出交到剪贴板和 UI。
pub fn bound_copied_text(text: &str) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(COPY_MAX_CHARS).collect();
    let omitted = chars.count();
    if omitted == 0 {
        return head;
    }
    let mut out = head;
    let _ = write!(out, "\n… [truncated, {omitted} chars omitted]");
    out
}

/// 把复制终点夹到最多 `max_lines` 行。返回是否被夹过。
pub fn clamp_copy_end(start_line: i32, end: GridPoint, max_lines: usize) -> (GridPoint, bool) {
    let max_lines = max_lines.max(1) as i32;
    let limit = start_line.saturating_add(max_lines);
    if end.line > limit || (end.line == limit && end.column > 0) {
        (
            GridPoint {
                line: limit,
                column: 0,
            },
            true,
        )
    } else {
        (end, false)
    }
}

/// 从原始字节流里切出 OSC 133 / 633 标记。其它字节原样交回，包括标记序列本身
/// （调用方仍要喂给 VTE）。跨 `feed` 的半截序列留在内部。
#[derive(Debug, Default)]
pub struct OscSplitter {
    state: SplitState,
    held: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SplitState {
    #[default]
    Ground,
    Esc,
    Osc,
    /// 已经在 OSC 里又看到 ESC，等 `\` 组成 ST。
    OscEsc,
    /// 超长 OSC：字节已经交还调用方，这里只等到结束符。
    Skip,
    SkipEsc,
}

/// 一次切分的产出。`raw` 仍需按序写入终端。
#[derive(Debug)]
pub enum SplitItem {
    Bytes(Vec<u8>),
    Marker {
        raw: Vec<u8>,
        /// `A` / `B` / `C` / `D`。
        kind: u8,
        exit_code: Option<i32>,
    },
}

impl OscSplitter {
    pub fn push(&mut self, data: &[u8], out: &mut Vec<SplitItem>) {
        let mut plain: Vec<u8> = Vec::new();
        let flush_plain = |plain: &mut Vec<u8>, out: &mut Vec<SplitItem>| {
            if !plain.is_empty() {
                out.push(SplitItem::Bytes(std::mem::take(plain)));
            }
        };
        for &byte in data {
            match self.state {
                SplitState::Ground => {
                    if byte == 0x1b {
                        flush_plain(&mut plain, out);
                        self.held.clear();
                        self.held.push(byte);
                        self.state = SplitState::Esc;
                    } else {
                        plain.push(byte);
                    }
                }
                SplitState::Esc => {
                    if byte == b']' {
                        self.held.push(byte);
                        self.state = SplitState::Osc;
                    } else if byte == 0x1b {
                        plain.extend_from_slice(&self.held);
                        self.held.clear();
                        self.held.push(byte);
                    } else {
                        self.held.push(byte);
                        plain.extend_from_slice(&self.held);
                        self.held.clear();
                        self.state = SplitState::Ground;
                    }
                }
                SplitState::Osc => match byte {
                    0x07 => {
                        self.held.push(byte);
                        self.emit_osc(out);
                    }
                    0x1b => {
                        self.held.push(byte);
                        self.state = SplitState::OscEsc;
                    }
                    _ => {
                        self.held.push(byte);
                        if self.held.len() > OSC_HOLD_LIMIT {
                            plain.extend_from_slice(&self.held);
                            self.held.clear();
                            self.state = SplitState::Skip;
                        }
                    }
                },
                SplitState::OscEsc => {
                    if byte == b'\\' {
                        self.held.push(byte);
                        self.emit_osc(out);
                    } else if byte == 0x1b {
                        self.held.push(byte);
                        self.state = SplitState::OscEsc;
                    } else {
                        self.held.push(byte);
                        self.state = SplitState::Osc;
                        if self.held.len() > OSC_HOLD_LIMIT {
                            plain.extend_from_slice(&self.held);
                            self.held.clear();
                            self.state = SplitState::Skip;
                        }
                    }
                }
                SplitState::Skip => {
                    plain.push(byte);
                    self.state = match byte {
                        0x07 => SplitState::Ground,
                        0x1b => SplitState::SkipEsc,
                        _ => SplitState::Skip,
                    };
                }
                SplitState::SkipEsc => {
                    plain.push(byte);
                    self.state = if byte == b'\\' {
                        SplitState::Ground
                    } else if byte == 0x1b {
                        SplitState::SkipEsc
                    } else {
                        SplitState::Skip
                    };
                }
            }
        }
        flush_plain(&mut plain, out);
    }

    fn emit_osc(&mut self, out: &mut Vec<SplitItem>) {
        let raw = std::mem::take(&mut self.held);
        self.state = SplitState::Ground;
        if let Some((kind, exit_code)) = parse_marker(&raw) {
            out.push(SplitItem::Marker {
                raw,
                kind,
                exit_code,
            });
        } else {
            out.push(SplitItem::Bytes(raw));
        }
    }
}

/// `raw` 含前导 ESC 和结束符。认 `133;X` / `633;X`。
fn parse_marker(raw: &[u8]) -> Option<(u8, Option<i32>)> {
    let payload = osc_payload(raw)?;
    let text = std::str::from_utf8(payload).ok()?.trim();
    let rest = text
        .strip_prefix("133;")
        .or_else(|| text.strip_prefix("633;"))?;
    let mut parts = rest.split(';');
    let code = parts.next()?.trim();
    if code.len() != 1 {
        return None;
    }
    let kind = code.as_bytes()[0];
    if !matches!(kind, b'A' | b'B' | b'C' | b'D') {
        return None;
    }
    let exit_code = if kind == b'D' {
        parse_exit(parts.next())
    } else {
        None
    };
    Some((kind, exit_code))
}

fn parse_exit(field: Option<&str>) -> Option<i32> {
    let field = field?.trim();
    let digits: String = field.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

/// 去掉 ESC ] 前缀和 BEL / ST 后缀。
fn osc_payload(raw: &[u8]) -> Option<&[u8]> {
    let rest = raw.strip_prefix(b"\x1b]")?;
    if let Some(body) = rest.strip_suffix(b"\x07") {
        return Some(body);
    }
    rest.strip_suffix(b"\x1b\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(raw: &str) -> Option<(u8, Option<i32>)> {
        parse_marker(raw.as_bytes())
    }

    #[test]
    fn parses_finalterm_and_vscode_markers() {
        assert_eq!(marker("\x1b]133;A\x07"), Some((b'A', None)));
        assert_eq!(marker("\x1b]133;B\x07"), Some((b'B', None)));
        assert_eq!(marker("\x1b]133;C\x07"), Some((b'C', None)));
        assert_eq!(marker("\x1b]133;D;7\x07"), Some((b'D', Some(7))));
        assert_eq!(marker("\x1b]133;D\x07"), Some((b'D', None)));
        assert_eq!(marker("\x1b]133;D;0;aid=1\x1b\\"), Some((b'D', Some(0))));
        assert_eq!(marker("\x1b]633;A\x07"), Some((b'A', None)));
        assert_eq!(marker("\x1b]633;D;3\x07"), Some((b'D', Some(3))));
        assert_eq!(marker("\x1b]133;A;cl=m\x07"), Some((b'A', None)));
    }

    #[test]
    fn ignores_iterm_props_and_titles() {
        assert!(marker("\x1b]1337;RemoteHost=a@b\x07").is_none());
        assert!(marker("\x1b]133;P;Cwd=/tmp\x07").is_none());
        assert!(marker("\x1b]0;title\x07").is_none());
        assert!(marker("\x1b]133;Done\x07").is_none());
    }

    #[test]
    fn splitter_keeps_markers_across_chunks() {
        let mut splitter = OscSplitter::default();
        let mut out = Vec::new();
        splitter.push(b"hi\x1b]133;D;", &mut out);
        assert!(out.iter().all(|item| matches!(item, SplitItem::Bytes(_))));
        out.clear();
        splitter.push(b"9\x07$", &mut out);
        assert!(matches!(
            &out[0],
            SplitItem::Marker {
                kind: b'D',
                exit_code: Some(9),
                ..
            }
        ));
        match &out[1] {
            SplitItem::Bytes(bytes) => assert_eq!(bytes, b"$"),
            other => panic!("expected trailing text, got {other:?}"),
        }
    }

    #[test]
    fn three_commands_navigate_and_do_not_keep_prompt_redraws() {
        let mut index = CommandBlockIndex::default();
        let mut line = 0;
        for n in 1..=3 {
            index.on_marker(b'A', GridPoint { line, column: 0 }, None);
            index.on_marker(b'B', GridPoint { line, column: 2 }, None);
            line += 1;
            index.on_marker(b'C', GridPoint { line, column: 0 }, None);
            line += 1;
            index.on_marker(b'D', GridPoint { line, column: 0 }, Some(n));
        }
        // 下一条提示符，还没输入命令。
        index.on_marker(b'A', GridPoint { line, column: 0 }, None);
        assert_eq!(index.navigable_count(), 3);
        assert_eq!(index.exit_codes(), vec![Some(1), Some(2), Some(3)]);
        assert!(!index.shell_integration_seen() || index.navigable_count() == 3);

        assert_eq!(index.navigate(NavDir::Prev), BlockNav::ScrollTo(4));
        assert_eq!(index.navigate(NavDir::Prev), BlockNav::ScrollTo(2));
        assert_eq!(index.navigate(NavDir::Prev), BlockNav::ScrollTo(0));
        assert_eq!(index.navigate(NavDir::Prev), BlockNav::AtEdge);
        assert_eq!(index.navigate(NavDir::Next), BlockNav::ScrollTo(2));
        assert_eq!(index.navigate(NavDir::Next), BlockNav::ScrollTo(4));
        assert_eq!(index.navigate(NavDir::Next), BlockNav::Bottom);
        assert_eq!(index.navigate(NavDir::Next), BlockNav::AtEdge);
    }

    #[test]
    fn alt_screen_marker_does_not_open_a_block() {
        let mut index = CommandBlockIndex::default();
        index.on_marker_while_alt(b'A', None);
        index.on_marker_while_alt(b'D', Some(0));
        assert!(!index.shell_integration_seen());
        assert_eq!(index.navigable_count(), 0);
        assert_eq!(index.navigate(NavDir::Prev), BlockNav::NoIntegration);
    }

    #[test]
    fn fullscreen_command_stays_one_block() {
        let mut index = CommandBlockIndex::default();
        index.on_marker(b'A', GridPoint { line: 0, column: 0 }, None);
        index.on_marker(b'B', GridPoint { line: 0, column: 2 }, None);
        index.on_marker(b'C', GridPoint { line: 1, column: 0 }, None);
        index.mark_fullscreen();
        index.on_marker_while_alt(b'D', Some(0));
        index.on_marker(b'A', GridPoint { line: 1, column: 0 }, None);
        assert_eq!(index.navigable_count(), 1);
        assert_eq!(index.fullscreen_flags(), vec![true]);
        assert_eq!(index.exit_codes(), vec![Some(0)]);
    }

    #[test]
    fn copy_bounds_keep_the_head() {
        let text = "x".repeat(COPY_MAX_CHARS + 12);
        let bounded = bound_copied_text(&text);
        assert!(bounded.contains("[truncated, 12 chars omitted]"));
        assert!(bounded.starts_with(&"x".repeat(COPY_MAX_CHARS)));
        assert!(!bounded.contains(&"x".repeat(COPY_MAX_CHARS + 1)));
        assert_eq!(bound_copied_text("echo hi\nhi"), "echo hi\nhi");
    }

    #[test]
    fn clamp_copy_end_limits_line_span() {
        let (end, clipped) = clamp_copy_end(
            10,
            GridPoint {
                line: 40,
                column: 3,
            },
            5,
        );
        assert!(clipped);
        assert_eq!(
            end,
            GridPoint {
                line: 15,
                column: 0
            }
        );
        let (end, clipped) = clamp_copy_end(
            10,
            GridPoint {
                line: 12,
                column: 4,
            },
            5,
        );
        assert!(!clipped);
        assert_eq!(end.line, 12);
    }

    #[test]
    fn shift_and_prune_follow_scrollback() {
        let mut index = CommandBlockIndex::default();
        index.on_marker(b'B', GridPoint { line: 0, column: 0 }, None);
        index.on_marker(b'D', GridPoint { line: 2, column: 0 }, Some(0));
        index.shift_lines(-5);
        let block = index.copy_target().unwrap();
        assert_eq!(block.start_point().line, -5);
        assert_eq!(block.end.unwrap().line, -3);
        index.prune(-2);
        assert!(index.copy_target().is_err());
    }
}
