//! 云端同步 UI：账号主机列表、可选凭证信封、导出/导入包。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;
use crate::ui::file_dialog::FileDialog;

use crate::core::host_sync::{self, SyncKeyMaterial, SyncOutcome};
use crate::core::{
    AppSettings, AuditCategory, AuditEvent, AuditLogger, AuditOutcome, CloudSyncSettings,
    CredentialVault, FragmentManager, FragmentMergeReport, SessionManager, SortBy, TeamService,
};
use crate::core::team::{TeamFragmentCache, TeamState};
use crate::ui::team_ui::{paint_team_controls, TeamLoginForm, TeamUiAction};
use crate::i18n::{self, UiLanguage};
use crate::ui::credential_panel::CredentialPanel;
use crate::ui::chrome;
use crate::ui::layout_util;
use crate::ui::theme::{Theme, ThemeManager};

/// 导出/导入所需的本地路径与可变引用(由主窗口注入)
pub struct CloudSyncDeps<'a> {
    pub fragments_path: &'a PathBuf,
    pub sessions_path: &'a PathBuf,
    pub theme_path: &'a PathBuf,
    pub fragment_manager: &'a mut FragmentManager,
    pub theme_manager: &'a mut ThemeManager,
    pub session_manager: &'a mut SessionManager,
    pub credential_panel: &'a mut CredentialPanel,
    pub audit: Option<&'a AuditLogger>,
}

/// 同步面板(右侧栏)
pub struct CloudSyncPanel {
    pub open: bool,
    pub settings: CloudSyncSettings,
    pub message: String,
    /// 待宿主 `notify_error` 的失败文案。
    pending_toast_error: Option<String>,
    /// 从同步包导入片段时是否与现有条目按 id 合并(否则整库替换)
    pub merge_fragments_on_package_import: bool,
    pending_import_dir: Option<PathBuf>,
    /// 本帧 `SidePanel` 槽位(Central 之后 Foreground 重绘用)。
    last_panel_slot_rect: Option<egui::Rect>,
    passphrase_input: String,
    recovery_input: String,
    /// 刚生成、需要用户抄走的恢复码。本机另有 device_key 副本，云端没有。
    recovery_reveal: String,
    sync_rx: Option<Receiver<SyncOutcome>>,
    login_sync_armed: bool,
    last_auto_attempt: Option<Instant>,
}

impl CloudSyncPanel {
    pub fn new() -> Self {
        Self {
            open: false,
            settings: CloudSyncSettings::load(),
            message: String::new(),
            pending_toast_error: None,
            merge_fragments_on_package_import: true,
            pending_import_dir: None,
            last_panel_slot_rect: None,
            passphrase_input: String::new(),
            recovery_input: String::new(),
            recovery_reveal: String::new(),
            sync_rx: None,
            login_sync_armed: false,
            last_auto_attempt: None,
        }
    }

    pub fn arm_login_sync(&mut self) {
        self.login_sync_armed = true;
    }

    pub fn sync_in_flight(&self) -> bool {
        self.sync_rx.is_some()
    }

    pub fn should_start_auto(&self, logged_in: bool) -> bool {
        if !logged_in || self.sync_rx.is_some() {
            return false;
        }
        if !self.login_sync_armed && !self.auto_due() {
            return false;
        }
        self.settings.sync_host_metadata || self.settings.sync_credentials
    }

    pub fn mark_auto_started(&mut self) {
        self.login_sync_armed = false;
        self.last_auto_attempt = Some(Instant::now());
    }

    fn auto_due(&self) -> bool {
        let mins = self.settings.frequency_minutes;
        if mins == 0 {
            return false;
        }
        let wait = Duration::from_secs(u64::from(mins) * 60);
        if let Some(t) = self.last_auto_attempt {
            return t.elapsed() >= wait;
        }
        match self.settings.last_sync_unix {
            Some(ts) => chrono::Utc::now().timestamp().saturating_sub(ts) >= i64::from(mins) * 60,
            None => true,
        }
    }

    pub fn take_sync_receiver(&mut self) -> Option<Receiver<SyncOutcome>> {
        self.sync_rx.take()
    }

    pub fn restore_sync_receiver(&mut self, rx: Receiver<SyncOutcome>) {
        self.sync_rx = Some(rx);
    }

    pub fn set_sync_message(&mut self, msg: impl Into<String>) {
        self.message = msg.into();
    }

    pub fn note_sync_result(&mut self, message: String, is_error: bool) {
        if is_error {
            self.settings.mark_sync_err(message.clone());
            self.pending_toast_error = Some(message.clone());
        } else {
            self.settings.mark_sync_ok();
        }
        self.message = message;
    }

