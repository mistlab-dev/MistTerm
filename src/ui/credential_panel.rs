//! 凭证管理侧栏（设计文档 §6.2）：本地加密库 + 表单

use eframe::egui;

use crate::core::{
    AuditCategory, AuditEvent, AuditLogger, AuditOutcome, Credential, CredentialAuthKind,
    CredentialCategory, CredentialVault, HashiCorpVaultClient, SecretBackend, VaultKvRef,
    VaultSettings,
};
use crate::ui::chrome;
use crate::ui::layout_util::{self, SidePanelProfile};
use crate::ui::theme::Theme;

#[derive(Clone, Debug)]
pub enum CredentialPanelAction {
    /// 使用该凭证填充「新建会话」或状态提示（由主窗口处理）
    UseForQuickConnect(Credential),
}

pub struct CredentialPanel {
    pub open: bool,
    vault: CredentialVault,
    selected_id: Option<String>,
    /// 表单
    form_name: String,
    form_host: String,
    form_port: u16,
    form_username: String,
    form_secret: String,
    form_notes: String,
    form_tags: String,
    form_category: CredentialCategory,
    form_auth: CredentialAuthKind,
    form_use_vault: bool,
    form_vault_mount: String,
    form_vault_path: String,
    form_vault_field: String,
    vault_list_prefix: String,
    vault_list_entries: Vec<crate::core::VaultListEntry>,
    vault_list_busy: bool,
    search: String,
    status_msg: String,
}

impl CredentialPanel {
    pub fn new() -> Self {
        Self {
            open: false,
            vault: CredentialVault::new(),
            selected_id: None,
            form_name: String::new(),
            form_host: String::new(),
            form_port: 22,
            form_username: String::new(),
            form_secret: String::new(),
            form_notes: String::new(),
            form_tags: String::new(),
            form_category: CredentialCategory::Server,
            form_auth: CredentialAuthKind::Password,
            form_use_vault: false,
            form_vault_mount: String::new(),
            form_vault_path: String::new(),
            form_vault_field: "password".to_string(),
            vault_list_prefix: String::new(),
            vault_list_entries: Vec::new(),
            vault_list_busy: false,
            search: String::new(),
            status_msg: String::new(),
        }
    }

    pub fn vault(&self) -> &CredentialVault {
        &self.vault
    }

    pub fn reload_vault(&mut self) {
        self.vault = CredentialVault::new();
    }

    /// 外部（如同步包还原）更新了 `credentials.json` 后调用，刷新表单与缓存
    pub fn reload_after_external_file_replace(&mut self) {
        self.reload_vault();
        self.selected_id = None;
        self.clear_form();
        self.status_msg.clear();
    }

    fn clear_form(&mut self) {
        self.selected_id = None;
        self.form_name.clear();
        self.form_host.clear();
        self.form_port = 22;
        self.form_username.clear();
        self.form_secret.clear();
        self.form_notes.clear();
        self.form_tags.clear();
        self.form_category = CredentialCategory::Server;
        self.form_auth = CredentialAuthKind::Password;
        self.form_use_vault = false;
        self.form_vault_mount.clear();
        self.form_vault_path.clear();
        self.form_vault_field = "password".to_string();
    }

    fn load_cred(&mut self, c: &Credential) {
        self.selected_id = Some(c.id.clone());
        self.form_name = c.name.clone();
        self.form_host = c.host.clone();
        self.form_port = c.port;
        self.form_username = c.username.clone();
        self.form_secret = c.secret.clone();
        self.form_notes = c.notes.clone();
        self.form_tags = c.tags.join(", ");
        self.form_category = c.category;
        self.form_auth = c.auth;
        match &c.secret_backend {
            SecretBackend::LocalEncrypted => {
                self.form_use_vault = false;
            }
            SecretBackend::VaultKv {
                mount,
                path,
                field,
                ..
            } => {
                self.form_use_vault = true;
                self.form_vault_mount = mount.clone();
                self.form_vault_path = path.clone();
                self.form_vault_field = field.clone();
            }
        }
    }

