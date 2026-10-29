//! 终端视图
#![allow(dead_code)]
//!
//! 显示终端模拟器、处理输入输出、集成 SSH 连接。
//!
//! **与本文件相关的传文件入口(与 SFTP 侧栏、终端内 `rz` 并列，互不合并实现)**：
//! - **ZMODEM**：`rz` 检测 → `LrzszTransfer::start_send`(`zmodem2` + shell 泵)。
//! - **直传·SCP**：[`TerminalView::start_upload`](TerminalView::start_upload)(`crate::ssh::ScpUpload`，分块排在 shell 泵线程)。

use eframe::egui;
use arboard::Clipboard;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};
use crate::core::exec_history::ExecLogRecord;
use crate::core::interactive_exec::{apply_interactive_outcome, InteractiveExecDraft};
use crate::core::{ServerAuditEvent, ServerAuditProbe};
use crate::local_shell::LocalShellHandle;
use crate::ssh::{SshManager, SshConfig, SshMessage, SshSessionHandle, LrzszTransfer, TransferEvent, format_ssh_connect_error};
use alacritty_terminal::grid::Scroll;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::viewport_to_point;
use crate::terminal::{Terminal as VtTerminal, TerminalShellStyle};
use crate::terminal::style::{
    format_user_error_line, format_user_info_line, format_user_success_line, format_user_warn_line,
};
use crate::i18n::UiLanguage;
use crate::ui::layout_util;
use crate::ui::theme::Theme;

/// 同时输入时，把当前窗格的按键再发给其它窗格。
#[derive(Clone)]
pub(crate) enum BroadcastTarget {
    Local(LocalShellHandle),
    Ssh(SshSessionHandle),
}

impl BroadcastTarget {
    fn send(&self, data: &[u8]) -> Result<(), String> {
        match self {
            Self::Local(h) => h.send_input(data),
            Self::Ssh(h) => h.send_input(data),
        }
    }
}

/// 与 [`VtTerminal::content_epoch`] 组合，避免 PTY 无输出帧重复构建整屏 [`egui::text::LayoutJob`]。
struct TerminalVisualLayoutCache {
    vt_gen: u64,
    content_epoch: u64,
    cols: u32,
    rows: u32,
    font_bits: u32,
    line_height_bits: u32,
    fg: egui::Color32,
    bg: egui::Color32,
    /// `None` = 无选区；否则为规范化绝对选区(列 end 开区间)。
    selection: Option<(i32, usize, i32, usize)>,
    layout_job: egui::text::LayoutJob,
    /// 与 `layout_job` 同行同列的格子底色(不含 egui background expand)。
    cell_bgs: Vec<Vec<egui::Color32>>,
    formatted: String,
    /// 已排版的 galley；避免 TextEdit layouter 每帧 `layout_job` 打满 CPU。
    galley: Option<std::sync::Arc<egui::Galley>>,
    /// display-link 是否启用。关掉后下一帧重扫，不再出新装饰（已画下划线随缓存失效）。
    display_link_on: bool,
    /// display-link 命中：`(行号, 可点区间)`，与 layout 同键缓存（#90 / #120）。
    display_links: Vec<(usize, DisplaySpan)>,
}

impl TerminalVisualLayoutCache {
    fn matches(
        &self,
        vt_gen: u64,
        content_epoch: u64,
        cols: u32,
        rows: u32,
        font_bits: u32,
        line_height_bits: u32,
        fg: egui::Color32,
        bg: egui::Color32,
        selection: Option<(i32, usize, i32, usize)>,
        display_link_on: bool,
    ) -> bool {
        self.vt_gen == vt_gen
            && self.content_epoch == content_epoch
            && self.cols == cols
            && self.rows == rows
            && self.font_bits == font_bits
            && self.line_height_bits == line_height_bits
            && self.fg == fg
            && self.bg == bg
            && self.selection == selection
            && self.display_link_on == display_link_on
    }
}

/// Host 侧可点区间，来自 `ModuleAction::DecorateLink`。渲染与点击都认它，模块不碰 egui。
#[derive(Clone, Debug, PartialEq, Eq)]
struct DisplaySpan {
    start: usize,
    end: usize,
    action: String,
    text: String,
}

impl DisplaySpan {
    fn from_action(act: crate::capability::ModuleAction) -> Option<Self> {
        match act {
            crate::capability::ModuleAction::DecorateLink {
                start,
                end,
                action,
                text,
            } => Some(Self {
                start,
                end,
                action,
                text,
            }),
            _ => None,
        }
    }
}

/// 左键点击分发。非 http(s) 的 browser-open 忽略，与迁入前一致。
enum DisplayLinkClick {
    Copy,
    Browser,
    Runbook,
    Sftp,
}

fn classify_display_link_click(action: &str, text: &str) -> Option<DisplayLinkClick> {
    use crate::modules::display_link::{
        ACTION_BROWSER_OPEN, ACTION_COPY_OR_CONNECT, ACTION_RUNBOOK_RECALL, ACTION_SFTP_OPEN,
    };
    match action {
        ACTION_COPY_OR_CONNECT => Some(DisplayLinkClick::Copy),
        ACTION_BROWSER_OPEN => {
            if text.starts_with("http://") || text.starts_with("https://") {
                Some(DisplayLinkClick::Browser)
            } else {
                None
            }
        }
        ACTION_RUNBOOK_RECALL => Some(DisplayLinkClick::Runbook),
        ACTION_SFTP_OPEN => Some(DisplayLinkClick::Sftp),
        _ => None,
    }
}

/// 底栏展示的 SSH 连接状态(不写入 VTE scrollback)。
#[derive(Clone, Debug)]
pub struct ConnectionBarStatus {
    pub host_line: String,
    pub state_line: String,
    pub state_color: egui::Color32,
}

fn truncate_connection_status(s: &str, max_chars: usize) -> String {
    let s = s.trim();
    let mut it = s.chars();
    let head: String = it.by_ref().take(max_chars).collect();
    if it.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

/// 终端文本选择(绝对网格坐标：`Line.0` 可为负表示 scrollback，列 0-based)
#[derive(Clone, Debug, Default)]
struct Selection {
    start_line: i32,
    start_col: usize,
    end_line: i32,
    end_col: usize,
    active: bool,
    /// 普通单击记下的锚点(绝对网格坐标)，供随后 Shift+单击扩展；滚轮翻屏不影响。
    click_anchor: Option<(i32, usize)>,
}

impl Selection {
    fn is_empty(&self) -> bool {
        !self.active || (self.start_line == self.end_line && self.start_col == self.end_col)
    }

    fn normalize(&self) -> (i32, usize, i32, usize) {
        if (self.start_line, self.start_col) <= (self.end_line, self.end_col) {
            (self.start_line, self.start_col, self.end_line, self.end_col)
        } else {
            (self.end_line, self.end_col, self.start_line, self.start_col)
        }
    }

    fn clear(&mut self) {
        self.active = false;
        self.start_line = 0;
        self.start_col = 0;
        self.end_line = 0;
        self.end_col = 0;
        self.click_anchor = None;
    }

    /// 普通单击：清掉旧选区，只记锚点(不显示选区)。
    fn set_anchor_from_viewport(&mut self, display_offset: usize, row: usize, col: usize) {
        let p = viewport_to_point(display_offset, Point::new(row, Column(col)));
        self.clear();
        self.click_anchor = Some((p.line.0, p.column.0));
    }

    fn set_from_viewport(
        &mut self,
        display_offset: usize,
        start_row: usize,
        start_col: usize,
        end_row: usize,
        end_col: usize,
    ) {
        let sp = viewport_to_point(display_offset, Point::new(start_row, Column(start_col)));
        let ep = viewport_to_point(display_offset, Point::new(end_row, Column(end_col)));
        self.start_line = sp.line.0;
        self.start_col = sp.column.0;
        self.end_line = ep.line.0;
        self.end_col = ep.column.0;
        self.active = true;
    }

    fn set_end_from_viewport(&mut self, display_offset: usize, end_row: usize, end_col: usize) {
        let ep = viewport_to_point(display_offset, Point::new(end_row, Column(end_col)));
        self.end_line = ep.line.0;
        self.end_col = ep.column.0;
        self.active = true;
    }

    /// Shift+单击扩展选区：保留原锚点，只移动终点；无选区时以 `fallback_anchor`
    /// (绝对坐标，通常为终端光标)或点击处为锚点。坐标为绝对网格，故滚轮翻屏后仍正确。
    fn extend_from_viewport(
        &mut self,
        display_offset: usize,
        row: usize,
        col: usize,
        fallback_anchor: Option<(i32, usize)>,
    ) {
        let ep = viewport_to_point(display_offset, Point::new(row, Column(col)));
        if !self.active {
            let (al, ac) = self
                .click_anchor
                .or(fallback_anchor)
                .unwrap_or((ep.line.0, ep.column.0));
            self.start_line = al;
            self.start_col = ac;
        }
        self.end_line = ep.line.0;
        self.end_col = ep.column.0;
        self.active = true;
    }
}

/// 拖选越过文本区上/下边缘时的自动滚动行数(每秒)，与越界距离成正比。
/// 返回值 >0 表示向上翻历史(alacritty `Scroll::Delta` 正方向)。
fn drag_autoscroll_lines_per_sec(pointer_y: f32, top: f32, bottom: f32, cell_h: f32) -> f32 {
    let cell_h = cell_h.max(1.0);
    if pointer_y < top {
        ((top - pointer_y) / cell_h + 1.0).min(40.0) * 6.0
    } else if pointer_y > bottom {
        -((pointer_y - bottom) / cell_h + 1.0).min(40.0) * 6.0
    } else {
        0.0
    }
}

/// 右 dock 依赖 SSH 时的连接门闩(避免已断开仍显示「连接中」)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RightDockSshGate {
    NoSession,
    Ready,
    Connecting,
    Disconnected,
    Failed(String),
    /// 本机终端标签：无远端 SSH，SFTP/转发不可用
    LocalShell,
}

/// Actions from the centered connect/fail placeholder (polled by App).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionPlaceholderAction {
    None,
    CancelConnect,
    Retry,
    EditHost,
    DismissTab,
}

/// 终端连接类型：远端 SSH 或本机 PTY shell。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnectionKind {
    #[default]
    Ssh,
    Local,
}

/// 终端视图组件
pub struct TerminalView {
    /// 会话 ID
    session_id: Option<usize>,
    
    /// SSH 管理器
    ssh_manager: Option<SshManager>,
    
    /// SSH 消息接收器
    ssh_rx: Option<Receiver<SshMessage>>,
    
    /// SSH 会话句柄
    ssh_handle: Option<SshSessionHandle>,

    /// 本机 PTY 句柄（[`ConnectionKind::Local`]）
    local_handle: Option<LocalShellHandle>,

    /// SSH 远端或本机 shell
    connection_kind: ConnectionKind,
    
    /// VT100/ANSI 终端模拟器(含光标、屏幕、滚动历史)
    terminal: VtTerminal,
    
    /// 连接状态
    connected: bool,
    
    /// 连接错误信息
    error_message: Option<String>,
    
    /// 终端尺寸
    cols: u32,
    rows: u32,
    
    /// lrzsz 文件传输器
    lrzsz: LrzszTransfer,
    
    /// 命令片段面板可见性
    pub show_fragment_panel: bool,
    
    /// 是否需要弹出文件选择对话框(检测到 rz 命令时设置)
    pub pending_rz_upload: bool,
    
    /// 文件传输进度(文件名、已传字节、总字节)
    transfer_progress: Option<(String, u64, u64)>,
    /// 当前传输是否为本机→远端(`rz` 上传)；用于文案与收尾
    transfer_outgoing: bool,
    /// 连接成功后下一帧请求一次终端焦点(避免每帧 `request_focus` 与 PTY 光标叠成双光标)
    pending_focus_terminal: bool,

    font_size: f32,
    connected_at: Option<Instant>,
    connection_target: Option<(String, String)>,
    auto_follow_output: bool,
    /// 用户点击终端后保持键盘输入，直到点击终端外区域(IME 框按 Enter 会 surrender_focus，不能只看 has_focus)
    terminal_wants_keyboard: bool,
    terminal_focused: bool,
    rz_control_mode_until: Option<Instant>,
    /// 用户已在 shell 执行 `rz`/`lrz` 后，lrzsz 可能不发文本提示(如 `rz -bye`)而直接发 ZRQINIT。
    expect_rz_upload_until: Option<Instant>,
    /// 用户已在 shell 执行 `sz`/`lsz` 后，等待远端 ZRQINIT 以启动本机接收。
    expect_sz_download_until: Option<Instant>,
    upload_result_rx: Option<Receiver<Result<String, String>>>,
    command_usage: HashMap<String, u64>,
    /// FUNCTIONAL_SPEC §2.4：超长粘贴分片发送(>10KB 时每批 4096 字节、间隔 5ms)
    paste_pending: Vec<u8>,
    paste_next_chunk_at: Option<Instant>,
    /// 粘贴分片因输入队列满连续重试的次数
    paste_retries: u32,
    /// 「仅断开 SSH 保留画面」后为 true：键盘输入写入 [`Self::disconnected_input_buffer`] 而非 PTY(FUNCTIONAL_SPEC §2.4)。
    buffer_input_while_disconnected: bool,
    /// 断线期间缓存的待发送字节(上限 [`Self::OFFLINE_INPUT_CAP`])。
    disconnected_input_buffer: Vec<u8>,
    /// 重连且 shell 就绪后，若缓存非空则弹出是否重发。
    resend_offline_input_dialog_open: bool,
    /// 连接后恢复命令（#177）：由会话配置注入，shell 就绪后延迟执行一次。
    /// None = 未配置；Some(空串) 视为未配置。执行后清空，断线时清空。
    pending_post_connect_command: Option<String>,
    /// 恢复命令的定时执行时刻（连接成功后短暂延迟，等 shell 提示符就绪）。
    post_connect_fire_at: Option<Instant>,
    /// 整屏替换 VTE 或清屏时递增，与 [`VtTerminal::content_epoch`] 一并参与布局缓存键(FUNCTIONAL_SPEC §2.3.1)。
    vt_visual_generation: u64,
    visual_layout_cache: Option<TerminalVisualLayoutCache>,
    /// 诊断：最早一条未绘制输出的到达时刻（µs，见 ssh::manager::probe_now_us）。
    repaint_probe_received_us: Option<u64>,
    repaint_probe_cache_hit: bool,
    repaint_probe_row_hash: Option<u64>,
    repaint_probe_epoch: u64,
    /// 用户主动断开(`disconnect` / `disconnect_ssh_keep_buffer`)期间忽略随后到达的 `Disconnected`，避免误触发自动重连。
    local_disconnect_intent: bool,
    /// 本轮 `process_ssh_messages` 后若曾为「非主动」断开则置位；由宿主 `take()` 后清除。
    unexpected_disconnect_notified: bool,
    /// #93：意外断开会话 → 未聚焦标签提示（与重连用的 `unexpected_disconnect_notified` 分通道）。
    pending_attention_ended: bool,
    /// 连接成功/失败待宿主写入审计(`take_connect_audit` 取走)
    pending_connect_audit: Option<(bool, String)>,
    /// 服务器侧命令审计标记行解析(`MIST_AUDIT`)
    server_audit_probe: ServerAuditProbe,
    /// 待宿主展示的服务器侧审计事件
    pending_server_audit: Vec<ServerAuditEvent>,
    /// 当前连接主机无可用 agent 时显示降级横幅
    agent_audit_degraded: bool,
    /// 用户关闭降级横幅后，本连接周期内不再显示
    agent_audit_banner_dismissed: bool,
    /// 连接失败待宿主 Toast(`take_pending_toast_error` 取走)
    pending_toast_error: Option<String>,
    /// Connect placeholder button (`take_connection_placeholder_action`)
    pending_connection_placeholder_action: ConnectionPlaceholderAction,
    /// 拖入终端区域、待宿主处理的上传路径(§4.3.2)。
    pending_drop_upload_paths: Vec<PathBuf>,
    /// 大文件上传：用户选 ZMODEM 后先发 `rz -y`，握手检测到后再用此路径 `start_rz_upload`。
    zmodem_upload_after_rz_path: Option<PathBuf>,
    /// 终端文本选择状态
    selection: Selection,
    /// 拖选自动滚动的小数行累加(帧率无关)
    select_autoscroll_acc: f32,
    /// 右键「发送到 AI」后置位，由 `app` 读取选区并打开 AI 面板。
    pending_send_to_ai: bool,
    pending_send_tail_to_ai: bool,
    /// 点击「发送到 AI」当帧快照的选中文本，避免后续选区变化导致桥接拿到空串。
    pending_send_to_ai_text: Option<String>,
    /// 最近 PTY 输出尾部，用于跨 chunk 识别 ZMODEM 握手帧。
    zmodem_pty_detect_tail: Vec<u8>,
    /// 当前行已键入字节(用于 Ctrl+R 命令历史，近似 PTY 行缓冲)
    typed_line_buffer: String,
    /// 提示符或 `cd` 记下的当前目录。分屏时新窗格从这里启动。
    shell_cwd: Option<String>,
    /// SSH 新窗格看到提示符后要执行的 `cd`（本机窗格直接以该目录启动，不走这里）。
    pending_startup_cd: Option<String>,
    broadcast_targets: Vec<BroadcastTarget>,
    broadcast_suppress: bool,
    /// 最近写入 PTY 的当前行(自动化可能无 egui Text 事件，仍可从出站字节识别 `rz`)
    pty_outbound_line_buffer: String,
    /// Enter 提交后待取走的整行命令(供命令历史 / 会话日志)
    submitted_line: Option<String>,
    /// 宿主是否要求手敲命令在回车前先过本地命令审计(策略生效时由 app 每帧设置)。
    typed_enter_gate: bool,
    /// 手敲行按了回车但尚未发 `\r`，等宿主审计(`take_typed_enter_request` 取走)。
    typed_enter_request: Option<String>,
    /// 偏好与审计生效时，把回车提交的命令写成 exec_history JSONL。
    interactive_capture_on: bool,
    capture_session_id: String,
    capture_target: String,
    /// 下一次提交使用的结论（allowed / alert / confirmed）。空则按 allowed。
    pending_capture_outcome: String,
    open_capture: Option<InteractiveExecDraft>,
    pending_exec_records: Vec<ExecLogRecord>,
    /// 待写入会话日志的命令(片段执行、Enter 提交等)
    pending_log_commands: Vec<String>,
    /// 待写入会话日志的 PTY 输出块
    pending_log_output: Vec<Vec<u8>>,
    /// 查找命中高亮：`(行, 列, 长度)` 均为 1-based 字符下标
    search_highlight: Option<(usize, usize, usize)>,
    /// `show()` 每帧同步，用于无 `Context` 入参的路径(不影响 `process_ssh_messages` 内语言的一帧滞后)。
    ui_lang_last: UiLanguage,
    /// 透明 IME 捕获框(须持有文本光标，winit 才会 `set_ime_allowed`)
    ime_capture: String,
    /// 未上屏的预编辑。叠在光标处绘制；候选列表仍由系统输入法面板显示。
    ime_preedit: crate::ui::terminal_keys::ImePreedit,
    /// 最近一帧终端 IME 框的 egui Id；UI 输入(含中文全角)抢焦点时须 surrender，否则 Composition 仍进终端。
    last_ime_id: Option<egui::Id>,
    /// 点击 display-link 后待宿主处理的动作（SFTP 跳转需要宿主开面板）。
    pending_display_link_sftp: Option<String>,
    /// 点击错误码链接后待宿主处理的自动召回请求（#90 后续：attach + 自动发送触发 #66 召回）。
    pending_display_link_runbook: Option<String>,
    /// 右键 IP 链接后待宿主处理的「新开连接」请求（#90 后续）。
    pending_display_link_connect: Option<String>,
    /// 右键按下帧缓存的可点区间（菜单跨帧时 `interact_pointer_pos` 会丢，需自持，#111）。
    context_menu_display_link: Option<DisplaySpan>,
    /// 扩展模块注册表（#117 / #119 / #120）：Host 接线，PTY 完整行分发与右键菜单贡献。
    /// output-trigger 经此注册表的 `on_output_line` 产出 Notify，不再自带引擎。
    module_registry: std::sync::Arc<crate::capability::ModuleRegistry>,
    /// 未完成行缓冲（供 module on_output_line 跨 chunk 拼行，#117）。
    module_output_line_acc: String,
    /// 模块产出的动作缓存（#117 分发骨架）。
    pending_module_actions: Vec<crate::capability::ModuleAction>,
    /// 模块右键菜单点击产生的 action key，待宿主或外界取走。
    pending_module_menu_action: Option<String>,
}

impl TerminalView {
    /// 断线缓存输入上限(字节)
    const OFFLINE_INPUT_CAP: usize = 64 * 1024;

    /// Scroll 内容与视口边框的极小余量，避免偶发裁切一个字形
    const INNER_TEXT_SLACK: f32 = 0.0;
    /// ScrollArea **内容区内宽**(已不含纵向滚动条)→ TextEdit.desired_width
    #[inline]
    fn text_width_in_scroll_viewport(scroll_inner_width: f32) -> f32 {
        (scroll_inner_width - Self::INNER_TEXT_SLACK).max(64.0)
    }

    fn measure_terminal_line_height(
        fonts: &egui::text::Fonts,
        font_size: f32,
        _color: egui::Color32,
    ) -> f32 {
        use crate::terminal::style::TERMINAL_LINE_HEIGHT_FACTOR;
        let font_id = egui::FontId::monospace(font_size);
        // egui 的 row_height = ascent - descent + line_gap；Consolas 等 line_gap 偏大时，
        // 每行字形贴顶、下方空一截。终端格高夹到字号的 FACTOR 倍，与 WT 观感接近。
        let natural = fonts.row_height(&font_id);
        natural
            .min(font_size * TERMINAL_LINE_HEIGHT_FACTOR)
            .max(font_size)
    }

    fn terminal_cell_metrics(ui: &egui::Ui, font_size: f32, color: egui::Color32) -> (f32, f32) {
        ui.ctx().fonts(|fonts| {
            let font_id = egui::FontId::monospace(font_size);
            let width_galley = fonts.layout_no_wrap("M".to_string(), font_id, color);
            let line_height = Self::measure_terminal_line_height(fonts, font_size, color);
            ((width_galley.size().x / 64.0).max(6.0), line_height)
        })
    }

    #[inline]
    fn line_char_len(lines: &[&str], line: usize) -> Option<usize> {
        lines.get(line).map(|s| s.chars().count())
    }

    /// display-link 命中扫描（#90 / #120）：视口每一行走 `on_output_line` → `DecorateLink`。
    /// 只问 display-link 这一个模块，避免把重扫发给 trigger。禁用时返回空（≈ 模块不存在）。
    /// 仅在 layout 缓存重建时跑一次，随 galley 复用。
    fn scan_display_links(
        registry: &crate::capability::ModuleRegistry,
        formatted: &str,
    ) -> Vec<(usize, DisplaySpan)> {
        if !registry.is_enabled(crate::modules::display_link::MODULE_ID) {
            return Vec::new();
        }
        formatted
            .lines()
            .enumerate()
            .flat_map(|(row, line)| {
                registry
                    .on_output_line_for(
                        crate::modules::display_link::MODULE_ID,
                        &crate::capability::OutputLineCtx::viewport(line),
                    )
                    .into_iter()
                    .filter_map(move |act| DisplaySpan::from_action(act).map(|span| (row, span)))
            })
            .collect()
    }

    /// 链接字节区间 → 字符列区间（egui 光标/绘制都是列粒度）。
    /// 字节偏移落在字符边界外时返回 `None`（防御性，扫描层不会产生这种偏移）。
    fn display_link_char_range(line: &str, link: &DisplaySpan) -> Option<(usize, usize)> {
        let start = line.get(..link.start)?.chars().count();
        let end = line.get(..link.end)?.chars().count();
        Some((start, end))
    }

    /// 指针位置命中的 display-link（行号 + 克隆链接）。
    fn display_link_at(
        display_links: &[(usize, DisplaySpan)],
        galley: &egui::Galley,
        text_draw_pos: egui::Pos2,
        pos: egui::Pos2,
        lines: &[&str],
    ) -> Option<(usize, DisplaySpan)> {
        let (row_i, col) = Self::terminal_pointer_row_col(text_draw_pos, pos, galley, lines);
        let line = lines.get(row_i)?;
        display_links.iter().find_map(|(r, link)| {
            if *r != row_i {
                return None;
            }
            let (s, e) = Self::display_link_char_range(line, link)?;
            (col >= s && col < e).then(|| (*r, link.clone()))
        })
    }

    /// display-link 下划线：idle / hover 都不超过 1 物理像素，accent 带透明度。
    fn display_link_underline_stroke(
        theme: &Theme,
        ctx: &egui::Context,
        hovered: bool,
    ) -> egui::Stroke {
        let alpha = if hovered { 180 } else { 150 };
        egui::Stroke::new(theme.hairline_width(ctx), theme.accent_alpha(alpha))
    }

    /// 点选/拖选命中：必须跟 galley 行盒一致。
    /// 均匀 `cell_h = scroll_h/rows` 会与 epaint 每行 `round_to_pixel` 累积漂移，
    /// HiDPI 下常偏到上一行（选区高亮对、指针下的行不对）。
    fn terminal_pointer_row_col(
        text_draw_pos: egui::Pos2,
        pos: egui::Pos2,
        galley: &egui::Galley,
        lines: &[&str],
    ) -> (usize, usize) {
        if galley.rows.is_empty() {
            return (0, 0);
        }
        let cursor = galley.cursor_from_pos(pos - text_draw_pos);
        let row_i = cursor.rcursor.row.min(galley.rows.len().saturating_sub(1));
        let mut col = cursor.rcursor.column;
        if let Some(len) = Self::line_char_len(lines, row_i) {
            col = col.min(len);
        } else if let Some(grow) = galley.rows.get(row_i) {
            col = col.min(grow.char_count_excluding_newline());
        }
        (row_i, col)
    }

