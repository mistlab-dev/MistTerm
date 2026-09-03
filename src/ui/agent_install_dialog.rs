//! Command audit Agent installation guide.
//!
//! The client deliberately does not execute remote installation commands. This dialog
//! explains enrollment, installation, binding, and heartbeat verification, and lets the
//! user copy the documented command for an administrator to run on the audited host.

use eframe::egui;

use crate::ui::{chrome, layout_util};
use crate::ui::theme::Theme;

pub fn show_agent_install_modal(
    ctx: &egui::Context,
    theme: &Theme,
    open: &mut bool,
    command_copied: &mut bool,
) {
    if !*open {
        return;
    }
    let mut dialog_open = *open;
    let mut should_close = false;
    let modal_sz = egui::vec2(680.0, 560.0);
    let title = crate::i18n::tr(ctx, "Install command audit Agent", "安装命令审计 Agent");
    let command = "curl -sL https://mistlab.dev/install | bash -s -- <team_id> <api_base> <enroll_token>";

    chrome::modal_window("agent_install", theme, ctx)
        .open(&mut dialog_open)
        .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
        .movable(true)
        .resizable(true)
        .default_size(modal_sz)
        .show(ctx, |ui| {
            chrome::modal_content_frame(theme).show(ui, |ui| {
                chrome::modal_header_title_only(ui, theme, title, theme.font_size_modal_title());
                ui.label(
                    egui::RichText::new(crate::i18n::tr(
                        ctx,
                        "Install on the audited server as root. MistTerm only provides guidance; it never runs this command remotely.",
                        "请在被审计服务器上以 root 身份安装。MistTerm 只提供向导，不会远程执行此命令。",
                    ))
                    .color(theme.text_tertiary())
                    .size(theme.font_size_caption()),
                );
                ui.add_space(theme.spacing_sm());

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(390.0)
                    .show(ui, |ui| {
                        step(ui, theme, ctx, "1", "Create an enrollment token", "生成一次性安装令牌", "A team administrator creates a one-time enrollment token in the MistTeam console. It is bound to the team and should be pasted only on the intended host.", "团队管理员在 MistTeam 控制台生成一次性安装令牌。令牌绑定团队，只应粘贴到目标主机。",);
                        step(ui, theme, ctx, "2", "Run the installer", "运行安装脚本", "Run the command below on the audited server with root privileges. You may replace <api_base> with the team API base when needed.", "在被审计服务器上以 root 权限运行下面的命令；必要时可将 <api_base> 替换为团队 API 基址。",);
                        ui.add_space(theme.spacing_xs());
                        ui.horizontal(|ui| {
                            ui.add(egui::Label::new(egui::RichText::new(command).monospace()).wrap(true));
                            if ui.button(crate::i18n::tr(ctx, "Copy", "复制")).clicked() {
                                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                    if clipboard.set_text(command).is_ok() {
                                        *command_copied = true;
                                    }
                                }
                            }
                        });
                        if *command_copied {
                            ui.label(egui::RichText::new(crate::i18n::tr(ctx, "Command copied to clipboard.", "命令已复制到剪贴板。")) .color(theme.accent_color()));
                        }
                        step(ui, theme, ctx, "3", "Bind and configure sshd", "绑定并配置 sshd", "The installer enrolls the host, writes /etc/mist-agent/config, installs the transparent wrapper, and adds an sshd ForceCommand drop-in. It validates sshd before reloading it.", "安装脚本会注册主机、写入 /etc/mist-agent/config、安装透明 wrapper，并添加 sshd ForceCommand 配置；重载前会先校验 sshd。",);
                        step(ui, theme, ctx, "4", "Check the heartbeat", "检查心跳", "On the audited host, run `mist-agent status`. In MistTeam, confirm the Agent appears online and that its host, team, and last heartbeat are correct.", "在被审计主机执行 `mist-agent status`；在 MistTeam 中确认 Agent 在线，并核对主机、团队和最近心跳。",);
                        step(ui, theme, ctx, "5", "Removal and recovery", "卸载与恢复", "Use `mist-agent-uninstall` on the host to remove the drop-in and agent files. Existing SSH connections are not interrupted; always verify access before closing the maintenance session.", "在主机执行 `mist-agent-uninstall` 移除配置和文件。现有 SSH 连接不会被中断；关闭维护会话前请先验证访问。",);
                    });
                ui.add_space(theme.spacing_sm());
                ui.horizontal(|ui| {
                    if ui.button(crate::i18n::tr(ctx, "Close", "关闭")).clicked() {
                        should_close = true;
                    }
                });
            });
        });

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        should_close = true;
    }
    if should_close {
        dialog_open = false;
    }
    *open = dialog_open;
}

fn step(
    ui: &mut egui::Ui,
    theme: &Theme,
    ctx: &egui::Context,
    number: &str,
    title_en: &str,
    title_zh: &str,
    body_en: &str,
    body_zh: &str,
) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(number).strong().color(theme.accent_color()));
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(crate::i18n::tr(ctx, title_en, title_zh)).strong());
            ui.label(egui::RichText::new(crate::i18n::tr(ctx, body_en, body_zh)).color(theme.text_secondary()));
        });
    });
    ui.add_space(theme.spacing_sm());
}
