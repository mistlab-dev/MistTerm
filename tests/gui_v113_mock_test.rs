//! v1.1.3 GUI mock 点验：纯 egui 上下文，不连团队 API / SSH。
//!
//! 模拟用户打开审计时间线、Agent 安装向导、「问：我们怎么」对话框，
//! 并走通拦截推荐 / 知识检索 / 入库候选的本地逻辑。

use eframe::egui::{self, Key};
use mistterm::core::{
    candidate_from_failed_command, clean_ask_intent, retrieve_team_knowledge,
    suggest_compliant_after_block_with_env, AuditTimeline, FragmentStats, KnowledgeHit,
    KnowledgeSource, SuggestionEnvContext, TimelineEntry, TimelineOutcome,
};
use mistterm::ui::agent_install_dialog::show_agent_install_modal;
use mistterm::ui::ask_knowledge_dialog::{show_ask_knowledge_modal, AskKnowledgeUiAction};
use mistterm::ui::audit_timeline_dialog::{show_audit_timeline_modal, AuditTimelineUiAction};
use mistterm::ui::theme::Theme;

fn mock_theme() -> Theme {
    Theme::light()
}

fn mock_team_cleanup() -> FragmentStats {
    let mut f = FragmentStats::new(
        "mock-team-clean".into(),
        "清理日志标准流程".into(),
        "find /var/log -name '*.log' -mtime +7 -delete".into(),
        "ops".into(),
    );
    f.tags = vec!["prod".into(), "log".into()];
    f.usage_count = 9;
    f
}

fn mock_personal_df() -> FragmentStats {
    FragmentStats::new(
        "mock-personal-df".into(),
        "查看磁盘".into(),
        "df -h".into(),
        "ops".into(),
    )
}

/// 模拟打开「问：我们怎么」→ 检索有命中 → 点「用到终端」。
#[test]
fn gui_mock_ask_knowledge_hit_then_use() {
    let theme = mock_theme();
    let team = mock_team_cleanup();
    let personal = mock_personal_df();
    let mut query = "问：我们怎么清理日志".to_string();
    let hits = retrieve_team_knowledge(&query, &[team], &[personal], &[], None, 5);
    assert!(!hits.is_empty());
    assert_eq!(hits[0].source, KnowledgeSource::TeamFragment);
    assert!(hits[0].anchor.starts_with("fragment:"));

    let mut open = true;
    let mut action = AskKnowledgeUiAction::None;
    let mut used = false;

    egui::__run_test_ctx(|ctx| {
        show_ask_knowledge_modal(
            ctx,
            &theme,
            &mut open,
            &mut query,
            &hits,
            true,
            &mut action,
        );
    });
    assert!(open, "dialog should stay open until user closes");

    // 模拟用户点「用到终端」
    action = AskKnowledgeUiAction::UseHit(0);
    if let AskKnowledgeUiAction::UseHit(i) = action {
        assert!(hits[i].fragment.is_some());
        used = true;
        open = false;
    }
    assert!(used);
    assert!(!open);
}

/// 模拟无命中 → 展示「暂无团队替代」→ 点「问 AI」。
#[test]
fn gui_mock_ask_knowledge_miss_then_ask_model() {
    let theme = mock_theme();
    let mut query = "我们怎么部署量子传送门".to_string();
    let hits: Vec<KnowledgeHit> = retrieve_team_knowledge(
        &query,
        &[],
        &[],
        &[],
        None,
        5,
    );
    assert!(hits.is_empty());
    assert_eq!(clean_ask_intent(&query), "部署量子传送门");

    let mut open = true;
    let mut action = AskKnowledgeUiAction::None;
    egui::__run_test_ctx(|ctx| {
        show_ask_knowledge_modal(
            ctx,
            &theme,
            &mut open,
            &mut query,
            &hits,
            true,
            &mut action,
        );
    });

    // 模拟点「问 AI（非团队知识）」
    action = AskKnowledgeUiAction::AskModel;
    assert!(matches!(action, AskKnowledgeUiAction::AskModel));
    open = false;
    assert!(!open);
}