    /// 块状光标：与格子同高同顶(`row.height()`)，勿另取 fonts.row_height 以免和 LayoutJob 行高不一致。
    fn terminal_block_cursor_rect(
        text_draw_pos: egui::Pos2,
        galley: &egui::Galley,
        cur: crate::terminal::ViewportCursor,
        cell_w: f32,
    ) -> egui::Rect {
        let row_i = cur.row.min(galley.rows.len().saturating_sub(1));
        let row = &galley.rows[row_i];
        let y = text_draw_pos.y + row.rect.min.y;
        let h = row.height().max(1.0);
        let x = text_draw_pos.x + row.x_offset(cur.col);
        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(cell_w, h))
    }

    /// 自绘格子/选区底色：高度夹在「本行顶→下一行顶」并留 1px 缝，避免 AA/亚像素压住下一行字形。
    fn paint_terminal_cell_backgrounds(
        painter: &egui::Painter,
        text_draw_pos: egui::Pos2,
        galley: &egui::Galley,
        cell_bgs: &[Vec<egui::Color32>],
        terminal_bg: egui::Color32,
        cell_w: f32,
    ) {
        for (row_i, row_bgs) in cell_bgs.iter().enumerate() {
            let Some(grow) = galley.rows.get(row_i) else {
                continue;
            };
            let y = text_draw_pos.y + grow.rect.min.y;
            let next_y = galley
                .rows
                .get(row_i + 1)
                .map(|r| text_draw_pos.y + r.rect.min.y)
                .unwrap_or_else(|| y + grow.height());
            // 与下一行顶留 1px，消除选区底压行观感(不做 expand)。
            let h = (next_y - y - 1.0).max(1.0);
            let mut x0 = 0usize;
            while x0 < row_bgs.len() {
                let bg = row_bgs[x0];
                if bg == terminal_bg || bg.a() == 0 {
                    x0 += 1;
                    continue;
                }
                let mut x1 = x0 + 1;
                while x1 < row_bgs.len() && row_bgs[x1] == bg {
                    x1 += 1;
                }
                let left = text_draw_pos.x + grow.x_offset(x0);
                let right = (text_draw_pos.x + grow.x_offset(x1)).max(left + cell_w * 0.5);
                painter.rect_filled(
                    egui::Rect::from_min_max(egui::pos2(left, y), egui::pos2(right, y + h)),
                    0.0,
                    bg,
                );
                x0 = x1;
            }
        }
    }

    fn ime_committed_text<'a>(text: &'a str, modifiers: egui::Modifiers) -> Option<&'a str> {
        if text.is_empty() || modifiers.command || modifiers.ctrl {
            None
        } else {
            Some(text)
        }
    }

    /// #92：在块状光标矩形上，按 preedit 文本加宽 IME 上报区（系统选字窗贴着预编辑宽度）。
    fn ime_rect_for_preedit(
        ui: &egui::Ui,
        cursor_rect: egui::Rect,
        preedit: &str,
        font_size: f32,
        cell_w: f32,
        clip: egui::Rect,
    ) -> egui::Rect {
        let mut rect = cursor_rect;
        if !preedit.is_empty() {
            let preedit_w = ui
                .fonts(|fonts| {
                    fonts
                        .layout_no_wrap(
                            preedit.to_owned(),
                            egui::FontId::monospace(font_size),
                            egui::Color32::WHITE,
                        )
                        .size()
                        .x
                })
                .max(cell_w);
            rect = egui::Rect::from_min_size(
                rect.min,
                egui::vec2(preedit_w.max(rect.width()), rect.height()),
            );
        }
        // 不超出终端可视区，避免候选窗锚到屏外。
        rect.intersect(clip)
    }

    /// 在 PTY 光标处放置透明单行框，供系统 IME 定位并启用 `set_ime_allowed`。
    fn show_terminal_ime_capture(
        &mut self,
        ui: &mut egui::Ui,
        ime_id: egui::Id,
        ime_rect: egui::Rect,
        cell_h: f32,
    ) {
        self.last_ime_id = Some(ime_id);
        ui.allocate_ui_at_rect(ime_rect, |ui| {
            ui.set_min_size(egui::vec2(ime_rect.width().max(8.0), ime_rect.height().max(cell_h)));
            ui.visuals_mut().override_text_color = Some(egui::Color32::TRANSPARENT);
            // 勿绘制 TextEdit 竖线光标；终端仅用下方块状光标 + 本框仅向 winit 报告 IME 位置
            ui.visuals_mut().text_cursor =
                egui::Stroke::new(0.0, egui::Color32::TRANSPARENT);
            ui.add(
                egui::TextEdit::singleline(&mut self.ime_capture)
                    .id(ime_id)
                    .frame(false)
                    .margin(egui::vec2(0.0, 0.0))
                    .lock_focus(true)
                    .desired_width(ime_rect.width().max(8.0)),
            );
        });
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                ime_id,
                crate::ui::terminal_keys::terminal_keyboard_event_filter(),
            );
        });
        self.ime_capture.clear();
        if let Some(mut state) = egui::widgets::text_edit::TextEditState::load(ui.ctx(), ime_id) {
            state.set_ccursor_range(Some(egui::text::CCursorRange::one(
                egui::text::CCursor::new(0),
            )));
            state.store(ui.ctx(), ime_id);
        }
    }

    /// 在光标处画出未上屏的预编辑（下划线 + 浅底）。候选窗由系统 IME 面板负责。
    fn paint_ime_preedit(
        &self,
        ui: &egui::Ui,
        theme: &Theme,
        origin: egui::Pos2,
        clip: egui::Rect,
    ) {
        let text = self.ime_preedit.text();
        if text.is_empty() {
            return;
        }
        let color = theme.text_primary();
        let font_id = egui::FontId::monospace(self.font_size);
        let galley = ui.fonts(|fonts| fonts.layout_no_wrap(text.to_owned(), font_id, color));
        let mut rect = egui::Rect::from_min_size(origin, galley.size());
        // 下划线落在行盒内，避免压到下一行。
        let underline_y = (origin.y + rect.height() - 1.0).min(clip.bottom() - 1.0);
        rect.max.y = underline_y.max(rect.min.y + 1.0);
        let painter = ui.painter().with_clip_rect(clip);
        painter.rect_filled(rect, 0.0, theme.accent_alpha(48));
        painter.galley(origin, galley);
        painter.hline(
            origin.x..=origin.x + rect.width().max(8.0),
            underline_y,
            egui::Stroke::new(1.0, theme.accent_color()),
        );
    }

    /// 把键盘/IME 让给 UI TextEdit(AI 草稿、偏好等)。
    /// 半角 ASCII 的 `Event::Text` 可被多个监听者看到；全角标点走 IME Composition，只进「当前 IME 目标」。
    /// 若不 surrender 终端 IME 框，全角 `，。` 会进已隐藏的 ime_capture 并被清空，AI 框像「打不出来」。
    pub fn release_keyboard_for_ui_input(&mut self, ctx: &egui::Context) {
        self.terminal_wants_keyboard = false;
        self.terminal_focused = false;
        self.pending_focus_terminal = false;
        if let Some(id) = self.last_ime_id.take() {
            ctx.memory_mut(|m| m.surrender_focus(id));
        }
        self.ime_capture.clear();
        self.ime_preedit.clear();
        // 打开 AI/表单时清掉拖选灰底，避免盖住终端内容。
        if !self.selection.is_empty() {
            self.selection.clear();
            self.visual_layout_cache = None;
        }
    }

    #[inline]
    fn is_terminal_token_char(ch: char) -> bool {
        ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | '\\' | '@' | '+' | '=')
    }

    /// 双击时按终端 token 规则选中一个词元。
    /// 字母数字与 `_-.\/@+=` 算同一个词（含 base64 padding）。冒号是边界（`host:path`、提示符末尾的 `:` 不会被带上）。
    fn terminal_token_range_at(line: &str, col: usize) -> Option<(usize, usize)> {
        let chars: Vec<char> = line.chars().collect();
        if chars.is_empty() {
            return None;
        }
        let len = chars.len();
        // 严格模式：只允许命中“当前字符”或“紧邻左侧字符”，绝不跨空格跳词。
        let mut idx = col.min(len.saturating_sub(1));
        if !Self::is_terminal_token_char(chars[idx]) {
            if idx > 0 && Self::is_terminal_token_char(chars[idx - 1]) {
                idx -= 1;
            } else {
                return None;
            }
        }

        while idx > 0 && Self::is_terminal_token_char(chars[idx - 1]) {
            idx -= 1;
        }
        let start = idx;
        let mut end = start;
        while end < len && Self::is_terminal_token_char(chars[end]) {
            end += 1;
        }
        Some((start, end))
    }

    /// 本机状态行：truecolor ANSI(随主题)，避免被 §2.3.2 输出压暗后在黑底上几乎看不见。
    fn feed_user_error_line(&mut self, theme: &Theme, ctx: &egui::Context, message: &str) {
        let prefix = crate::i18n::tr(ctx, "Error: ", "错误：");
        let line = format_user_error_line(theme, prefix, message);
        self.terminal.feed(line.as_bytes());
    }

    fn feed_user_info_line(&mut self, theme: &Theme, message: &str) {
        let line = format_user_info_line(theme, message);
        self.terminal.feed(line.as_bytes());
    }

    fn feed_user_success_line(&mut self, theme: &Theme, message: &str) {
        let line = format_user_success_line(theme, message);
        self.terminal.feed(line.as_bytes());
    }

    fn feed_user_warn_line(&mut self, theme: &Theme, message: &str) {
        let line = format_user_warn_line(theme, message);
        self.terminal.feed(line.as_bytes());
    }

    /// §4.3.2：拖放文件到终端区域时收集路径(由宿主决定 SCP / ZMODEM)。
    fn collect_file_drops_into(ui: &egui::Ui, pending: &mut Vec<PathBuf>) {
        if ui.ctx().input(|i| i.raw.dropped_files.is_empty()) {
            return;
        }
        let rect = ui.clip_rect();
        let inside = ui.ctx().input(|i| {
            i.pointer
                .interact_pos()
                .map(|p| rect.contains(p))
                .unwrap_or(false)
        });
        if !inside {
            return;
        }
        ui.ctx().input(|i| {
            for f in &i.raw.dropped_files {
                if let Some(p) = &f.path {
                    if !pending.iter().any(|x| x == p) {
                        pending.push(p.clone());
                    }
                }
            }
        });
    }

    /// 本帧拖入但无指针位置的文件(从 Finder 等其他应用拖入时，系统拖拽期间窗口收不到
    /// 鼠标移动，egui 指针位置为空)：[`Self::collect_file_drops_into`] 无法判定落在哪个
    /// 终端，交由宿主投给活动终端。有指针位置时返回空，仍按落点归属终端。
    pub(crate) fn unplaced_dropped_files(input: &egui::InputState) -> Vec<PathBuf> {
        if input.pointer.interact_pos().is_some() {
            return Vec::new();
        }
        input
            .raw
            .dropped_files
            .iter()
            .filter_map(|f| f.path.clone())
            .collect()
    }

    #[inline]
    fn locale_last(&self) -> crate::i18n::Locale {
        crate::i18n::Locale::from(self.ui_lang_last)
    }

    fn contains_shell_prompt_fragment(text: &str) -> bool {
        text.contains("$ ") || text.contains("# ") || text.contains("> ")
    }

    /// 上传旁路开启时默认不把 PTY 画进 VTE(避免 ZMODEM 二进制污染)；设 `MISTTERM_ZMODEM_MIRROR_RZ_TEXT=1` 可把**疑似纯文本**片段镜像到终端，便于看 `rz -vv`。
    fn mirror_rz_text_to_vte_enabled() -> bool {
        std::env::var("MISTTERM_ZMODEM_MIRROR_RZ_TEXT")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    }

    /// 可安全映到 VTE：不得含 ZMODEM ZDLE，以免损坏模拟器状态。
    fn pty_chunk_safe_to_mirror_vte(data: &[u8]) -> bool {
        if data.is_empty() || data.contains(&0x18) {
            return false;
        }
        data.iter().all(|&b| {
            matches!(b, b'\n' | b'\r' | b'\t' | b'\x08' | b'\x1b')
                || (0x20..0x7f).contains(&b)
        })
    }


    /// 创建新的终端视图
    pub fn new() -> Self {
        let download_dir = crate::core::local_dir::current().dir;

        Self {
            session_id: None,
            ssh_manager: None,
            ssh_rx: None,
            ssh_handle: None,
            local_handle: None,
            connection_kind: ConnectionKind::Ssh,
            terminal: VtTerminal::new(80, 24),
            connected: false,
            error_message: None,
            cols: 80,
            rows: 24,
            lrzsz: LrzszTransfer::new(&download_dir.to_string_lossy()),
            show_fragment_panel: false,
            pending_rz_upload: false,
            transfer_progress: None,
            transfer_outgoing: false,
            pending_focus_terminal: false,
            font_size: crate::platform::DEFAULT_TERMINAL_FONT_SIZE,
            connected_at: None,
            connection_target: None,
            auto_follow_output: true,
            terminal_wants_keyboard: false,
            terminal_focused: false,
            rz_control_mode_until: None,
            expect_rz_upload_until: None,
            expect_sz_download_until: None,
            upload_result_rx: None,
            command_usage: HashMap::new(),
            paste_pending: Vec::new(),
            paste_next_chunk_at: None,
            paste_retries: 0,
            buffer_input_while_disconnected: false,
            disconnected_input_buffer: Vec::new(),
            resend_offline_input_dialog_open: false,
            pending_post_connect_command: None,
            post_connect_fire_at: None,
            vt_visual_generation: 0,
            visual_layout_cache: None,
            repaint_probe_received_us: None,
            repaint_probe_cache_hit: false,
            repaint_probe_row_hash: None,
            repaint_probe_epoch: 0,
            local_disconnect_intent: false,
            unexpected_disconnect_notified: false,
            pending_attention_ended: false,
            pending_connect_audit: None,
            server_audit_probe: ServerAuditProbe::new(),
            pending_server_audit: Vec::new(),
            agent_audit_degraded: false,
            agent_audit_banner_dismissed: false,
            pending_toast_error: None,
            pending_connection_placeholder_action: ConnectionPlaceholderAction::None,
            pending_drop_upload_paths: Vec::new(),
            zmodem_upload_after_rz_path: None,
            selection: Selection::default(),
            select_autoscroll_acc: 0.0,
            pending_send_to_ai: false,
            pending_send_tail_to_ai: false,
            pending_send_to_ai_text: None,
            typed_line_buffer: String::new(),
            shell_cwd: None,
            pending_startup_cd: None,
            broadcast_targets: Vec::new(),
            broadcast_suppress: false,
            zmodem_pty_detect_tail: Vec::new(),
            pty_outbound_line_buffer: String::new(),
            submitted_line: None,
            typed_enter_gate: false,
            typed_enter_request: None,
            interactive_capture_on: false,
            capture_session_id: String::new(),
            capture_target: String::new(),
            pending_capture_outcome: String::new(),
            open_capture: None,
            pending_exec_records: Vec::new(),
            pending_log_commands: Vec::new(),
            pending_log_output: Vec::new(),
            search_highlight: None,
            ui_lang_last: UiLanguage::default(),
            ime_capture: String::new(),
            ime_preedit: crate::ui::terminal_keys::ImePreedit::default(),
            last_ime_id: None,
            pending_display_link_sftp: None,
            pending_display_link_runbook: None,
            pending_display_link_connect: None,
            context_menu_display_link: None,
            module_registry: std::sync::Arc::new(crate::capability::ModuleRegistry::builtin()),
            module_output_line_acc: String::new(),
            pending_module_actions: Vec::new(),
            pending_module_menu_action: None,
        }
    }

    pub fn set_search_highlight(&mut self, highlight: Option<(usize, usize, usize)>) {
        if self.search_highlight != highlight {
            self.search_highlight = highlight;
            self.visual_layout_cache = None;
        }
    }

    pub fn take_pending_log_output(&mut self) -> Option<Vec<u8>> {
        self.pending_log_output.pop()
    }

    /// Enter 提交后取走当前行命令(供命令历史记录)
    pub fn take_submitted_line(&mut self) -> Option<String> {
        self.submitted_line.take()
    }

    /// 取走待写入会话日志的命令
    pub fn take_pending_log_command(&mut self) -> Option<String> {
        if self.pending_log_commands.is_empty() {
            None
        } else {
            Some(self.pending_log_commands.remove(0))
        }
    }

    /// 宿主设置：本地命令审计策略是否生效。关闭时回车行为与以往完全一致。
    pub fn set_typed_enter_gate(&mut self, on: bool) {
        self.typed_enter_gate = on;
    }

    pub fn interactive_capture_is_on(&self) -> bool {
        self.interactive_capture_on
    }

    /// 本窗格是否把回车提交的命令写入 exec_history。关掉时不再攒输出。
    pub fn set_interactive_capture(&mut self, on: bool, session_id: &str, target: &str) {
        if self.capture_session_id != session_id {
            self.capture_session_id = session_id.to_string();
        }
        if self.capture_target != target {
            self.capture_target = target.to_string();
        }
        if self.interactive_capture_on && !on {
            self.seal_open_capture();
        }
        self.interactive_capture_on = on;
    }

    /// 审计放行前标明这条记录的结论。须在 [`Self::release_typed_enter`] 之前调用。
    pub fn set_pending_capture_outcome(&mut self, outcome: &str) {
        self.pending_capture_outcome = outcome.to_string();
    }

    /// 服务器事后改结论。`blocked` / `cancelled` 立刻封口，避免再等提示符。
    pub fn note_server_audit_outcome(&mut self, command: &str, outcome: &str) {
        let matches_cmd = |cmd: &str| command.is_empty() || cmd == command;
        let open_matches = self
            .open_capture
            .as_ref()
            .map(|draft| matches_cmd(draft.command()))
            .unwrap_or(false);
        if open_matches {
            if let Some(draft) = self.open_capture.as_mut() {
                draft.set_outcome(outcome);
            }
            if matches!(outcome, "blocked" | "cancelled") {
                if let Some(draft) = self.open_capture.take() {
                    self.pending_exec_records.push(draft.into_record());
                }
            }
            return;
        }
        if let Some(rec) = self
            .pending_exec_records
            .iter_mut()
            .rev()
            .find(|rec| matches_cmd(&rec.command))
        {
            apply_interactive_outcome(rec, outcome);
        }
    }

    /// 取走已结束、尚未落盘的交互式记录。
    pub fn take_interactive_exec_records(&mut self) -> Vec<ExecLogRecord> {
        std::mem::take(&mut self.pending_exec_records)
    }

    fn seal_open_capture(&mut self) {
        if let Some(draft) = self.open_capture.take() {
            self.pending_exec_records.push(draft.into_record());
        }
    }

    fn flush_interactive_to_disk(&mut self) {
        self.seal_open_capture();
        for rec in self.pending_exec_records.drain(..) {
            crate::core::exec_history::append_record(&rec);
        }
    }

    fn begin_interactive_capture(&mut self, command: &str) {
        // 用掉这次提交的结论，避免口令行 / 捕获关闭时把标签留到下一条命令。
        let outcome = if self.pending_capture_outcome.is_empty() {
            "allowed".to_string()
        } else {
            std::mem::take(&mut self.pending_capture_outcome)
        };
        if !self.interactive_capture_on || !self.connected {
            return;
        }
        let command = command.trim();
        if command.is_empty() {
            return;
        }
        if self.terminal.is_alt_screen()
            || self.lrzsz.is_active()
            || self.transfer_progress.is_some()
        {
            return;
        }
        if Self::looks_like_secret_prompt(&self.terminal.cursor_line_text()) {
            return;
        }
        self.seal_open_capture();
        self.open_capture = Some(InteractiveExecDraft::start(
            &self.capture_session_id,
            &self.capture_target,
            command,
            &outcome,
        ));
    }

    fn observe_interactive_output(&mut self, data: &[u8]) {
        if self.open_capture.is_none() || data.is_empty() {
            return;
        }
        let finished = self
            .open_capture
            .as_mut()
            .map(|draft| draft.feed(data))
            .unwrap_or(false);
        if finished {
            if let Some(draft) = self.open_capture.take() {
                self.pending_exec_records.push(draft.into_record());
            }
        }
    }

    /// 取走等待审计的手敲行。宿主须随后调用 [`Self::release_typed_enter`](放行)或丢弃(拦截/取消)。
    pub fn take_typed_enter_request(&mut self) -> Option<String> {
        self.typed_enter_request.take()
    }

    /// 审计放行后真正提交本行：记历史并向 PTY 发 `\r`。
    pub fn release_typed_enter(&mut self) {
        self.commit_typed_line_on_enter();
        if let Err(e) = self.send_pty_input(b"\r") {
            log::error!("PTY write (enter): {}", e);
        }
    }

    /// 键盘回车：策略生效且本行可信时先交宿主审计，否则照旧直接发 `\r`。
    fn submit_enter_key(&mut self) {
        if self.typed_enter_request.is_some() {
            // 上一次回车仍在等审计，不重复提交。
            return;
        }
        if let Some(line) = self.typed_enter_gate_line() {
            self.typed_enter_request = Some(line);
            return;
        }
        self.release_typed_enter();
    }

    /// 需要审计的手敲行；本地行缓冲不可信或不是 shell 命令行时返回 `None`(保持原行为)。
    fn typed_enter_gate_line(&self) -> Option<String> {
        if !self.typed_enter_gate || !self.connected {
            return None;
        }
        if self.terminal.is_alt_screen()
            || self.lrzsz.is_active()
            || self.transfer_progress.is_some()
        {
            return None;
        }
        let line = self.typed_line_buffer.trim();
        if line.is_empty() {
            return None;
        }
        if Self::looks_like_secret_prompt(&self.terminal.cursor_line_text()) {
            return None;
        }
        Some(line.to_string())
    }

    /// 光标所在行像密码 / 口令提示(sudo、ssh、su 等)时不审计，避免把密码当命令处理。
    fn looks_like_secret_prompt(line: &str) -> bool {
        let l = line.to_lowercase();
        l.contains("password") || l.contains("passphrase") || l.contains("密码")
    }

    /// 记录本行提交(历史/rz·sz 探测)；**不**决定是否向 PTY 发回车。
    /// 方向键改行、历史回忆时本地 buffer 常为空，若据此拦截 `\r` 会导致「能左右移却不能回车」。
    fn commit_typed_line_on_enter(&mut self) {
        let line = self.typed_line_buffer.trim().to_string();
        self.typed_line_buffer.clear();
        if line.is_empty() {
            return;
        }
        self.note_typed_cd(&line);
        if crate::ui::terminal_shell_command::is_rz_shell_command(&line) {
            self.note_rz_shell_command_submitted();
        }
        if crate::ui::terminal_shell_command::is_sz_shell_command(&line) {
            self.note_sz_shell_command_submitted();
        }
        self.submitted_line = Some(line.clone());
        self.pending_log_commands.push(line.clone());
        self.begin_interactive_capture(&line);
    }

    fn expect_rz_upload_active(&self) -> bool {
        self.expect_rz_upload_until
            .map(|u| Instant::now() <= u)
            .unwrap_or(false)
            || self.zmodem_upload_after_rz_path.is_some()
    }

    fn note_rz_shell_command_submitted(&mut self) {
        self.expect_rz_upload_until = Some(Instant::now() + Duration::from_secs(45));
    }

    fn expect_sz_download_active(&self) -> bool {
        self.expect_sz_download_until
            .map(|u| Instant::now() <= u)
            .unwrap_or(false)
    }

    fn note_sz_shell_command_submitted(&mut self) {
        self.expect_sz_download_until = Some(Instant::now() + Duration::from_secs(30));
    }

    fn clear_expect_sz_download(&mut self) {
        self.expect_sz_download_until = None;
    }

    fn clear_expect_rz_upload(&mut self) {
        self.expect_rz_upload_until = None;
    }

    fn enqueue_log_command(&mut self, command: &str) {
        let trimmed = command.trim();
        if !trimmed.is_empty() {
            self.pending_log_commands.push(trimmed.to_string());
        }
    }

    fn track_typed_text(&mut self, text: &str) {
        for ch in text.chars() {
            if ch == '\n' || ch == '\r' {
                continue;
            }
            if ch.is_control() && ch != '\t' {
                continue;
            }
            self.typed_line_buffer.push(ch);
        }
    }

    /// 本行缓冲跟踪控制键：Ctrl+U / Ctrl+C 清空整行，Ctrl+W 删前一个词(与 readline 一致)。
    fn track_ctrl_byte(&mut self, b: u8) {
        match b {
            0x15 | 0x03 => {
                self.typed_line_buffer.clear();
                self.pty_outbound_line_buffer.clear();
            }
            0x17 => {
                let t = self.typed_line_buffer.trim_end_matches(' ').len();
                self.typed_line_buffer.truncate(t);
                let cut = self.typed_line_buffer.rfind(' ').map(|i| i + 1).unwrap_or(0);
                self.typed_line_buffer.truncate(cut);
            }
            0x08 => self.track_backspace(),
            _ => {}
        }
    }

    /// 被拦截/取消的手敲行：字符早已逐键发到远端，向该窗格发 Ctrl+U 清掉远端输入行，并清空本地缓冲，
    /// 否则下一次回车会把旧内容和新输入拼在一起发出去。
    pub fn discard_typed_line(&mut self) {
        self.typed_enter_request = None;
        self.typed_line_buffer.clear();
        self.pty_outbound_line_buffer.clear();
        if let Err(e) = self.send_pty_input(&[0x15]) {
            log::error!("PTY write (kill-line): {}", e);
        }
    }

    fn track_backspace(&mut self) {
        if !self.typed_line_buffer.is_empty() {
            self.typed_line_buffer.pop();
        }
        if !self.pty_outbound_line_buffer.is_empty() {
            self.pty_outbound_line_buffer.pop();
        }
    }

    fn note_pty_outbound_for_rz(&mut self, data: &[u8]) {
        for &b in data {
            if b == b'\r' || b == b'\n' {
                let line = self.pty_outbound_line_buffer.trim().to_string();
                self.pty_outbound_line_buffer.clear();
                if crate::ui::terminal_shell_command::is_rz_shell_command(&line) {
                    self.note_rz_shell_command_submitted();
                }
                if crate::ui::terminal_shell_command::is_sz_shell_command(&line) {
                    self.note_sz_shell_command_submitted();
                }
            } else if b == 0x7f || b == 0x08 {
                self.pty_outbound_line_buffer.pop();
            } else if (0x20..=0x7e).contains(&b) || b == b'\t' {
                if self.pty_outbound_line_buffer.len() < 512 {
                    self.pty_outbound_line_buffer.push(b as char);
                }
            }
        }
    }

    fn scroll_to_bottom_on_user_input(&mut self) {
        if !self.terminal.is_scrolled_to_bottom() {
            self.terminal.scroll_to_bottom();
            self.visual_layout_cache = None;
            self.auto_follow_output = true;
        }
    }

    fn has_pty_writer(&self) -> bool {
        match self.connection_kind {
            ConnectionKind::Local => self
                .local_handle
                .as_ref()
                .map(|h| h.is_alive())
                .unwrap_or(false),
            ConnectionKind::Ssh => self.ssh_handle.is_some(),
        }
    }

    pub fn is_local_shell(&self) -> bool {
        self.connection_kind == ConnectionKind::Local
    }

    fn send_pty_input(&mut self, data: &[u8]) -> Result<(), String> {
        let __t0 = std::time::Instant::now();
        if !data.is_empty() {
            // 键入时清选区，避免拖选灰底残留盖住新输出(对标 Windows Terminal)。
            if !self.selection.is_empty() {
                self.selection.clear();
                self.visual_layout_cache = None;
            }
            self.scroll_to_bottom_on_user_input();
        }
        let __t1 = std::time::Instant::now();
        if self.is_local_shell() {
            let __res = self
                .local_handle
                .as_ref()
                .ok_or_else(|| "local shell handle missing".to_string())?
                .send_input(data);
            let __t3 = std::time::Instant::now();
            let __total = (__t3 - __t0).as_micros() as u64;
            if __total >= 5_000 {
                log::warn!(
                    "TERM_SLOW send_pty_input(local) total={}us n={}",
                    __total,
                    data.len()
                );
            }
            return self.finish_broadcast(data, __res);
        }
        self.note_pty_outbound_for_rz(data);
        let __t2 = std::time::Instant::now();
        let __res = self
            .ssh_handle
            .as_ref()
            .ok_or_else(|| "ssh handle missing".to_string())?
            .send_input(data);
        let __t3 = std::time::Instant::now();
        let __total = (__t3 - __t0).as_micros() as u64;
        if __total >= 5_000 {
            log::warn!(
                "TERM_SLOW send_pty_input total={}us n={} first_byte={:02x} stages: scroll={}us rz_note={}us pump_send={}us",
                __total,
                data.len(),
                data.first().copied().unwrap_or(0),
                (__t1 - __t0).as_micros(),
                (__t2 - __t1).as_micros(),
                (__t3 - __t2).as_micros(),
            );
        }
        self.finish_broadcast(data, __res)
    }

    fn finish_broadcast(&self, data: &[u8], res: Result<(), String>) -> Result<(), String> {
        if res.is_ok() {
            self.fanout_broadcast(data);
        }
        res
    }

    fn fanout_broadcast(&self, data: &[u8]) {
        if self.broadcast_suppress || data.is_empty() || self.broadcast_targets.is_empty() {
            return;
        }
        if self.lrzsz.is_active() || self.transfer_progress.is_some() {
            return;
        }
        for target in &self.broadcast_targets {
            if let Err(e) = target.send(data) {
                log::debug!("broadcast input: {e}");
            }
        }
    }

    pub(crate) fn shell_cwd(&self) -> Option<&str> {
        self.shell_cwd.as_deref()
    }

    pub(crate) fn clone_input_target(&self) -> Option<BroadcastTarget> {
        if self.is_local_shell() {
            self.local_handle.clone().map(BroadcastTarget::Local)
        } else {
            self.ssh_handle.clone().map(BroadcastTarget::Ssh)
        }
    }

    pub(crate) fn set_broadcast_targets(&mut self, targets: Vec<BroadcastTarget>) {
        self.broadcast_targets = targets;
    }

    pub(crate) fn clear_broadcast_targets(&mut self) {
        self.broadcast_targets.clear();
    }

    /// SSH 分屏：等提示符出现后再 `cd` 到源窗格目录。
    pub(crate) fn arm_startup_directory(&mut self, cwd: &str) {
        let cwd = cwd.trim();
        if cwd.is_empty() {
            return;
        }
        self.shell_cwd = Some(cwd.to_string());
        self.pending_startup_cd = Some(cwd.to_string());
    }

    fn maybe_send_startup_cd(&mut self) {
        if self.pending_startup_cd.is_none() {
            return;
        }
        let line = self.terminal.cursor_line_text();
        if !prompt_ready_for_cd(&line) {
            return;
        }
        let Some(path) = self.pending_startup_cd.take() else {
            return;
        };
        let bytes = startup_cd_payload(&path);
        self.broadcast_suppress = true;
        if let Err(e) = self.send_pty_input(&bytes) {
            log::debug!("startup cd: {e}");
        }
        self.broadcast_suppress = false;
    }

    fn note_cwd_from_screen(&mut self) {
        if let Some(cwd) = parse_prompt_cwd(&self.terminal.cursor_line_text()) {
            self.shell_cwd = Some(cwd);
        }
    }

    fn note_typed_cd(&mut self, line: &str) {
        let home = if self.is_local_shell() {
            dirs::home_dir().map(|p| p.display().to_string())
        } else {
            None
        };
        if let Some(next) = apply_cd_line(self.shell_cwd.as_deref(), line, home.as_deref()) {
            self.shell_cwd = Some(next);
        }
    }

    fn push_zmodem_detect_tail(&mut self, data: &[u8]) {
        const TAIL_CAP: usize = 512;
        if data.is_empty() {
            return;
        }
        self.zmodem_pty_detect_tail.extend_from_slice(data);
        if self.zmodem_pty_detect_tail.len() > TAIL_CAP {
            let drop = self.zmodem_pty_detect_tail.len() - TAIL_CAP;
            self.zmodem_pty_detect_tail.drain(..drop);
        }
    }

    fn zmodem_detect_window(&self) -> &[u8] {
        self.zmodem_pty_detect_tail.as_slice()
    }

    /// 连接到 SSH 服务器
    /// #177：注入连接后恢复命令（来自会话配置；空串 = 不执行）。
    /// 每次 connect 前调用一次，避免上个会话的命令残留。
    pub fn set_post_connect_command(&mut self, cmd: &str) {
        self.pending_post_connect_command = if cmd.trim().is_empty() {
            None
        } else {
            Some(cmd.to_string())
        };
        self.post_connect_fire_at = None;
    }

    pub fn connect(
        &mut self,
        _theme: &Theme,
        host: &str,
        port: u16,
        username: &str,
        password: &str,
        private_key_path: &str,
        use_ssh_agent: bool,
        keepalive_enabled: bool,
        keepalive_interval_secs: u32,
        keepalive_count_max: u8,
        proxy_jump: &str,
        proxy_command: &str,
        jump_hops: Vec<crate::ssh::JumpHop>,
        local_forwards: Vec<crate::ssh::LocalPortForward>,
        remote_forwards: Vec<crate::ssh::RemotePortForward>,
        dynamic_forwards: Vec<crate::ssh::DynamicPortForward>,
    ) {
        let interval = if keepalive_enabled {
            keepalive_interval_secs.max(1)
        } else {
            0
        };
        let config = SshConfig {
            host: host.to_string(),
            port,
            username: username.to_string(),
            password: password.to_string(),
            private_key_path: private_key_path.to_string(),
            use_ssh_agent,
            keepalive_interval_secs: interval,
            keepalive_count_max: keepalive_count_max.max(1),
            proxy_jump: proxy_jump.to_string(),
            proxy_command: proxy_command.to_string(),
            jump_hops,
            local_forwards,
            remote_forwards,
            dynamic_forwards,
        };

        let (manager, rx) = SshManager::new();
        
        match manager.create_session_async(config) {
            Ok(session_id) => {
                self.buffer_input_while_disconnected = false;
                self.disconnected_input_buffer.clear();
                self.resend_offline_input_dialog_open = false;
                self.connection_kind = ConnectionKind::Ssh;
                self.local_handle = None;
                self.session_id = Some(session_id);
                self.ssh_manager = Some(manager);
                self.ssh_rx = Some(rx);
                self.connected = false;
                self.error_message = None;
                self.terminal = VtTerminal::new(self.cols as usize, self.rows as usize);
                self.vt_visual_generation = self.vt_visual_generation.wrapping_add(1);
                self.visual_layout_cache = None;
                self.local_disconnect_intent = false;
                self.connected_at = None;
                self.agent_audit_degraded = false;
                self.agent_audit_banner_dismissed = false;
                self.connection_target = Some((username.to_string(), host.to_string()));
            }
            Err(e) => {
                let msg = format_ssh_connect_error(
                    self.ui_lang_last,
                    &format!("Failed to create session: {}", e),
                );
                self.pending_toast_error = Some(msg.clone());
                self.error_message = Some(msg);
            }
        }
    }

    /// 打开本机默认 shell（不写入侧栏会话列表）。
    pub fn connect_local_shell(&mut self) -> Result<(), String> {
        self.connect_local_shell_in(None)
    }

    /// 在给定目录打开本机 shell。`cwd` 为空或目录不存在时用用户主目录。
    pub fn connect_local_shell_in(&mut self, cwd: Option<&str>) -> Result<(), String> {
        let dir = resolve_local_cwd(cwd);
        let cwd_text = dir.display().to_string();
        let (rx, handle) =
            crate::local_shell::spawn_local_shell_in(self.cols, self.rows, &dir)?;
        self.shell_cwd = Some(cwd_text);
        self.pending_startup_cd = None;
        self.buffer_input_while_disconnected = false;
        self.disconnected_input_buffer.clear();
        self.resend_offline_input_dialog_open = false;
        self.connection_kind = ConnectionKind::Local;
        self.session_id = Some(handle.session_id);
        self.local_handle = Some(handle);
        self.ssh_manager = None;
        self.ssh_handle = None;
        self.ssh_rx = Some(rx);
        self.connected = false;
        self.error_message = None;
        self.terminal = VtTerminal::new(self.cols as usize, self.rows as usize);
        self.vt_visual_generation = self.vt_visual_generation.wrapping_add(1);
        self.visual_layout_cache = None;
        self.local_disconnect_intent = false;
        self.connected_at = None;
        self.agent_audit_degraded = false;
        self.agent_audit_banner_dismissed = false;
        self.connection_target = Some(("local".to_string(), "local".to_string()));
        self.pending_rz_upload = false;
        self.zmodem_upload_after_rz_path = None;
        self.expect_rz_upload_until = None;
        self.expect_sz_download_until = None;
        Ok(())
    }

    /// 显示终端视图。`column_width` 须为宿主在 **标签栏下方** 为终端列 `allocate_ui_with_layout` 的宽度(勿用 `clip_rect`，常为整窗宽)。
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        column_width: f32,
        terminal_search_open: bool,
        capture_pty_keyboard: bool,
    ) {
        self.ui_lang_last = crate::i18n::language(ui.ctx());
        // 先处理网络与键盘，再绘制，避免输入/输出滞后一帧
        crate::ssh::set_repaint_context(ui.ctx());
        let terminal_dirty = self.process_ssh_messages(theme);
        self.flush_paste_queue(ui.ctx());
        self.process_transfer_events(theme, ui.ctx());

        if self.transfer_progress.is_some() || self.lrzsz.is_active() {
            let cancel_shortcut = ui.input_mut(|i| {
                i.consume_key(
                    egui::Modifiers {
                        ctrl: true,
                        ..Default::default()
                    },
                    egui::Key::C,
                )
            });
            if cancel_shortcut {
                if let Err(e) = self.cancel_zmodem_transfer() {
                    let msg = format!(
                        "{}: {}",
                        crate::i18n::tr(ui.ctx(), "Stop transfer failed", "停止传输失败"),
                        e
                    );
                    self.feed_user_warn_line(theme, &msg);
                } else {
                    self.feed_user_warn_line(
                        theme,
                        crate::i18n::tr(ui.ctx(), "Transfer stopped", "传输已停止"),
                    );
                }
            }
        }

        // Ctrl + 滚轮：缩放终端字体(不改变 PTY 行列，仅视觉)
        let wheel = ui.ctx().input(|i| {
            let z = i.scroll_delta.y;
            if self.terminal_focused
                && (self.connected || self.buffer_input_while_disconnected)
                && i.modifiers.ctrl
                && !i.modifiers.shift
                && z.abs() > 0.5
            {
                Some(z)
            } else {
                None
            }
        });
        if let Some(z) = wheel {
            let step = if z > 0.0 { 1.0 } else { -1.0 };
            self.set_font_size(self.font_size + step);
        }

        if capture_pty_keyboard {
            self.capture_inline_input(ui);
        }
        if self.connected {
            // ZMODEM 上传时 PTY 回包经 UI 旁路进 lrzsz；须高频重绘以免 `mpsc` 积压导致「等 ZACK」式卡顿。
            if self.lrzsz.is_upload_pty_capture() {
                ui.ctx().request_repaint();
            } else if terminal_dirty || self.repaint_probe_received_us.is_some() {
                // 仅有输出/队列未排空时拉高帧率(top/vim 等)；空闲靠光标闪烁或用户输入触发重绘，避免单核打满。
                ui.ctx().request_repaint_after(Duration::from_millis(
                    crate::terminal::style::TERMINAL_LIVE_REPAINT_MS,
                ));
            }
        }

        let column_width = column_width.clamp(1.0, 16_384.0);
        // 宿主传入的列宽(已扣右侧 dock)；勿仅用 available_width，否则中央区 max_rect 仍可能盖住右栏。
        ui.set_max_width(column_width);

        // 吃满宿主矩形，但不得超过 column_width(否则 Central 后绘会盖住右栏)
        let mut fill_region = ui.available_size();
        if fill_region.x.is_finite() {
            fill_region.x = fill_region.x.min(column_width);
        }
        if fill_region.x.is_finite()
            && fill_region.y.is_finite()
            && fill_region.x > 0.0
            && fill_region.y > 0.0
        {
            ui.set_min_size(fill_region);
        }
        // 进度条在 Frame 内先占位；若仍用全高算行列，网格会高于 ScrollArea，滚动与「│」光标错位
        // 与 Frame 内底部进度条占位一致(分隔线 + 两行文案 + ProgressBar)
        const TRANSFER_FOOTER_H: f32 = 72.0;
        // README §2.4：主底栏承担状态；PTY 尺寸在 ScrollArea 内容区内按真实 viewport 同步(见下方)
        // 格高被夹到字号×FACTOR 后，首行字形会高出格子顶边；标签栏移到顶栏后(macOS / 沉浸式)
        // 终端上方不再有 tab 行兜住，首行会被裁掉一截。顶边按溢出量补齐。
        let mut content_margin = theme.terminal_content_margin();
        content_margin.top += ui.ctx().fonts(|f| {
            let natural = f.row_height(&egui::FontId::monospace(self.font_size));
            let cell = Self::measure_terminal_line_height(f, self.font_size, theme.text_primary());
            // 字体声明的行高不一定罩得住真实字形(macOS 上 Menlo/回退字体)，
            // 再给一个按字号的保底，任何字体/字号首行都不会被顶栏裁到。
            (natural - cell).max(self.font_size * 0.4).ceil()
        });
        egui::Frame::none()
            .fill(theme.bg_terminal_color())
            .inner_margin(content_margin)
            .show(ui, |ui| {
                // Scroll 滑道、Multiline/TextEdit(code_editor) 背板都用「极暗底色」；
                // 设为终端同色，否则会露出比 bg_terminal 更浅的灰条。
                let prev_extreme = ui.visuals().extreme_bg_color;
                ui.visuals_mut().extreme_bg_color = theme.bg_terminal_color();
                let inner_fill = ui.available_size();
                if inner_fill.x.is_finite()
                    && inner_fill.y.is_finite()
                    && inner_fill.x > 0.0
                    && inner_fill.y > 0.0
                {
                    ui.set_min_size(inner_fill);
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                    // 同上：收窄的 min_rect 会让 Frame.fill 不按整列铺开，左侧/上出现 bg_body 「灰框」
                    let col_w = ui.available_width().min(column_width);
                    let col_h = ui.available_height();
                    ui.set_max_width(column_width);
                    ui.set_min_width(col_w.max(1.0));
                    ui.set_min_height(col_h.max(1.0));
                    Self::collect_file_drops_into(ui, &mut self.pending_drop_upload_paths);
                    let footer_h = if self.transfer_progress.is_some() {
                        TRANSFER_FOOTER_H
                    } else {
                        0.0
                    };
                    let scroll_h = (ui.available_height() - footer_h).max(80.0);
                    // 最后一行贴着裁剪边时，字形会被窗口底边切掉，提示符就像掉下去了。
                    let bottom_pad = ui.ctx().fonts(|fonts| {
                        Self::measure_terminal_line_height(
                            fonts,
                            self.font_size,
                            theme.text_primary(),
                        )
                    });
                    let fit_h = (scroll_h - bottom_pad).max(48.0);

                    // 必须先按视口同步 PTY 行列，再构建 LayoutJob。
                    // 若先构图再 sync：galley 仍是旧行数，而容器已是新高度 + BOTTOM 对齐 → Tab 下出现大块空白。
                    let shell = TerminalShellStyle::from_theme(theme);
                    let cell_w =
                        Self::terminal_cell_metrics(ui, self.font_size, theme.text_primary()).0;
                    self.sync_pty_size_with_ui(
                        ui,
                        egui::vec2(col_w.max(1.0), fit_h.max(1.0)),
                        theme,
                    );

                    let font_bits = self.font_size.to_bits();
                    // 行高铺满 fit_h，底下留 bottom_pad，避免最后一行被裁。
                    let rows_n = self.rows.max(1) as f32;
                    let line_height = fit_h / rows_n;
                    let line_height_bits = line_height.to_bits();
                    let selection_key = if self.selection.is_empty() {
                        None
                    } else {
                        let (sl, sc, el, ec) = self.selection.normalize();
                        Some((sl, sc, el, ec))
                    };
                    let selection_arg = selection_key.map(|(sl, sc, el, ec)| {
                        (
                            sl,
                            sc,
                            el,
                            ec,
                            theme.color_terminal_selection(),
                            theme.color_terminal_selection_fg(),
                        )
                    });
                    let display_link_on = self
                        .module_registry
                        .is_enabled(crate::modules::display_link::MODULE_ID);
                    let cache_key = (
                        self.vt_visual_generation,
                        self.terminal.content_epoch(),
                        self.cols,
                        self.rows,
                        font_bits,
                        line_height_bits,
                        shell.default_fg,
                        shell.terminal_bg,
                        selection_key,
                        display_link_on,
                    );
                    self.repaint_probe_cache_hit = false;
                    let (layout_job, cell_bgs, display_owned) =
                        if let Some(ref c) = self.visual_layout_cache {
                            if c.matches(
                                cache_key.0,
                                cache_key.1,
                                cache_key.2,
                                cache_key.3,
                                cache_key.4,
                                cache_key.5,
                                cache_key.6,
                                cache_key.7,
                                cache_key.8,
                                cache_key.9,
                            ) {
                                self.repaint_probe_cache_hit = true;
                                (
                                    c.layout_job.clone(),
                                    c.cell_bgs.clone(),
                                    c.formatted.clone(),
                                )
                            } else {
                                let (layout_job, cell_bgs) = self.terminal.get_layout_job(
                                    self.font_size,
                                    line_height,
                                    cell_w,
                                    &shell,
                                    self.search_highlight,
                                    selection_arg,
                                );
                                let formatted = self.terminal.get_formatted_output();
                                let display_links =
                                    Self::scan_display_links(&self.module_registry, &formatted);
                                self.visual_layout_cache = Some(TerminalVisualLayoutCache {
                                    vt_gen: cache_key.0,
                                    content_epoch: cache_key.1,
                                    cols: cache_key.2,
                                    rows: cache_key.3,
                                    font_bits: cache_key.4,
                                    line_height_bits: cache_key.5,
                                    fg: cache_key.6,
                                    bg: cache_key.7,
                                    selection: cache_key.8,
                                    display_link_on: cache_key.9,
                                    layout_job: layout_job.clone(),
                                    cell_bgs: cell_bgs.clone(),
                                    formatted: formatted.clone(),
                                    galley: None,
                                    display_links,
                                });
                                (layout_job, cell_bgs, formatted)
                            }
                        } else {
                            let (layout_job, cell_bgs) = self.terminal.get_layout_job(
                                self.font_size,
                                line_height,
                                cell_w,
                                &shell,
                                self.search_highlight,
                                selection_arg,
                            );
                            let formatted = self.terminal.get_formatted_output();
                            let display_links =
                                Self::scan_display_links(&self.module_registry, &formatted);
                            self.visual_layout_cache = Some(TerminalVisualLayoutCache {
                                vt_gen: cache_key.0,
                                content_epoch: cache_key.1,
                                cols: cache_key.2,
                                rows: cache_key.3,
                                font_bits: cache_key.4,
                                line_height_bits: cache_key.5,
                                fg: cache_key.6,
                                bg: cache_key.7,
                                selection: cache_key.8,
                                display_link_on: cache_key.9,
                                layout_job: layout_job.clone(),
                                cell_bgs: cell_bgs.clone(),
                                formatted: formatted.clone(),
                                galley: None,
                                display_links,
                            });
                            (layout_job, cell_bgs, formatted)
                        };

                    let painted_galley = {
                        let reuse = self.visual_layout_cache.as_ref().and_then(|c| {
                            if c.matches(
                                cache_key.0,
                                cache_key.1,
                                cache_key.2,
                                cache_key.3,
                                cache_key.4,
                                cache_key.5,
                                cache_key.6,
                                cache_key.7,
                                cache_key.8,
                                cache_key.9,
                            ) {
                                c.galley.clone()
                            } else {
                                None
                            }
                        });
                        if let Some(g) = reuse {
                            g
                        } else {
                            let g = ui.ctx().fonts(|f| f.layout_job(layout_job.clone()));
                            if let Some(c) = self.visual_layout_cache.as_mut() {
                                c.galley = Some(g.clone());
                            }
                            g
                        }
                    };

                    let prev_scroll_w = ui.spacing().scroll_bar_width;
                    let prev_inactive_fill = ui.visuals().widgets.inactive.bg_fill;
                    ui.spacing_mut().scroll_bar_width = theme.terminal_scroll_bar_width();
                    ui.visuals_mut().widgets.inactive.bg_fill =
                        theme.terminal_scroll_bar_track_fill();
                    // ScrollArea 在内容不足 max_height 时会收缩，导致提示符浮在中间；外层固定 scroll_h 视口
                    let scroll_output = ui
                        .allocate_ui_with_layout(
                            egui::vec2(col_w.max(1.0), scroll_h),
                            egui::Layout::top_down(egui::Align::LEFT),
                            |ui| {
                                let viewport = ui.max_rect();
                                ui.set_min_height(scroll_h);
                                ui.set_max_height(scroll_h);
                                // 钉在「视口减去一行留白」上，最后一行提示符和窗口底边隔开。
                                let view_content_h = (scroll_h - bottom_pad).max(1.0);
                                let overflow =
                                    (painted_galley.size().y - view_content_h).max(0.0);
                                egui::ScrollArea::vertical()
                        .id_source(ui.id().with("terminal_scroll"))
                        .auto_shrink([false, false])
                        .vertical_scroll_offset(overflow)
                        .enable_scrolling(false)
                        .drag_to_scroll(false)
                        .scroll_bar_visibility(
                            egui::containers::scroll_area::ScrollBarVisibility::AlwaysHidden,
                        )
                        .max_height(scroll_h)
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                            ui.set_min_width(ui.available_width().max(1.0));
                            let vw = ui.available_width().max(1.0);
                            // 先铺与格子一致的底色，避免 PTY 行高取整后 galley 略矮露出「另一块」底色
                            let pre = ui
                                .available_rect_before_wrap()
                                .intersect(ui.clip_rect());
                            if pre.width() > 0.0 && pre.height() > 0.0 {
                                ui.painter().rect_filled(pre, 0.0, shell.terminal_bg);
                            }
                            let edit_w = Self::text_width_in_scroll_viewport(vw);
                            let cell_w =
                                Self::terminal_cell_metrics(ui, self.font_size, theme.text_primary())
                                    .0;
                            // 与 LayoutJob 行高一致，避免光标/点选与底边余量错位。
                            let cell_h = line_height;
                            // `String` + 可编辑会触发 egui 插入/IME 光标，与 VT 里「│」叠成双光标；用 `&str` 只读缓冲只保留 PTY 光标
                            let mut display_view: &str = display_owned.as_str();
                            let galley_for_layout = painted_galley.clone();
                            // 文本区高度贴齐视口；行高已按 scroll_h/rows 均摊，galley 应≈scroll_h。
                            let te_h = scroll_h;
                            let mut layouter = |_ui: &egui::Ui, text: &str, _wrap_width: f32| {
                                let _ = text;
                                galley_for_layout.clone()
                            };
                            // 勿让 TextEdit 自带拖选(visuals.selection 灰底)与终端格网选区叠成双色
                            let prev_sel_bg = ui.visuals().selection.bg_fill;
                            let prev_sel_fg = ui.visuals().selection.stroke.color;
                            ui.visuals_mut().selection.bg_fill = egui::Color32::TRANSPARENT;
                            ui.visuals_mut().selection.stroke.color = egui::Color32::TRANSPARENT;
                            // VT 从视口顶铺格子。勿 BOTTOM：galley 略矮于容器时会把整屏沉底，Tab 下留出大块空白。
                            // PTY 行列已在 ScrollArea 外按 scroll_h 同步。
                            // 勿用 ui.add_sized：其内部布局是 centered_and_justified(居中)，
                            // 会把比视口宽的行居中，导致提示符左侧被裁。用 LEFT 布局容器固定视口高。
                            let te_output = ui
                                .allocate_ui_with_layout(
                                    egui::vec2(vw, te_h),
                                    egui::Layout::top_down(egui::Align::LEFT),
                                    |ui| {
                                        ui.set_min_width(vw);
                                        ui.set_min_height(te_h);
                                        ui.set_max_height(te_h);
                                        egui::TextEdit::multiline(&mut display_view)
                                            .id_source(ui.id().with("terminal_text_area"))
                                            .margin(egui::vec2(0.0, 0.0))
                                            .horizontal_align(egui::Align::LEFT)
                                            .vertical_align(egui::Align::TOP)
                                            .font(egui::TextStyle::Monospace)
                                            .desired_width(edit_w)
                                            // 勿用 desired_rows：egui 按 style 行高估算，常大于 LayoutJob 行高，
                                            // 会把 response 撑高；再配合旧 BOTTOM 对齐就会顶上留白。
                                            .desired_rows(1)
                                            .code_editor()
                                            .interactive(false)
                                            .frame(false)
                                            .layouter(&mut layouter)
                                            .show(ui)
                                    },
                                )
                                .inner;
                            let response = te_output.response;
                            let galley = te_output.galley;
                            let text_draw_pos = te_output.text_draw_pos;
                            let text_top = text_draw_pos.y;
                            let text_rect = response.rect;
                            let select_id = response.id.with("terminal_select");
                            // 命中区向左含文本区外的内边距：用户常在首字符左侧点起点(如「50」左边)，
                            // 否则这一下落在 text_rect 外、不记锚点，随后 Shift+单击只能从光标起选。
                            let mut select_rect = text_rect;
                            select_rect.min.x -= 8.0;
                            let select_resp =
                                ui.interact(select_rect, select_id, egui::Sense::click_and_drag());
                            let primary_double_clicked =
                                select_resp.double_clicked_by(egui::PointerButton::Primary);
                            let primary_drag_started =
                                select_resp.drag_started_by(egui::PointerButton::Primary);
                            let primary_dragged =
                                select_resp.dragged_by(egui::PointerButton::Primary);
                            let primary_clicked =
                                select_resp.clicked_by(egui::PointerButton::Primary);
                            let shift_held = ui.input(|i| i.modifiers.shift);
                            ui.visuals_mut().selection.bg_fill = prev_sel_bg;
                            ui.visuals_mut().selection.stroke.color = prev_sel_fg;
                            {
                                let mut te_state = te_output.state;
                                te_state.set_cursor_range(None);
                                te_state.store(ui.ctx(), response.id);
                            }
                            let display_lines: Vec<&str> = display_owned.lines().collect();
                            if log::log_enabled!(target: "repaint_lag", log::Level::Debug) {
                                use std::hash::{Hash, Hasher};
                                let cur = self.terminal.viewport_cursor();
                                let row_txt = cur
                                    .and_then(|c| display_lines.get(c.row).copied())
                                    .unwrap_or("");
                                let mut h = std::collections::hash_map::DefaultHasher::new();
                                row_txt.hash(&mut h);
                                let hv = h.finish();
                                let epoch = self.terminal.content_epoch();
                                let row_changed = self.repaint_probe_row_hash != Some(hv);
                                if row_changed || self.repaint_probe_epoch != epoch {
                                    // 只记长度、位置与是否变化，绝不记录行内容或哈希。
                                    log::debug!(
                                        target: "repaint_lag",
                                        "frame paint epoch={} cache_hit={} cursor={:?} row_len={} row_changed={} galley_rows={} text_rect_h={:.1}",
                                        epoch,
                                        self.repaint_probe_cache_hit,
                                        cur.map(|c| (c.row, c.col)),
                                        row_txt.trim_end().chars().count(),
                                        row_changed,
                                        galley.rows.len(),
                                        text_rect.height(),
                                    );
                                    self.repaint_probe_row_hash = Some(hv);
                                    self.repaint_probe_epoch = epoch;
                                }
                            }
                            // 仅当存在非默认底色(ANSI/选区)时才自绘底并再铺一层 galley；
                            // 否则 TextEdit 已画过字，二次铺屏会让 debug 下单核打满、界面像卡住。
                            let needs_cell_bg = cell_bgs.iter().flatten().any(|c| {
                                *c != shell.terminal_bg && c.a() > 0
                            });
                            if needs_cell_bg {
                                let painter =
                                    ui.painter().clone().with_layer_id(response.layer_id);
                                // TextEdit 已经画过一层字。只补选区底再叠第二层时，
                                // 没被底色盖住的字会叠成两层，看起来整屏变亮。先铺终端底再画一次。
                                let galley_rect =
                                    egui::Rect::from_min_size(text_draw_pos, galley.size());
                                let cover = text_rect.union(galley_rect);
                                if cover.width() > 0.0 && cover.height() > 0.0 {
                                    painter.rect_filled(cover, 0.0, shell.terminal_bg);
                                }
                                Self::paint_terminal_cell_backgrounds(
                                    &painter,
                                    text_draw_pos,
                                    &galley,
                                    &cell_bgs,
                                    shell.terminal_bg,
                                    cell_w,
                                );
                                painter.galley(text_draw_pos, galley.clone());
                                if let Some(t) = self.repaint_probe_received_us.take() {
                                    let ms = crate::ssh::probe_now_us().saturating_sub(t) as f64 / 1000.0;
                                    if ms > 200.0 {
                                        log::warn!(target: "repaint_lag", "SLOW output->frame {:.1} ms (>200ms)", ms);
                                    } else {
                                        log::debug!(target: "repaint_lag", "frame drawn after output {:.1} ms", ms);
                                    }
                                }
                            }
                            // display-link（#90）：命中区间画下划线，悬停变手型。
                            // 与格子底色无关，任何帧都画（下划线在字形上方一层）。
                            let display_links = self
                                .visual_layout_cache
                                .as_ref()
                                .map(|c| c.display_links.clone())
                                .unwrap_or_default();
                            let hover_link = if display_links.is_empty() {
                                None
                            } else {
                                let pointer = ui.input(|i| i.pointer.interact_pos());
                                pointer
                                    .filter(|p| text_rect.contains(*p))
                                    .and_then(|p| {
                                        Self::display_link_at(
                                            &display_links,
                                            &galley,
                                            text_draw_pos,
                                            p,
                                            &display_lines,
                                        )
                                    })
                            };
                            if !display_links.is_empty() {
                                let painter =
                                    ui.painter().clone().with_layer_id(response.layer_id);
                                for (row_i, link) in &display_links {
                                    let Some(grow) = galley.rows.get(*row_i) else {
                                        continue;
                                    };
                                    let Some(line) = display_lines.get(*row_i) else {
                                        continue;
                                    };
                                    let Some((s, e)) = Self::display_link_char_range(line, link)
                                    else {
                                        continue;
                                    };
                                    let hovered = hover_link
                                        .as_ref()
                                        .is_some_and(|(hr, hl)| hr == row_i && hl.text == link.text);
                                    let y = theme.snap_y_to_pixel(
                                        ui.ctx(),
                                        text_draw_pos.y + grow.rect.min.y + grow.height(),
                                    );
                                    let left = text_draw_pos.x + grow.x_offset(s);
                                    let right = (text_draw_pos.x + grow.x_offset(e)).max(left + 1.0);
                                    painter.line_segment(
                                        [egui::pos2(left, y), egui::pos2(right, y)],
                                        Self::display_link_underline_stroke(theme, ui.ctx(), hovered),
                                    );
                                }
                                if hover_link.is_some() {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                    ui.ctx().request_repaint();
                                }
                            }
                            let ime_id = response.id.with("terminal_ime");
                            if !terminal_search_open {
                                if ui.input(|i| i.pointer.any_click()) {
                                    let in_terminal = ui.input(|i| {
                                        i.pointer.interact_pos().is_some_and(|pos| {
                                            text_rect.contains(pos)
                                                || select_resp
                                                    .interact_pointer_pos()
                                                    .is_some_and(|p| text_rect.contains(p))
                                        })
                                    });
                                    self.terminal_wants_keyboard = in_terminal;
                                }
                                if self.pending_focus_terminal {
                                    self.terminal_wants_keyboard = true;
                                    self.pending_focus_terminal = false;
                                }
                            }
                            self.terminal_focused = self.terminal_wants_keyboard
                                && !terminal_search_open
                                && capture_pty_keyboard;
                            if !self.terminal_focused {
                                self.ime_preedit.clear();
                            }

                            if self.terminal_focused {
                                ui.memory_mut(|m| m.request_focus(ime_id));
                            }

                            // 块状闪烁光标(UI 层绘制，勿再用 │ 字符占位)。
                            // 预编辑叠在同一格上，实心块会盖住第一个字形。
                            if self.terminal_focused
                                && !terminal_search_open
                                && self.selection.is_empty()
                                && self.ime_preedit.is_empty()
                            {
                                if let Some(cur) = self.terminal.viewport_cursor() {
                                    let t = ui.input(|i| i.time);
                                    let phase = (t
                                        / crate::terminal::style::TERMINAL_CURSOR_BLINK_PERIOD_SECS)
                                        .floor() as i64
                                        % 2;
                                    if phase == 0 {
                                        let cursor_rect = Self::terminal_block_cursor_rect(
                                            text_draw_pos,
                                            &galley,
                                            cur,
                                            cell_w,
                                        );
                                        let painter =
                                            ui.painter().clone().with_layer_id(response.layer_id);
                                        painter.rect_filled(
                                            cursor_rect,
                                            0.0,
                                            theme.color_terminal_cursor_block(),
                                        );
                                    }
                                }
                                ui.ctx().request_repaint_after(
                                    std::time::Duration::from_millis(60),
                                );
                            }

                            // 中文/日文 IME：须有 TextEdit 文本光标，winit 才会启用输入法。
                            // #92：锚点 = viewport_cursor 块状矩形；preedit 非空时加宽到预编辑宽再上报。
                            if capture_pty_keyboard
                                && self.terminal_wants_keyboard
                                && !terminal_search_open
                            {
                                let cursor_rect = if let Some(cur) =
                                    self.terminal.viewport_cursor()
                                {
                                    Self::terminal_block_cursor_rect(
                                        text_draw_pos,
                                        &galley,
                                        cur,
                                        cell_w,
                                    )
                                } else {
                                    egui::Rect::from_min_size(
                                        egui::pos2(text_rect.min.x, text_top),
                                        egui::vec2(cell_w.max(8.0), cell_h.max(12.0)),
                                    )
                                };
                                let ime_rect = Self::ime_rect_for_preedit(
                                    ui,
                                    cursor_rect,
                                    self.ime_preedit.text(),
                                    self.font_size,
                                    cell_w,
                                    text_rect,
                                );
                                self.show_terminal_ime_capture(ui, ime_id, ime_rect, cell_h);
                            }
                            if self.terminal_focused && !self.ime_preedit.is_empty() {
                                if let Some(cur) = self.terminal.viewport_cursor() {
                                    let cursor_rect = Self::terminal_block_cursor_rect(
                                        text_draw_pos,
                                        &galley,
                                        cur,
                                        cell_w,
                                    );
                                    self.paint_ime_preedit(ui, theme, cursor_rect.min, text_rect);
                                }
                            }

                            // === 文本选择(独立 interact 层，避免 TextEdit 抢鼠标) ===
                            // 滚轮浏览 scrollback(与 alacritty 一致：Delta>0 向上翻历史)
                            if (select_resp.hovered()
                                || response.hovered()
                                || self.terminal_focused)
                                && ui.input(|i| !i.modifiers.ctrl && i.scroll_delta.y.abs() > 0.5)
                            {
                                let dy = ui.input(|i| i.scroll_delta.y);
                                let lines = (dy / cell_h).round().clamp(-64.0, 64.0) as i32;
                                if lines != 0 {
                                    self.terminal.scroll_display(Scroll::Delta(lines));
                                    self.visual_layout_cache = None;
                                }
                            }

                            if !primary_dragged {
                                self.select_autoscroll_acc = 0.0;
                            }
                            if (primary_clicked || primary_drag_started || primary_double_clicked)
                                && shift_held
                            {
                                // Shift+单击/Shift+拖：扩展已有选区终点，不新建选区、不发 PTY。
                                if let Some(pos) = select_resp.interact_pointer_pos() {
                                    let (row_i, col) = Self::terminal_pointer_row_col(
                                        text_draw_pos,
                                        pos,
                                        &galley,
                                        &display_lines,
                                    );
                                    let offset = self.terminal.display_offset();
                                    let anchor = self.terminal.viewport_cursor().map(|c| {
                                        let p = viewport_to_point(
                                            offset,
                                            Point::new(c.row, Column(c.col)),
                                        );
                                        (p.line.0, p.column.0)
                                    });
                                    self.selection.extend_from_viewport(offset, row_i, col, anchor);
                                    ui.ctx().request_repaint();
                                }
                            } else if primary_double_clicked {
                                if let Some(pos) = select_resp.interact_pointer_pos() {
                                    let (row_i, col) = Self::terminal_pointer_row_col(
                                        text_draw_pos,
                                        pos,
                                        &galley,
                                        &display_lines,
                                    );
                                    if let Some(line_text) = display_lines.get(row_i) {
                                        if let Some((start, end)) =
                                            Self::terminal_token_range_at(line_text, col)
                                        {
                                            self.selection.set_from_viewport(
                                                self.terminal.display_offset(),
                                                row_i,
                                                start,
                                                row_i,
                                                end,
                                            );
                                            ui.ctx().request_repaint();
                                        }
                                    }
                                }
                            } else if primary_clicked {
                                // display-link（#90）：单击命中链接时优先于选区锚点。
                                if let Some(pos) = select_resp.interact_pointer_pos() {
                                    if let Some((_, link)) = Self::display_link_at(
                                        &display_links,
                                        &galley,
                                        text_draw_pos,
                                        pos,
                                        &display_lines,
                                    ) {
                                        self.handle_display_link_click(ui.ctx(), &link);
                                    } else {
                                        let (row_i, col) = Self::terminal_pointer_row_col(
                                            text_draw_pos,
                                            pos,
                                            &galley,
                                            &display_lines,
                                        );
                                        self.selection.set_anchor_from_viewport(
                                            self.terminal.display_offset(),
                                            row_i,
                                            col,
                                        );
                                        self.visual_layout_cache = None;
                                        ui.ctx().request_repaint();
                                    }
                                }
                            } else if primary_drag_started {
                                // 以按下点(而非越过拖动阈值后的位置)为起点
                                let origin = ui
                                    .input(|i| i.pointer.press_origin())
                                    .or_else(|| select_resp.interact_pointer_pos());
                                if let Some(pos) = origin {
                                    let (row_i, col) = Self::terminal_pointer_row_col(
                                        text_draw_pos,
                                        pos,
                                        &galley,
                                        &display_lines,
                                    );
                                    self.selection.set_from_viewport(
                                        self.terminal.display_offset(),
                                        row_i,
                                        col,
                                        row_i,
                                        col,
                                    );
                                    ui.ctx().request_repaint();
                                }
                            } else if primary_dragged {
                                let drag_distance = ui.input(|i| {
                                    i.pointer
                                        .press_origin()
                                        .zip(i.pointer.interact_pos())
                                        .map(|(o, p)| o.distance(p))
                                        .unwrap_or(0.0)
                                });
                                if drag_distance >= 3.0 {
                                    // 越过上/下边缘：按距离自动滚动 scrollback，终点随之延伸
                                    if let Some(pos) = select_resp.interact_pointer_pos() {
                                        let lps = drag_autoscroll_lines_per_sec(
                                            pos.y,
                                            text_rect.top(),
                                            text_rect.bottom(),
                                            cell_h,
                                        );
                                        if lps != 0.0 {
                                            let dt = ui.input(|i| i.stable_dt).min(0.1);
                                            self.select_autoscroll_acc += lps * dt;
                                            let whole = self.select_autoscroll_acc.trunc();
                                            if whole != 0.0 {
                                                self.select_autoscroll_acc -= whole;
                                                self.terminal
                                                    .scroll_display(Scroll::Delta(whole as i32));
                                                self.visual_layout_cache = None;
                                            }
                                            ui.ctx().request_repaint();
                                        } else {
                                            self.select_autoscroll_acc = 0.0;
                                        }
                                    }
                                    if let Some(pos) = select_resp.interact_pointer_pos() {
                                        let (row_i, col) = Self::terminal_pointer_row_col(
                                            text_draw_pos,
                                            pos,
                                            &galley,
                                            &display_lines,
                                        );
                                        self.selection.set_end_from_viewport(
                                            self.terminal.display_offset(),
                                            row_i,
                                            col,
                                        );
                                        ui.ctx().request_repaint();
                                    }
                                }
                            }

                            // 选区已写入 LayoutJob 的 TextFormat.background(字形之下)，勿再后画矩形。
                            // display-link 右键：egui 在 secondary_down（按下帧）就创建菜单，须同帧缓存命中；
                            // 仅等 secondary_clicked（抬起）会晚一帧，且 interact_pointer_pos 常为空。
                            if select_resp.hovered()
                                && ui.input(|i| i.pointer.secondary_pressed())
                            {
                                let pos = ui
                                    .input(|i| i.pointer.interact_pos())
                                    .or_else(|| ui.input(|i| i.pointer.hover_pos()));
                                self.context_menu_display_link = pos.and_then(|pos| {
                                    Self::display_link_at(
                                        &display_links,
                                        &galley,
                                        text_draw_pos,
                                        pos,
                                        &display_lines,
                                    )
                                    .map(|(_, link)| link)
                                });
                            }
                            select_resp.context_menu(|ui| {
                                crate::ui::chrome::apply_context_menu_style(ui, theme);
                                // 模块只贡献条目（#117 / #120）。「新开连接」由 display-link 在
                                // 按下帧缓存到 IP 时贡献；菜单跨帧仍读这份缓存，不读已经丢掉的指针（#111）。
                                // 选区项仍在下面，不跟链接命中绑在一起。
                                let cached_link = self.context_menu_display_link.clone();
                                let menu_ctx = crate::capability::MenuCtx {
                                    link_action: cached_link.as_ref().map(|l| l.action.as_str()),
                                    link_text: cached_link.as_ref().map(|l| l.text.as_str()),
                                };
                                let mod_items = self.module_registry.context_menu_items(&menu_ctx);
                                if !mod_items.is_empty() {
                                    let lang = crate::i18n::language(ui.ctx());
                                    for item in mod_items {
                                        let label = if lang == crate::i18n::UiLanguage::Zh {
                                            &item.label_zh
                                        } else {
                                            &item.label_en
                                        };
                                        if crate::ui::chrome::popup_menu_button(ui, theme, label).clicked() {
                                            if item.action
                                                == crate::modules::display_link::ACTION_NEW_CONNECTION
                                            {
                                                if cached_link.as_ref().is_some_and(|l| {
                                                    l.action
                                                        == crate::modules::display_link::ACTION_COPY_OR_CONNECT
                                                }) {
                                                    self.pending_display_link_connect =
                                                        cached_link.as_ref().map(|l| l.text.clone());
                                                }
                                                self.context_menu_display_link = None;
                                            } else {
                                                self.pending_module_menu_action = Some(item.action);
                                            }
                                            ui.close_menu();
                                        }
                                    }
                                    ui.separator();
                                }

                                if !self.selection.is_empty() {
                                    let copy_accel = crate::platform::terminal_copy_accel();
                                    if crate::ui::chrome::popup_menu_button_shortcut(
                                        ui,
                                        theme,
                                        crate::i18n::tr(ui.ctx(), "Copy selection", "复制选中"),
                                        Some(&copy_accel),
                                    )
                                        .clicked()
                                    {
                                        let text = self.get_selected_text();
                                        if !text.is_empty() {
                                            ui.ctx().copy_text(text);
                                        }
                                        ui.close_menu();
                                    }
                                    if crate::ui::chrome::popup_menu_button(
                                        ui,
                                        theme,
                                        crate::i18n::tr(ui.ctx(), "Send to AI", "发送到 AI"),
                                    )
                                        .clicked()
                                    {
                                        let text = self.get_selected_text();
                                        self.pending_send_to_ai_text =
                                            if text.trim().is_empty() { None } else { Some(text) };
                                        self.pending_send_to_ai = true;
                                        ui.close_menu();
                                    }
                                    ui.separator();
                                }
                                if crate::ui::chrome::popup_menu_button(
                                    ui,
                                    theme,
                                    crate::i18n::tr(
                                        ui.ctx(),
                                        "Send last 50 lines to AI",
                                        "最近 50 行发送到 AI",
                                    ),
                                )
                                .clicked()
                                {
                                    self.pending_send_tail_to_ai = true;
                                    ui.close_menu();
                                }
                                if crate::ui::chrome::popup_menu_button(
                                    ui,
                                    theme,
                                    crate::i18n::tr(ui.ctx(), "Copy all", "复制全部"),
                                )
                                .clicked()
                                {
                                    if let Ok(mut clip) = Clipboard::new() {
                                        let _ = clip.set_text(self.terminal.get_formatted_output());
                                    }
                                    ui.close_menu();
                                }
                                let paste_accel = crate::platform::terminal_paste_accel();
                                if crate::ui::chrome::popup_menu_button_shortcut(
                                    ui,
                                    theme,
                                    crate::i18n::tr(ui.ctx(), "Paste", "粘贴"),
                                    Some(&paste_accel),
                                )
                                .clicked()
                                {
                                    if let Ok(mut clip) = Clipboard::new() {
                                        if let Ok(text) = clip.get_text() {
                                            self.paste_text(&text, ui.ctx());
                                        }
                                    }
                                    ui.close_menu();
                                }
                                ui.separator();
                                if crate::ui::chrome::popup_menu_button(
                                    ui,
                                    theme,
                                    crate::i18n::tr(ui.ctx(), "Clear screen", "清屏"),
                                )
                                .clicked()
                                {
                                    self.clear_screen();
                                    ui.close_menu();
                                }
                                if crate::ui::chrome::popup_menu_button(
                                    ui,
                                    theme,
                                    crate::i18n::tr(ui.ctx(), "Font +", "字体 +"),
                                )
                                .clicked()
                                {
                                    self.set_font_size(self.font_size + 1.0);
                                    ui.close_menu();
                                }
                                if crate::ui::chrome::popup_menu_button(
                                    ui,
                                    theme,
                                    crate::i18n::tr(ui.ctx(), "Font −", "字体 -"),
                                )
                                .clicked()
                                {
                                    self.set_font_size(self.font_size - 1.0);
                                    ui.close_menu();
                                }
                            });
                        });
                                self.paint_connection_placeholder(ui, theme, viewport);
                            })
                        .inner;

                    ui.spacing_mut().scroll_bar_width = prev_scroll_w;
                    ui.visuals_mut().widgets.inactive.bg_fill = prev_inactive_fill;

                    // 离开底部(display_offset>0)则停止自动跟随；滚回最新输出后恢复
                    self.auto_follow_output = self.terminal.is_scrolled_to_bottom();
                    let _ = scroll_output;

                    let mut stop_transfer_clicked = false;
                    if let Some(ref progress) = self.transfer_progress {
                        ui.add_space(theme.spacing_sm());
                        ui.separator();
                        let dir = if self.transfer_outgoing {
                            crate::i18n::tr(ui.ctx(), "Local → remote", "本机 → 远端")
                        } else {
                            crate::i18n::tr(ui.ctx(), "Remote → local", "远端 → 本机")
                        };
                        ui.horizontal(|ui| {
                            crate::ui::icons::icon_label_row(
                                ui,
                                crate::ui::icons::IconId::Zmodem,
                                "ZMODEM",
                                theme.font_size_body(),
                                6.0,
                                |t| t.strong().color(theme.accent_color()),
                            );
                            ui.label(
                                egui::RichText::new(dir).color(theme.text_secondary()),
                            );
                            ui.label(egui::RichText::new("·").color(theme.text_tertiary()));
                            ui.label(
                                egui::RichText::new(&progress.0)
                                    .monospace()
                                    .color(theme.text_primary()),
                            );
                        });
                        let percent = if progress.2 > 0 {
                            (progress.1 as f32 / progress.2 as f32 * 100.0).min(100.0)
                        } else {
                            0.0
                        };
                        let detail = format!(
                            "  {} / {} · {:.1}%",
                            human_readable_size(progress.1),
                            human_readable_size(progress.2.max(1)),
                            percent
                        );
                        let bar_w = Self::text_width_in_scroll_viewport(ui.available_width().max(1.0));
                        crate::ui::chrome::progress_bar_labeled(
                            ui,
                            theme,
                            percent / 100.0,
                            &detail,
                            Some(bar_w),
                        );
                        ui.horizontal(|ui| {
                            if crate::ui::chrome::modal_danger_button(
                                ui,
                                theme,
                                crate::i18n::tr(ui.ctx(), "Stop transfer", "停止传输"),
                            )
                            .clicked()
                            {
                                stop_transfer_clicked = true;
                            }
                            ui.add_space(theme.spacing_sm());
                            ui.label(
                                egui::RichText::new(crate::i18n::tr(
                                    ui.ctx(),
                                    "or press Ctrl+C",
                                    "或按 Ctrl+C",
                                ))
                                .small()
                                .color(theme.text_tertiary()),
                            );
                        });
                    }
                    if stop_transfer_clicked {
                        let msg = match self.cancel_zmodem_transfer() {
                            Ok(()) => crate::i18n::tr(ui.ctx(), "Transfer stopped", "传输已停止").to_string(),
                            Err(e) => format!(
                                "{}: {}",
                                crate::i18n::tr(ui.ctx(), "Stop transfer failed", "停止传输失败"),
                                e
                            ),
                        };
                        self.feed_user_warn_line(theme, &msg);
                    }
                });
                ui.visuals_mut().extreme_bg_color = prev_extreme;
            });
        self.render_resend_offline_dialog(ui.ctx(), theme);
        self.fire_post_connect_command(ui.ctx());
    }

    /// `viewport`：ScrollArea **内容区**(inner)的宽 × 可视区高度(与 `max_height(scroll_h)` 一致)，已不含 Frame inner_margin 与纵向滚动条占位。
    fn sync_pty_size_with_ui(&mut self, ui: &egui::Ui, viewport: egui::Vec2, theme: &Theme) {
        let usable_width = Self::text_width_in_scroll_viewport(viewport.x).max(120.0);
        let usable_height = viewport.y.max(48.0);

        // 用真实字体测量单字符网格尺寸，避免 80x24 误差
        let (cell_w, cell_h) = ui.ctx().fonts(|fonts| {
            let font_id = egui::FontId::monospace(self.font_size);
            // 用长串测平均字宽：单个 "M" 的 galley 宽度可能小于实际渲染步进（逐字形像素对齐），导致多算一列、最右列被裁掉
            let width_galley = fonts.layout_no_wrap("M".repeat(64), font_id, theme.text_primary());
            let line_height =
                Self::measure_terminal_line_height(fonts, self.font_size, theme.text_primary());
            ((width_galley.size().x / 64.0).max(6.0), line_height.max(12.0))
        });

        let cols = (usable_width / cell_w).floor().clamp(20.0, 512.0) as u32;
        let rows = (usable_height / cell_h).floor().clamp(5.0, 256.0) as u32;

        if cols != self.cols || rows != self.rows {
            self.resize(cols, rows);
        }
    }

    /// 非活动标签仅消费 SSH 接收队列；若有新内容写入 VTE 则返回 `true`(用于低频重绘)。
    pub fn pump_ssh_only(&mut self, theme: &Theme) -> bool {
        self.process_ssh_messages(theme)
    }

    /// FUNCTIONAL_SPEC §2.4：粘贴分片写入 PTY(每批 ≤4096 字节、不切断 UTF-8，间隔 5ms)。
    ///
    /// 只有写入成功才从队列移除；输入队列满(远端/本地 shell 一时消化不过来)时保留本片、
    /// 稍后重试，不再像旧实现那样先 drain 再发、失败即丢。
    fn flush_paste_queue(&mut self, ctx: &egui::Context) {
        use crate::ui::terminal_paste::{next_chunk_len, PASTE_CHUNK};
        const GAP: Duration = Duration::from_millis(5);
        const RETRY_GAP: Duration = Duration::from_millis(20);
        const MAX_RETRIES: u32 = 500; // ≈10 秒无进展才放弃

        if self.paste_pending.is_empty() {
            self.paste_next_chunk_at = None;
            self.paste_retries = 0;
            return;
        }
        if !self.connected || !self.has_pty_writer() {
            if !self.paste_pending.is_empty() {
                log::warn!("paste aborted: connection gone, {} bytes unsent", self.paste_pending.len());
            }
            self.paste_pending.clear();
            self.paste_next_chunk_at = None;
            self.paste_retries = 0;
            return;
        }

        let now = Instant::now();
        if let Some(t) = self.paste_next_chunk_at {
            if now < t {
                ctx.request_repaint_after(t - now);
                return;
            }
        }

        let n = next_chunk_len(&self.paste_pending, PASTE_CHUNK);
        let chunk: Vec<u8> = self.paste_pending[..n].to_vec();
        match self.send_pty_input(&chunk) {
            Ok(()) => {
                self.paste_pending.drain(..n);
                self.paste_retries = 0;
                if self.paste_pending.is_empty() {
                    self.paste_next_chunk_at = None;
                } else {
                    self.paste_next_chunk_at = Some(now + GAP);
                    ctx.request_repaint_after(GAP);
                }
            }
            Err(e) if e.contains("queue full") && self.paste_retries < MAX_RETRIES => {
                self.paste_retries += 1;
                log::debug!("PTY write (paste chunk) busy, retry {}: {}", self.paste_retries, e);
                self.paste_next_chunk_at = Some(now + RETRY_GAP);
                ctx.request_repaint_after(RETRY_GAP);
            }
            Err(e) => {
                log::error!(
                    "PTY write (paste chunk): {} — {} bytes unsent",
                    e,
                    self.paste_pending.len()
                );
                self.pending_toast_error = Some(format!(
                    "粘贴未完成：还有 {} 字节没能写入终端（{}）",
                    self.paste_pending.len(),
                    e
                ));
                self.paste_pending.clear();
                self.paste_next_chunk_at = None;
                self.paste_retries = 0;
            }
        }
    }

    /// 处理 SSH 消息；若终端缓冲有更新则返回 `true`。
    /// 每帧有消息条数/字节预算，避免海量输出一次喂进 VTE 导致主线程卡死数十秒。
    fn process_ssh_messages(&mut self, theme: &Theme) -> bool {
        const MAX_SSH_MSGS_PER_FRAME: usize = 96;
        const MAX_SSH_BYTES_PER_FRAME: usize = 256 * 1024;

        let mut vte_dirty = false;
        let mut more_pending = false;
        if let Some(ref rx) = self.ssh_rx {
            let mut batch = Vec::new();
            let mut bytes = 0usize;
            loop {
                if batch.len() >= MAX_SSH_MSGS_PER_FRAME || bytes >= MAX_SSH_BYTES_PER_FRAME {
                    more_pending = true;
                    break;
                }
                match rx.try_recv() {
                    Ok(msg) => {
                        if let SshMessage::Output { data, .. } = &msg {
                            bytes = bytes.saturating_add(data.len());
                        }
                        batch.push(msg);
                    }
                    Err(_) => break,
                }
            }
            if !batch.is_empty() {
                let oldest = crate::ssh::take_oldest_pending_us();
                if let Some(t) = oldest {
                    let now = crate::ssh::probe_now_us();
                    log::debug!(target: "repaint_lag", "drain msgs={} bytes={} delay_since_recv_ms={:.1} more_pending={}", batch.len(), bytes, now.saturating_sub(t) as f64 / 1000.0, more_pending);
                    if self.repaint_probe_received_us.is_none() {
                        self.repaint_probe_received_us = Some(t);
                    }
                }
            }
            for msg in batch {
                match msg {
                    SshMessage::Output { data, .. } => {
                        if self.is_local_shell() {
                            if !data.is_empty() {
                                self.pending_log_output.push(data.clone());
                                self.observe_interactive_output(&data);
                                self.dispatch_output_to_modules(&data);
                                self.terminal.feed(&data);
                                vte_dirty = true;
                            }
                            continue;
                        }
                        self.push_zmodem_detect_tail(&data);
                        let detect_buf = self.zmodem_detect_window();
                        // 上传(本机 sz→远端 rz)时，PTY 上的 ZMODEM 帧只能经 SSH 泵线程到达此处；
                        // 必须旁路给 lrzsz，不得在另一线程对 Channel 再 read。
                        let text = String::from_utf8_lossy(&data);
                        let is_rz_prompt_text = text.contains("rz rz rz")
                            || text.contains("Awaiting rz")
                            || text.contains("rz waiting to receive");
                        let zrqinit_in_data = self.lrzsz.detect_zmodem_download(detect_buf);
                        let rz_invitation = self.lrzsz.detect_rz_pty_invitation(detect_buf);
                        let windows_rz_wait = self.lrzsz.detect_windows_rz_wait_invitation(detect_buf);
                        let expect_rz_upload = self.expect_rz_upload_active();
                        // `rz -bye` 握手常早于 Enter；Windows lrzsz 发 ASCII `**B01…`。
                        let is_rz_upload_handshake = is_rz_prompt_text
                            || windows_rz_wait
                            || (expect_rz_upload && rz_invitation);

                        if !self.lrzsz.is_upload_pty_capture() {
                            if self.lrzsz.is_active() {
                                if self.lrzsz.feed_receive_data(&data) {
                                    continue;
                                }
                            } else if zrqinit_in_data
                                && !is_rz_upload_handshake
                                && self.expect_sz_download_active()
                            {
                                if let Some(pump_tx) =
                                    self.ssh_handle.as_ref().map(|h| h.shell_pump_tx())
                                {
                                    let resolved = crate::core::local_dir::current();
                                    if let Some(missing) = &resolved.missing {
                                        let warn = match self.ui_lang_last {
                                            crate::i18n::UiLanguage::En => format!(
                                                "Default local folder {} not found; saving to {} instead.",
                                                missing.display(),
                                                resolved.dir.display()
                                            ),
                                            crate::i18n::UiLanguage::Zh => format!(
                                                "默认本地目录 {} 不存在，已改存到 {}。",
                                                missing.display(),
                                                resolved.dir.display()
                                            ),
                                        };
                                        self.feed_user_warn_line(theme, &warn);
                                    }
                                    self.lrzsz.set_download_dir(resolved.dir);
                                    match self.lrzsz.start_receive_pump(pump_tx) {
                                        Ok(()) => {
                                            if self.lrzsz.feed_receive_data(&data) {
                                                continue;
                                            }
                                        }
                                        Err(e) => {
                                            self.error_message = Some(crate::i18n::localize_backend_error(
                                                self.ui_lang_last,
                                                &format!("ZMODEM receive failed: {e}"),
                                            ));
                                        }
                                    }
                                }
                            }
                        }

                        // 尽早打开旁路：选文件前 `rz` 发出的 ZRQINIT 若只进 VTE，`start_send` 会永远等下一轮超时。
                        if is_rz_upload_handshake && self.connected {
                            if let Some(path) = self.zmodem_upload_after_rz_path.take() {
                                log::info!(
                                    "rz handshake ready; starting queued ZMODEM upload: {}",
                                    path.display()
                                );
                                self.pending_rz_upload = false;
                                self.rz_control_mode_until =
                                    Some(Instant::now() + Duration::from_secs(90));
                                self.lrzsz.begin_rz_handshake_capture();
                                self.lrzsz.feed_send_pty_output(&data);
                                if let Some(ref h) = self.ssh_handle {
                                    self.lrzsz.register_shell_pump_upload_feed(h);
                                }
                                if Self::mirror_rz_text_to_vte_enabled()
                                    && Self::pty_chunk_safe_to_mirror_vte(&data)
                                {
                                    self.terminal.feed(&data);
                                    vte_dirty = true;
                                }
                                if let Err(e) = self.start_rz_upload(path.as_path()) {
                                    self.error_message = Some(crate::i18n::localize_backend_error(
                                        self.ui_lang_last,
                                        &e,
                                    ));
                                    self.lrzsz.end_rz_handshake_capture();
                                }
                                continue;
                            }
                            if !self.pending_rz_upload
                                && !self.lrzsz.is_upload_pty_capture()
                                && !self.lrzsz.is_active()
                            {
                            log::info!(
                                "Detected rz upload handshake (text={}, windows_rz={}, invitation={})",
                                is_rz_prompt_text,
                                windows_rz_wait,
                                rz_invitation
                            );
                            self.rz_control_mode_until =
                                Some(Instant::now() + Duration::from_secs(30));
                            self.lrzsz.begin_rz_handshake_capture();
                            self.lrzsz.feed_send_pty_output(&data);
                            if let Some(ref h) = self.ssh_handle {
                                self.lrzsz.register_shell_pump_upload_feed(h);
                            }
                            if crate::ui::sftp_panel::SftpPanel::gui_automation_enabled() {
                                if let Some(path) =
                                    crate::ui::sftp_panel::SftpPanel::gui_automation_zmodem_local_path()
                                {
                                    log::info!(
                                        "GUI automation: starting ZMODEM upload {}",
                                        path.display()
                                    );
                                    if let Err(e) = self.start_rz_upload(path.as_path()) {
                                        self.error_message =
                                            Some(crate::i18n::localize_backend_error(
                                                self.ui_lang_last,
                                                &e,
                                            ));
                                        self.lrzsz.end_rz_handshake_capture();
                                    }
                                    continue;
                                }
                            }
                            self.pending_rz_upload = true;
                            if Self::mirror_rz_text_to_vte_enabled()
                                && Self::pty_chunk_safe_to_mirror_vte(&data)
                            {
                                self.terminal.feed(&data);
                                vte_dirty = true;
                            }
                            continue; // 不显示在终端
                            }
                        }

                        self.lrzsz.feed_send_pty_output(&data);
                        if self.lrzsz.is_upload_pty_capture() {
                            // 与 `rz_control_mode` 里「误判 shell 提示符」无关：二进制 ZACK 等绝不能进 VTE(否则 0x18 被当 C1)。
                            if Self::mirror_rz_text_to_vte_enabled()
                                && Self::pty_chunk_safe_to_mirror_vte(&data)
                            {
                                self.terminal.feed(&data);
                                vte_dirty = true;
                            }
                            continue;
                        }

                        let (display_data, audit_events) = self.server_audit_probe.feed(&data);
                        if !audit_events.is_empty() {
                            self.pending_server_audit.extend(audit_events);
                            // 审计行常被整行剥离：无 VTE 脏位则不 request_repaint，Toast 会一直卡着。
                            vte_dirty = true;
                        }
                        if let Some(until) = self.rz_control_mode_until {
                            if Instant::now() <= until {
                                // 控制模式内默认吞掉输出，避免显示 **B0... / ccc|... 等握手噪音。
                                // 仅在看到 shell 提示符时退出控制模式并恢复正常显示。
                                let raw_text = String::from_utf8_lossy(&display_data);
                                if Self::contains_shell_prompt_fragment(&raw_text) {
                                    self.rz_control_mode_until = None;
                                } else {
                                    continue;
                                }
                            } else {
                                self.rz_control_mode_until = None;
                            }
                        }
                        if !display_data.is_empty() {
                            self.pending_log_output.push(display_data.clone());
                            self.observe_interactive_output(&display_data);
                            self.dispatch_output_to_modules(&display_data);
                            self.terminal.feed(&display_data);
                            vte_dirty = true;
                            self.note_cwd_from_screen();
                            self.maybe_send_startup_cd();
                        }
                    }
                    SshMessage::Connected { .. } => {
                        if let Some((_, host)) = &self.connection_target {
                            self.pending_connect_audit = Some((true, host.clone()));
                        }
                        self.connected = true;
                        self.connected_at = Some(Instant::now());
                        self.terminal_wants_keyboard = true;
                        self.terminal_focused = true;
                        self.pending_focus_terminal = true;
                        self.auto_follow_output = true;

                        if self.is_local_shell() {
                            // local_handle 已在 connect_local_shell 挂好
                        } else if let Some(ref manager) = self.ssh_manager {
                            // 启动交互式 shell
                            if let Some(session_id) = self.session_id {
                                match manager.start_interactive_shell(session_id, self.cols, self.rows) {
                                    Ok(handle) => {
                                        self.ssh_handle = Some(handle);
                                        if !self.disconnected_input_buffer.is_empty() {
                                            self.resend_offline_input_dialog_open = true;
                                        }
                                        // #177：连接/重连成功后排队恢复命令。
                                        // 延迟 ~600ms 等 shell 提示符就绪，避免命令
                                        // 被 shell 初始化输出吞掉；仅对显式配置生效。
                                        if let Some(cmd) = self.pending_post_connect_command.take() {
                                            if !cmd.trim().is_empty() {
                                                self.post_connect_fire_at =
                                                    Some(Instant::now() + Duration::from_millis(600));
                                                // 暂存回 pending，到点再真正取走执行
                                                self.pending_post_connect_command = Some(cmd);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        let msg = format_ssh_connect_error(
                                            self.ui_lang_last,
                                            &format!("Failed to start shell: {}", e),
                                        );
                                        self.pending_toast_error = Some(msg.clone());
                                        self.error_message = Some(msg);
                                    }
                                }
                            }
                        }
                    }
                    SshMessage::Error { error, .. } => {
                        let msg = format_ssh_connect_error(self.ui_lang_last, &error);
                        if let Some((_, host)) = &self.connection_target {
                            self.pending_connect_audit = Some((false, host.clone()));
                        }
                        self.pending_toast_error = Some(msg.clone());
                        self.error_message = Some(msg.clone());
                        self.connected_at = None;
                        self.auto_follow_output = true;
                    }
                    SshMessage::Disconnected { .. } => {
                        if self.local_disconnect_intent {
                            self.local_disconnect_intent = false;
                        } else {
                            self.unexpected_disconnect_notified = true;
                            // #93 SessionEnded：非主动断开才提示未聚焦标签。
                            self.pending_attention_ended = true;
                        }
                        self.connected = false;
                        self.terminal_wants_keyboard = false;
                        self.terminal_focused = false;
                        self.connected_at = None;
                        self.auto_follow_output = true;
                        self.resend_offline_input_dialog_open = false;
                        self.seal_open_capture();
                    }
                    SshMessage::UserCommand { command, .. } => {
                        *self.command_usage.entry(command).or_insert(0) += 1;
                    }
                }
            }
        }
        vte_dirty || more_pending
    }

    pub fn command_usage_snapshot(&self) -> Vec<(String, u64)> {
        self.command_usage
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect()
    }

    /// 处理文件传输事件
    fn process_transfer_events(&mut self, theme: &Theme, ctx: &egui::Context) {
        while let Some(event) = self.lrzsz.try_recv_event() {
            match event {
                TransferEvent::FileStart {
                    filename,
                    size,
                    outgoing,
                } => {
                    self.transfer_outgoing = outgoing;
                    self.transfer_progress = Some((filename.clone(), 0, size));
                    ctx.request_repaint();
                    // 开始状态由底部进度条展示，不再写入 VTE，避免插在 rz 行与远端回显之间
                }
                TransferEvent::FileProgress { received, total, .. } => {
                    if let Some(ref mut progress) = self.transfer_progress {
                        *progress = (progress.0.clone(), received, total);
                    }
                    ctx.request_repaint();
                }
                TransferEvent::FileComplete { filename, path } => {
                    self.transfer_progress = None;
                    let was_outgoing = self.transfer_outgoing;
                    let line = if was_outgoing {
                        format!(
                            "{}{} -> {}",
                            crate::i18n::tr(ctx, "Upload complete: ", "上传完成："),
                            path.display(),
                            format!("./{filename}")
                        )
                    } else {
                        format!(
                            "{}{} -> {}",
                            crate::i18n::tr(ctx, "Receive complete: ", "接收完成："),
                            filename,
                            path.display()
                        )
                    };
                    self.transfer_outgoing = false;
                    if was_outgoing {
                        self.clear_expect_rz_upload();
                    } else {
                        self.clear_expect_sz_download();
                    }
                    self.feed_user_success_line(theme, &line);
                    if was_outgoing {
                        // rz 上传后远端提示符常紧跟收尾输出；成功行后立即补空行，避免粘在同一行。
                        self.terminal.feed(b"\r\n");
                    }
                }
                TransferEvent::FileError { filename, error } => {
                    if let Some(ref h) = self.ssh_handle {
                        self.lrzsz.unregister_shell_pump_upload_feed(h);
                    }
                    self.transfer_progress = None;
                    self.transfer_outgoing = false;
                    self.clear_expect_rz_upload();
                    self.clear_expect_sz_download();
                    self.feed_user_error_line(theme, ctx, &format!(
                        "{} {}: {}",
                        crate::i18n::tr(ctx, "Transfer failed", "传输失败"),
                        filename,
                        error
                    ));
                    self.flush_ssh_pty_size_after_transfer();
                }
                TransferEvent::TransferComplete => {
                    if let Some(ref h) = self.ssh_handle {
                        self.lrzsz.unregister_shell_pump_upload_feed(h);
                    }
                    // 传输完成，恢复终端交互状态
                    self.auto_follow_output = true;
                    self.rz_control_mode_until = None;
                    self.clear_expect_rz_upload();
                    self.clear_expect_sz_download();
                    self.transfer_outgoing = false;
                    // 上传期间可能暂缓了 resize_pty，此处与远端对齐当前 UI 网格
                    self.flush_ssh_pty_size_after_transfer();
                }
            }
        }
    }

    /// 断线重连时由宿主在 `disconnect` / `connect` 之间保留/恢复(`connect()` 会清空缓存)。
    pub fn offline_input_snapshot(&self) -> (Vec<u8>, bool) {
        (
            self.disconnected_input_buffer.clone(),
            self.buffer_input_while_disconnected,
        )
    }

    pub fn restore_offline_input_snapshot(&mut self, buf: Vec<u8>, flag: bool) {
        self.disconnected_input_buffer = buf;
        self.buffer_input_while_disconnected = flag;
    }

    fn append_offline_bytes(&mut self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        self.scroll_to_bottom_on_user_input();
        let cap = Self::OFFLINE_INPUT_CAP;
        if self.disconnected_input_buffer.len() >= cap {
            return;
        }
        let take = (cap - self.disconnected_input_buffer.len()).min(data.len());
        self.disconnected_input_buffer.extend_from_slice(&data[..take]);
    }

    /// 断线保留画面时：把按键写入本地缓冲(与 PTY 路径相同的字节序列，便于重发)。
    fn capture_inline_input_disconnected(&mut self, ui: &egui::Ui) {
        if self.resend_offline_input_dialog_open {
            return;
        }
        let mut pending_paste: Option<String> = None;
        let mut ctrl_to_send: Vec<u8> = Vec::new();

        ui.input_mut(|i| {
            let tab_plain = i.consume_key(egui::Modifiers::NONE, egui::Key::Tab);
            let tab_shift = i.consume_key(
                egui::Modifiers {
                    shift: true,
                    ..Default::default()
                },
                egui::Key::Tab,
            );
            if tab_plain || tab_shift {
                self.append_offline_bytes(b"\t");
                // EventFilter 需上一帧已聚焦才生效；首帧 Tab 仍可能失焦
                self.pending_focus_terminal = true;
            }
            if crate::ui::terminal_keys::forward_non_text_keys(i, |bytes| {
                self.append_offline_bytes(bytes);
            }) {
                self.pending_focus_terminal = true;
            }
            let mut sent_ctrl = [false; 32];
            if crate::ui::terminal_keys::forward_ctrl_keys(i, |b| {
                if b < 32 {
                    sent_ctrl[b as usize] = true;
                }
                ctrl_to_send.push(b);
            }) && !ctrl_to_send.is_empty() {
                // pending_focus set after input_mut
            }
            if crate::ui::terminal_keys::forward_alt_meta_keys(i, |bytes| {
                self.append_offline_bytes(bytes);
            }) {
                self.pending_focus_terminal = true;
            }

            let mut backspace_key = false;
            let mut delete_key = false;
            for event in &i.events {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    ..
                } = event
                {
                    if *key == egui::Key::Backspace {
                        backspace_key = true;
                    }
                    if *key == egui::Key::Delete {
                        delete_key = true;
                    }
                }
            }

            for event in &i.events {
                self.ime_preedit.apply_event(event);
            }
            for event in &i.events {
                match event {
                    egui::Event::CompositionEnd(text) => {
                        if let Some(text) = Self::ime_committed_text(text, i.modifiers) {
                            self.append_offline_bytes(text.as_bytes());
                        }
                    }
                    egui::Event::Text(text) => {
                        if text == "\n" || text == "\r" {
                            continue;
                        }
                        if i.modifiers.command {
                            continue;
                        }
                        if crate::ui::terminal_keys::try_forward_ctrl_text_byte(
                            text,
                            i.modifiers.ctrl,
                            &mut sent_ctrl,
                            |b| ctrl_to_send.push(b),
                        ) {
                            continue;
                        }
                        if i.modifiers.ctrl {
                            continue;
                        }
                        if text == "\t" {
                            continue;
                        }
                        if text == "\u{7f}" {
                            if backspace_key || delete_key {
                                continue;
                            }
                            #[cfg(target_os = "macos")]
                            {
                                self.append_offline_bytes(b"\x1b[3~");
                                continue;
                            }
                            #[cfg(not(target_os = "macos"))]
                            {
                                self.append_offline_bytes(&[0x7f]);
                                continue;
                            }
                        }
                        self.append_offline_bytes(text.as_bytes());
                    }
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        if modifiers.command {
                            // 仅拦截 ⌘V→离线缓冲；⌘C/⌘A 等留给侧栏/弹窗内 TextEdit
                            if *key == egui::Key::V {
                                if let Ok(mut clip) = Clipboard::new() {
                                    if let Ok(text) = clip.get_text() {
                                        pending_paste = Some(text);
                                    }
                                }
                            }
                        } else {
                        match key {
                            egui::Key::V if modifiers.ctrl && modifiers.shift => {
                                if let Ok(mut clip) = Clipboard::new() {
                                    if let Ok(text) = clip.get_text() {
                                        pending_paste = Some(text);
                                    }
                                }
                            }
                            egui::Key::Enter => {
                                self.append_offline_bytes(b"\r");
                            }
                            egui::Key::Backspace => {
                                self.append_offline_bytes(&[0x7f]);
                            }
                            egui::Key::Delete => {
                                self.append_offline_bytes(b"\x1b[3~");
                            }
                            egui::Key::Tab => {}
                            _ => {}
                        }
                        }
                    }
                    _ => {}
                }
            }
        });
        if !ctrl_to_send.is_empty() {
            self.pending_focus_terminal = true;
            for b in ctrl_to_send {
                self.append_offline_bytes(&[b]);
            }
        }
        if let Some(text) = pending_paste {
            let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
            self.append_offline_bytes(normalized.as_bytes());
        }
        ui.ctx().request_repaint_after(Duration::from_millis(50));
    }

    /// #177：到点执行连接后恢复命令（会话显式配置，默认空不执行）。
    /// 执行一次即清空；断线时由 disconnect 路径清空，避免旧命令跨会话泄漏。
    fn fire_post_connect_command(&mut self, ctx: &egui::Context) {
        let Some(fire_at) = self.post_connect_fire_at else {
            return;
        };
        if Instant::now() < fire_at {
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        self.post_connect_fire_at = None;
        let Some(cmd) = self.pending_post_connect_command.take() else {
            return;
        };
        if cmd.trim().is_empty() || !self.connected || !self.has_pty_writer() {
            // shell 未就绪 / 已断开：丢弃，不跨连接执行
            return;
        }
        log::info!("post-connect restore command firing ({} runes)", cmd.chars().count());
        self.send_command(&cmd);
    }

    fn render_resend_offline_dialog(&mut self, ctx: &egui::Context, theme: &Theme) {
        if !self.resend_offline_input_dialog_open {
            return;
        }
        let n = self.disconnected_input_buffer.len();
        if n == 0 {
            self.resend_offline_input_dialog_open = false;
            return;
        }
        let preview = String::from_utf8_lossy(&self.disconnected_input_buffer[..n.min(200)]);
        let preview_esc = preview.replace('\r', "\\r").replace('\n', "\\n");

        let mut open = true;
        let mut close_via_header = false;
        let modal_sz = layout_util::modal_confirm_size(ctx);
        crate::ui::chrome::modal_window("resend_offline_input", theme, ctx)
            .open(&mut open)
            .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
            .resizable(false)
            .show(ctx, |ui| {
                crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                if crate::ui::chrome::modal_header(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "Queued input while offline", "断线期间暂存的输入"),
                    crate::ui::chrome::modal_title_font_size(theme),
                ) {
                    close_via_header = true;
                }
                let body_line = match crate::i18n::language(ctx) {
                    UiLanguage::En => format!(
                        "{} bytes queued. Send to the remote shell now?",
                        n
                    ),
                    UiLanguage::Zh => format!(
                        "共 {} 字节，是否发送到当前远程 shell?",
                        n
                    ),
                };
                ui.label(
                    egui::RichText::new(body_line)
                        .size(theme.font_size_medium())
                        .color(theme.text_secondary()),
                );
                if !preview_esc.is_empty() {
                    ui.add_space(theme.spacing_sm());
                    ui.label(
                        egui::RichText::new(format!(
                            "{}{}",
                            crate::i18n::tr(ctx, "Preview: ", "预览："),
                            preview_esc
                        ))
                            .monospace()
                            .size(theme.font_size_panel_title())
                            .color(theme.text_tertiary()),
                    );
                }
                ui.add_space(theme.spacing_list_item_x());
                crate::ui::chrome::modal_footer_actions(ui, theme, |ui, th| {
                    if crate::ui::chrome::modal_primary_button_with_icon(
                        ui,
                        th,
                        crate::ui::icons::IconId::Upload,
                        crate::i18n::tr(ui.ctx(), "Send to remote", "发送到远端"),
                    )
                    .clicked()
                    {
                        if self.has_pty_writer() {
                            let chunks: Vec<Vec<u8>> = self
                                .disconnected_input_buffer
                                .chunks(4096)
                                .map(|c| c.to_vec())
                                .collect();
                            for chunk in chunks {
                                let _ = self.send_pty_input(&chunk);
                            }
                        }
                        self.disconnected_input_buffer.clear();
                        self.buffer_input_while_disconnected = false;
                        self.resend_offline_input_dialog_open = false;
                    }
                    if crate::ui::chrome::modal_secondary_icon_button(
                        ui,
                        th,
                        crate::ui::icons::IconId::Trash,
                        crate::i18n::tr(ui.ctx(), "Discard buffer", "丢弃缓存"),
                    )
                    .clicked()
                    {
                        self.disconnected_input_buffer.clear();
                        self.buffer_input_while_disconnected = false;
                        self.resend_offline_input_dialog_open = false;
                    }
                });
                });
            });
        if !open || close_via_header {
            self.resend_offline_input_dialog_open = false;
            if close_via_header {
                self.disconnected_input_buffer.clear();
                self.buffer_input_while_disconnected = false;
            }
        }
    }

    /// 将键盘事件直接写入 PTY，由远端 shell 回显，避免「本地预览 + 回显」叠字(如 lsls)
    fn capture_inline_input(&mut self, ui: &egui::Ui) {
        if self.resend_offline_input_dialog_open {
            return;
        }
        if self.buffer_input_while_disconnected && !self.connected {
            if self.terminal_focused {
                self.capture_inline_input_disconnected(ui);
            }
            return;
        }
        if !self.connected {
            return;
        }
        if !self.has_pty_writer() {
            return;
        }
        // 须用 terminal_focused(含 capture_pty_keyboard)，勿仅看 wants：
        // AI 输入聚焦时 wants 可能仍为 true，否则 ,. 等 Text 会被写进 PTY 且干扰输入框。
        if !self.terminal_focused {
            return;
        }

        let mut pending_paste: Option<String> = None;
        let mut ctrl_to_send: Vec<u8> = Vec::new();
        let mut do_copy = false;

        ui.input_mut(|i| {
            if crate::ui::terminal_keys::consume_terminal_paste_shortcut(i) {
                if let Ok(mut clip) = Clipboard::new() {
                    if let Ok(text) = clip.get_text() {
                        pending_paste = Some(text);
                    }
                }
            } else if crate::ui::terminal_keys::consume_terminal_copy_shortcut(i) {
                do_copy = true;
            }
            // 强制拦截 Tab 焦点遍历，把 Tab/Shift+Tab 交给终端
            let tab_plain = i.consume_key(egui::Modifiers::NONE, egui::Key::Tab);
            let tab_shift = i.consume_key(
                egui::Modifiers {
                    shift: true,
                    ..Default::default()
                },
                egui::Key::Tab,
            );
            if tab_plain || tab_shift {
                let _ = self.send_pty_input(b"\t");
                // EventFilter 需上一帧已聚焦才生效；首帧 Tab 仍可能失焦
                self.pending_focus_terminal = true;
            }
            // Esc / F1–F12 / Insert / 带修饰方向键等(egui 常无 Text 事件)
            if crate::ui::terminal_keys::forward_non_text_keys(i, |bytes| {
                let _ = self.send_pty_input(bytes);
            }) {
                // EventFilter 生效前的兜底(帧初 Tab/Esc/方向键抢焦点)
                self.pending_focus_terminal = true;
            }
            let mut sent_ctrl = [false; 32];
            if crate::ui::terminal_keys::forward_ctrl_keys(i, |b| {
                if b < 32 {
                    sent_ctrl[b as usize] = true;
                }
                ctrl_to_send.push(b);
            }) {
                // pending_focus set after input_mut
            }
            if crate::ui::terminal_keys::forward_alt_meta_keys(i, |bytes| {
                let _ = self.send_pty_input(bytes);
            }) {
                self.pending_focus_terminal = true;
            }
            // 同一帧内可能既有 Key 又有 Text(如 Delete / 退格)，避免重复或错发
            let mut backspace_key = false;
            let mut delete_key = false;
            // Text(\r/\n) 与 Key::Enter 常同帧到达；只发一次 `\r`。
            let mut enter_sent = false;
            for event in &i.events {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    ..
                } = event
                {
                    if *key == egui::Key::Backspace {
                        backspace_key = true;
                    }
                    if *key == egui::Key::Delete {
                        delete_key = true;
                    }
                }
            }

            for event in &i.events {
                self.ime_preedit.apply_event(event);
                match event {
                    egui::Event::CompositionStart => {
                        log::debug!(target: "repaint_lag", "ime composition start");
                    }
                    egui::Event::CompositionUpdate(t) => {
                        log::debug!(target: "repaint_lag", "ime preedit update len={}", t.chars().count());
                    }
                    egui::Event::Text(t) => {
                        log::debug!(target: "repaint_lag", "key text event len={}", t.chars().count());
                    }
                    egui::Event::CompositionEnd(t) => {
                        log::debug!(target: "repaint_lag", "ime commit len={}", t.chars().count());
                    }
                    _ => {}
                }
            }
            for event in &i.events {
                match event {
                    egui::Event::CompositionEnd(text) => {
                        if let Some(text) = Self::ime_committed_text(text, i.modifiers) {
                            self.track_typed_text(text);
                            if let Err(e) = self.send_pty_input(text.as_bytes()) {
                                log::error!("PTY write (ime): {}", e);
                            }
                        }
                    }
                    egui::Event::Text(text) => {
                        // Enter：部分自动化(如 pywinauto)只发 Text(\r) 不发 Key::Enter。
                        if text == "\n" || text == "\r" {
                            if !enter_sent {
                                self.submit_enter_key();
                                enter_sent = true;
                            }
                            continue;
                        }
                        if i.modifiers.command {
                            continue;
                        }
                        if crate::ui::terminal_keys::try_forward_ctrl_text_byte(
                            text,
                            i.modifiers.ctrl,
                            &mut sent_ctrl,
                            |b| ctrl_to_send.push(b),
                        ) {
                            continue;
                        }
                        if i.modifiers.ctrl {
                            continue;
                        }
                        // Tab 已在 consume_key 阶段处理，避免重复
                        if text == "\t" {
                            continue;
                        }
                        // U+007F：常与 Forward Delete 或退格绑定；已由 Key 处理则不再发
                        if text == "\u{7f}" {
                            if backspace_key || delete_key {
                                continue;
                            }
                            // macOS 上 Fn+Delete 等常只来 Text(DEL)，应发 xterm「向前删」而非裸 0x7f(易被当成怪字符/像空格)
                            #[cfg(target_os = "macos")]
                            {
                                if let Err(e) = self.send_pty_input(b"\x1b[3~") {
                                    log::error!("PTY write (delete seq): {}", e);
                                }
                                continue;
                            }
                            #[cfg(not(target_os = "macos"))]
                            {
                                self.track_backspace();
                                if let Err(e) = self.send_pty_input(&[0x7f]) {
                                    log::error!("PTY write (del): {}", e);
                                }
                                continue;
                            }
                        }
                        self.track_typed_text(text);
                        if let Err(e) = self.send_pty_input(text.as_bytes()) {
                            log::error!("PTY write (text): {}", e);
                        }
                    }
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        if modifiers.command {
                            // macOS：⌘C 复制 / ⌘V 粘贴(复制在帧初 consume；此处仅兜底 ⌘V)
                            if *key == egui::Key::V {
                                if let Ok(mut clip) = Clipboard::new() {
                                    if let Ok(text) = clip.get_text() {
                                        pending_paste = Some(text);
                                    }
                                }
                            }
                        } else {
                        match key {
                            egui::Key::Enter => {
                                if !enter_sent {
                                    self.submit_enter_key();
                                    enter_sent = true;
                                }
                            }
                            egui::Key::Backspace => {
                                self.track_backspace();
                                if let Err(e) = self.send_pty_input(&[0x7f]) {
                                    log::error!("PTY write (bs): {}", e);
                                }
                            }
                            // xterm / xterm-256color：Forward Delete 为 CSI 3 ~
                            egui::Key::Delete => {
                                if let Err(e) = self.send_pty_input(b"\x1b[3~") {
                                    log::error!("PTY write (delete): {}", e);
                                }
                            }
                            egui::Key::Tab => {
                                // Tab 已在 consume_key 阶段统一发送
                            }
                            _ => {}
                        }
                        }
                    }
                    _ => {}
                }
            }
        });
        if do_copy {
            let _ = self.shortcut_copy_to_clipboard(ui.ctx());
        }
        if !ctrl_to_send.is_empty() {
            self.pending_focus_terminal = true;
            for b in ctrl_to_send {
                if b == 0x03 && self.lrzsz.is_active() {
                    let _ = self.cancel_zmodem_transfer();
                    continue;
                }
                self.track_ctrl_byte(b);
                if let Err(e) = self.send_pty_input(&[b]) {
                    log::error!("PTY write (ctrl): {}", e);
                }
            }
        }
        if let Some(text) = pending_paste {
            self.paste_text(&text, ui.ctx());
        }
    }

    /// 粘贴或执行一整段命令：只写 PTY，不把内容再写入本地 buffer(回显由远端负责)
    pub fn send_command(&mut self, command: &str) {
        if !self.connected {
            return;
        }
        self.typed_line_buffer.clear();
        let normalized = command.replace("\r\n", "\n").replace('\r', "\n");
        let lines: Vec<&str> = normalized.lines().collect();
        for line in &lines {
            self.enqueue_log_command(line);
        }
        for line in &lines {
            if crate::ui::terminal_shell_command::is_rz_shell_command(line) {
                self.note_rz_shell_command_submitted();
            }
            if crate::ui::terminal_shell_command::is_sz_shell_command(line) {
                self.note_sz_shell_command_submitted();
            }
        }
        if !self.has_pty_writer() {
            return;
        }
        for line in lines {
            let mut payload = line.to_string();
            payload.push('\r');
            if let Err(e) = self.send_pty_input(payload.as_bytes()) {
                log::error!("PTY write (command): {}", e);
            }
        }
    }

    /// 粘贴文本到终端：不自动补回车；远端开了 bracketed paste 时包上 200~/201~；
    /// 一律进分片队列发送(FUNCTIONAL_SPEC §2.4)，写失败不丢片。
    fn paste_text(&mut self, text: &str, ctx: &egui::Context) {
        use crate::ui::terminal_paste::{build_paste_payload, exceeds_canon_line_limit, TTY_CANON_LINE_MAX};
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        if !self.connected {
            if self.buffer_input_while_disconnected {
                self.append_offline_bytes(normalized.as_bytes());
                ctx.request_repaint_after(Duration::from_millis(50));
            }
            return;
        }
        if !self.has_pty_writer() {
            return;
        }
        // 单行粘贴计入本行缓冲，回车时与手敲内容一起审计；多行粘贴自带换行，不在此跟踪。
        if !normalized.contains('\n') {
            self.track_typed_text(&normalized);
        }
        let bracketed = self.terminal.is_bracketed_paste();
        // 远端在行模式(cat/read 等)时，单行 >4095 字节会被 tty 内核截断，终端无法绕过，提示用户。
        if !bracketed && exceeds_canon_line_limit(&normalized) {
            self.pending_toast_error = Some(format!(
                "粘贴内容有单行超过 {} 字节：若远端是 cat、read 等按行读取的程序，超出部分会被系统截断。建议改用 SFTP 上传或在 vim 中粘贴。",
                TTY_CANON_LINE_MAX
            ));
        }
        self.paste_pending
            .extend_from_slice(&build_paste_payload(&normalized, bracketed));
        self.flush_paste_queue(ctx);
    }

    pub fn clear_screen(&mut self) {
        // 清空滚动历史，保留当前屏幕内容(类似 tmux clear-history)
        // 同时把光标移到屏幕顶部，方便继续输入
        self.terminal.clear_history();
        self.terminal.feed(b"\x1b[H");  // 光标移到左上角
        self.vt_visual_generation = self.vt_visual_generation.wrapping_add(1);
        self.visual_layout_cache = None;
        self.selection.clear();
    }

    /// 复制到剪贴板。`fallback_all` 为 true 时无选区则复制整屏(菜单「复制」)；
    /// false 时仅复制选区(Ctrl+Shift+C，与 Windows Terminal 一致)。
    /// 走 `ctx.copy_text`（eframe/winit 剪贴板），避免 UI 线程直调 arboard 与外部
    /// PowerShell/自动化争用 OpenClipboard 时把整窗卡成「未响应」。
    fn copy_to_clipboard(&self, ctx: &egui::Context, fallback_all: bool) -> Option<String> {
        let text = self.get_selected_text();
        let text = if text.is_empty() && fallback_all {
            self.terminal.get_formatted_output()
        } else {
            text
        };
        if text.is_empty() {
            return None;
        }
        ctx.copy_text(text.clone());
        Some(text)
    }

    /// 菜单「复制」：优先选区，否则复制当前屏格式化输出。
    pub(crate) fn menu_copy_to_clipboard(&self, ctx: &egui::Context) -> bool {
        self.copy_to_clipboard(ctx, true).is_some()
    }

    /// 快捷键 Ctrl+Shift+C：仅复制选区；无选区时不复制。
    /// 返回放进剪贴板的文字(无选区为 `None`)，供调用方在帧末核对是否被别的控件覆盖。
    pub(crate) fn shortcut_copy_to_clipboard(&self, ctx: &egui::Context) -> Option<String> {
        self.copy_to_clipboard(ctx, false)
    }

    /// 菜单「粘贴」：从系统剪贴板粘贴到 PTY。
    pub(crate) fn menu_paste_from_clipboard(&mut self, ctx: &egui::Context) {
        if let Ok(mut clip) = Clipboard::new() {
            if let Ok(text) = clip.get_text() {
                self.paste_text(&text, ctx);
            }
        }
    }

    /// 当前标签页是否有可复制的终端选区。
    pub(crate) fn has_active_selection(&self) -> bool {
        !self.selection.is_empty()
    }

    /// 菜单「全选」：选中当前视口全部内容。
    pub(crate) fn menu_select_all(&mut self) {
        let offset = self.terminal.display_offset();
        let rows = self.rows.max(1) as usize;
        let cols = self.cols.max(1) as usize;
        self.selection.set_from_viewport(offset, 0, 0, rows.saturating_sub(1), cols.saturating_sub(1));
    }

    /// display-link 点击分发（#90）：
    /// - copy-or-connect：直接复制
    /// - browser-open：系统浏览器（http/https 才放行）
    /// - runbook-recall：把错误码交给宿主自动召回
    /// - sftp-open：置 pending，由宿主开 SFTP 面板并跳转
    fn handle_display_link_click(&mut self, ctx: &egui::Context, link: &DisplaySpan) {
        match classify_display_link_click(&link.action, &link.text) {
            Some(DisplayLinkClick::Copy) => {
                ctx.copy_text(link.text.clone());
            }
            Some(DisplayLinkClick::Browser) => {
                let _ = crate::platform::open_url(&link.text);
            }
            Some(DisplayLinkClick::Runbook) => {
                // #90 review 后续：不再走「发送到 AI」的 attach 路径（用户还得手发），
                // 改走自动召回：宿主取走后 attach 错误码 + 预填问题自动发送，
                // send_message 内部 schedule_runbook_recall 直接出卡片。
                self.pending_display_link_runbook = Some(link.text.clone());
            }
            Some(DisplayLinkClick::Sftp) => {
                self.pending_display_link_sftp = Some(link.text.clone());
            }
            None => {}
        }
    }

    /// 宿主取走待跳转的 SFTP 路径（display-link 点击产生，#90）。
    pub fn take_pending_display_link_sftp(&mut self) -> Option<String> {
        self.pending_display_link_sftp.take()
    }

    /// 宿主取走待自动召回的错误码（display-link 点击产生，#90 后续）。
    pub fn take_pending_display_link_runbook(&mut self) -> Option<String> {
        self.pending_display_link_runbook.take()
    }

    /// 宿主取走待新开连接的 IP（右键 display-link 产生，#90 后续）。
    pub fn take_pending_display_link_connect(&mut self) -> Option<String> {
        self.pending_display_link_connect.take()
    }

    /// 设置或共享 ModuleRegistry（#117）。换表后丢掉布局缓存，避免沿用另一份模块状态下的装饰。
    pub fn set_module_registry(&mut self, reg: std::sync::Arc<crate::capability::ModuleRegistry>) {
        self.module_registry = reg;
        self.visual_layout_cache = None;
    }

    /// 获取当前持有的 ModuleRegistry 引用。
    pub fn module_registry(&self) -> &std::sync::Arc<crate::capability::ModuleRegistry> {
        &self.module_registry
    }

    /// 取走模块产生的未消费动作（#117 分发骨架）。
    pub fn take_pending_module_actions(&mut self) -> Vec<crate::capability::ModuleAction> {
        std::mem::take(&mut self.pending_module_actions)
    }

    /// 取走模块右键菜单点击项（#117）。
    pub fn take_pending_module_menu_action(&mut self) -> Option<String> {
        self.pending_module_menu_action.take()
    }

    /// 扫描 PTY 输出中的完整行，在 UI/编排线程分发给各 enabled 模块的 on_output_line（#117）。
    /// 注意：仅在 UI 线程处理收到的 SshMessage::Output 时调用，绝不在 SSH 泵线程运行。
    fn dispatch_output_to_modules(&mut self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }
        let lines =
            crate::capability::split_pty_output_lines(&mut self.module_output_line_acc, chunk);
        for line in lines {
            let ctx = crate::capability::OutputLineCtx::pty(&line);
            let actions = self.module_registry.on_output_line(&ctx);
            // Notify 入队给宿主。DecorateLink 不从这条 PTY 流画到格子上：
            // 原始行的字节区间对不齐视口，display-link 只在 Viewport 重扫时产出装饰（#120）。
            // 流式 DecorateLink 若入队，会把 Notify 挤出下面的上限。
            for act in actions {
                if let crate::capability::ModuleAction::Notify {
                    severity,
                    text,
                    attention,
                    ..
                } = &act
                {
                    log::debug!(
                        target: "capability",
                        "module notify: sev={:?} att={} text={}",
                        severity,
                        attention,
                        text
                    );
                }
                if !matches!(act, crate::capability::ModuleAction::DecorateLink { .. }) {
                    self.pending_module_actions.push(act);
                }
            }
            const CAP: usize = 32;
            if self.pending_module_actions.len() > CAP {
                let drain = self.pending_module_actions.len() - CAP;
                self.pending_module_actions.drain(0..drain);
            }
        }
        // 与 #112 pending trigger 队列一致：Notify 最多留 8 条，丢掉最早的，避免 Toast 风暴。
        self.cap_pending_notifies(8);
    }

    fn cap_pending_notifies(&mut self, cap: usize) {
        let notify_count = self
            .pending_module_actions
            .iter()
            .filter(|a| matches!(a, crate::capability::ModuleAction::Notify { .. }))
            .count();
        if notify_count <= cap {
            return;
        }
        let mut drop_n = notify_count - cap;
        self.pending_module_actions.retain(|a| {
            if drop_n > 0 && matches!(a, crate::capability::ModuleAction::Notify { .. }) {
                drop_n -= 1;
                false
            } else {
                true
            }
        });
    }

    /// 取走 Notify。其它动作留在队列里（PTY 路径不入队 DecorateLink，见 #120）。
    pub fn take_pending_notifies(&mut self) -> Vec<crate::capability::ModuleAction> {
        let pending = std::mem::take(&mut self.pending_module_actions);
        let mut notifies = Vec::new();
        for act in pending {
            match act {
                n @ crate::capability::ModuleAction::Notify { .. } => notifies.push(n),
                other => self.pending_module_actions.push(other),
            }
        }
        notifies
    }

    pub fn take_pending_send_to_ai(&mut self) -> bool {
        let v = self.pending_send_to_ai;
        self.pending_send_to_ai = false;
        v
    }

    pub fn take_pending_send_to_ai_text(&mut self) -> Option<String> {
        self.pending_send_to_ai_text.take()
    }

    /// 当前选区文本(无选区时为空)。
    pub fn selected_text(&self) -> String {
        self.get_selected_text()
    }

    /// 终端缓冲区底部最近 `max_lines` 行(含 scrollback)。
    pub fn tail_plain_text(&self, max_lines: usize) -> String {
        self.terminal.tail_plain_text(max_lines)
    }

    pub fn ai_session_meta(&self, session_name: Option<String>) -> crate::core::TerminalSessionMeta {
        let (username, host) = self
            .connection_target
            .as_ref()
            .map(|(u, h)| (Some(u.clone()), Some(h.clone())))
            .unwrap_or((None, None));
        crate::core::TerminalSessionMeta {
            host,
            username,
            session_name,
        }
    }

    pub fn take_pending_send_tail_to_ai(&mut self) -> bool {
        let v = self.pending_send_tail_to_ai;
        self.pending_send_tail_to_ai = false;
        v
    }

    /// 获取选中的文本(按绝对网格坐标从缓冲区读取，滚动后仍正确)
    fn get_selected_text(&self) -> String {
        if self.selection.is_empty() {
            return String::new();
        }
        let (start_l, start_c, end_l, end_c) = self.selection.normalize();
        let start = Point::new(Line(start_l), Column(start_c));
        let end = Point::new(Line(end_l), Column(end_c));
        self.terminal.text_in_point_range(start, end)
    }

    pub fn send_ctrl_c(&self) -> Result<(), String> {
        let loc = self.locale_last();
        if !self.connected {
            return Err(loc.tr("Not connected", "当前未连接").to_string());
        }
        let Some(handle) = self.ssh_handle.as_ref() else {
            return Err(loc.tr("PTY not ready", "PTY 未就绪").to_string());
        };
        if self.lrzsz.is_active() {
            self.lrzsz.cancel_active_transfer();
        }
        handle.send_input(&[0x03])
    }

    pub fn cancel_zmodem_transfer(&mut self) -> Result<(), String> {
        let loc = self.locale_last();
        if !self.connected {
            return Err(loc.tr("Not connected", "当前未连接").to_string());
        }
        let Some(handle) = self.ssh_handle.clone() else {
            return Err(loc.tr("PTY not ready", "PTY 未就绪").to_string());
        };
        self.lrzsz.unregister_shell_pump_upload_feed(&handle);
        self.lrzsz.cancel_active_transfer();
        self.pending_rz_upload = false;
        self.end_rz_handshake_capture();
        self.clear_rz_control_mode();
        self.clear_expect_rz_upload();
        self.transfer_progress = None;
        self.transfer_outgoing = false;
        self.auto_follow_output = true;
        self.pending_focus_terminal = true;
        self.terminal_wants_keyboard = true;
        self.terminal_focused = true;
        // ZMODEM 协议态下单个 Ctrl+C 不一定被远端 rz/sz 识别；高优先级发 CAN 序列再补 ETX。
        let cancel = vec![
            0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, // CAN x8
            0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, // erase noise
            0x03, // ETX
        ];
        match handle.send_priority_interrupt(cancel) {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = handle.send_zmodem(vec![
                    0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18,
                    0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08,
                ]);
                let _ = handle.send_input(&[0x03]);
                Err(e)
            }
        }
    }

    pub fn clear_rz_control_mode(&mut self) {
        self.rz_control_mode_until = None;
    }

    /// 用户在文件选择器中取消 rz 上传后，强制恢复交互态(含 Ctrl+C 终止远端 rz)
    pub fn cancel_rz_upload_selection(&mut self) {
        self.pending_rz_upload = false;
        self.end_rz_handshake_capture();
        self.clear_rz_control_mode();
        self.clear_expect_rz_upload();
        self.transfer_progress = None;
        self.transfer_outgoing = false;
        self.auto_follow_output = true;
        // 文件对话框关闭后主动抢回终端焦点，避免「取消后不能输入」
        self.pending_focus_terminal = true;
        self.terminal_wants_keyboard = true;
        self.terminal_focused = true;
        if self.lrzsz.is_active() {
            self.lrzsz.cancel_active_transfer();
        }
        if let Some(handle) = self.ssh_handle.as_ref() {
            let _ = handle.send_input(&[0x03]);
        }
    }

    /// 用户取消 rz 文件选择时关闭 PTY 旁路(与 `app` 里取消 pick 配套)
    pub fn end_rz_handshake_capture(&mut self) {
        if let Some(ref h) = self.ssh_handle {
            self.lrzsz.unregister_shell_pump_upload_feed(h);
        }
        self.lrzsz.end_rz_handshake_capture();
    }

    /// 基于当前交互式 PTY 通道执行 rz 对端对应的 ZMODEM 发送
    pub fn start_rz_upload(&mut self, path: &Path) -> Result<(), String> {
        let loc = self.locale_last();
        if !self.connected {
            return Err(loc.tr("Not connected", "当前未连接").to_string());
        }
        let path_str = path
            .to_str()
            .ok_or_else(|| loc.tr("File path contains invalid characters", "文件路径包含非法字符").to_string())?;
        let handle = self
            .ssh_handle
            .as_ref()
            .ok_or_else(|| loc.tr("PTY not ready", "PTY 未就绪").to_string())?;
        let pump_tx = handle.shell_pump_tx();
        self.rz_control_mode_until = Some(Instant::now() + Duration::from_secs(90));
        log::info!(
            "start_rz_upload: path={} session_id={:?}",
            path.display(),
            handle.session_id
        );
        match self.lrzsz.start_send(path_str, pump_tx) {
            Ok(()) => Ok(()),
            Err(e) => {
                self.lrzsz.end_rz_handshake_capture();
                Err(e)
            }
        }
    }

    /// 轮询后台上传结果(有结果时返回并清空任务)
    pub fn poll_upload_result(&mut self) -> Option<Result<String, String>> {
        let Some(rx) = self.upload_result_rx.as_ref() else {
            return None;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.upload_result_rx = None;
                Some(result)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.upload_result_rx = None;
                Some(Err(
                    self.locale_last()
                        .tr("Upload task ended unexpectedly", "上传任务异常中断")
                        .to_string(),
                ))
            }
        }
    }

    pub fn start_upload(&mut self, path: &Path) -> Result<(), String> {
        let loc_ui = self.locale_last();
        if self.upload_result_rx.is_some() {
            return Err(
                loc_ui
                    .tr("An upload task is already in progress", "已有上传任务正在进行中")
                    .to_string(),
            );
        }

        let handle = self.ssh_handle.as_ref().ok_or_else(|| {
            loc_ui
                .tr("No SSH session", "没有 SSH 会话")
                .to_string()
        })?;

        let file_name = path
            .file_name()
            .ok_or_else(|| {
                loc_ui
                    .tr("Invalid file path", "无效的文件路径")
                    .to_string()
            })?
            .to_string_lossy()
            .to_string();

        let remote_path = format!("./{}", file_name);
        let (tx, rx) = mpsc::channel::<Result<String, String>>();
        crate::ssh::ScpUpload::start(handle, path, remote_path, loc_ui, tx)?;
        self.upload_result_rx = Some(rx);
        Ok(())
    }

    /// SFTP 等：返回 [`SshSessionHandle`]。SFTP 必须经 shell 泵命令队列独占执行 Session，
    /// 否则与 PTY 读循环争用 libssh2 内部 mutex 会出现 `Timeout waiting for status message`。
    pub fn sftp_session_for_ops(&self) -> Option<SshSessionHandle> {
        if !self.connected {
            return None;
        }
        self.ssh_handle.clone()
    }

    /// 连接目标主机名（复盘草稿标题用，#67）；本机会话返回空。
    pub fn connection_target_text(&self) -> Option<String> {
        match &self.connection_target {
            Some((_, host)) if host != "local" => Some(host.clone()),
            _ => None,
        }
    }

    /// 当前生效的本机目录（偏好「默认本地目录」，与 SFTP 本机侧共用）。
    pub fn download_dir(&self) -> crate::core::local_dir::ResolvedLocalDir {
        crate::core::local_dir::current()
    }

    pub fn set_font_size(&mut self, size: f32) {
        let n = size.clamp(10.0, 24.0);
        if (n - self.font_size).abs() > f32::EPSILON {
            self.visual_layout_cache = None;
        }
        self.font_size = n;
    }

    pub fn font_size(&self) -> f32 {
        self.font_size
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// 终端网格是否持有键盘焦点(用于全局快捷键与 Delete 等不与 PTY 抢键)
    pub fn is_terminal_focused(&self) -> bool {
        self.terminal_focused
    }

    /// 用户已点击终端并希望输入(Enter 后 IME 失焦时仍保持，直到点击终端外)。
    pub fn wants_terminal_keyboard(&self) -> bool {
        self.terminal_wants_keyboard
    }

    pub fn ssh_session_handle(&self) -> Option<SshSessionHandle> {
        self.ssh_handle.clone()
    }

    pub fn ssh_session_id(&self) -> Option<usize> {
        self.session_id
    }

    /// 克隆 SSH 管理器，供监控面板 exec 采集等与 PTY 并行的操作使用。
    pub fn ssh_manager_clone(&self) -> Option<SshManager> {
        self.ssh_manager.as_ref().cloned()
    }

    /// 是否处于连接建立中(认证/启动 shell 阶段)
    pub fn is_connecting(&self) -> bool {
        if self.is_local_shell() {
            return !self.connected
                && self.error_message.is_none()
                && self.local_handle.is_some();
        }
        !self.connected && self.error_message.is_none() && self.ssh_manager.is_some()
    }

    /// 右 dock(SFTP / 端口转发等)是否可绑定当前 SSH 会话。
    pub fn right_dock_ssh_gate(&self) -> RightDockSshGate {
        if self.is_local_shell() {
            return RightDockSshGate::LocalShell;
        }
        if self.ssh_manager.is_none() && self.connection_target.is_none() {
            return RightDockSshGate::NoSession;
        }
        if self.is_connected() {
            RightDockSshGate::Ready
        } else if self.is_connecting() {
            RightDockSshGate::Connecting
        } else if let Some(err) = self.error_message.clone() {
            RightDockSshGate::Failed(err)
        } else {
            RightDockSshGate::Disconnected
        }
    }

    /// 还没连上时，黑屏中央写清正在连接或失败原因，并提供可操作按钮。
    fn paint_connection_placeholder(&mut self, ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect) {
        if self.connected || rect.width() < 40.0 || rect.height() < 40.0 {
            return;
        }
        let is_connecting = self.is_connecting();
        let err = self.error_message.clone();
        if !is_connecting && err.is_none() {
            return;
        }

        let mut action = ConnectionPlaceholderAction::None;
        let max_w = (rect.width() - 64.0).clamp(200.0, 420.0);
        // 勿用 centered_and_justified：Frame 会继承 main_justify，首个 label 被撑满整高，
        // 按钮行可用高度被压成 0（dogfood：只见文案不见「取消/重试/编辑/关闭」）。
        ui.allocate_ui_at_rect(rect, |ui| {
            ui.set_clip_rect(rect.intersect(ui.clip_rect()));
            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                let size_id = ui.id().with("connection_placeholder_block_h");
                let prev_h = ui
                    .ctx()
                    .data(|d| d.get_temp::<f32>(size_id))
                    .unwrap_or(96.0);
                let top_pad = ((ui.available_height() - prev_h) * 0.5).max(0.0);
                ui.add_space(top_pad);

                let frame_resp = egui::Frame::none()
                    .fill(theme.bg_window_color())
                    .rounding(8.0)
                    .inner_margin(egui::Margin::symmetric(18.0, 14.0))
                    .show(ui, |ui| {
                        // Frame 内再次钉死非 justify 的纵向布局，避免继承外层怪异 Layout。
                        ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                            ui.set_min_width(max_w);
                            ui.set_max_width(max_w);
                            if let Some(err) = err.as_deref() {
                                ui.label(
                                    egui::RichText::new(err)
                                        .size(theme.font_size_body())
                                        .color(theme.red_color()),
                                );
                                ui.add_space(theme.spacing_md());
                                // 按钮组整体居中，与上方文案同轴（勿贴左/贴右）。
                                let gap = theme.spacing_sm().max(8.0);
                                let row_w = theme.size_modal_footer_btn_min_w_secondary() * 2.0
                                    + theme.size_modal_footer_btn_min_w_primary()
                                    + gap * 2.0;
                                let pad = ((max_w - row_w) * 0.5).max(0.0);
                                ui.horizontal(|ui| {
                                    ui.add_space(pad);
                                    ui.spacing_mut().item_spacing.x = gap;
                                    if crate::ui::chrome::modal_secondary_button(
                                        ui,
                                        theme,
                                        crate::i18n::tr(ui.ctx(), "Close tab", "关闭标签"),
                                    )
                                    .clicked()
                                    {
                                        action = ConnectionPlaceholderAction::DismissTab;
                                    }
                                    if crate::ui::chrome::modal_secondary_button(
                                        ui,
                                        theme,
                                        crate::i18n::tr(ui.ctx(), "Edit host", "编辑主机"),
                                    )
                                    .clicked()
                                    {
                                        action = ConnectionPlaceholderAction::EditHost;
                                    }
                                    if crate::ui::chrome::modal_primary_button(
                                        ui,
                                        theme,
                                        crate::i18n::tr(ui.ctx(), "Retry", "重试"),
                                    )
                                    .clicked()
                                    {
                                        action = ConnectionPlaceholderAction::Retry;
                                    }
                                });
                            } else if is_connecting {
                                let host = self.connection_server_text();
                                let text = if self.is_local_shell() {
                                    crate::i18n::tr(
                                        ui.ctx(),
                                        "Opening local terminal…",
                                        "正在打开本机终端…",
                                    )
                                    .to_string()
                                } else {
                                    format!(
                                        "{} {}",
                                        crate::i18n::tr(ui.ctx(), "Connecting", "正在连接"),
                                        host
                                    )
                                };
                                ui.label(
                                    egui::RichText::new(text)
                                        .size(theme.font_size_body())
                                        .color(theme.text_secondary()),
                                );
                                ui.add_space(theme.spacing_md());
                                let btn_w = theme.size_modal_footer_btn_min_w_secondary();
                                let pad = ((max_w - btn_w) * 0.5).max(0.0);
                                ui.horizontal(|ui| {
                                    ui.add_space(pad);
                                    if crate::ui::chrome::modal_secondary_button(
                                        ui,
                                        theme,
                                        crate::i18n::tr(ui.ctx(), "Cancel", "取消"),
                                    )
                                    .clicked()
                                    {
                                        action = ConnectionPlaceholderAction::CancelConnect;
                                    }
                                });
                            }
                        });
                    });
                ui.ctx()
                    .data_mut(|d| d.insert_temp(size_id, frame_resp.response.rect.height()));
            });
        });
        if action != ConnectionPlaceholderAction::None {
            self.pending_connection_placeholder_action = action;
        }
    }

    /// 底栏连接状态(不写入终端 scrollback)。
    pub fn connection_status_for_bar(&self, theme: &Theme) -> Option<ConnectionBarStatus> {
        if self.ssh_manager.is_none() && self.connection_target.is_none() {
            return None;
        }
        let host_line = self.connection_server_text();
        let (state_line, state_color) = if let Some(err) = self.error_message.as_deref() {
            (
                truncate_connection_status(err, 36),
                theme.red_color(),
            )
        } else if self.connected {
            (
                self.connection_duration_text(),
                theme.green_color(),
            )
        } else if self.is_connecting() {
            (
                self.locale_last()
                    .tr("Connecting…", "正在连接…")
                    .to_string(),
                theme.accent_color(),
            )
        } else {
            (
                self.locale_last()
                    .tr("Disconnected", "已断开")
                    .to_string(),
                theme.amber_color(),
            )
        };
        Some(ConnectionBarStatus {
            host_line,
            state_line,
            state_color,
        })
    }

    /// README §2.6 连接时长格式
    pub fn connection_duration_text(&self) -> String {
        let loc = self.locale_last();
        let Some(connected_at) = self.connected_at else {
            return loc.tr("Not connected", "未连接").to_string();
        };
        let elapsed = connected_at.elapsed().as_secs();
        let mins = elapsed / 60;
        let hours = elapsed / 3600;
        let days = hours / 24;
        if days > 0 {
            format!(
                "{} {}d {}h",
                loc.tr("Connected", "已连接"),
                days,
                hours % 24
            )
        } else if hours > 0 {
            format!(
                "{} {}h {}m",
                loc.tr("Connected", "已连接"),
                hours,
                mins % 60
            )
        } else if mins > 0 {
            format!(
                "{} {} {}",
                loc.tr("Connected", "已连接"),
                mins,
                loc.tr("min", "分钟")
            )
        } else {
            loc.tr("Just connected", "刚连接").to_string()
        }
    }

    /// 连接错误摘要(供主窗口状态栏展示)
    pub fn connection_error_text(&self) -> Option<&str> {
        self.error_message.as_deref()
    }

    pub fn connection_server_text(&self) -> String {
        if let Some((username, host)) = &self.connection_target {
            return format!("{}@{}", username, host);
        }
        self.locale_last()
            .tr("No session selected", "未选择会话")
            .to_string()
    }

    /// 将当前 `cols`/`rows` 同步到 SSH PTY(上传中跳过 `resize_pty` 后由传输结束路径调用)
    fn flush_ssh_pty_size_after_transfer(&mut self) {
        if let Some(ref handle) = self.ssh_handle {
            if let Err(e) = handle.resize_pty(self.cols, self.rows) {
                log::error!("resize_pty after transfer failed: {}", e);
            } else {
                log::debug!(
                    "Synced resize_pty after transfer: {}x{}",
                    self.cols,
                    self.rows
                );
            }
        }
    }

    /// 调整终端尺寸
    pub fn resize(&mut self, cols: u32, rows: u32) {
        self.cols = cols;
        self.rows = rows;
        self.terminal.resize(cols as usize, rows as usize);

        if self.is_local_shell() {
            if let Some(ref handle) = self.local_handle {
                if let Err(e) = handle.resize_pty(cols, rows) {
                    log::error!("Failed to resize local PTY: {}", e);
                }
            }
            return;
        }
        if let Some(ref handle) = self.ssh_handle {
            // 文件选择关闭等会导致 UI 行数突变并立刻 resize_pty；与 ZMODEM 首轮帧并发时，
            // 远端 `rz` 易在握手中被 SIGWINCH 打乱，表现为约 10s 无入站再续(见用户日志)。
            if self.lrzsz.is_upload_pty_capture() {
                log::debug!(
                    "Deferring resize_pty during ZMODEM rz upload; UI is {}x{}, will sync after transfer",
                    cols,
                    rows
                );
                return;
            }
            if let Err(e) = handle.resize_pty(cols, rows) {
                log::error!("Failed to resize PTY: {}", e);
            }
        }
    }

    /// Abort an in-progress connect and show a cancelled message (placeholder stays actionable).
    pub fn cancel_in_flight_connect(&mut self, ctx: &egui::Context) {
        self.disconnect();
        self.error_message = Some(
            crate::i18n::tr(ctx, "Connection cancelled", "已取消连接").to_string(),
        );
    }

    /// 断开连接(关闭 SSH 并清空本地终端网格，用于移除标签等场景)
    pub fn disconnect(&mut self) {
        self.flush_interactive_to_disk();
        self.local_disconnect_intent = true;
        if let Some(ref h) = self.local_handle {
            h.request_stop();
        }
        if let Some(ref h) = self.ssh_handle {
            self.lrzsz.unregister_shell_pump_upload_feed(h);
        }
        self.pending_rz_upload = false;
        self.end_rz_handshake_capture();
        self.clear_rz_control_mode();
        self.connected = false;
        self.ssh_handle = None;
        self.local_handle = None;
        self.connection_kind = ConnectionKind::Ssh;
        self.ssh_manager = None;
        self.ssh_rx = None;
        self.session_id = None;
        self.terminal = VtTerminal::new(self.cols as usize, self.rows as usize);
        self.vt_visual_generation = self.vt_visual_generation.wrapping_add(1);
        self.visual_layout_cache = None;
        self.error_message = None;
        self.transfer_progress = None;
        self.transfer_outgoing = false;
        self.pending_focus_terminal = false;
        self.connected_at = None;
        self.terminal_wants_keyboard = false;
        self.terminal_focused = false;
        self.paste_pending.clear();
        self.paste_next_chunk_at = None;
        self.buffer_input_while_disconnected = false;
        self.disconnected_input_buffer.clear();
        self.resend_offline_input_dialog_open = false;
        // #177：断线丢弃未执行的恢复命令，避免旧命令跨会话泄漏
        self.pending_post_connect_command = None;
        self.post_connect_fire_at = None;
    }

    /// 仅断开 SSH，保留当前屏幕与滚动缓冲(FUNCTIONAL_SPEC §1.3.5：Tab 保留、输出冻结、不可再输入)
    pub fn disconnect_ssh_keep_buffer(&mut self) {
        self.flush_interactive_to_disk();
        self.local_disconnect_intent = true;
        if let Some(ref h) = self.local_handle {
            h.request_stop();
        }
        if let Some(ref h) = self.ssh_handle {
            self.lrzsz.unregister_shell_pump_upload_feed(h);
        }
        self.pending_rz_upload = false;
        self.end_rz_handshake_capture();
        self.clear_rz_control_mode();
        self.connected = false;
        self.ssh_handle = None;
        self.local_handle = None;
        // 保留 connection_kind=Local 以便 UI 文案；rx 关闭后不再读写
        self.ssh_manager = None;
        self.ssh_rx = None;
        self.session_id = None;
        self.error_message = None;
        self.transfer_progress = None;
        self.transfer_outgoing = false;
        self.pending_focus_terminal = false;
        self.connected_at = None;
        self.terminal_wants_keyboard = false;
        self.terminal_focused = false;
        self.paste_pending.clear();
        self.paste_next_chunk_at = None;
        self.buffer_input_while_disconnected = true;
        self.resend_offline_input_dialog_open = false;
        let disconnected_banner = match self.ui_lang_last {
            UiLanguage::Zh => concat!(
                "\r\n\x1b[33m[已断开 SSH；键盘输入将暂存，重连后可选择是否发送到远端]",
                "\x1b[0m\r\n"
            ),
            UiLanguage::En => concat!(
                "\r\n\x1b[33m[SSH disconnected; keystrokes will be queued and can be resent after reconnecting]",
                "\x1b[0m\r\n"
            ),
        };
        self.terminal.feed(disconnected_banner.as_bytes());
    }

    pub const ERR_FRAGMENT_NOT_CONNECTED: &'static str = "__mistterm_fragment_not_connected";
    pub const ERR_FRAGMENT_NO_SSH_HANDLE: &'static str = "__mistterm_fragment_no_ssh_handle";
    pub const FRAGMENT_SEND_FAILED_PREFIX: &'static str = "__mistterm_fragment_send_failed:";

    /// 多行命令没法只放进输入行：shell 会一行一行直接执行。
    pub const ERR_INSERT_MULTILINE: &'static str = "__mistterm_insert_multiline";

    /// 是否是多行命令(去掉空行后超过一行)。
    pub fn is_multiline_command(command: &str) -> bool {
        command
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count()
            > 1
    }

    /// 把命令放进当前输入行，不加回车、不执行，由用户检查后自己按回车。
    pub fn insert_command_text(&mut self, command: &str) -> Result<(), String> {
        if !self.connected {
            return Err(Self::ERR_FRAGMENT_NOT_CONNECTED.to_string());
        }
        if Self::is_multiline_command(command) {
            return Err(Self::ERR_INSERT_MULTILINE.to_string());
        }
        let loc = self.locale_last();
        if !self.has_pty_writer() {
            return Err(loc
                .tr("SSH session handle unavailable", "连接句柄不可用")
                .to_string());
        }
        let line = command.trim_matches(|c| c == '\r' || c == '\n').trim_end();
        self.send_pty_input(line.as_bytes())
            .map_err(|e| format!("{}: {}", loc.tr("Send failed", "发送失败"), e))?;
        // 「用到终端」放进输入行的文本也计入本行，回车时与后续手敲内容作为整行审计。
        self.track_typed_text(line);
        Ok(())
    }

    /// 插入命令片段(自动添加回车)
    pub fn insert_fragment(&mut self, command: &str) -> Result<(), String> {
        if !self.connected {
            return Err(Self::ERR_FRAGMENT_NOT_CONNECTED.to_string());
        }
        let loc = self.locale_last();
        if !self.has_pty_writer() {
            return Err(loc
                .tr("SSH session handle unavailable", "连接句柄不可用")
                .to_string());
        }
        let input = format!("{}\r", command);
        self.send_pty_input(input.as_bytes()).map_err(|e| {
            format!(
                "{}: {}",
                loc.tr("Send failed", "发送失败"),
                e
            )
        })
    }

    /// 取走拖入终端区域的上传路径(宿主每帧至多处理一次)。
    pub fn take_drop_upload_paths(&mut self) -> Vec<PathBuf> {
        std::mem::take(&mut self.pending_drop_upload_paths)
    }

    /// 非用户主动断开时为 `true` 一次(供自动重连等)；读取后清除。
    pub fn take_unexpected_disconnect_notified(&mut self) -> bool {
        std::mem::take(&mut self.unexpected_disconnect_notified)
    }

    /// #93：VTE BEL（响铃），宿主聚合到未聚焦标签。
    pub fn take_pending_bell(&mut self) -> bool {
        self.terminal.take_pending_bell()
    }

    /// #93：会话意外结束（与重连通知分通道）。
    pub fn take_pending_attention_ended(&mut self) -> bool {
        std::mem::take(&mut self.pending_attention_ended)
    }

    /// 取走待上报的连接结果(`success`, `host`)。
    pub fn take_connect_audit(&mut self) -> Option<(bool, String)> {
        self.pending_connect_audit.take()
    }

    /// 取走服务器侧命令审计事件(agent 经 PTY 回传的 `MIST_AUDIT` 行)。
    pub fn take_server_audit_events(&mut self) -> Vec<ServerAuditEvent> {
        std::mem::take(&mut self.pending_server_audit)
    }

    /// 更新服务器侧审计可用性；返回 `Some(true)` 表示刚从降级恢复。
    pub fn set_agent_audit_available(&mut self, available: bool) -> Option<bool> {
        let was_degraded = self.agent_audit_degraded;
        self.agent_audit_degraded = !available;
        if available {
            self.agent_audit_banner_dismissed = false;
            if was_degraded {
                return Some(true);
            }
        }
        None
    }

    pub fn agent_audit_banner_visible(&self) -> bool {
        self.connected && self.agent_audit_degraded && !self.agent_audit_banner_dismissed
    }

    pub fn dismiss_agent_audit_banner(&mut self) {
        self.agent_audit_banner_dismissed = true;
    }

    pub fn take_pending_toast_error(&mut self) -> Option<String> {
        self.pending_toast_error.take()
    }

    pub fn take_connection_placeholder_action(&mut self) -> ConnectionPlaceholderAction {
        let a = self.pending_connection_placeholder_action;
        self.pending_connection_placeholder_action = ConnectionPlaceholderAction::None;
        a
    }

    /// 大文件走 ZMODEM：向 PTY 发送 `rz -y` 并在握手就绪后用 `path` 启动上传(FUNCTIONAL_SPEC §4.3)。
    pub fn queue_zmodem_upload_after_rz(&mut self, path: PathBuf) {
        self.zmodem_upload_after_rz_path = Some(path);
        self.pending_rz_upload = false;
        self.note_rz_shell_command_submitted();
        if self.connected && self.ssh_handle.is_some() {
            let _ = self.send_pty_input(b"\nrz -y\r");
        }
    }

    /// 在完整终端缓冲(含 scrollback)中搜索。
    pub fn search_all(&self, query: &str, ignore_case: bool) -> Vec<crate::terminal::SearchHit> {
        self.terminal.search_all(query, ignore_case)
    }

    /// 滚到命中行并返回视口内高亮坐标(1-based)。
    pub fn reveal_search_hit(
        &mut self,
        hit: crate::terminal::SearchHit,
    ) -> Option<(usize, usize)> {
        self.visual_layout_cache = None;
        self.terminal.reveal_search_hit(hit)
    }
}