    pub fn spawn_account_sync(
        &mut self,
        sessions: &[crate::core::SessionConfig],
        vault: &[crate::core::Credential],
        api_base: &str,
        access_token: &str,
        user_id: &str,
        zh: bool,
    ) {
        if self.sync_rx.is_some() {
            return;
        }
        if access_token.is_empty() {
            self.message = if zh {
                "未登录。本机 SSH 不受影响；登录团队账号后才能云同步。".into()
            } else {
                "Not signed in. Local SSH is unchanged; sign in to the team account to sync.".into()
            };
            return;
        }
        let stored = SyncKeyMaterial::load().for_user(user_id);
        let passphrase = if self.passphrase_input.trim().is_empty() {
            stored.passphrase
        } else {
            self.passphrase_input.trim().to_string()
        };
        let recovery = if self.recovery_input.trim().is_empty() {
            stored.recovery_code
        } else {
            self.recovery_input.trim().to_string()
        };
        let req = host_sync::SyncRequest {
            api_base: api_base.to_string(),
            access_token: access_token.to_string(),
            user_id: user_id.to_string(),
            zh,
            sync_hosts: self.settings.sync_host_metadata,
            sync_credentials: self.settings.sync_credentials,
            passphrase,
            recovery_code: recovery,
            sessions: sessions.to_vec(),
            vault: vault.to_vec(),
            state: host_sync::HostSyncState::load_for_user(user_id),
            now: chrono::Utc::now().timestamp(),
        };
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(host_sync::run_account_sync(req));
        });
        self.sync_rx = Some(rx);
        self.message = if zh {
            "正在与云端同步…".into()
        } else {
            "Syncing with the cloud…".into()
        };
    }

    fn save_sync_passphrase(&mut self, user_id: &str, zh: bool) {
        let pass = self.passphrase_input.trim().to_string();
        if pass.is_empty() {
            self.push_toast_error(if zh {
                "请先输入同步口令。口令不会上传，云端只保存密文。".into()
            } else {
                "Enter a sync passphrase first. It is not uploaded; the cloud stores ciphertext only.".into()
            });
            return;
        }
        let mut key = SyncKeyMaterial::load().for_user(user_id);
        let rotated = !key.recovery_code.is_empty() && key.passphrase != pass;
        key.user_id = user_id.to_string();
        key.passphrase = pass;
        if key.recovery_code.is_empty() {
            key.recovery_code = host_sync::generate_recovery_code();
        }
        self.recovery_reveal = key.recovery_code.clone();
        key.save();
        self.message = if zh {
            if rotated {
                "同步口令已更新。恢复码未变；请保管下面的恢复码。云端解封不依赖本机密钥。".into()
            } else {
                "已保存同步口令，并生成恢复码。请立即抄下恢复码。丢失口令和恢复码后，云端凭证无法解开，本机数据不会被删。".into()
            }
        } else if rotated {
            "Sync passphrase updated. The recovery code is unchanged. Cloud unwrap does not use this machine's local key.".into()
        } else {
            "Sync passphrase saved and a recovery code generated. Copy the recovery code now. If both are lost, cloud credentials cannot be unwrapped. Local data is not deleted.".into()
        };
    }

    fn reveal_recovery_code(&mut self, user_id: &str, zh: bool) {
        let key = SyncKeyMaterial::load().for_user(user_id);
        if key.recovery_code.is_empty() {
            self.message = if zh {
                "还没有恢复码。先设置同步口令。".into()
            } else {
                "No recovery code yet. Set a sync passphrase first.".into()
            };
            return;
        }
        self.recovery_reveal = key.recovery_code;
        self.message = if zh {
            "恢复码只存在本机加密存储和你抄下的副本里，服务器没有。".into()
        } else {
            "The recovery code exists only in this machine's encrypted local store and in the copy you saved. The server does not have it.".into()
        };
    }

    fn current_passphrase(&self, user_id: &str) -> (String, String) {
        let stored = SyncKeyMaterial::load().for_user(user_id);
        let passphrase = if self.passphrase_input.trim().is_empty() {
            stored.passphrase
        } else {
            self.passphrase_input.trim().to_string()
        };
        let recovery = if self.recovery_input.trim().is_empty() {
            stored.recovery_code
        } else {
            self.recovery_input.trim().to_string()
        };
        (passphrase, recovery)
    }

    pub fn take_pending_toast_error(&mut self) -> Option<String> {
        self.pending_toast_error.take()
    }

    fn push_toast_error(&mut self, msg: String) {
        self.message = msg.clone();
        self.pending_toast_error = Some(msg);
    }

    fn save_settings(&mut self, lang: UiLanguage) {
        let loc = i18n::Locale::from(lang);
        match self.settings.save() {
            Ok(()) => self.message = loc.tr("Settings saved", "已保存设置").to_string(),
            Err(e) => {
                self.push_toast_error(format!(
                    "{}{}",
                    loc.tr("Save failed: ", "保存失败："),
                    e
                ));
            }
        }
    }

    fn paint_account_sync(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &Theme,
        lang: UiLanguage,
        deps: &mut CloudSyncDeps<'_>,
        team_service: &mut Option<&mut TeamService>,
    ) {
        let loc = i18n::Locale::from(lang);
        let zh = matches!(lang, UiLanguage::Zh);
        let user_id = team_service
            .as_ref()
            .and_then(|svc| svc.state.user.as_ref())
            .map(|u| u.id.clone())
            .unwrap_or_default();
        let logged_in = team_service
            .as_ref()
            .map(|svc| svc.is_logged_in())
            .unwrap_or(false);

        chrome::form_field_label(
            ui,
            theme,
            loc.tr("Account sync", "账号同步"),
        );
        chrome::settings_elevated_card(ui, theme, |ui| {
            chrome::form_checkbox(
                ui,
                theme,
                &mut self.settings.sync_host_metadata,
                loc.tr("Sync host list after sign-in", "登录后同步主机列表"),
            );
            ui.label(
                chrome::rich_caption(
                    theme,
                    loc.tr(
                        "Name, host, port, user, group, auth hint, and key path only. Passwords and private key files stay on this machine.",
                        "只同步名称、主机、端口、用户、分组、认证提示和密钥路径。密码和私钥文件留在本机。",
                    ),
                )
                .color(theme.color_form_hint()),
            );
            ui.add_space(theme.spacing_xs());
            chrome::form_checkbox(
                ui,
                theme,
                &mut self.settings.sync_credentials,
                loc.tr("Sync credentials (off by default)", "同步凭证（默认关闭）"),
            );
            ui.label(
                chrome::rich_caption(
                    theme,
                    loc.tr(
                        "Off by default. When on, session passwords and vault secrets are sealed with your sync passphrase before upload. The server stores ciphertext only. You keep the passphrase and recovery code; if both are lost, cloud credentials cannot be opened and local data is not deleted. A wrong passphrase is reported and does not overwrite the cloud copy.",
                        "默认关闭。开启后，会话密码和凭证库秘密先用同步口令加密再上传。服务器只存密文。口令和恢复码由你保管；两者都丢失时，云端凭证无法打开，本机数据不会被删。口令错误会提示，且不会覆盖云端副本。",
                    ),
                )
                .color(theme.color_form_hint()),
            );
            egui::CollapsingHeader::new(loc.tr("Advanced", "高级"))
                .id_salt("cloud_sync_credentials_advanced")
                .show(ui, |ui| {
                    ui.label(
                        chrome::rich_caption(
                            theme,
                            loc.tr(
                                "Technical: sealed with Argon2id + AES-256-GCM; server stores ciphertext and a kdf hint. This machine's device key cannot unwrap another machine's envelope.",
                                "技术细节：使用 Argon2id + AES-256-GCM 封存；服务器只存密文和密钥派生提示。本机密钥不能解开另一台机器的信封。",
                            ),
                        )
                        .color(theme.color_form_hint()),
                    );
                });
            ui.add_space(theme.spacing_xs());
            chrome::form_field_label(ui, theme, loc.tr("Sync passphrase", "同步口令"));
            chrome::form_singleline_field(
                ui,
                theme,
                egui::Id::new("mistterm_sync_passphrase"),
                &mut self.passphrase_input,
                loc.tr("Not uploaded", "不会上传"),
                ui.available_width(),
                true,
            );
            ui.add_space(theme.spacing_xs());
            chrome::form_field_label(ui, theme, loc.tr("Recovery code", "恢复码"));
            chrome::form_singleline_field(
                ui,
                theme,
                egui::Id::new("mistterm_sync_recovery"),
                &mut self.recovery_input,
                loc.tr("Optional, to unwrap", "可选，用于解封"),
                ui.available_width(),
                false,
            );
            if !logged_in {
                ui.label(
                    chrome::rich_caption(
                        theme,
                        loc.tr(
                            "Sign in above to sync. Local SSH keeps working.",
                            "先在上方登录再同步。未登录时本机 SSH 保持原样。",
                        ),
                    )
                    .color(theme.color_form_hint()),
                );
            }
            ui.add_space(theme.spacing_sm());
            let gap = theme.spacing_sm();
            let half = ((ui.available_width() - gap) / 2.0).max(theme.size_control_btn_min_w());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                if chrome::panel_action_button_with_icon_ex_min_w(
                    ui,
                    theme,
                    crate::ui::icons::IconId::Check,
                    loc.tr("Save passphrase", "保存口令"),
                    true,
                    half,
                )
                .clicked()
                {
                    self.save_sync_passphrase(&user_id, zh);
                    let _ = self.settings.save();
                }
                if chrome::panel_action_primary_button_with_icon_ex_min_w(
                    ui,
                    theme,
                    crate::ui::icons::IconId::Cloud,
                    loc.tr("Sync now", "立即同步"),
                    true,
                    half,
                )
                .clicked()
                {
                    let _ = self.settings.save();
                    if let Some(service) = team_service.as_mut() {
                        if !service.is_logged_in() {
                            self.set_sync_message(if zh {
                                "未登录。本机 SSH 不受影响；登录团队账号后才能云同步。"
                            } else {
                                "Not signed in. Local SSH is unchanged; sign in to the team account to sync."
                            });
                        } else {
                            match service.access_token_for_api() {
                                Ok(token) => {
                                    let uid = service
                                        .state
                                        .user
                                        .as_ref()
                                        .map(|u| u.id.clone())
                                        .unwrap_or_default();
                                    let api = service.api_base();
                                    let sessions = deps.session_manager.list_sessions().to_vec();
                                    let vault = deps.credential_panel.vault().list();
                                    self.spawn_account_sync(
                                        &sessions, &vault, &api, &token, &uid, zh,
                                    );
                                }
                                Err(e) => self.push_toast_error(e),
                            }
                        }
                    }
                }
            });
            ui.add_space(theme.spacing_xs());
            if chrome::panel_action_button_with_icon_ex_min_w(
                ui,
                theme,
                crate::ui::icons::IconId::Key,
                loc.tr("Show recovery code", "显示恢复码"),
                true,
                ui.available_width(),
            )
            .clicked()
            {
                self.reveal_recovery_code(&user_id, zh);
            }
            if !self.recovery_reveal.is_empty() {
                ui.add_space(theme.spacing_xs());
                ui.label(
                    chrome::rich_caption(theme, &self.recovery_reveal)
                        .strong()
                        .color(theme.accent_color()),
                );
                ui.label(
                    chrome::rich_caption(
                        theme,
                        loc.tr(
                            "Copy this recovery code and store it outside this machine.",
                            "请抄下恢复码，并保管在这台机器之外。",
                        ),
                    )
                    .color(theme.red_color()),
                );
            }
            if self.sync_in_flight() {
                ui.label(chrome::rich_caption(
                    theme,
                    loc.tr("Sync in progress…", "同步进行中…"),
                ));
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
        });
    }

    fn paint_capability_banner(ui: &mut egui::Ui, theme: &Theme, lang: UiLanguage) {
        let loc = i18n::Locale::from(lang);
        chrome::form_field_label(ui, theme, loc.tr("Local pack", "本地同步包"));
        chrome::settings_elevated_card(ui, theme, |ui| {
            ui.label(
                chrome::rich_caption(theme, loc.tr("Ready", "当前可用"))
                    .strong()
                    .color(theme.green_color()),
            );
            ui.label(chrome::rich_caption(
                theme,
                loc.tr(
                    "Signed-in host list sync, optional end-to-end credentials, and an encrypted export pack.",
                    "登录后同步主机列表，凭证可选手动端到端同步，另有加密导出包。",
                ),
            ));
            chrome::settings_row_divider(ui, theme);
            ui.label(
                chrome::rich_caption(
                    theme,
                    loc.tr(
                        "Local SSH does not require an account. Team sign-in tokens are never exported.",
                        "未登录时本机 SSH 与原来一样。同步包不含团队登录令牌。",
                    ),
                )
                .color(theme.color_form_hint()),
            );
        });
    }

    fn paint_sync_status(ui: &mut egui::Ui, theme: &Theme, settings: &CloudSyncSettings, lang: UiLanguage) {
        let loc = i18n::Locale::from(lang);
        if let Some(ts) = settings.last_sync_unix {
            let t = chrono::DateTime::from_timestamp(ts, 0)
                .map(|x| x.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "—".to_string());
            ui.label(chrome::rich_caption(
                theme,
                &format!(
                    "{}{}",
                    loc.tr("Last action: ", "最近操作："),
                    t
                ),
            ));
        } else {
            ui.label(chrome::rich_caption(
                theme,
                loc.tr("Last action: none yet", "最近操作：尚未记录"),
            ));
        }
        if !settings.last_error.is_empty() {
            ui.label(
                chrome::rich_caption(theme, &settings.last_error).color(theme.red_color()),
            );
        }
    }

    fn package_requires_import_confirm(dir: &Path, settings: &CloudSyncSettings) -> bool {
        let sessions = dir.join("sessions.json");
        let hosts = dir.join(host_sync::EXPORT_HOSTS_FILE);
        let credentials = dir.join("credentials.json");
        let envelope = dir.join(host_sync::EXPORT_CREDS_FILE);
        (settings.sync_sessions && (sessions.exists() || hosts.exists()))
            || (settings.sync_credentials && (credentials.exists() || envelope.exists()))
    }

    fn run_export(&mut self, deps: &CloudSyncDeps<'_>, lang: UiLanguage, user_id: &str) {
        let loc = i18n::Locale::from(lang);
        let Some(parent) = FileDialog::new()
            .set_title(loc.tr(
                "Choose export folder",
                "选择导出目录",
            ))
            .pick_folder()
        else {
            return;
        };

        let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
        let dest = parent.join(format!("mistterm-sync-{}", stamp));
        if let Err(e) = fs::create_dir_all(&dest) {
            let msg = format!(
                "{}{}",
                loc.tr("Create folder failed: ", "创建目录失败："),
                e
            );
            self.settings.mark_sync_err(msg.clone());
            self.push_toast_error(msg);
            return;
        }

        let mut err: Option<String> = None;
        let mut wrote: Vec<String> = Vec::new();

        if self.settings.sync_fragments && deps.fragments_path.exists() {
            match fs::copy(deps.fragments_path, dest.join("fragments.json")) {
                Ok(_) => wrote.push("fragments.json".into()),
                Err(e) => {
                    err = Some(match lang {
                        UiLanguage::En => format!("fragments.json: {e}"),
                        UiLanguage::Zh => format!("fragments.json：{e}"),
                    });
                }
            }
        }

        if err.is_none() && self.settings.sync_sessions && deps.sessions_path.exists() {
            match fs::copy(deps.sessions_path, dest.join("sessions.json")) {
                Ok(_) => wrote.push("sessions.json".into()),
                Err(e) => {
                    err = Some(match lang {
                        UiLanguage::En => format!("sessions.json: {e}"),
                        UiLanguage::Zh => format!("sessions.json：{e}"),
                    });
                }
            }
        }

        if err.is_none() && self.settings.sync_themes && deps.theme_path.exists() {
            match fs::copy(deps.theme_path, dest.join("theme.json")) {
                Ok(_) => wrote.push("theme.json".into()),
                Err(e) => {
                    err = Some(match lang {
                        UiLanguage::En => format!("theme.json: {e}"),
                        UiLanguage::Zh => format!("theme.json：{e}"),
                    });
                }
            }
        }

        if err.is_none() && self.settings.sync_shortcuts {
            let settings_path = AppSettings::default_path();
            if settings_path.exists() {
                match fs::copy(&settings_path, dest.join("settings_snapshot.json")) {
                    Ok(_) => wrote.push("settings_snapshot.json".into()),
                    Err(e) => {
                        err = Some(format!("settings_snapshot.json：{}", e));
                    }
                }
            }
        }

        if err.is_none() && self.settings.sync_team_config {
            let team_state = TeamState::config_path();
            if team_state.exists() {
                match fs::copy(&team_state, dest.join("team_state.json")) {
                    Ok(_) => wrote.push("team_state.json".into()),
                    Err(e) => err = Some(format!("team_state.json：{}", e)),
                }
            }
            let team_frag = TeamFragmentCache::cache_path();
            if err.is_none() && team_frag.exists() {
                match fs::copy(&team_frag, dest.join("team_fragments_cache.json")) {
                    Ok(_) => wrote.push("team_fragments_cache.json".into()),
                    Err(e) => err = Some(format!("team_fragments_cache.json：{}", e)),
                }
            }
        }

        if err.is_none() && (self.settings.sync_sessions || self.settings.sync_credentials) {
            let (passphrase, recovery) = self.current_passphrase(user_id);
            let zh = matches!(lang, UiLanguage::Zh);
            match host_sync::write_export_files(
                &dest,
                deps.session_manager.list_sessions(),
                &deps.credential_panel.vault().list(),
                self.settings.sync_credentials,
                &passphrase,
                &recovery,
                zh,
            ) {
                Ok(files) => wrote.extend(files),
                Err(e) => err = Some(e),
            }
        }

        if let Ok(s) = serde_json::to_string_pretty(&self.settings) {
            if fs::write(dest.join("cloud_sync_snapshot.json"), s).is_ok() {
                wrote.push("cloud_sync_snapshot.json".into());
            }
        }

        if let Some(e) = err {
            self.settings.mark_sync_err(e.clone());
            self.push_toast_error(e);
        } else {
            self.settings.mark_sync_ok();
            if let Some(audit) = deps.audit {
                audit.record(
                    AuditEvent::new(
                        AuditCategory::Config,
                        "cloud_sync.export",
                        AuditOutcome::Success,
                    )
                    .with_detail(serde_json::json!({
                        "dest": dest.display().to_string(),
                        "files": wrote,
                    })),
                );
            }
            let preview = if wrote.is_empty() {
                loc.tr(
                    "(Nothing selected to export or sources missing)",
                    "(未勾选可导出项或源文件缺失)",
                )
                .to_string()
            } else {
                wrote.join(match lang {
                    UiLanguage::En => ", ",
                    UiLanguage::Zh => "、",
                })
            };
            self.message = match lang {
                UiLanguage::En => format!(
                    "Exported to {}\nIncluded: {}",
                    dest.display(),
                    preview
                ),
                UiLanguage::Zh => format!(
                    "已导出到 {}\n包含：{}",
                    dest.display(),
                    preview
                ),
            };
        }
    }

    fn pick_import_folder(
        &mut self,
        deps: &mut CloudSyncDeps<'_>,
        lang: UiLanguage,
        team_service: &mut Option<&mut TeamService>,
    ) {
        let loc = i18n::Locale::from(lang);
        let Some(dir) = FileDialog::new()
            .set_title(loc.tr(
                "Choose sync pack folder (fragments.json, sessions.json, …)",
                "选择同步包目录(含 fragments.json、sessions.json 等)",
            ))
            .pick_folder()
        else {
            return;
        };
        if Self::package_requires_import_confirm(&dir, &self.settings) {
            self.pending_import_dir = Some(dir);
            self.message = loc
                .tr(
                    "Sync pack chosen. If sessions or credentials will be overwritten, confirm below.",
                    "已选择同步包。若勾选覆盖会话或凭证库，请先确认后继续。",
                )
                .to_string();
        } else {
            self.message = self.perform_import_package(&dir, deps, lang, team_service);
        }
    }

    fn perform_import_package(
        &mut self,
        dir: &Path,
        deps: &mut CloudSyncDeps<'_>,
        lang: UiLanguage,
        team_service: &mut Option<&mut TeamService>,
    ) -> String {
        let merge_fragments = self.merge_fragments_on_package_import;
        let user_id = team_service
            .as_ref()
            .and_then(|svc| svc.state.user.as_ref())
            .map(|u| u.id.clone())
            .unwrap_or_default();
        let (passphrase, recovery) = self.current_passphrase(&user_id);
        let zh = matches!(lang, UiLanguage::Zh);
        let settings = &mut self.settings;
        let loc = i18n::Locale::from(lang);
        let mut parts = Vec::<String>::new();

        let frag_src = dir.join("fragments.json");
        if settings.sync_fragments && frag_src.exists() {
            match FragmentManager::import_from_json_path(
                &frag_src,
                merge_fragments,
                deps.fragment_manager,
            ) {
                Ok(FragmentMergeReport {
                    added,
                    skipped_duplicate_id,
                }) => {
                    if merge_fragments {
                        deps.fragment_manager.sort(SortBy::UsageCount);
                    }
                    if deps.fragment_manager.save(deps.fragments_path).is_ok() {
                        if merge_fragments {
                            parts.push(match lang {
                                UiLanguage::En => format!(
                                    "Fragments: added {added}, skipped duplicate id {skipped_duplicate_id}"
                                ),
                                UiLanguage::Zh => format!(
                                    "命令片段：新增 {added}，跳过已有 id {skipped_duplicate_id}"
                                ),
                            });
                        } else {
                            parts.push(match lang {
                                UiLanguage::En => format!("Fragments: replaced with {added} entries"),
                                UiLanguage::Zh => format!("命令片段：已替换为 {added} 条"),
                            });
                        }
                    } else {
                        return match lang {
                            UiLanguage::En => format!("Write {} failed", deps.fragments_path.display()),
                            UiLanguage::Zh => format!("写入 {} 失败", deps.fragments_path.display()),
                        };
                    }
                }
                Err(e) => {
                    return format!(
                        "{}{}",
                        loc.tr("Failed to read fragments: ", "读取 fragments 失败："),
                        e
                    );
                }
            }
        }

        let hosts_src = dir.join(host_sync::EXPORT_HOSTS_FILE);
        let mut imported_portable_hosts = false;
        let mut imported_portable_creds = false;
        if settings.sync_sessions && hosts_src.exists() {
            match host_sync::import_portable_pack(
                dir,
                deps.session_manager.list_sessions(),
                &deps.credential_panel.vault().list(),
                &passphrase,
                &recovery,
                settings.sync_credentials,
                zh,
            ) {
                Ok(imported) => {
                    let note = host_sync::format_notes(&imported.notes, zh);
                    deps.session_manager.replace_sessions(imported.sessions);
                    if let Some(entries) = imported.vault {
                        if let Err(e) = deps.credential_panel.vault_mut().replace_all(entries) {
                            return format!(
                                "{}{e}",
                                loc.tr("Import credentials failed: ", "导入 credentials 失败：")
                            );
                        }
                        imported_portable_creds = true;
                        parts.push(
                            loc.tr(
                                "Credentials: unwrapped from the pack passphrase",
                                "凭证：已用同步口令从导出包解封",
                            )
                            .to_string(),
                        );
                    }
                    imported_portable_hosts = true;
                    let mut line = loc
                        .tr(
                            "Hosts: merged from the pack (passwords unchanged unless the pack included credentials)",
                            "主机：已从同步包合并（未含凭证时保留本机密码）",
                        )
                        .to_string();
                    if !note.is_empty() {
                        line.push('\n');
                        line.push_str(&note);
                    }
                    parts.push(line);
                }
                Err(e) => return e,
            }
        }

        let sessions_src = dir.join("sessions.json");
        if settings.sync_sessions && !imported_portable_hosts && sessions_src.exists() {
            match deps
                .session_manager
                .import_sessions_from_file_path(&sessions_src)
            {
                Ok(()) => {
                    parts.push(
                        loc.tr("Sessions: restored from pack", "会话：已从包还原")
                            .to_string(),
                    );
                }
                Err(e) => {
                    return format!(
                        "{}{}",
                        loc.tr("Import sessions failed: ", "导入 sessions 失败："),
                        e
                    );
                }
            }
        }

        let theme_src = dir.join("theme.json");
        if settings.sync_themes && theme_src.exists() {
            if let Ok(txt) = fs::read_to_string(&theme_src) {
                if let Ok(tm) = serde_json::from_str::<ThemeManager>(&txt) {
                    *deps.theme_manager = tm;
                    deps.theme_manager.save();
                    parts.push(
                        loc.tr("Theme: restored from pack", "主题：已从包还原")
                            .to_string(),
                    );
                }
            }
        }

        let settings_src = dir.join("settings_snapshot.json");
        if settings.sync_shortcuts && settings_src.exists() {
            match fs::copy(&settings_src, AppSettings::default_path()) {
                Ok(_) => {
                    parts.push(
                        loc.tr(
                            "Settings: restored from pack (restart may be needed for some options)",
                            "偏好设置：已从包还原(部分选项可能需重启生效)",
                        )
                        .to_string(),
                    );
                }
                Err(e) => {
                    return format!(
                        "{}{}",
                        loc.tr("Import settings failed: ", "导入 settings 失败："),
                        e
                    );
                }
            }
        }

        let team_state_src = dir.join("team_state.json");
        if settings.sync_team_config && team_state_src.exists() {
            match fs::copy(&team_state_src, TeamState::config_path()) {
                Ok(_) => {
                    if let Some(svc) = team_service.as_mut() {
                        svc.state = TeamState::load();
                    }
                    parts.push(
                        loc.tr("Team state: restored (tokens not in pack)", "团队状态：已还原(包内不含令牌)")
                            .to_string(),
                    );
                }
                Err(e) => {
                    return format!(
                        "{}{}",
                        loc.tr("Import team_state failed: ", "导入 team_state 失败："),
                        e
                    );
                }
            }
        }

        let team_frag_src = dir.join("team_fragments_cache.json");
        if settings.sync_team_config && team_frag_src.exists() {
            match fs::copy(&team_frag_src, TeamFragmentCache::cache_path()) {
                Ok(_) => parts.push(
                    loc.tr("Team fragments cache: restored", "团队片段缓存：已还原")
                        .to_string(),
                ),
                Err(e) => {
                    return format!(
                        "{}{}",
                        loc.tr("Import team_fragments_cache failed: ", "导入团队片段缓存失败："),
                        e
                    );
                }
            }
        }

        let credentials_src = dir.join("credentials.json");
        if settings.sync_credentials && !imported_portable_creds && credentials_src.exists() {
            match CredentialVault::restore_from_file_into_default_location(&credentials_src) {
                Ok(()) => {
                    deps.credential_panel.reload_after_external_file_replace();
                    parts.push(
                        loc.tr(
                            "Credentials: overwritten; sidebar cache refreshed",
                            "凭证库：已从包覆盖并刷新侧栏缓存",
                        )
                        .to_string(),
                    );
                }
                Err(e) => {
                    return format!(
                        "{}{}",
                        loc.tr("Import credentials failed: ", "导入 credentials 失败："),
                        e
                    );
                }
            }
        }

        if parts.is_empty() {
            let _ = settings.save();
            return loc
                .tr(
                    "Nothing importable here (enable items and ensure files exist)",
                    "所选目录无可导入项(请勾选同步项且包内需含对应文件)",
                )
                .to_string();
        }

        settings.record_manual_import_ok();
        if let Some(audit) = deps.audit {
            audit.record(
                AuditEvent::new(
                    AuditCategory::Config,
                    "cloud_sync.import",
                    AuditOutcome::Success,
                )
                .with_detail(serde_json::json!({
                    "dir": dir.display().to_string(),
                    "summary": parts,
                })),
            );
        }
        parts.join(" · ")
    }

    /// 仅占右栏布局槽(正文在 Central 之后 [`show_foreground_panel`] 绘制)。
    #[inline]
    pub(crate) fn last_panel_slot_rect(&self) -> Option<egui::Rect> {
        self.last_panel_slot_rect
    }

    pub fn show_side_panel(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        right_dock_outer_left: &mut Option<f32>,
        dock_col_w: f32,
    ) {
        if !self.open {
            self.last_panel_slot_rect = None;
            return;
        }

        let (def_w, min_w, max_w) =
            crate::ui::layout_util::right_dock_resize_bounds(dock_col_w);
        ctx.root_ui(|root| {
            egui::Panel::right("cloud_sync_panel")
                .default_size(def_w)
                .min_size(min_w)
                .max_size(max_w)
                .resizable(true)
                .frame(crate::ui::chrome::right_dock_placeholder_frame(theme))
                .show(root, |ui| {
                    let h = ui.available_height().max(1.0);
                    let w = ui.available_width().max(1.0);
                    ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
                });
        });
        let dock_inset = theme.spacing_right_dock_screen_inset();
        // #206 S1：Panel::show 不再返回 response；槽位改由 PanelState 落盘矩形推得。
        let slot = layout_util::side_panel_state_rect(ctx, "cloud_sync_panel")
            .map(|r| {
                let screen = ctx.content_rect();
                let mut s = r;
                s.min.x = screen.max.x - s.width();
                s.max.x = screen.max.x - dock_inset;
                s
            })
            .unwrap_or(egui::Rect::NOTHING);
        crate::ui::chrome::paint_right_dock_slot_gap(ctx, theme, slot);
        self.last_panel_slot_rect = Some(slot);
        if let Some(slot) = self.last_panel_slot_rect {
            layout_util::record_right_dock_panel_rect(&slot, right_dock_outer_left);
        }
        let _ = theme;
    }

    /// Central 之后绘制云端同步正文(避免被 CentralPanel 盖住)。
    pub fn show_foreground_panel(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        deps: &mut CloudSyncDeps<'_>,
        close_panel: &mut bool,
        mut team_service: Option<&mut TeamService>,
        mut team_form: Option<&mut TeamLoginForm>,
        mut app_settings: Option<&mut AppSettings>,
    ) -> TeamUiAction {
        if !self.open {
            return TeamUiAction::None;
        }

        let mut team_action = TeamUiAction::None;

        let screen = ctx.content_rect();
        let dock_inset = theme.spacing_right_dock_screen_inset();
        let Some(slot) = layout_util::right_dock_foreground_slot(
            self.last_panel_slot_rect,
            ctx,
            "cloud_sync_panel",
            layout_util::SidePanelProfile::Standard,
            None,
            dock_inset,
        ) else {
            return team_action;
        };
        let geom = chrome::prepare_right_dock_foreground_geom(slot, screen, theme);
        let layer_id = chrome::right_dock_foreground_layer_id("mistterm_cloud_sync_fg");
        chrome::paint_right_dock_foreground_shell(ctx, layer_id, geom.paint, theme);
        chrome::show_right_dock_foreground_body(
            "mistterm_cloud_sync_fg",
            ctx,
            theme,
            &geom,
            layout_util::SidePanelProfile::Standard,
            |ui, _body_w| {
                let lang = i18n::language(ctx);
                let loc = i18n::Locale::from(lang);
                let panel_w = ui.available_width();
                ui.set_max_width(panel_w);
                let mut header_closed = false;
                theme.frame_right_dock_header_band().show(ui, |ui| {
                    header_closed = chrome::dock_panel_title_close_only(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Cloud,
                        i18n::tr(ctx, "Cloud Sync", "云端同步"),
                        i18n::tr(ctx, "Close Cloud Sync", "关闭云端同步"),
                    );
                });
                if header_closed {
                    *close_panel = true;
                }
                chrome::right_dock_header_divider(ui, theme);

                let cloud_scroll_h = layout_util::scroll_area_fill_height(ui, 8.0);
                egui::ScrollArea::vertical()
                    .id_salt("cloud_sync_panel_scroll")
                    .auto_shrink([false, true])
                    .max_height(cloud_scroll_h)
                    .show(ui, |ui| {
                        let content_w = layout_util::set_width_to_available(ui);
                        ui.set_min_width(content_w);
                        ui.spacing_mut().item_spacing.y = 0.0;

                        Self::paint_capability_banner(ui, theme, lang);

                        if let Some(service) = team_service.as_mut() {
                            if let Some(form) = team_form.as_mut() {
                                if let Some(settings) = app_settings.as_mut() {
                                    chrome::form_field_label(
                                        ui,
                                        theme,
                                        i18n::tr(ctx, "Team account", "团队账户"),
                                    );
                                    chrome::settings_elevated_card(ui, theme, |ui| {
                                        let action = paint_team_controls(
                                            ui,
                                            ctx,
                                            theme,
                                            service,
                                            form,
                                            deps.audit,
                                            content_w
                                                - theme.spacing_search_input_x().max(14.0) * 2.0,
                                            "cloud_sync_team",
                                        );
                                        if action != TeamUiAction::None {
                                            team_action = action;
                                        }
                                        if matches!(action, TeamUiAction::LoggedOut) {
                                            let _ = settings.save();
                                        }
                                    });
                                }
                            }
                        }

                        self.paint_account_sync(ui, ctx, theme, lang, deps, &mut team_service);

                        chrome::form_field_label(
                            ui,
                            theme,
                            i18n::tr(ctx, "Pack contents", "包内包含项"),
                        );
                        chrome::settings_elevated_card(ui, theme, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                if chrome::panel_action_icon_button(
                                    ui,
                                    theme,
                                    crate::ui::icons::IconId::Check,
                                    i18n::tr(ctx, "Select all", "全选"),
                                )
                                .clicked()
                                {
                                    self.settings.sync_sessions = true;
                                    self.settings.sync_fragments = true;
                                    self.settings.sync_themes = true;
                                    self.settings.sync_credentials = true;
                                    self.settings.sync_shortcuts = true;
                                    self.settings.sync_team_config = true;
                                }
                                if chrome::panel_action_icon_button(
                                    ui,
                                    theme,
                                    crate::ui::icons::IconId::Server,
                                    i18n::tr(ctx, "Core only", "仅核心"),
                                )
                                .clicked()
                                {
                                    self.settings.sync_sessions = true;
                                    self.settings.sync_fragments = true;
                                    self.settings.sync_themes = true;
                                    self.settings.sync_credentials = false;
                                    self.settings.sync_shortcuts = false;
                                    self.settings.sync_team_config = false;
                                }
                                if chrome::panel_action_icon_button(
                                    ui,
                                    theme,
                                    crate::ui::icons::IconId::Trash,
                                    i18n::tr(ctx, "Clear", "清空"),
                                )
                                .clicked()
                                {
                                    self.settings.sync_sessions = false;
                                    self.settings.sync_fragments = false;
                                    self.settings.sync_themes = false;
                                    self.settings.sync_credentials = false;
                                    self.settings.sync_shortcuts = false;
                                    self.settings.sync_team_config = false;
                                }
                            });
                            ui.add_space(theme.spacing_xs());
                            ui.columns(2, |cols| {
                                chrome::form_checkbox(
                                    &mut cols[0],
                                    theme,
                                    &mut self.settings.sync_sessions,
                                    loc.tr("Sessions", "会话"),
                                );
                                chrome::form_checkbox(
                                    &mut cols[1],
                                    theme,
                                    &mut self.settings.sync_fragments,
                                    loc.tr("Fragments", "命令片段"),
                                );
                                chrome::form_checkbox(
                                    &mut cols[0],
                                    theme,
                                    &mut self.settings.sync_themes,
                                    loc.tr("Themes", "主题"),
                                );
                                chrome::form_checkbox(
                                    &mut cols[1],
                                    theme,
                                    &mut self.settings.sync_credentials,
                                    loc.tr("Credentials", "凭证库"),
                                );
                                chrome::form_checkbox(
                                    &mut cols[0],
                                    theme,
                                    &mut self.settings.sync_shortcuts,
                                    loc.tr(
                                        "Preferences",
                                        "偏好设置",
                                    ),
                                );
                                chrome::form_checkbox(
                                    &mut cols[1],
                                    theme,
                                    &mut self.settings.sync_team_config,
                                    loc.tr("Team config (no tokens)", "团队配置(不含令牌)"),
                                );
                            });
                            chrome::settings_row_divider(ui, theme);
                            ui.label(
                                chrome::rich_caption(
                                    theme,
                                    loc.tr(
                                        "Hosts are exported without passwords or private key files. Credentials, if checked, are sealed with the sync passphrase—not this machine's local key.",
                                        "主机会导出，不含密码和私钥文件。勾选凭证时用同步口令加密，不用本机密钥。",
                                    ),
                                )
                                .color(theme.color_form_hint()),
                            );
                        });

                        chrome::form_field_label(
                            ui,
                            theme,
                            loc.tr("Schedule & import", "间隔与导入"),
                        );
                        chrome::settings_elevated_card(ui, theme, |ui| {
                            chrome::form_control_row(ui, theme, |ui| {
                                chrome::form_inline_label(
                                    ui,
                                    theme,
                                    loc.tr("Every (min)", "每隔(分钟)"),
                                );
                                chrome::form_u32_stepper(
                                    ui,
                                    theme,
                                    egui::Id::new("cloud_sync_freq_min"),
                                    &mut self.settings.frequency_minutes,
                                    0..=24 * 60,
                                    5,
                                );
                                ui.label(chrome::rich_caption(
                                    theme,
                                    loc.tr("0 = manual only", "填 0 表示仅手动同步"),
                                ));
                            });
                            ui.label(
                                chrome::rich_caption(
                                    theme,
                                    loc.tr(
                                        "Interval applies to host-list sync and team fragment sync while signed in. 0 = manual only. Export/import stays manual.",
                                        "登录后，间隔用于主机列表同步和团队片段同步。填 0 表示仅手动。导出/导入始终手动。",
                                    ),
                                )
                                .color(theme.color_form_hint()),
                            );
                            chrome::settings_row_divider(ui, theme);
                            chrome::form_checkbox(
                                ui,
                                theme,
                                &mut self.merge_fragments_on_package_import,
                                loc.tr(
                                    "Merge with existing library (skip entries already present)",
                                    "与现有库合并，跳过已有条目",
                                ),
                            );
                            ui.label(
                                chrome::rich_caption(
                                    theme,
                                    loc.tr(
                                        "When off, the whole library is replaced. Session/credential import may overwrite local files.",
                                        "关闭上项则整库替换。会话/凭证导入可能覆盖本机文件。",
                                    ),
                                )
                                .color(theme.color_form_hint()),
                            );
                        });

                        chrome::form_field_label(
                            ui,
                            theme,
                            loc.tr("Actions", "操作"),
                        );
                        chrome::settings_elevated_card(ui, theme, |ui| {
                            Self::paint_sync_status(ui, theme, &self.settings, lang);
                            chrome::settings_row_divider(ui, theme);
                            // 三枚操作按钮通栏等分同一行。
                            let gap = theme.spacing_sm();
                            let third = ((ui.available_width() - gap * 2.0) / 3.0)
                                .max(theme.size_control_btn_min_w());
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = gap;
                                if chrome::panel_action_button_with_icon_ex_min_w(
                                    ui,
                                    theme,
                                    crate::ui::icons::IconId::Check,
                                    loc.tr("Save selection & interval", "保存勾选与间隔"),
                                    true,
                                    third,
                                )
                                .clicked()
                                {
                                    self.save_settings(lang);
                                }
                                if chrome::panel_action_primary_button_with_icon_ex_min_w(
                                    ui,
                                    theme,
                                    crate::ui::icons::IconId::Upload,
                                    loc.tr("Export sync pack…", "导出同步包…"),
                                    true,
                                    third,
                                )
                                .on_hover_text(loc.tr(
                                    "Create mistterm-sync-timestamp folder and copy selected files",
                                    "新建 mistterm-sync-时间戳 目录并复制勾选文件",
                                ))
                                .clicked()
                                {
                                    let user_id = team_service
                                        .as_ref()
                                        .and_then(|svc| svc.state.user.as_ref())
                                        .map(|u| u.id.clone())
                                        .unwrap_or_default();
                                    self.run_export(deps, lang, &user_id);
                                }
                                if chrome::panel_action_button_with_icon_ex_min_w(
                                    ui,
                                    theme,
                                    crate::ui::icons::IconId::Package,
                                    loc.tr("Import from sync pack…", "从同步包导入…"),
                                    true,
                                    third,
                                )
                                .clicked()
                                {
                                    self.pick_import_folder(deps, lang, &mut team_service);
                                }
                            });
                        });
                    });

                if let Some(dir) = self.pending_import_dir.clone() {
                    ui.add_space(theme.spacing_panel_gap());
                    ui.group(|ui| {
                        crate::ui::icons::icon_label_row(
                            ui,
                            crate::ui::icons::IconId::Warning,
                            loc.tr("Confirm import", "导入确认"),
                            theme.font_size_body(),
                            6.0,
                            |t| t.strong().color(theme.red_color()),
                        );
                        ui.label(
                            egui::RichText::new(format!(
                                "{}{}",
                                loc.tr("Pack path: ", "包路径： "),
                                dir.display()
                            ))
                            .small(),
                        );
                        if self.settings.sync_sessions && dir.join("sessions.json").exists() {
                            ui.label(loc.tr(
                                "• Will replace the current session list with sessions.json from the pack.",
                                "• 将用包内 sessions.json 替换当前会话列表。",
                            ));
                        }
                        if self.settings.sync_credentials && dir.join("credentials.json").exists() {
                            ui.label(loc.tr(
                                "• Will overwrite local encrypted credentials (same device key required).",
                                "• 将用包内凭证库覆盖本机加密文件(需同源设备密钥才解密)。",
                            ));
                        }
                        ui.horizontal(|ui| {
                            if chrome::panel_action_icon_button(
                                ui,
                                theme,
                                crate::ui::icons::IconId::Cross,
                                loc.tr("Cancel", "取消"),
                            )
                            .clicked() {
                                self.pending_import_dir = None;
                                self.message =
                                    loc.tr("Import cancelled", "已取消导入").to_string();
                            }
                            if chrome::panel_action_primary_icon_button(
                                ui,
                                theme,
                                crate::ui::icons::IconId::Check,
                                loc.tr("Confirm import", "确认导入"),
                            )
                                .clicked() {
                                let msg = self.perform_import_package(
                                    &dir,
                                    deps,
                                    lang,
                                    &mut team_service,
                                );
                                self.message = msg;
                                self.pending_import_dir = None;
                            }
                        });
                    });
                }

                if !self.message.is_empty() {
                    ui.add_space(theme.spacing_sm());
                    egui::Frame::NONE
                        .fill(theme.color_subtle_inset_fill())
                        .corner_radius(theme.radius_list_item())
                        .inner_margin(egui::Margin::symmetric(8.0 as i8, 6.0 as i8))
                        .show(ui, |ui| {
                            ui.label(
                                chrome::rich_caption(theme, &self.message)
                                    .color(theme.text_secondary()),
                            );
                        });
                }
            },
        );

        if *close_panel {
            self.pending_import_dir = None;
            self.open = false;
        }

        team_action
    }
}
