//! 统一应用事件与操作总线（Action Bus）。
//!
//! 用于消灭 Panel 之间错综复杂的 `&mut` 借用和嵌套闭包回调，
//! 将 UI 的「用户意图 (Action)」与底层「状态跃迁 (State Transition)」解耦。

use std::collections::VecDeque;

/// 应用全局动作意图
#[derive(Debug, Clone)]
pub enum AppAction {
    /// 触发会话连接（按 ID 或名称）
    ConnectSession(String),
    /// 切换活动右侧 Dock（如 AI、监控等）
    OpenRightDock(RightDockKind),
    /// 关闭右侧 Dock
    CloseRightDock,
    /// 复制文本至剪贴板
    CopyToClipboard(String),
    /// 弹出全局 Toast 通知
    Notify {
        message: String,
        level: NotificationLevel,
    },
    /// AI 模块：附带上下文到输入框
    AiAttachContext {
        source: Option<String>,
        text: String,
    },
    /// AI 模块：在当前活动终端执行建议命令
    AiExecTerminalCommand(String),
    /// 终端选区注入 AI
    AttachTerminalSelectionToAi,
    /// 终端尾部缓冲区注入 AI
    AttachTerminalTailToAi(usize),
    /// 结构化日志中的最近报错注入 AI
    AttachRecentFailureToAi,
    /// 触发快速连接对话框
    OpenQuickConnectDialog,
    /// 触发设置对话框
    OpenPreferencesDialog,
    /// 触发命令片段管理对话框
    OpenSnippetsDialog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightDockKind {
    Ai,
    Monitor,
    Sftp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationLevel {
    Info,
    Success,
    Warning,
    Error,
}

/// 动作总线队列（每帧排队，帧末集中消费处理）
#[derive(Default)]
pub struct ActionBus {
    queue: VecDeque<AppAction>,
}

impl ActionBus {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }

    /// 发送动作到总线
    pub fn dispatch(&mut self, action: AppAction) {
        self.queue.push_back(action);
    }

    /// 消费下一个动作
    pub fn pop(&mut self) -> Option<AppAction> {
        self.queue.pop_front()
    }

    /// 清空所有动作
    pub fn clear(&mut self) {
        self.queue.clear();
    }

    /// 检查是否有待处理动作
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}