/// 人类可读的文件大小格式
fn human_readable_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * 1024 * 1024;
    
    if size >= GB {
        format!("{:.2} GB", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.2} MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.2} KB", size as f64 / KB as f64)
    } else {
        format!("{} B", size)
    }
}

fn resolve_local_cwd(raw: Option<&str>) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return home;
    };
    if raw == "~" {
        return home;
    }
    if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        return home.join(rest);
    }
    PathBuf::from(raw)
}

fn prompt_ready_for_cd(line: &str) -> bool {
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

/// 从常见提示符里取出当前目录。认不出来就返回 `None`。
fn parse_prompt_cwd(line: &str) -> Option<String> {
    let t = line.trim();
    if let Some(rest) = t.strip_prefix("PS ") {
        let path = rest.trim().trim_end_matches('>').trim();
        if looks_like_dir(path) {
            return Some(path.to_string());
        }
    }
    if t.ends_with('>') {
        let path = t.trim_end_matches('>').trim();
        if looks_like_dir(path) {
            return Some(path.to_string());
        }
        if let Some(path) = last_path_token(path) {
            return Some(path);
        }
    }
    if t.ends_with('$') || t.ends_with('#') || t.ends_with('%') {
        let body = t
            .trim_end_matches(|c| c == '$' || c == '#' || c == '%' || c == ' ')
            .trim();
        if let Some(idx) = body.rfind(':') {
            let path = body[idx + 1..].trim();
            if path.starts_with('/') || path.starts_with('~') {
                return Some(path.to_string());
            }
        }
        // macOS zsh 常见 `user@host ~/src %`，Linux 自定义提示符也可能不带冒号。
        if let Some(path) = last_path_token(body) {
            return Some(path);
        }
    }
    None
}

fn last_path_token(body: &str) -> Option<String> {
    body.split_whitespace()
        .rev()
        .find(|token| looks_like_dir(token))
        .map(|token| token.to_string())
}

fn looks_like_dir(path: &str) -> bool {
    looks_like_win_path(path) || path.starts_with('/') || path.starts_with('~')
}

fn looks_like_win_path(path: &str) -> bool {
    let b = path.as_bytes();
    b.len() >= 3 && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
}

/// 根据刚提交的 `cd` / `Set-Location` 更新目录。认不出命令时返回 `None`。
fn apply_cd_line(cwd: Option<&str>, line: &str, local_home: Option<&str>) -> Option<String> {
    let rest = strip_cd_invocation(line)?;
    let arg = normalize_cd_arg(rest);
    if arg == "-" {
        return None;
    }
    if arg.is_empty() || arg == "~" {
        return local_home
            .map(|s| s.to_string())
            .or_else(|| Some("~".to_string()));
    }
    if let Some(rest) = arg.strip_prefix("~/").or_else(|| arg.strip_prefix("~\\")) {
        if let Some(home) = local_home {
            return Some(join_dir(home, rest));
        }
        return Some(format!("~/{rest}"));
    }
    if is_absolute_dir(&arg) {
        return Some(arg);
    }
    let base = cwd.filter(|s| !s.is_empty())?;
    Some(join_dir(base, &arg))
}

fn strip_cd_invocation(line: &str) -> Option<&str> {
    let t = line.trim();
    let lower = t.to_ascii_lowercase();
    if lower == "cd" || lower.starts_with("cd ") || lower.starts_with("cd\t") {
        return Some(t[2..].trim_start());
    }
    const SL: &str = "set-location";
    if lower == SL {
        return Some("");
    }
    if lower.starts_with("set-location ") || lower.starts_with("set-location\t") {
        return Some(t[SL.len()..].trim_start());
    }
    None
}

fn normalize_cd_arg(rest: &str) -> String {
    let mut parts = rest.split_whitespace();
    let mut arg = String::new();
    for part in parts.by_ref() {
        let l = part.to_ascii_lowercase();
        if l == "/d" || l == "--" || l == "-literalpath" || l == "-path" {
            continue;
        }
        if l.starts_with('-') {
            continue;
        }
        arg = part.to_string();
        break;
    }
    arg.trim_matches(|c| c == '"' || c == '\'').to_string()
}

fn is_absolute_dir(path: &str) -> bool {
    looks_like_win_path(path) || path.starts_with('/') || path.starts_with('\\')
}

fn join_dir(base: &str, rel: &str) -> String {
    if is_absolute_dir(rel) {
        return rel.to_string();
    }
    let win = base.contains('\\') || rel.contains('\\');
    let sep = if win { '\\' } else { '/' };
    let mut out = base.trim_end_matches(['/', '\\']).to_string();
    for part in rel.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            if let Some(i) = out.rfind(['/', '\\']) {
                let head = &out[..i];
                if i == 0 {
                    out.truncate(1);
                } else if head.len() == 2 && head.as_bytes()[1] == b':' {
                    out.truncate(i + 1);
                } else {
                    out.truncate(i);
                }
            }
            continue;
        }
        if out.is_empty() {
            out.push_str(part);
        } else {
            out.push(sep);
            out.push_str(part);
        }
    }
    out
}

