//! 帮助弹窗：结构化排版（快速入门 / 快捷键 / 功能指南），外链文档在浏览器打开。

use crate::platform::{shortcuts, DOCS_INDEX_URL};
use crate::ui::chrome;
use crate::ui::layout_util;
use crate::ui::theme::Theme;
use eframe::egui::{self, FontId, RichText, Ui};

const WEBSITE_URL: &str = "https://mistlab.dev";
const GITHUB_URL: &str = "https://github.com/mistlab-dev/MistTerm";
const WEBSITE_TIP_EN: &str = "Visit https://mistlab.dev for the team platform and fragment marketplace.";
const WEBSITE_TIP_ZH: &str = "访问 https://mistlab.dev 使用团队平台与片段市场。";
const LINKS_TIP_EN: &str = "Docs: https://github.com/mistlab-dev/MistTerm/tree/main/docs\nIssues: https://github.com/mistlab-dev/MistTerm/issues";
const LINKS_TIP_ZH: &str = "文档：https://github.com/mistlab-dev/MistTerm/tree/main/docs\n问题反馈：https://github.com/mistlab-dev/MistTerm/issues";

struct QuickStep {
    title: &'static str,
    detail: &'static str,
    keys: Vec<String>,
}

fn quick_steps(ctx: &egui::Context) -> Vec<QuickStep> {
    vec![
        QuickStep {
            title: crate::i18n::tr(ctx, "Connect to a server", "连接服务器"),
            detail: crate::i18n::tr(
                ctx,
                "Pick or create a connection on the left sidebar. Double-click or Enter opens a terminal tab.",
                "左侧选择或新建连接；双击 / 回车打开终端标签。",
            ),
            keys: vec![shortcuts::accel("N"), shortcuts::accel("T")],
        },
        QuickStep {
            title: crate::i18n::tr(ctx, "Command snippets", "命令片段"),
            detail: crate::i18n::tr(
                ctx,
                "Bottom bar → Snippets to open the fragment library. Browse market templates or create your own. Click to send to terminal.",
                "底栏 → 片段图标打开片段库。浏览市场模板或创建自定义片段，点击即发送到终端。",
            ),
            keys: vec![shortcuts::accel("K")],
        },
        QuickStep {
            title: crate::i18n::tr(ctx, "File transfer (SFTP)", "文件传输（SFTP）"),
            detail: crate::i18n::tr(
                ctx,
                "View → SFTP to open the file panel. Drag & drop upload, right-click for download. Also supports ZMODEM (rz/sz) in terminal.",
                "菜单「视图」→ SFTP 打开文件面板。拖拽上传，右键下载。终端内也支持 ZMODEM（rz/sz）。",
            ),
            keys: vec![],
        },
        QuickStep {
            title: crate::i18n::tr(ctx, "Team & sync", "团队与同步"),
            detail: crate::i18n::tr(
                ctx,
                "Tools → Team Login to connect to MistLab team server. Sync sessions, snippets, and credentials across devices.",
                "菜单「工具」→ 团队登录，连接 MistLab 团队服务器。跨设备同步会话、片段和凭证。",
            ),
            keys: vec![],
        },
        QuickStep {
            title: crate::i18n::tr(ctx, "Search terminal output", "搜索终端输出"),
            detail: crate::i18n::tr(
                ctx,
                "Search visible output in the current terminal; use command history when connected.",
                "搜索当前屏输出；已连接时可用命令历史。",
            ),
            keys: vec![
                shortcuts::accel("F"),
                shortcuts::terminal_history_accel().to_string(),
            ],
        },
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HelpPage {
    #[default]
    QuickStart,
    Shortcuts,
    Features,
}

impl HelpPage {
    fn label(self, ctx: &egui::Context) -> &'static str {
        match self {
            Self::QuickStart => crate::i18n::tr(ctx, "Quick start", "快速入门"),
            Self::Shortcuts => crate::i18n::tr(ctx, "Keyboard shortcuts", "键盘快捷键"),
            Self::Features => crate::i18n::tr(ctx, "Feature guide", "功能指南"),
        }
    }
}

#[derive(Default)]
pub struct HelpDocsDialog {
    pub open: bool,
    pub page: HelpPage,
}

impl HelpDocsDialog {
    pub fn open_page(&mut self, page: HelpPage) {
        self.page = page;
        self.open = true;
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        shortcuts_text: &str,
        status_message: &mut String,
    ) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        let mut should_close = false;
        let modal_sz = egui::vec2(600.0, 560.0);
        chrome::modal_window("help_docs_modal", theme, ctx)
            .open(&mut open)
            .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
            .movable(true)
            .resizable(true)
            .default_size(modal_sz)
            .show(ctx, |ui| {
                chrome::modal_content_frame(theme).show(ui, |ui| {
                    if chrome::modal_header(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Help", "帮助"),
                        theme.font_size_prominent(),
                    ) {
                        should_close = true;
                    }
                    render_help_tabs(ui, theme, ctx, &mut self.page);
                    ui.add_space(theme.spacing_md());
                    egui::Frame::none()
                        .fill(theme.color_subtle_inset_fill())
                        .stroke(egui::Stroke::new(1.0, theme.border_divider_color()))
                        .rounding(theme.radius_panel())
                        .inner_margin(egui::Margin::symmetric(
                            theme.spacing_body_pad(),
                            theme.spacing_body_pad(),
                        ))
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(360.0)
                                .auto_shrink([false; 2])
                                .show(ui, |ui| {
                                    match self.page {
                                        HelpPage::QuickStart => render_quick_start(ui, theme, ctx),
                                        HelpPage::Shortcuts => {
                                            render_shortcuts(ui, theme, ctx, shortcuts_text)
                                        }
                                        HelpPage::Features => render_features(ui, theme, ctx),
                                    }
                                });
                        });
                    ui.add_space(theme.spacing_md());
                    // Bottom link bar
                    render_bottom_links(ui, theme, ctx, status_message);
                });
            });
        self.open = open && !should_close;
    }
}

