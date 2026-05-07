//! 凭证管理侧栏（设计文档 §6.2）：本地加密库 + 表单

use eframe::egui;

use crate::core::{
    Credential, CredentialAuthKind, CredentialCategory, CredentialVault,
};
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
    }

    fn parse_tags(s: &str) -> Vec<String> {
        s.split(&[',', '，', ';', '；'][..])
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect()
    }

    pub fn show_side_panel(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        action_out: &mut Option<CredentialPanelAction>,
    ) -> bool {
        if !self.open {
            return false;
        }

        let mut close_panel = false;
        egui::SidePanel::right("credential_panel")
            .default_width(360.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("🔐 凭证库");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("✕").clicked() {
                            close_panel = true;
                        }
                    });
                });
                ui.small(
                    egui::RichText::new(format!("存储：{}", self.vault.path().display()))
                        .color(theme.fg_low_color()),
                );
                ui.separator();

                ui.horizontal(|ui| {
                    if ui.button("➕ 新建").clicked() {
                        self.clear_form();
                        self.status_msg = "新建凭证".to_string();
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut self.search)
                            .hint_text("搜索…")
                            .desired_width(f32::INFINITY),
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

                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_min_width(120.0);
                        egui::ScrollArea::vertical()
                            .max_height(260.0)
                            .show(ui, |ui| {
                                let categories = [
                                    CredentialCategory::Server,
                                    CredentialCategory::Database,
                                    CredentialCategory::SshKey,
                                    CredentialCategory::Api,
                                    CredentialCategory::Other,
                                ];
                                for cat in categories {
                                    let subs: Vec<&Credential> =
                                        list.iter().filter(|c| c.category == cat).collect();
                                    if subs.is_empty() {
                                        continue;
                                    }
                                    ui.collapsing(
                                        format!("{} {}", cat.emoji(), cat.label_zh()),
                                        |ui| {
                                            for c in subs {
                                                let sel =
                                                    self.selected_id.as_deref() == Some(c.id.as_str());
                                                if ui
                                                    .selectable_label(sel, &c.name)
                                                    .clicked()
                                                {
                                                    self.load_cred(c);
                                                }
                                            }
                                        },
                                    );
                                }
                            });
                    });

                    ui.separator();

                    ui.vertical(|ui| {
                        ui.label("名称");
                        ui.add(egui::TextEdit::singleline(&mut self.form_name).desired_width(f32::INFINITY));
                        ui.horizontal(|ui| {
                            ui.label("类别");
                            egui::ComboBox::from_id_source("cred_cat")
                                .selected_text(self.form_category.label_zh())
                                .show_ui(ui, |ui| {
                                    for v in [
                                        CredentialCategory::Server,
                                        CredentialCategory::Database,
                                        CredentialCategory::SshKey,
                                        CredentialCategory::Api,
                                        CredentialCategory::Other,
                                    ] {
                                        if ui
                                            .selectable_label(self.form_category == v, v.label_zh())
                                            .clicked()
                                        {
                                            self.form_category = v;
                                        }
                                    }
                                });
                        });
                        ui.horizontal(|ui| {
                            ui.label("认证");
                            egui::ComboBox::from_id_source("cred_auth")
                                .selected_text(self.form_auth.label_zh())
                                .show_ui(ui, |ui| {
                                    for v in [
                                        CredentialAuthKind::Password,
                                        CredentialAuthKind::SshKey,
                                        CredentialAuthKind::Token,
                                    ] {
                                        if ui
                                            .selectable_label(self.form_auth == v, v.label_zh())
                                            .clicked()
                                        {
                                            self.form_auth = v;
                                        }
                                    }
                                });
                        });
                        ui.label("主机");
                        ui.add(egui::TextEdit::singleline(&mut self.form_host).desired_width(f32::INFINITY));
                        ui.horizontal(|ui| {
                            ui.label("端口");
                            ui.add(egui::DragValue::new(&mut self.form_port));
                        });
                        ui.label("用户名（可选）");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.form_username).desired_width(f32::INFINITY),
                        );
                        ui.label(format!("密钥 / {}", self.form_auth.label_zh()));
                        ui.add(
                            egui::TextEdit::multiline(&mut self.form_secret)
                                .desired_width(f32::INFINITY)
                                .desired_rows(3)
                                .password(!matches!(self.form_auth, CredentialAuthKind::SshKey)),
                        );
                        ui.label("标签（逗号分隔）");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.form_tags).desired_width(f32::INFINITY),
                        );
                        ui.label("备注");
                        ui.add(
                            egui::TextEdit::multiline(&mut self.form_notes)
                                .desired_width(f32::INFINITY)
                                .desired_rows(2),
                        );

                        ui.horizontal(|ui| {
                            if ui.button("保存").clicked() && !self.form_name.trim().is_empty() {
                                let now = chrono::Utc::now().timestamp();
                                let id = self
                                    .selected_id
                                    .clone()
                                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                                let prior = self.vault.get(&id);
                                let c = Credential {
                                    id: id.clone(),
                                    name: self.form_name.trim().to_string(),
                                    category: self.form_category,
                                    host: self.form_host.trim().to_string(),
                                    port: self.form_port.max(1),
                                    username: self.form_username.trim().to_string(),
                                    auth: self.form_auth,
                                    secret: self.form_secret.clone(),
                                    notes: self.form_notes.clone(),
                                    tags: Self::parse_tags(&self.form_tags),
                                    created_at: prior.as_ref().map(|p| p.created_at).unwrap_or(now),
                                    updated_at: now,
                                };
                                if self.vault.upsert(c).is_ok() {
                                    self.status_msg = "已保存".to_string();
                                    self.selected_id = Some(id);
                                    self.reload_vault();
                                } else {
                                    self.status_msg = "保存失败".to_string();
                                }
                            }
                            if ui.button("删除").clicked() {
                                if let Some(id) = self.selected_id.clone() {
                                    if self.vault.remove(&id).unwrap_or(false) {
                                        self.clear_form();
                                        self.status_msg = "已删除".to_string();
                                    }
                                }
                            }
                            if ui.button("用于连接…").clicked() {
                                if let Some(id) = &self.selected_id {
                                    if let Some(c) = self.vault.get(id) {
                                        *action_out = Some(CredentialPanelAction::UseForQuickConnect(c));
                                    }
                                }
                            }
                        });
                        if !self.status_msg.is_empty() {
                            ui.small(
                                egui::RichText::new(&self.status_msg).color(theme.fg_low_color()),
                            );
                        }
                    });
                });
            });

        close_panel
    }
}