fn startup_cd_payload(path: &str) -> Vec<u8> {
    if looks_like_win_path(path) || path.contains('\\') {
        let esc = path.replace('\'', "''");
        format!("Set-Location -LiteralPath '{esc}'\r").into_bytes()
    } else {
        let esc = path.replace('\'', "'\\''");
        format!("cd -- '{esc}'\r").into_bytes()
    }
}

impl Default for TerminalView {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod display_link_stroke_tests {
    use super::*;

    #[test]
    fn display_link_underline_is_one_physical_pixel() {
        let ctx = egui::Context::default();
        let _ = ctx.run(
            egui::RawInput {
                pixels_per_point: Some(2.0),
                ..Default::default()
            },
            |_| {},
        );
        let theme = Theme::dark();
        let idle = TerminalView::display_link_underline_stroke(&theme, &ctx, false);
        let hover = TerminalView::display_link_underline_stroke(&theme, &ctx, true);
        let ppp = ctx.pixels_per_point();
        assert!((idle.width * ppp - 1.0).abs() < 0.01, "idle {}", idle.width);
        assert!((hover.width * ppp - 1.0).abs() < 0.01, "hover {}", hover.width);
        assert!(hover.width <= theme.hairline_width(&ctx) + f32::EPSILON);
        assert!(idle.color.a() >= 140 && idle.color.a() <= 180);
        assert!(hover.color.a() >= 140 && hover.color.a() <= 180);
        assert!(hover.color.a() >= idle.color.a());
        assert_ne!(idle.color.a(), 255);
    }
}

#[cfg(test)]
mod drop_tests {
    use super::*;

