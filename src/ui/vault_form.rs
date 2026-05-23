//! 新建/编辑会话与凭证面板共用的 Vault KV 引用表单

use crate::core::{SecretBackend, VaultSettings};
use crate::ui::chrome;
use crate::ui::theme::Theme;
use eframe::egui;

/// Vault 引用编辑状态（与 [`SecretBackend::VaultKv`] 对应）
#[derive(Debug, Clone)]
pub struct VaultSecretForm {
    pub use_vault: bool,
    pub mount: String,
    pub path: String,
    pub field: String,
}

impl Default for VaultSecretForm {
    fn default() -> Self {
        Self {
            use_vault: false,
            mount: String::new(),
            path: String::new(),
            field: "password".to_string(),
        }
    }
}

impl VaultSecretForm {
    pub fn from_backend(backend: &SecretBackend, default_mount: &str) -> Self {
        match backend {
            SecretBackend::VaultKv {
                mount,
                path,
                field,
                ..
            } => Self {
                use_vault: true,
                mount: mount.clone(),
                path: path.clone(),
                field: field.clone(),
            },
            SecretBackend::LocalEncrypted => Self {
                use_vault: false,
                mount: default_mount.to_string(),
                path: String::new(),
                field: "password".to_string(),
            },
        }
    }

    pub fn to_backend(&self, vault_settings: &VaultSettings) -> SecretBackend {
        if self.use_vault {
            SecretBackend::VaultKv {
                mount: if self.mount.trim().is_empty() {
                    vault_settings.default_mount.clone()
                } else {
                    self.mount.trim().to_string()
                },
                path: self.path.trim().to_string(),
                field: if self.field.trim().is_empty() {
                    "password".to_string()
                } else {
                    self.field.trim().to_string()
                },
                version: None,
            }
        } else {
            SecretBackend::LocalEncrypted
        }
    }

    /// 在会话/凭证表单中绘制 Vault 区块（`id_prefix` 用于 egui 控件 id 去重）
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        field_w: f32,
        vault_settings: &VaultSettings,
        id_prefix: &str,
    ) {
        let ctx_owned = ui.ctx().clone();
        chrome::form_checkbox(
            ui,
            theme,
            &mut self.use_vault,
            crate::i18n::tr(
                &ctx_owned,
                "Store password/key in HashiCorp Vault (KV)",
                "密码/密钥存于 HashiCorp Vault（KV）",
            ),
        );
        if !self.use_vault {
            return;
        }
        if !vault_settings.enabled {
            ui.label(
                egui::RichText::new(crate::i18n::tr(
                    ui.ctx(),
                    "Enable Vault in Preferences and set server URL first.",
                    "请先在偏好设置中启用 Vault 并配置地址",
                ))
                    .size(theme.font_size_caption())
                    .color(theme.red_color()),
            );
        }
        if self.mount.is_empty() {
            self.mount = vault_settings.default_mount.clone();
        }
        chrome::form_field_label(ui, theme, "Vault mount");
        chrome::form_singleline_field(
            ui,
            theme,
            egui::Id::new(format!("{id_prefix}_vault_mount")),
            &mut self.mount,
            "secret",
            field_w,
            false,
        );
        chrome::form_field_label(
            ui,
            theme,
            crate::i18n::tr(ui.ctx(), "path", "路径 path"),
        );
        chrome::form_singleline_field(
            ui,
            theme,
            egui::Id::new(format!("{id_prefix}_vault_path")),
            &mut self.path,
            "ssh/prod/app",
            field_w,
            false,
        );
        chrome::form_field_label(
            ui,
            theme,
            crate::i18n::tr(ui.ctx(), "field", "字段 field"),
        );
        chrome::form_singleline_field(
            ui,
            theme,
            egui::Id::new(format!("{id_prefix}_vault_field")),
            &mut self.field,
            "password",
            field_w,
            false,
        );
    }
}
