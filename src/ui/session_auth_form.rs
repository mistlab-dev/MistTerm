//! 新建 / 编辑会话共用的认证类型行（含 FIDO2 / OpenSSH sk）。

use crate::core::session::SessionAuthMode;
use crate::ssh::fido2_connect_hint;
use crate::ui::theme::Theme;

use super::MistTermApp;

pub(super) fn show_auth_fields(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    theme: &Theme,
    id_prefix: &str,
    form_w: f32,
    auth_mode: &mut SessionAuthMode,
    use_ssh_agent: &mut bool,
    private_key_path: &mut String,
) {
    MistTermApp::ui_field_label(
        ui,
        theme,
        crate::i18n::tr(ctx, "Authentication", "认证方式"),
    );
    let selected = auth_mode_label(ctx, *auth_mode);
    crate::ui::chrome::form_combo(
        ui,
        theme,
        format!("{id_prefix}_auth_mode"),
        selected,
        form_w,
        |ui| {
            for mode in [SessionAuthMode::Standard, SessionAuthMode::Fido2SecurityKey] {
                let label = auth_mode_label(ctx, mode);
                if ui.selectable_value(auth_mode, mode, label).clicked()
                    && mode.is_fido2()
                {
                    *use_ssh_agent = true;
                }
            }
        },
    );

    MistTermApp::ui_field_label(
        ui,
        theme,
        if auth_mode.is_fido2() {
            crate::i18n::tr(ctx, "Security key path", "安全密钥路径")
        } else {
            crate::i18n::tr(ctx, "SSH private key path", "SSH 私钥路径")
        },
    );
    let key_hint = if auth_mode.is_fido2() {
        crate::i18n::tr(
            ctx,
            "~/.ssh/id_ed25519_sk (empty = any sk key in the agent)",
            "~/.ssh/id_ed25519_sk (留空则用代理里的 sk 密钥)",
        )
    } else {
        crate::i18n::tr(
            ctx,
            "~/.ssh/id_rsa (empty = password or default keys)",
            "~/.ssh/id_rsa(留空则用密码或系统默认密钥)",
        )
    };
    MistTermApp::ui_form_singleline(
        ui,
        theme,
        &format!("{id_prefix}_private_key_path"),
        private_key_path,
        key_hint,
        form_w,
        false,
    );

    let caption = if auth_mode.is_fido2() {
        fido2_form_caption(ctx)
    } else {
        crate::i18n::tr(
            ctx,
            "Auth: password optional if you use a key or SSH agent; empty key path uses password or default keys (~/.ssh/id_*).",
            "认证：使用密钥或 SSH Agent 时可留空密码；私钥路径留空则用密码或系统默认密钥（~/.ssh/id_*）。",
        )
        .to_string()
    };
    ui.label(
        egui::RichText::new(caption)
            .size(theme.font_size_caption())
            .color(theme.color_form_hint()),
    );

    if auth_mode.is_fido2() {
        *use_ssh_agent = true;
        ui.label(
            egui::RichText::new(crate::i18n::tr(
                ctx,
                "Password and non-sk keys are not used. Windows, macOS, and Linux each sign through that system's OpenSSH agent.",
                "不会使用密码或非 sk 密钥。Windows、macOS、Linux 都通过该系统的 OpenSSH agent 签名。",
            ))
            .size(theme.font_size_caption())
            .color(theme.color_form_hint()),
        );
    } else {
        crate::ui::chrome::form_checkbox(ui, theme, use_ssh_agent, agent_checkbox_label(ctx));
    }
}

fn auth_mode_label(ctx: &egui::Context, mode: SessionAuthMode) -> &'static str {
    match mode {
        SessionAuthMode::Standard => crate::i18n::tr(
            ctx,
            "Password, key file, or SSH agent",
            "密码、密钥文件或 SSH Agent",
        ),
        SessionAuthMode::Fido2SecurityKey => crate::i18n::tr(
            ctx,
            "FIDO2 / security key (OpenSSH)",
            "FIDO2 / 安全密钥 (OpenSSH)",
        ),
    }
}

fn fido2_form_caption(ctx: &egui::Context) -> String {
    let hint = fido2_connect_hint(crate::i18n::language(ctx));
    let steps = crate::i18n::tr(
        ctx,
        "Load the key first with ssh-add (ecdsa-sk or ed25519-sk). The handbook has the shortest steps for Windows, macOS, and Linux.",
        "请先用 ssh-add 加载密钥 (ecdsa-sk 或 ed25519-sk)。手册里有 Windows、macOS、Linux 的最短步骤。",
    );
    format!("{hint} {steps}")
}

fn agent_checkbox_label(ctx: &egui::Context) -> &'static str {
    #[cfg(target_os = "windows")]
    {
        crate::i18n::tr(
            ctx,
            "Use SSH agent (Pageant / ssh-agent)",
            "使用 SSH Agent（Pageant / ssh-agent）",
        )
    }
    #[cfg(not(target_os = "windows"))]
    {
        crate::i18n::tr(
            ctx,
            "Use SSH agent (ssh-agent)",
            "使用 SSH Agent（ssh-agent）",
        )
    }
}