    fn frame(ctx: &egui::Context, events: Vec<egui::Event>, dropped: &[&str]) -> Vec<PathBuf> {
        let raw = egui::RawInput {
            events,
            dropped_files: dropped
                .iter()
                .map(|p| egui::DroppedFile {
                    path: Some(PathBuf::from(p)),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let mut out = Vec::new();
        let _ = ctx.run(raw, |ctx| {
            out = ctx.input(TerminalView::unplaced_dropped_files);
        });
        out
    }

    #[test]
    fn drop_from_other_app_without_pointer_goes_to_host() {
        let ctx = egui::Context::default();
        frame(&ctx, vec![egui::Event::PointerMoved(egui::pos2(50.0, 50.0))], &[]);
        frame(&ctx, vec![egui::Event::PointerGone], &[]);
        frame(&ctx, vec![], &[]);
        assert_eq!(
            frame(&ctx, vec![], &["/tmp/a.bin"]),
            vec![PathBuf::from("/tmp/a.bin")]
        );
    }

    #[test]
    fn drop_with_known_pointer_is_left_to_the_pane_under_it() {
        let ctx = egui::Context::default();
        frame(&ctx, vec![egui::Event::PointerMoved(egui::pos2(50.0, 50.0))], &[]);
        assert!(frame(&ctx, vec![], &["/tmp/a.bin"]).is_empty());
    }
}

#[cfg(test)]
mod insert_only_tests {
    use super::*;

    #[test]
    fn multiline_detection_ignores_blank_lines() {
        assert!(!TerminalView::is_multiline_command("df -h"));
        assert!(!TerminalView::is_multiline_command("df -h\n"));
        assert!(!TerminalView::is_multiline_command("\n  df -h  \r\n\n"));
        assert!(TerminalView::is_multiline_command("cd /tmp\nls"));
        assert!(TerminalView::is_multiline_command("cd /tmp\r\nls"));
    }
}

#[cfg(test)]
mod typed_enter_gate_tests {
    use super::*;

    fn gated_view(line: &str) -> TerminalView {
        let mut v = TerminalView::new();
        v.connected = true;
        v.set_typed_enter_gate(true);
        v.track_typed_text(line);
        v
    }

    #[test]
    fn gate_off_or_disconnected_keeps_old_behavior() {
        let mut v = gated_view("rm -rf /tmp/x");
        v.set_typed_enter_gate(false);
        assert_eq!(v.typed_enter_gate_line(), None);
        let mut v = gated_view("rm -rf /tmp/x");
        v.connected = false;
        assert_eq!(v.typed_enter_gate_line(), None);
    }

    #[test]
    fn gate_returns_trimmed_line() {
        let v = gated_view("  systemctl restart nginx  ");
        assert_eq!(
            v.typed_enter_gate_line().as_deref(),
            Some("systemctl restart nginx")
        );
        assert_eq!(gated_view("   ").typed_enter_gate_line(), None);
    }

    #[test]
    fn gate_skips_alt_screen_and_secret_prompts() {
        let mut v = gated_view(":wq");
        v.terminal.feed(b"\x1b[?1049h");
        assert_eq!(v.typed_enter_gate_line(), None);

        let mut v = gated_view("hunter2");
        v.terminal.feed(b"[sudo] password for tian: ");
        assert_eq!(v.typed_enter_gate_line(), None);
        assert!(TerminalView::looks_like_secret_prompt(
            "Enter passphrase for key"
        ));
        assert!(TerminalView::looks_like_secret_prompt("请输入密码："));
        assert!(!TerminalView::looks_like_secret_prompt("tian@demo:~$ "));
    }

    #[test]
    fn gated_enter_is_held_until_released() {
        let mut v = gated_view("systemctl restart nginx");
        v.submit_enter_key();
        // 回车被扣住：未记历史，行缓冲保留，等宿主审计。
        assert_eq!(v.take_submitted_line(), None);
        assert_eq!(
            v.take_typed_enter_request().as_deref(),
            Some("systemctl restart nginx")
        );
        assert_eq!(v.typed_line_buffer, "systemctl restart nginx");
        assert_eq!(v.take_typed_enter_request(), None);
    }

    #[test]
    fn single_line_paste_is_part_of_the_audited_line() {
        let mut v = gated_view("sudo ");
        v.track_typed_text("shutdown -h now");
        assert_eq!(
            v.typed_enter_gate_line().as_deref(),
            Some("sudo shutdown -h now")
        );
    }

    #[test]
    fn ctrl_keys_and_backspace_update_line_buffer() {
        let mut v = gated_view("systemctl restart nginx");
        v.track_backspace();
        assert_eq!(v.typed_line_buffer, "systemctl restart ngin");
        v.track_ctrl_byte(0x17);
        assert_eq!(v.typed_line_buffer, "systemctl restart ");
        v.track_ctrl_byte(0x15);
        assert_eq!(v.typed_line_buffer, "");
        v.track_typed_text("ls");
        v.track_ctrl_byte(0x03);
        assert_eq!(v.typed_enter_gate_line(), None);
    }

    #[test]
    fn discard_clears_held_line_so_next_enter_is_clean() {
        let mut v = gated_view("systemctl restart nginx");
        v.submit_enter_key();
        assert!(v.take_typed_enter_request().is_some());
        v.discard_typed_line();
        assert_eq!(v.typed_line_buffer, "");
        v.track_typed_text("uptime");
        assert_eq!(v.typed_enter_gate_line().as_deref(), Some("uptime"));
    }
}

#[cfg(test)]
mod interactive_exec_terminal_tests {
    use super::*;

    fn capturing_view(line: &str) -> TerminalView {
        let mut v = TerminalView::new();
        v.connected = true;
        v.set_interactive_capture(true, "sess-9", "10.1.2.3");
        v.track_typed_text(line);
        v
    }

    #[test]
    fn interactive_exec_capture_off_does_not_record() {
        let mut v = capturing_view("echo hi");
        v.set_interactive_capture(false, "sess-9", "10.1.2.3");
        v.release_typed_enter();
        v.observe_interactive_output(b"\r\nhi\r\nuser@host:~$ ");
        assert!(v.take_interactive_exec_records().is_empty());
    }

    #[test]
    fn interactive_exec_typed_command_becomes_one_record() {
        let mut v = capturing_view("echo hi");
        v.set_pending_capture_outcome("allowed");
        v.release_typed_enter();
        assert!(
            v.take_interactive_exec_records().is_empty(),
            "still waiting for the prompt"
        );
        v.observe_interactive_output(b"\r\nhi\r\nuser@host:~$ ");
        let rows = v.take_interactive_exec_records();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].target, "10.1.2.3");
        assert_eq!(rows[0].command, "echo hi");
        assert!(rows[0].timestamp.contains('T'));
        assert!(rows[0].output_summary.contains("hi"));
        assert!(rows[0].tags.iter().any(|t| t == "allowed"));
        v.observe_interactive_output(b"user@host:~$ ");
        assert!(v.take_interactive_exec_records().is_empty());
    }

    #[test]
    fn interactive_exec_skips_secret_prompt_and_alt_screen() {
        let mut v = capturing_view("hunter2");
        v.terminal.feed(b"[sudo] password for tian: ");
        v.release_typed_enter();
        v.observe_interactive_output(b"\r\nuser@host:~$ ");
        assert!(v.take_interactive_exec_records().is_empty());

        let mut v = capturing_view("dd");
        v.terminal.feed(b"\x1b[?1049h");
        v.release_typed_enter();
        v.observe_interactive_output(b"\r\nuser@host:~$ ");
        assert!(v.take_interactive_exec_records().is_empty());
    }

    #[test]
    fn interactive_exec_server_block_closes_open_command() {
        let mut v = capturing_view("rm -rf /");
        v.release_typed_enter();
        v.note_server_audit_outcome("rm -rf /", "blocked");
        let rows = v.take_interactive_exec_records();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].command, "rm -rf /");
        assert!(rows[0].tags.iter().any(|t| t == "blocked"));
        assert!(!rows[0].ok);
        assert!(rows[0].exit_code.is_none());
    }