    fn build_secret_backend(&self, vault_settings: &VaultSettings) -> SecretBackend {
        if self.form_use_vault {
            SecretBackend::VaultKv {
                mount: if self.form_vault_mount.trim().is_empty() {
                    vault_settings.default_mount.clone()
                } else {
                    self.form_vault_mount.trim().to_string()
                },
                path: self.form_vault_path.trim().to_string(),
                field: if self.form_vault_field.trim().is_empty() {
                    "password".to_string()
                } else {
                    self.form_vault_field.trim().to_string()
                },
                version: None,
            }
        } else {
            SecretBackend::LocalEncrypted
        }
    }

    fn refresh_vault_list(&mut self, vault_settings: &VaultSettings) {
        self.vault_list_busy = true;
        self.vault_list_entries.clear();
        if !vault_settings.enabled {
            self.status_msg = "HashiCorp Vault 未启用".to_string();
            self.vault_list_busy = false;
            return;
        }
        let mount = if self.form_vault_mount.trim().is_empty() {
            vault_settings.default_mount.clone()
        } else {
            self.form_vault_mount.trim().to_string()
        };
        match HashiCorpVaultClient::new(vault_settings.clone()) {
            Ok(client) => match client.list_kv(&mount, &self.vault_list_prefix) {
                Ok(entries) => {
                    self.vault_list_entries = entries;
                    self.status_msg =
                        format!("Vault 密钥列表：{} 项", self.vault_list_entries.len());
                }
                Err(e) => self.status_msg = format!("拉取 Vault 列表失败：{e}"),
            },
            Err(e) => self.status_msg = format!("Vault 客户端错误：{e}"),
        }
        self.vault_list_busy = false;
    }

    fn parse_tags(s: &str) -> Vec<String> {
        s.split(&[',', '，', ';', '；'][..])
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect()
    }

