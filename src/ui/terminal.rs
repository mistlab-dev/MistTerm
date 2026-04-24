//! 终端视图
//!
//! 显示终端模拟器、处理输入输出

use eframe::egui;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// 终端视图组件
pub struct TerminalView {
    /// 会话 ID
    session_id: String,
    
    /// 终端内容（简化版，实际需要完整的终端模拟器）
    terminal_content: String,
    
    /// 是否已连接
    connected: Arc<AtomicBool>,
    
    /// 滚动位置
    scroll_offset: f32,
}

impl TerminalView {
    /// 创建新的终端视图
    pub fn new(session_id: String) -> Self {
        let session_id_clone = session_id.clone();
        Self {
            session_id,
            terminal_content: format!("MistTerm - 会话：{}\n连接中...\n", session_id_clone),
            connected: Arc::new(AtomicBool::new(false)),
            scroll_offset: 0.0,
        }
    }

    /// 显示终端视图
    pub fn show(&mut self, ui: &mut egui::Ui) {
        // 终端区域
        let available_size = ui.available_size();
        
        egui::Frame::none()
            .fill(ui.visuals().panel_fill)
            .inner_margin(egui::Margin::same(8.0))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    // 终端内容区
                    egui::ScrollArea::vertical()
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(&self.terminal_content)
                                    .wrap(false)
                                    .sense(egui::Sense::focusable_noninteractive())
                            );
                        });

                    // 输入区
                    ui.add_space(8.0);
                    
                    ui.horizontal(|ui| {
                        ui.label(">");
                        ui.text_edit_singleline(&mut String::new());
                    });
                });
            });
    }

    /// 设置连接状态
    pub fn set_connected(&mut self, connected: bool) {
        self.connected.store(connected, Ordering::SeqCst);
        if connected {
            self.terminal_content = format!("MistTerm - 会话：{}\n已连接\n\n", self.session_id);
        }
    }

    /// 添加输出内容
    pub fn append_output(&mut self, text: &str) {
        self.terminal_content.push_str(text);
        // 自动滚动到底部
        self.scroll_offset = f32::MAX;
    }

    /// 发送输入
    pub fn send_input(&mut self, input: &str) {
        self.terminal_content.push_str(&format!("\r\n$ {}\r\n", input));
    }
}