    #[test]
    fn interactive_exec_server_block_retags_record_already_closed_by_prompt() {
        let mut v = capturing_view("rm -rf /");
        v.release_typed_enter();
        v.observe_interactive_output(b"\r\nuser@host:~$ ");
        v.note_server_audit_outcome("rm -rf /", "blocked");
        let rows = v.take_interactive_exec_records();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].tags.iter().any(|t| t == "blocked"));
        assert!(!rows[0].tags.iter().any(|t| t == "allowed"));
        assert!(!rows[0].ok);
    }
}

#[cfg(test)]
mod selection_extend_tests {
    use super::*;

    #[test]
    fn double_click_stops_at_colon() {
        let line = "root@host:~/src";
        let host_at = line.find("host").unwrap();
        let (start, end) = TerminalView::terminal_token_range_at(line, host_at).unwrap();
        assert_eq!(&line[start..end], "root@host");
        let path_at = line.find("src").unwrap();
        let (start, end) = TerminalView::terminal_token_range_at(line, path_at).unwrap();
        assert_eq!(&line[start..end], "/src");
        let colon = line.find(':').unwrap();
        let (start, end) = TerminalView::terminal_token_range_at(line, colon).unwrap();
        assert_eq!(&line[start..end], "root@host");
    }

    #[test]
    fn double_click_includes_base64_padding() {
        let line = "echo YWJjZA== done";
        let letter = line.find('Y').unwrap();
        let (start, end) = TerminalView::terminal_token_range_at(line, letter).unwrap();
        assert_eq!(&line[start..end], "YWJjZA==");

        let pad = line.find('=').unwrap();
        let (start, end) = TerminalView::terminal_token_range_at(line, pad).unwrap();
        assert_eq!(&line[start..end], "YWJjZA==");
        let (start, end) = TerminalView::terminal_token_range_at(line, pad + 1).unwrap();
        assert_eq!(&line[start..end], "YWJjZA==");

        let line_single = "tok YQ= end";
        let letter = line_single.find('Y').unwrap();
        let (start, end) = TerminalView::terminal_token_range_at(line_single, letter).unwrap();
        assert_eq!(&line_single[start..end], "YQ=");
        let pad = line_single.find('=').unwrap();
        let (start, end) = TerminalView::terminal_token_range_at(line_single, pad).unwrap();
        assert_eq!(&line_single[start..end], "YQ=");
    }