fn render_help_tabs(ui: &mut Ui, theme: &Theme, ctx: &egui::Context, page: &mut HelpPage) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_sm();
        for tab in [HelpPage::QuickStart, HelpPage::Shortcuts, HelpPage::Features] {
            let selected = *page == tab;
            let label = RichText::new(tab.label(ctx))
                .size(theme.font_size_connection_name())
                .color(if selected {
                    theme.text_primary()
                } else {
                    theme.color_form_hint()
                })
                .strong();
            if ui.selectable_label(selected, label).clicked() {
                *page = tab;
            }
        }
    });
}

fn render_quick_start(ui: &mut Ui, theme: &Theme, ctx: &egui::Context) {
    ui.label(
        RichText::new("MistTerm")
            .size(theme.font_size_empty_state())
            .strong()
            .color(theme.text_primary()),
    );
    ui.label(
        RichText::new(crate::i18n::tr(
            ctx,
            "Modern SSH terminal · Quick start guide",
            "现代 SSH 终端 · 快速上手",
        ))
            .size(theme.font_size_panel_title())
            .color(theme.color_form_hint()),
    );
    ui.add_space(theme.spacing_lg());
    let steps = quick_steps(ctx);
    for (i, step) in steps.iter().enumerate() {
        render_step_row(ui, theme, i + 1, step);
        if i + 1 < steps.len() {
            ui.add_space(theme.spacing_md());
        }
    }
    ui.add_space(theme.spacing_lg());

    // Website link
    let tip = crate::i18n::tr(ctx, WEBSITE_TIP_EN, WEBSITE_TIP_ZH);
    render_tip_box(
        ui,
        theme,
        crate::i18n::tr(ctx, "Official site", "官网"),
        tip,
    );
}

fn render_shortcuts(ui: &mut Ui, theme: &Theme, ctx: &egui::Context, raw: &str) {
    ui.label(
        RichText::new(crate::i18n::tr(
            ctx,
            "Keyboard shortcuts",
            "键盘快捷键",
        ))
            .size(theme.font_size_connection_name())
            .strong()
            .color(theme.text_primary()),
    );
    ui.add_space(theme.spacing_md());

    let mut lines = raw.lines().map(str::trim).filter(|l| !l.is_empty());
    if let Some(intro) = lines.next() {
        ui.label(
            RichText::new(intro)
                .size(theme.font_size_panel_title())
                .italics()
                .color(theme.color_form_hint()),
        );
        ui.add_space(theme.spacing_md());
    }
    for line in lines {
        let Some((keys, desc)) = line.split_once('—').or_else(|| line.split_once('-')) else {
            ui.label(
                RichText::new(line)
                    .size(theme.font_size_panel_title())
                    .color(theme.color_form_label()),
            );
            continue;
        };
        let keys = keys.trim();
        let desc = desc.trim();
        ui.add_space(theme.spacing_sm());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_md();
            ui.allocate_ui_with_layout(
                egui::vec2(148.0, 0.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    for (i, part) in keys.split('/').map(str::trim).enumerate() {
                        if i > 0 {
                            ui.label(
                                RichText::new("/")
                                    .size(theme.font_size_panel_title())
                                    .color(theme.color_form_hint()),
                            );
                        }
                        kbd_chip(ui, theme, part);
                    }
                },
            );
            ui.label(
                RichText::new(desc)
                    .size(theme.font_size_panel_title())
                    .color(theme.color_text_input_text()),
            );
        });
    }
}

// ── Feature guide page ─────────────────────────────────────────────