/// 模拟检索按钮：输入后按 Enter 触发 Search。
#[test]
fn gui_mock_ask_knowledge_search_action() {
    let theme = mock_theme();
    let mut query = "清理日志".to_string();
    let mut hits = Vec::new();
    let mut open = true;
    let mut action = AskKnowledgeUiAction::None;
    let mut searched = false;

    egui::__run_test_ctx(|ctx| {
        ctx.input_mut(|i| {
            i.events.push(egui::Event::Key {
                key: Key::Enter,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
        });
        show_ask_knowledge_modal(
            ctx,
            &theme,
            &mut open,
            &mut query,
            &hits,
            searched,
            &mut action,
        );
    });

    assert!(
        matches!(action, AskKnowledgeUiAction::Search),
        "Enter with non-empty query should request Search, got {action:?}"
    );
    if matches!(action, AskKnowledgeUiAction::Search) {
        let team = mock_team_cleanup();
        hits = retrieve_team_knowledge(&query, &[team], &[], &[], None, 5);
        searched = true;
        action = AskKnowledgeUiAction::None;
    }
    assert!(searched);
    assert!(!hits.is_empty());

    egui::__run_test_ctx(|ctx| {
        show_ask_knowledge_modal(
            ctx,
            &theme,
            &mut open,
            &mut query,
            &hits,
            searched,
            &mut action,
        );
    });
}

/// 模拟审计时间线：灌入 mock 事件 → 按主机/结果筛选 → 清空。
#[test]
fn gui_mock_audit_timeline_filter_and_clear() {
    let theme = mock_theme();
    let mut timeline = AuditTimeline::new();
    timeline.push(TimelineEntry::new(
        "2026-09-05T01:00:00.000Z",
        "prod-1.example",
        "rm -rf /",
        TimelineOutcome::Blocked,
        "CREAD-006",
        "服务器策略",
    ));
    timeline.push(TimelineEntry::new(
        "2026-09-05T01:01:00.000Z",
        "staging-2",
        "cat /etc/shadow",
        TimelineOutcome::Confirmed,
        "sensitive",
        "服务器策略",
    ));
    timeline.push(TimelineEntry::new(
        "2026-09-05T01:02:00.000Z",
        "prod-1.example",
        "ls /var/log",
        TimelineOutcome::Allowed,
        "allow",
        "服务器策略",
    ));

    let mut host_filter = "prod-1".to_string();
    let mut outcome_filter = Some(TimelineOutcome::Blocked);
    let filtered = timeline.filter(host_filter.trim(), outcome_filter);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].command, "rm -rf /");

    let mut open = true;
    let mut action = AuditTimelineUiAction::None;
    egui::__run_test_ctx(|ctx| {
        show_audit_timeline_modal(
            ctx,
            &theme,
            &mut open,
            &timeline,
            &mut host_filter,
            &mut outcome_filter,
            &mut action,
        );
    });
    assert!(open);

    // 模拟点「清空」
    action = AuditTimelineUiAction::Clear;
    if matches!(action, AuditTimelineUiAction::Clear) {
        timeline.clear();
    }
    assert!(timeline.filter("", None).is_empty());
}

/// Escape 关闭审计时间线。
#[test]
fn gui_mock_audit_timeline_escape_closes() {
    let theme = mock_theme();
    let timeline = AuditTimeline::new();
    let mut open = true;
    let mut host = String::new();
    let mut outcome = None;
    let mut action = AuditTimelineUiAction::None;

    egui::__run_test_ctx(|ctx| {
        ctx.input_mut(|i| {
            i.events.push(egui::Event::Key {
                key: Key::Escape,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
        });
        show_audit_timeline_modal(
            ctx,
            &theme,
            &mut open,
            &timeline,
            &mut host,
            &mut outcome,
            &mut action,
        );
    });
    assert!(!open, "Escape should close audit timeline");
}

/// 模拟 Agent 安装向导打开 / 复制 / Escape 关闭。
#[test]
fn gui_mock_agent_install_open_copy_escape() {
    let theme = mock_theme();
    let mut open = true;
    let mut copied = false;

    egui::__run_test_ctx(|ctx| {
        show_agent_install_modal(ctx, &theme, &mut open, &mut copied);
    });
    assert!(open);

    // 模拟点「复制」
    copied = true;
    egui::__run_test_ctx(|ctx| {
        show_agent_install_modal(ctx, &theme, &mut open, &mut copied);
    });
    assert!(copied);

    egui::__run_test_ctx(|ctx| {
        ctx.input_mut(|i| {
            i.events.push(egui::Event::Key {
                key: Key::Escape,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
        });
        show_agent_install_modal(ctx, &theme, &mut open, &mut copied);
    });
    assert!(!open);
}

/// 拦截推荐：mock 环境标签过滤 + 无命中入库候选。
#[test]
fn gui_mock_block_suggest_env_and_candidate() {
    let mut prod = mock_team_cleanup();
    prod.tags = vec!["prod".into(), "log".into()];
    let mut staging = FragmentStats::new(
        "stg".into(),
        "预发清理".into(),
        "truncate -s 0 /tmp/app.log".into(),
        "ops".into(),
    );
    staging.tags = vec!["staging".into(), "log".into()];

    let env = SuggestionEnvContext::from_session("db.prod.local", "red", &["prod".into()]);
    let hit = suggest_compliant_after_block_with_env(
        "rm -rf /var/log",
        &[prod.clone(), staging],
        &[],
        Some(&env),
    )
    .expect("prod tagged cleanup");
    assert_eq!(hit.fragment.id, "mock-team-clean");
    assert_eq!(hit.source, "team");

    // 无库命中 → 失败路径候选（GUI Toast 主/次按钮背后的逻辑）
    let cand = candidate_from_failed_command("systemctl restart broken-svc", &[]).unwrap();
    assert_eq!(cand.reason.as_str(), "failed_path");
    assert!(!cand.command.is_empty());
}