    #[test]
    fn extend_keeps_anchor_across_scroll() {
        let mut sel = Selection::default();
        // 在 display_offset=0 第 2 行第 3 列点下锚点
        sel.set_from_viewport(0, 2, 3, 2, 3);
        // 向上翻 50 行后 Shift+单击第 0 行第 1 列
        sel.extend_from_viewport(50, 0, 1, None);
        assert_eq!((sel.start_line, sel.start_col), (2, 3));
        assert_eq!((sel.end_line, sel.end_col), (-50, 1));
        assert_eq!(sel.normalize(), (-50, 1, 2, 3));
        assert!(!sel.is_empty());
    }

    #[test]
    fn extend_without_selection_uses_fallback_anchor() {
        let mut sel = Selection::default();
        sel.extend_from_viewport(10, 5, 0, Some((20, 4)));
        assert_eq!((sel.start_line, sel.start_col), (20, 4));
        assert_eq!((sel.end_line, sel.end_col), (-5, 0));
        assert_eq!(sel.normalize(), (-5, 0, 20, 4));
    }

    #[test]
    fn extend_without_anchor_is_empty_at_click() {
        let mut sel = Selection::default();
        sel.extend_from_viewport(0, 3, 7, None);
        assert!(sel.is_empty());
        assert!(sel.active);
    }