struct FeatureSection {
    icon: &'static str,
    title: &'static str,
    desc_en: &'static str,
    desc_zh: &'static str,
}

fn feature_sections() -> Vec<FeatureSection> {
    vec![
        FeatureSection {
            icon: "📁",
            title: "SFTP",
            desc_en: "Built-in SFTP file browser. Drag & drop upload, right-click download, navigate remote filesystem side by side with the terminal.",
            desc_zh: "内置 SFTP 文件浏览器。拖拽上传、右键下载，终端旁浏览远程文件系统。",
        },
        FeatureSection {
            icon: "🔧",
            title: "Command Snippets",
            desc_en: "Personal snippet library + marketplace with 60+ pre-built templates for Linux, Docker, Kubernetes, networking, databases, and more. Click to execute.",
            desc_zh: "个人片段库 + 市场，内置 60+ 运维模板（Linux、Docker、K8s、网络、数据库等），点击即执行。",
        },
        FeatureSection {
            icon: "📊",
            title: "System Monitor",
            desc_en: "Real-time CPU, memory, disk, and network monitoring for connected hosts. No agent required — uses standard SSH commands.",
            desc_zh: "实时查看远程主机 CPU、内存、磁盘、网络状态。无需安装代理，通过 SSH 命令获取。",
        },
        FeatureSection {
            icon: "👥",
            title: "Team Platform",
            desc_en: "Connect to MistLab team server for shared sessions, credential management, fragment analytics, and role-based access control.",
            desc_zh: "连接 MistLab 团队服务器，共享会话、凭证管理、片段使用分析、角色权限控制。",
        },
        FeatureSection {
            icon: "☁️",
            title: "Cloud Sync",
            desc_en: "Sync sessions, snippets, and credentials across devices via the team server. Tools → Cloud Sync to set up.",
            desc_zh: "通过团队服务器跨设备同步会话、片段和凭证。菜单「工具」→ 云端同步。",
        },
        FeatureSection {
            icon: "🔐",
            title: "Vault SSH CA",
            desc_en: "Team server issues short-lived SSH certificates via HashiCorp Vault SSH CA (server-side). The desktop app reads credentials from Vault KV for team servers.",
            desc_zh: "团队服务端通过 HashiCorp Vault SSH CA 签发短期 SSH 证书（服务端职责）。桌面端从 Vault KV 读取团队服务器凭证。",
        },
        FeatureSection {
            icon: "🤖",
            title: "AI Assistant",
            desc_en: "Built-in AI panel for command suggestions, error analysis, and terminal assistance. Configure your API key in Preferences.",
            desc_zh: "内置 AI 面板，提供命令建议、错误分析和终端辅助。在偏好设置中配置 API Key。",
        },
        FeatureSection {
            icon: "📤",
            title: "ZMODEM Transfer",
            desc_en: "Native ZMODEM support — use rz/sz commands directly in the terminal for file transfers with progress indication.",
            desc_zh: "原生 ZMODEM 支持，终端内直接使用 rz/sz 命令传输文件，带进度显示。",
        },
        FeatureSection {
            icon: "📝",
            title: "Batch Execution",
            desc_en: "Execute commands across multiple servers simultaneously. Select sessions, write your command, and run in parallel.",
            desc_zh: "批量在多台服务器并行执行命令。选择会话，输入命令，一键并行执行。",
        },
        FeatureSection {
            icon: "📋",
            title: "Session Logs",
            desc_en: "Automatic session logging with searchable history. Audit trails for compliance and review.",
            desc_zh: "自动会话记录，可搜索历史。满足审计合规和操作回溯需求。",
        },
    ]
}

fn render_features(ui: &mut Ui, theme: &Theme, ctx: &egui::Context) {
    let is_en = crate::i18n::language(ctx) == crate::i18n::UiLanguage::En;

    ui.label(
        RichText::new(crate::i18n::tr(
            ctx,
            "Feature guide",
            "功能指南",
        ))
            .size(theme.font_size_connection_name())
            .strong()
            .color(theme.text_primary()),
    );
    ui.add_space(theme.spacing_md());

    for section in feature_sections() {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_md();
            ui.label(
                RichText::new(section.icon)
                    .size(theme.font_size_connection_name()),
            );
            ui.vertical(|ui| {
                ui.set_min_width(ui.available_width());
                ui.label(
                    RichText::new(section.title)
                        .size(theme.font_size_connection_name())
                        .strong()
                        .color(theme.text_primary()),
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new(if is_en { section.desc_en } else { section.desc_zh })
                        .size(theme.font_size_panel_title())
                        .color(theme.color_form_label()),
                );
            });
        });
        ui.add_space(theme.spacing_md());
    }

    ui.add_space(theme.spacing_sm());
    let more = crate::i18n::tr(ctx, LINKS_TIP_EN, LINKS_TIP_ZH);
    render_tip_box(
        ui,
        theme,
        crate::i18n::tr(ctx, "Links", "相关链接"),
        more,
    );
}

