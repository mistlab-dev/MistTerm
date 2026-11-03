//! 终端搜索栏状态（#301 P1c，纯搬家）。
//!
//! app.rs 原有 6 个 `terminal_search*`/`show_terminal_search` 散字段；
//! 开关/命中/高亮/步进方法仍留 MistTermApp（摸 tabs 与终端内部状态，属编排层）。

/// 终端内搜索（Ctrl+F）的 UI 状态。
#[derive(Clone, Debug)]
pub struct TerminalSearchState {
    pub open: bool,
    /// 打开后是否把焦点交给输入框（首开一次，切走即清）
    pub pending_focus: bool,
    pub query: String,
    pub ignore_case: bool,
    pub hits: Vec<crate::terminal::SearchHit>,
    pub cur: usize,
}

impl Default for TerminalSearchState {
    fn default() -> Self {
        Self {
            open: false,
            pending_focus: false,
            query: String::new(),
            ignore_case: true,
            hits: Vec::new(),
            cur: 0,
        }
    }
}
