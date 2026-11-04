//! 弹窗开关集合（#301 P1c，纯搬家）。
//!
//! app.rs 原有 9 个散装的 `show_*_dialog: bool`（不含右侧 dock 四面板开关——
//! 它们与 ActiveRightDock/dock 布局方法强耦合，留在 MistTermApp）。
//! 语义不变：true = 对应弹窗当前打开。

/// 按弹窗分组的开关位。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DialogFlags {
    pub new_session: bool,
    pub edit_session: bool,
    pub about: bool,
    pub licenses: bool,
    pub preferences: bool,
    pub ai_settings: bool,
    pub audit_timeline: bool,
    pub agent_install: bool,
    pub ask_knowledge: bool,
    /// #335：空态「打开已有连接」主机选择弹层。
    pub open_existing_connection: bool,
}