    fn show_credential_list(ui: &mut egui::Ui, theme: &Theme, list: &[Credential], selected_id: &Option<String>, load: &mut impl FnMut(&Credential)) {
        let list_h = layout_util::clamp_f32(ui.available_height() * 0.28, 72.0, 200.0);
        let prev_extreme = ui.visuals().extreme_bg_color;
        ui.visuals_mut().extreme_bg_color = theme.color_scroll_extreme_bg();
        egui::ScrollArea::vertical()
            .id_source("credential_panel_list")
            .max_height(list_h)
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                ui.set_width(ui.max_rect().width());
                let categories = [
                    CredentialCategory::Server,
                    CredentialCategory::Database,
                    CredentialCategory::SshKey,
                    CredentialCategory::Api,
                    CredentialCategory::Other,
                ];
                let mut any = false;
                for cat in categories {
                    let subs: Vec<&Credential> = list.iter().filter(|c| c.category == cat).collect();
                    if subs.is_empty() {
                        continue;
                    }
                    any = true;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let px = theme.font_size_section_title();
                        let (r, _) =
                            ui.allocate_exact_size(egui::vec2(px, px), egui::Sense::hover());
                        crate::ui::icons::paint_icon(
                            ui,
                            r,
                            crate::ui::icons::credential_category_icon(cat),
                            theme.text_primary(),
                            px,
                        );
                        ui.vertical(|ui| {
                            ui.collapsing(cat.label_zh(), |ui| {
                                for c in subs {
                                    let sel = selected_id.as_deref() == Some(c.id.as_str());
                                    if ui.selectable_label(sel, &c.name).clicked() {
                                        load(c);
                                    }
                                }
                            });
                        });
                    });
                }
                if !any {
                    ui.label(
                        egui::RichText::new("暂无凭证，点「新建」添加")
                            .color(theme.text_tertiary()),
                    );
                }
            });
        ui.visuals_mut().extreme_bg_color = prev_extreme;
    }

    fn show_credential_form(
        ui: &mut egui::Ui,
        theme: &Theme,
        field_w: f32,
        vault_settings: &VaultSettings,
        audit: &AuditLogger,
        panel: &mut CredentialPanel,
        action_out: &mut Option<CredentialPanelAction>,
    ) {
        chrome::form_field_label(ui, theme, "名称");
        chrome::form_singleline_field(
            ui,
            theme,
            egui::Id::new("cred_form_name"),
            &mut panel.form_name,
            "",
            field_w,
            false,
        );
        ui.horizontal(|ui| {
            chrome::form_field_label(ui, theme, "类别");
            egui::ComboBox::from_id_source("cred_cat")
                .selected_text(panel.form_category.label_zh())
                .show_ui(ui, |ui| {
                    crate::ui::chrome::apply_menu_popup_style(ui, theme);
                    for v in [
                        CredentialCategory::Server,
                        CredentialCategory::Database,
                        CredentialCategory::SshKey,
                        CredentialCategory::Api,
                        CredentialCategory::Other,
                    ] {
                        if ui
                            .selectable_label(panel.form_category == v, v.label_zh())
                            .clicked()
                        {
                            panel.form_category = v;
                        }
                    }
                });
        });
        ui.horizontal(|ui| {
            chrome::form_field_label(ui, theme, "认证");
            egui::ComboBox::from_id_source("cred_auth")
                .selected_text(panel.form_auth.label_zh())
                .show_ui(ui, |ui| {
                    crate::ui::chrome::apply_menu_popup_style(ui, theme);
                    for v in [
                        CredentialAuthKind::Password,
                        CredentialAuthKind::SshKey,
                        CredentialAuthKind::Token,
                    ] {
                        if ui
                            .selectable_label(panel.form_auth == v, v.label_zh())
                            .clicked()
                        {
                            panel.form_auth = v;
                        }
                    }
                });
        });
        chrome::form_field_label(ui, theme, "主机");
        chrome::form_singleline_field(
            ui,
            theme,
            egui::Id::new("cred_form_host"),
            &mut panel.form_host,
            "example.com",
            field_w,
            false,
        );
        ui.horizontal(|ui| {
            chrome::form_field_label(ui, theme, "端口");
            ui.add(egui::DragValue::new(&mut panel.form_port));
        });
        chrome::form_field_label(ui, theme, "用户名（可选）");
        chrome::form_singleline_field(
            ui,
            theme,
            egui::Id::new("cred_form_username"),
            &mut panel.form_username,
            "root",
            field_w,
            false,
        );
        ui.checkbox(&mut panel.form_use_vault, "机密存于 HashiCorp Vault（KV）");
        if panel.form_use_vault {
            if panel.form_vault_mount.is_empty() {
                panel.form_vault_mount = vault_settings.default_mount.clone();
            }
            chrome::form_field_label(ui, theme, "Vault mount");
            chrome::form_singleline_field(
                ui,
                theme,
                egui::Id::new("cred_vault_mount"),
                &mut panel.form_vault_mount,
                "secret",
                field_w,
                false,
            );
            chrome::form_field_label(ui, theme, "路径 path");
            chrome::form_singleline_field(
                ui,
                theme,
                egui::Id::new("cred_vault_path"),
                &mut panel.form_vault_path,
                "ssh/prod",
                field_w,
                false,
            );
            chrome::form_field_label(ui, theme, "字段 field");
            chrome::form_singleline_field(
                ui,
                theme,
                egui::Id::new("cred_vault_field"),
                &mut panel.form_vault_field,
                "password",
                field_w,
                false,
            );
            ui.horizontal(|ui| {
                if ui.button("测试读取").clicked() && vault_settings.enabled {
                    match HashiCorpVaultClient::new(vault_settings.clone()) {
                        Ok(client) => {
                            let reference = VaultKvRef {
                                mount: panel.form_vault_mount.clone(),
                                path: panel.form_vault_path.clone(),
                                field: panel.form_vault_field.clone(),
                                version: None,
                            };
                            match client.read_kv(&reference) {
                                Ok(_) => {
                                    audit.record(
                                        AuditEvent::new(
                                            AuditCategory::Vault,
                                            "vault.secret.read",
                                            AuditOutcome::Success,
                                        )
                                        .with_detail(serde_json::json!({
                                            "mount": reference.mount,
                                            "path": reference.path,
                                        })),
                                    );
                                    panel.status_msg = "已从 Vault 读取机密".to_string();
                                }
                                Err(e) => {
                                    audit.record(
                                        AuditEvent::new(
                                            AuditCategory::Vault,
                                            "vault.secret.read",
                                            AuditOutcome::Failure,
                                        )
                                        .with_detail(serde_json::json!({
                                            "mount": reference.mount,
                                            "path": reference.path,
                                            "error": e.to_string(),
                                        })),
                                    );
                                    panel.status_msg = format!("Vault 读取失败：{e}");
                                }
                            }
                        }
                        Err(e) => panel.status_msg = format!("Vault 客户端错误：{e}"),
                    }
                }
                if ui.button("写入 Vault").clicked()
                    && vault_settings.enabled
                    && !panel.form_secret.trim().is_empty()
                {
                    match HashiCorpVaultClient::new(vault_settings.clone()) {
                        Ok(client) => {
                            let reference = VaultKvRef {
                                mount: panel.form_vault_mount.clone(),
                                path: panel.form_vault_path.clone(),
                                field: panel.form_vault_field.clone(),
                                version: None,
                            };
                            match client.write_kv(&reference, panel.form_secret.trim()) {
                                Ok(()) => {
                                    audit.record(
                                        AuditEvent::new(
                                            AuditCategory::Vault,
                                            "vault.secret.write",
                                            AuditOutcome::Success,
                                        )
                                        .with_detail(serde_json::json!({
                                            "mount": reference.mount,
                                            "path": reference.path,
                                        })),
                                    );
                                    panel.status_msg = "已写入 Vault（本地条目仅存引用）".to_string();
                                    panel.form_secret.clear();
                                }
                                Err(e) => {
                                    audit.record(
                                        AuditEvent::new(
                                            AuditCategory::Vault,
                                            "vault.secret.write",
                                            AuditOutcome::Failure,
                                        )
                                        .with_detail(serde_json::json!({
                                            "mount": reference.mount,
                                            "path": reference.path,
                                            "error": e.to_string(),
                                        })),
                                    );
                                    panel.status_msg = format!("Vault 写入失败：{e}");
                                }
                            }
                        }
                        Err(e) => panel.status_msg = format!("Vault 客户端错误：{e}"),
                    }
                }
            });
        } else {
            chrome::form_field_label(
                ui,
                theme,
                &format!("密钥 / {}", panel.form_auth.label_zh()),
            );
            chrome::form_multiline_field(
                ui,
                theme,
                egui::Id::new("cred_form_secret"),
                &mut panel.form_secret,
                field_w,
                3,
                !matches!(panel.form_auth, CredentialAuthKind::SshKey),
            );
        }
        chrome::form_field_label(ui, theme, "标签（逗号分隔）");
        chrome::form_singleline_field(
            ui,
            theme,
            egui::Id::new("cred_form_tags"),
            &mut panel.form_tags,
            "prod, web",
            field_w,
            false,
        );
        chrome::form_field_label(ui, theme, "备注");
        chrome::form_multiline_field(
            ui,
            theme,
            egui::Id::new("cred_form_notes"),
            &mut panel.form_notes,
            field_w,
            2,
            false,
        );

        ui.horizontal(|ui| {
            if ui.button("保存").clicked() && !panel.form_name.trim().is_empty() {
                let now = chrono::Utc::now().timestamp();
                let id = panel
                    .selected_id
                    .clone()
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                let prior = panel.vault.get(&id);
                let secret_backend = panel.build_secret_backend(vault_settings);
                let secret = if secret_backend.is_vault() {
                    String::new()
                } else {
                    panel.form_secret.clone()
                };
                let c = Credential {
                    id: id.clone(),
                    name: panel.form_name.trim().to_string(),
                    category: panel.form_category,
                    host: panel.form_host.trim().to_string(),
                    port: panel.form_port.max(1),
                    username: panel.form_username.trim().to_string(),
                    auth: panel.form_auth,
                    secret,
                    notes: panel.form_notes.clone(),
                    tags: Self::parse_tags(&panel.form_tags),
                    created_at: prior.as_ref().map(|p| p.created_at).unwrap_or(now),
                    updated_at: now,
                    secret_backend,
                };
                if panel.vault.upsert(c.clone()).is_ok() {
                    let action = if panel.selected_id.is_some() {
                        "credential.update"
                    } else {
                        "credential.create"
                    };
                    audit.record(
                        AuditEvent::new(AuditCategory::Credential, action, AuditOutcome::Success)
                            .with_resource(&id)
                            .with_host(&c.host),
                    );
                    panel.status_msg = "已保存".to_string();
                    panel.selected_id = Some(id);
                    panel.reload_vault();
                } else {
                    panel.status_msg = "保存失败".to_string();
                }
            }
            if ui.button("删除").clicked() {
                if let Some(id) = panel.selected_id.clone() {
                    if panel.vault.remove(&id).unwrap_or(false) {
                        audit.record(
                            AuditEvent::new(
                                AuditCategory::Credential,
                                "credential.delete",
                                AuditOutcome::Success,
                            )
                            .with_resource(&id),
                        );
                        panel.clear_form();
                        panel.status_msg = "已删除".to_string();
                    }
                }
            }
            if ui.button("用于连接…").clicked() {
                if let Some(id) = &panel.selected_id {
                    if let Some(c) = panel.vault.get(id) {
                        *action_out = Some(CredentialPanelAction::UseForQuickConnect(c));
                    }
                }
            }
        });
        if !panel.status_msg.is_empty() {
            ui.small(egui::RichText::new(&panel.status_msg).color(theme.text_tertiary()));
        }
    }

    pub fn show_side_panel(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        vault_settings: &VaultSettings,
        audit: &AuditLogger,
        action_out: &mut Option<CredentialPanelAction>,
        right_dock_outer_left: &mut Option<f32>,
    ) -> bool {
        if !self.open {
            return false;
        }

        let mut close_panel = false;
        let (c_def, c_min, c_max) = layout_util::side_panel_widths(ctx, SidePanelProfile::Standard);
        let panel = egui::SidePanel::right("credential_panel")
            .default_width(c_def)
            .min_width(c_min)
            .max_width(c_max)
            .resizable(true)
            .frame(crate::ui::chrome::right_dock_panel_frame(theme))
            .show(ctx, |ui| {
                let panel_w = layout_util::dock_panel_content_width(ui, c_min, c_max);
                ui.set_max_width(panel_w);

                if chrome::dock_panel_title_close_only(
                    ui,
                    theme,
                    Some(crate::ui::icons::IconId::Key),
                    "凭证库",
                    chrome::DockPanelTitleStyle::DockHeading,
                    "关闭凭证库",
                ) {
                    close_panel = true;
                }
                ui.small(
                    egui::RichText::new(format!("存储：{}", self.vault.path().display()))
                        .color(theme.text_tertiary()),
                );
                ui.separator();

                ui.horizontal(|ui| {
                    if chrome::panel_toolbar_icon_button(ui, theme, Some(crate::ui::icons::IconId::Plus), "新建")
                        .clicked()
                    {
                        self.clear_form();
                        self.status_msg = "新建凭证".to_string();
                    }
                    let search_w = (panel_w - 88.0).max(120.0);
                    chrome::form_singleline_field(
                        ui,
                        theme,
                        egui::Id::new("credential_panel_search"),
                        &mut self.search,
                        "搜索凭证…",
                        search_w,
                        false,
                    );
                });
                ui.separator();

                let mut list: Vec<Credential> = self.vault.list();
                if !self.search.trim().is_empty() {
                    let q = self.search.to_lowercase();
                    list.retain(|c| {
                        c.name.to_lowercase().contains(&q)
                            || c.host.to_lowercase().contains(&q)
                            || c.username.to_lowercase().contains(&q)
                            || c.tags.iter().any(|t| t.to_lowercase().contains(&q))
                    });
                }
                list.sort_by(|a, b| a.name.cmp(&b.name));

                ui.label(crate::ui::chrome::rich_section_title(
                    theme,
                    "凭证列表",
                    theme.color_section_title(),
                ));
                let selected_id = self.selected_id.clone();
                Self::show_credential_list(ui, theme, &list, &selected_id, &mut |c| {
                    let prev = self.selected_id.as_deref();
                    if prev != Some(c.id.as_str()) {
                        audit.record(
                            AuditEvent::new(
                                AuditCategory::Credential,
                                "credential.view",
                                AuditOutcome::Success,
                            )
                            .with_resource(&c.id)
                            .with_host(&c.host),
                        );
                    }
                    self.load_cred(c);
                });

                if vault_settings.enabled {
                    ui.add_space(theme.spacing_panel_gap());
                    ui.label(crate::ui::chrome::rich_section_title(
                        theme,
                        "浏览 Vault",
                        theme.color_section_title(),
                    ));
                    ui.horizontal(|ui| {
                        ui.label("前缀");
                        ui.text_edit_singleline(&mut self.vault_list_prefix);
                        if ui.button("刷新列表").clicked() {
                            self.refresh_vault_list(vault_settings);
                        }
                    });
                    if self.vault_list_busy {
                        chrome::busy_row(ui, theme, "正在拉取 Vault 列表…");
                    } else if !self.vault_list_entries.is_empty() {
                        let browse_h =
                            layout_util::clamp_f32(ui.available_height() * 0.2, 48.0, 140.0);
                        egui::ScrollArea::vertical()
                            .id_source("credential_vault_browse")
                            .max_height(browse_h)
                            .show(ui, |ui| {
                                for e in &self.vault_list_entries {
                                    let label = if e.is_dir {
                                        format!("{}/", e.path)
                                    } else {
                                        e.path.clone()
                                    };
                                    if ui.selectable_label(false, label).clicked() {
                                        self.form_use_vault = true;
                                        if e.is_dir {
                                            self.vault_list_prefix = e.path.clone();
                                        } else {
                                            self.form_vault_path = e.path.clone();
                                        }
                                    }
                                }
                            });
                    }
                }

                ui.add_space(theme.spacing_panel_gap());
                ui.separator();
                ui.label(crate::ui::chrome::rich_section_title(
                    theme,
                    "编辑",
                    theme.color_section_title(),
                ));

                let field_w = (panel_w - 8.0).max(160.0);
                let form_scroll_h = layout_util::scroll_area_fill_height(ui, 120.0);
                let prev_extreme = ui.visuals().extreme_bg_color;
                ui.visuals_mut().extreme_bg_color = theme.color_scroll_extreme_bg();
                egui::ScrollArea::vertical()
                    .id_source("credential_panel_form")
                    .max_height(form_scroll_h)
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        ui.set_max_width(panel_w);
                        Self::show_credential_form(
                            ui,
                            theme,
                            field_w,
                            vault_settings,
                            audit,
                            self,
                            action_out,
                        );
                    });
                ui.visuals_mut().extreme_bg_color = prev_extreme;
            });
        layout_util::record_right_dock_panel(&panel.response, right_dock_outer_left);

        close_panel
    }
}