// ── Bottom link bar ────────────────────────────────────────────────

fn render_bottom_links(ui: &mut Ui, theme: &Theme, ctx: &egui::Context, status_message: &mut String) {
    ui.horizontal(|ui| {
        if chrome::modal_secondary_icon_button(
            ui,
            theme,
            crate::ui::icons::IconId::File,
            crate::i18n::tr(ctx, "Online docs", "在线文档"),
        )
        .clicked()
            && !crate::platform::open_url(DOCS_INDEX_URL)
        {
            *status_message = crate::i18n::tr(
                ctx,
                "Failed to open browser",
                "无法打开浏览器",
            )
            .to_string();
        }

        if chrome::modal_secondary_icon_button(
            ui,
            theme,
            crate::ui::icons::IconId::Alert,
            crate::i18n::tr(ctx, "Report issue", "问题反馈"),
        )
        .clicked()
        {
            let url = crate::platform::github_new_issue_url(env!("CARGO_PKG_VERSION"));
            if !crate::platform::open_url(&url) {
                *status_message = crate::i18n::tr(
                    ctx,
                    "Failed to open browser",
                    "无法打开浏览器",
                )
                .to_string();
            }
        }

        // Website link
        if chrome::modal_secondary_icon_button(
            ui,
            theme,
            crate::ui::icons::IconId::Brand,
            crate::i18n::tr(ctx, "Website", "官网"),
        )
        .clicked()
            && !crate::platform::open_url(WEBSITE_URL)
        {
            *status_message = crate::i18n::tr(
                ctx,
                "Failed to open browser",
                "无法打开浏览器",
            ).to_string();
        }

        // GitHub link
        if chrome::modal_secondary_icon_button(
            ui,
            theme,
            crate::ui::icons::IconId::Brand,
            crate::i18n::tr(ctx, "GitHub", "GitHub"),
        )
        .clicked()
            && !crate::platform::open_url(GITHUB_URL)
        {
            *status_message = crate::i18n::tr(
                ctx,
                "Failed to open browser",
                "无法打开浏览器",
            ).to_string();
        }
    });
}

// ── Shared widgets ─────────────────────────────────────────────────

fn render_step_row(ui: &mut Ui, theme: &Theme, index: usize, step: &QuickStep) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_md();
        let circle_r = 13.0;
        let circle_d = circle_r * 2.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(circle_d, circle_d), egui::Sense::hover());
        let center = rect.center();
        let painter = ui.painter().with_clip_rect(ui.max_rect());
        painter.circle_filled(center, circle_r, theme.accent_a13());
        painter.circle_stroke(center, circle_r, egui::Stroke::new(1.0, theme.accent_alpha(89)));
        painter.text(
            center,
            egui::Align2::CENTER_CENTER,
            format!("{index}"),
            FontId::proportional(theme.font_size_connection_name()),
            theme.accent_color(),
        );
        ui.vertical(|ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                RichText::new(step.title)
                    .size(theme.font_size_connection_name())
                    .strong()
                    .color(theme.text_primary()),
            );
            ui.add_space(2.0);
            ui.label(
                RichText::new(step.detail)
                    .size(theme.font_size_panel_title())
                    .color(theme.color_form_label()),
            );
            if !step.keys.is_empty() {
                ui.add_space(theme.spacing_sm());
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    for key in &step.keys {
                        kbd_chip(ui, theme, key);
                    }
                });
            }
        });
    });
}

fn kbd_chip(ui: &mut Ui, theme: &Theme, text: &str) {
    egui::Frame::none()
        .fill(theme.color_panel_toolbar_btn_fill())
        .stroke(egui::Stroke::new(1.0, theme.color_text_input_stroke()))
        .rounding(theme.radius_status_btn())
        .inner_margin(egui::Margin::symmetric(7.0, 3.0))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text)
                    .font(FontId::monospace(theme.font_size_connection_name()))
                    .color(theme.text_primary()),
            );
        });
}

fn render_tip_box(ui: &mut Ui, theme: &Theme, title: &str, body: &str) {
    egui::Frame::none()
        .fill(theme.accent_a10())
        .stroke(egui::Stroke::new(1.0, theme.accent_alpha(48)))
        .rounding(theme.radius_list_item())
        .inner_margin(egui::Margin::symmetric(
            theme.spacing_search_input_x(),
            theme.spacing_search_input_y(),
        ))
        .show(ui, |ui| {
            ui.label(
                RichText::new(title)
                    .size(theme.font_size_connection_name())
                    .strong()
                    .color(theme.accent_color()),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new(body)
                    .size(theme.font_size_panel_title())
                    .color(theme.color_form_label()),
            );
        });
}