    #[test]
    fn drag_end_tracks_offset_change() {
        let mut sel = Selection::default();
        sel.set_from_viewport(0, 10, 0, 10, 0);
        sel.set_end_from_viewport(30, 0, 0);
        assert_eq!(sel.normalize(), (-30, 0, 10, 0));
    }

    #[test]
    fn click_scroll_shift_click_keeps_anchor() {
        // seq 1 400 场景：翻到上方(offset=350)单击第 5 行第 0 列
        let mut sel = Selection::default();
        sel.set_anchor_from_viewport(350, 5, 0);
        assert!(sel.is_empty(), "plain click shows no selection");
        // 滚轮回到底部后 Shift+单击第 20 行第 3 列；光标在底部，不应被当锚点
        sel.extend_from_viewport(0, 20, 3, Some((39, 0)));
        assert_eq!((sel.start_line, sel.start_col), (-345, 0));
        assert_eq!(sel.normalize(), (-345, 0, 20, 3));
        // 再次 Shift+单击只移动终点
        sel.extend_from_viewport(100, 0, 2, Some((39, 0)));
        assert_eq!(sel.normalize(), (-345, 0, -100, 2));
    }

    #[test]
    fn plain_click_clears_old_selection() {
        let mut sel = Selection::default();
        sel.set_from_viewport(0, 1, 0, 3, 4);
        sel.set_anchor_from_viewport(0, 10, 2);
        assert!(sel.is_empty());
        sel.extend_from_viewport(0, 12, 0, None);
        assert_eq!(sel.normalize(), (10, 2, 12, 0));
    }

    #[test]
    fn autoscroll_speed_proportional() {
        assert_eq!(drag_autoscroll_lines_per_sec(50.0, 0.0, 100.0, 10.0), 0.0);
        let up1 = drag_autoscroll_lines_per_sec(-5.0, 0.0, 100.0, 10.0);
        let up2 = drag_autoscroll_lines_per_sec(-50.0, 0.0, 100.0, 10.0);
        assert!(up1 > 0.0 && up2 > up1);
        let dn = drag_autoscroll_lines_per_sec(150.0, 0.0, 100.0, 10.0);
        assert!(dn < 0.0);
    }
}

#[cfg(test)]
mod cwd_tests {
    use super::{apply_cd_line, join_dir, parse_prompt_cwd, startup_cd_payload};

    #[test]
    fn prompt_paths() {
        assert_eq!(
            parse_prompt_cwd("PS C:\\Users\\Admin>"),
            Some("C:\\Users\\Admin".into())
        );
        assert_eq!(
            parse_prompt_cwd("admin@host:~/src$"),
            Some("~/src".into())
        );
        assert_eq!(
            parse_prompt_cwd("admin@MacBook-Pro ~/src %"),
            Some("~/src".into())
        );
        assert_eq!(
            parse_prompt_cwd("root@debian:/var/log#"),
            Some("/var/log".into())
        );
        assert_eq!(
            parse_prompt_cwd("alice@host ~/work>"),
            Some("~/work".into())
        );
        assert_eq!(parse_prompt_cwd("echo hello"), None);
    }

    #[test]
    fn cd_joins_relative_and_parent() {
        assert_eq!(
            apply_cd_line(Some("C:\\Users\\Admin"), "cd Desktop", Some("C:\\Users\\Admin")),
            Some("C:\\Users\\Admin\\Desktop".into())
        );
        assert_eq!(
            apply_cd_line(Some("/var/log"), "cd ..", None),
            Some("/var".into())
        );
        assert_eq!(
            apply_cd_line(Some("/tmp"), "cd", Some("/home/a")),
            Some("/home/a".into())
        );
        assert_eq!(apply_cd_line(Some("/tmp"), "ls", None), None);
        assert_eq!(join_dir("/var/log", "nginx"), "/var/log/nginx");
    }

    #[test]
    fn startup_cd_quotes() {
        let win = String::from_utf8(startup_cd_payload("C:\\Users\\O'Brien")).unwrap();
        assert!(win.contains("Set-Location"));
        assert!(win.contains("O''Brien"));
        let posix = String::from_utf8(startup_cd_payload("/tmp/a b")).unwrap();
        assert!(posix.starts_with("cd -- "));
    }
}

#[cfg(test)]
mod host_wiring_tests {
    use super::*;
    use crate::capability::{
        BeforeSendCtx, DockContribution, MenuCtx, MenuItem, MistModule, ModuleAction, ModuleId,
        ModuleRegistry, OutputLineCtx, SendVerdict, Severity,
    };
    use std::sync::{Arc, Mutex};

    struct TestOutputModule {
        lines: Arc<Mutex<Vec<String>>>,
    }

    impl MistModule for TestOutputModule {
        fn id(&self) -> ModuleId {
            "test-output"
        }
        fn name_zh(&self) -> &str {
            "测试输出模块"
        }
        fn name_en(&self) -> &str {
            "Test Output Module"
        }
        fn on_output_line(&self, ctx: &OutputLineCtx<'_>) -> Vec<ModuleAction> {
            self.lines.lock().unwrap().push(ctx.line.to_string());
            vec![ModuleAction::Notify {
                severity: Severity::Info,
                text: format!("got: {}", ctx.line),
                attention: true,
                dedupe_key: String::new(),
            }]
        }
        fn contribute_context_menu(&self, _ctx: &MenuCtx<'_>) -> Vec<MenuItem> {
            vec![MenuItem {
                action: "test.action".into(),
                label_zh: "测试菜单项".into(),
                label_en: "Test Menu Item".into(),
            }]
        }
    }

    #[test]
    fn test_dispatch_output_to_modules_complete_lines() {
        let mut view = TerminalView::new();
        let lines_received = Arc::new(Mutex::new(Vec::new()));
        let reg = ModuleRegistry::with_modules(vec![Box::new(TestOutputModule {
            lines: lines_received.clone(),
        })]);
        view.set_module_registry(Arc::new(reg));

        // 写入不完整行
        view.dispatch_output_to_modules(b"hello ");
        assert!(lines_received.lock().unwrap().is_empty());
        assert!(view.take_pending_module_actions().is_empty());

        // 补全换行符（CRLF）
        view.dispatch_output_to_modules(b"world\r\n");
        assert_eq!(*lines_received.lock().unwrap(), vec!["hello world".to_string()]);
        
        let actions = view.take_pending_module_actions();
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            ModuleAction::Notify { text, .. } => assert_eq!(text, "got: hello world"),
            _ => panic!("unexpected action"),
        }
    }

    #[test]
    fn test_disabled_module_in_host_gets_no_events() {
        let mut view = TerminalView::new();
        let lines_received = Arc::new(Mutex::new(Vec::new()));
        let reg = ModuleRegistry::with_modules(vec![Box::new(TestOutputModule {
            lines: lines_received.clone(),
        })]);
        reg.set_enabled("test-output", false);
        view.set_module_registry(Arc::new(reg));

        view.dispatch_output_to_modules(b"line 1\n");
        assert!(lines_received.lock().unwrap().is_empty());
        assert!(view.take_pending_module_actions().is_empty());
    }

    fn view_with_builtin_modules() -> TerminalView {
        let mut view = TerminalView::new();
        let engine = std::sync::Arc::new(std::sync::Mutex::new(
            crate::core::CmdAuditEngine::new(),
        ));
        view.set_module_registry(Arc::new(ModuleRegistry::with_modules(
            crate::modules::builtin_modules(engine),
        )));
        view
    }

    #[test]
    fn output_trigger_module_notifies_on_complete_line() {
        let mut view = view_with_builtin_modules();
        view.dispatch_output_to_modules(b"noise\nECONNREFUSED here\r\npartial");
        let actions = view.take_pending_notifies();
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            ModuleAction::Notify {
                severity,
                text,
                attention,
                dedupe_key,
            } => {
                assert_eq!(*severity, Severity::Error);
                assert!(text.contains("ECONNREFUSED"));
                assert!(*attention);
                assert_eq!(dedupe_key, "TR-ERR-001");
            }
            other => panic!("expected notify, got {other:?}"),
        }
        // 半行还没换行，不应再产出。
        assert!(view.take_pending_notifies().is_empty());
        view.dispatch_output_to_modules(b" WARNING: disk almost full\n");
        let actions = view.take_pending_notifies();
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            ModuleAction::Notify {
                severity,
                dedupe_key,
                ..
            } => {
                assert_eq!(*severity, Severity::Warn);
                assert_eq!(dedupe_key, "TR-WARN-001");
            }
            other => panic!("expected warn notify, got {other:?}"),
        }
    }

    #[test]
    fn output_trigger_disabled_yields_no_notify() {
        let mut view = view_with_builtin_modules();
        view.module_registry
            .sync_effective_disabled(&["output-trigger".to_string()]);
        view.dispatch_output_to_modules(b"ECONNREFUSED\nBuild succeeded\nWARNING: x\n");
        assert!(view.take_pending_notifies().is_empty());

        view.module_registry.sync_effective_disabled(&[]);
        view.dispatch_output_to_modules(b"Build succeeded in 1s\n");
        let actions = view.take_pending_notifies();
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            ModuleAction::Notify {
                severity,
                dedupe_key,
                attention,
                ..
            } => {
                assert_eq!(*severity, Severity::Success);
                assert_eq!(dedupe_key, "TR-OK-001");
                assert!(*attention);
            }
            other => panic!("expected success notify, got {other:?}"),
        }
    }

    #[test]
    fn output_trigger_notify_queue_keeps_newest_eight() {
        let mut view = view_with_builtin_modules();
        let mut buf = Vec::new();
        for _ in 0..10 {
            buf.extend_from_slice(b"ECONNREFUSED\n");
        }
        view.dispatch_output_to_modules(&buf);
        assert_eq!(view.take_pending_notifies().len(), 8);
    }
}

#[cfg(test)]
mod display_link_host_tests {
    use super::*;
    use crate::capability::{MenuCtx, ModuleRegistry};
    use std::sync::Arc;
    use crate::modules::display_link::{
        ACTION_BROWSER_OPEN, ACTION_COPY_OR_CONNECT, ACTION_NEW_CONNECTION, ACTION_RUNBOOK_RECALL,
        ACTION_SFTP_OPEN, MODULE_ID,
    };

    fn official_registry() -> ModuleRegistry {
        ModuleRegistry::with_modules(crate::modules::builtin_modules(std::sync::Arc::new(
            std::sync::Mutex::new(crate::core::CmdAuditEngine::new()),
        )))
    }

    #[test]
    fn pty_dispatch_does_not_enqueue_display_link_decorations() {
        let mut view = TerminalView::new();
        view.set_module_registry(Arc::new(official_registry()));
        view.dispatch_output_to_modules(b"ECONNREFUSED 10.1.2.3\n");
        let _notifies = view.take_pending_notifies();
        assert!(
            view.take_pending_module_actions().is_empty(),
            "PTY 行不应把装饰留在动作队列"
        );
    }

    #[test]
    fn viewport_scan_respects_enabled_flag() {
        let reg = official_registry();
        let formatted = "failed to open /var/log/nginx/error.log\nECONNREFUSED 10.0.0.8\n";
        let hits = TerminalView::scan_display_links(&reg, formatted);
        assert!(hits.iter().any(|(_, s)| s.action == ACTION_SFTP_OPEN));
        assert!(hits
            .iter()
            .any(|(_, s)| s.text == "ECONNREFUSED" && s.action == ACTION_RUNBOOK_RECALL));
        assert!(hits
            .iter()
            .any(|(_, s)| s.text == "10.0.0.8" && s.action == ACTION_COPY_OR_CONNECT));

        let items = reg.context_menu_items(&MenuCtx {
            link_action: Some(ACTION_COPY_OR_CONNECT),
            link_text: Some("10.0.0.8"),
        });
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].action, ACTION_NEW_CONNECTION);
        assert!(reg
            .context_menu_items(&MenuCtx {
                link_action: None,
                link_text: None,
            })
            .is_empty());

        reg.set_enabled(MODULE_ID, false);
        assert!(TerminalView::scan_display_links(&reg, formatted).is_empty());
        assert!(reg
            .context_menu_items(&MenuCtx {
                link_action: Some(ACTION_COPY_OR_CONNECT),
                link_text: Some("10.0.0.8"),
            })
            .is_empty());
    }

    #[test]
    fn classify_click_keeps_http_guard() {
        assert!(matches!(
            classify_display_link_click(ACTION_COPY_OR_CONNECT, "1.2.3.4"),
            Some(DisplayLinkClick::Copy)
        ));
        assert!(matches!(
            classify_display_link_click(ACTION_BROWSER_OPEN, "https://a.example"),
            Some(DisplayLinkClick::Browser)
        ));
        assert!(classify_display_link_click(ACTION_BROWSER_OPEN, "ftp://a.example").is_none());
        assert!(matches!(
            classify_display_link_click(ACTION_RUNBOOK_RECALL, "EACCES"),
            Some(DisplayLinkClick::Runbook)
        ));
        assert!(matches!(
            classify_display_link_click(ACTION_SFTP_OPEN, "/var/log"),
            Some(DisplayLinkClick::Sftp)
        ));
        assert!(classify_display_link_click("nope", "x").is_none());
    }
}
