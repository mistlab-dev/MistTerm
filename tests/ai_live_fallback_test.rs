//! 使用外部 OpenAI 兼容端点做 live 冒烟（需环境变量，默认跳过）。
//!
//! ```powershell
//! $env:MIST_LIVE_AI_BASE='https://vectide.cn/v1'
//! $env:MIST_LIVE_AI_KEY='sk-...'
//! $env:MIST_LIVE_AI_MODEL='glm-5.1'
//! cargo test --test ai_live_fallback_test -- --test-threads=1 --nocapture
//! ```

use mistterm::core::{
    chat_completions_with_key, clean_ask_intent, retrieve_team_knowledge, test_connection_with_key,
    AppSettings, ChatMessage, FragmentStats,
};

fn live_cfg() -> Option<(String, String, String)> {
    let base = std::env::var("MIST_LIVE_AI_BASE").ok()?.trim().to_string();
    let key = std::env::var("MIST_LIVE_AI_KEY").ok()?.trim().to_string();
    if base.is_empty() || key.is_empty() {
        return None;
    }
    let model = std::env::var("MIST_LIVE_AI_MODEL")
        .unwrap_or_else(|_| "glm-5.1".into())
        .trim()
        .to_string();
    Some((base, key, model))
}

#[test]
fn live_save_settings_and_test_connection() {
    let Some((base, key, model)) = live_cfg() else {
        eprintln!("skip: set MIST_LIVE_AI_BASE / MIST_LIVE_AI_KEY");
        return;
    };
    let mut settings = AppSettings::load();
    settings.ai.enabled = true;
    settings.ai.base_url = base;
    settings.ai.model = model.clone();
    settings.ai.stream_responses = false;
    settings.ai.set_api_key(&key).expect("set key");
    settings.save().expect("save settings");

    test_connection_with_key(&settings.ai, &key).expect("connection/test chat should succeed");
    eprintln!("OK: saved AI settings model={model}");
}

#[test]
fn live_ask_miss_then_model_fallback_labeled() {
    let Some((base, key, model)) = live_cfg() else {
        eprintln!("skip: set MIST_LIVE_AI_BASE / MIST_LIVE_AI_KEY");
        return;
    };

    let query = "问：我们怎么部署量子传送门";
    let cleaned = clean_ask_intent(query);
    let hits = retrieve_team_knowledge(query, &[], &[], &[], None, 5);
    assert!(hits.is_empty(), "mock empty library should miss");
    assert_eq!(cleaned, "部署量子传送门");

    let mut settings = mistterm::core::AiSettings::default();
    settings.enabled = true;
    settings.base_url = base;
    settings.model = model;
    settings.stream_responses = false;
    settings.max_tokens = 256;
    settings.timeout_secs = 90;

    let system = "\
你是运维助手。[IMPORTANT] No team knowledge matched for this question. \
Clearly state that your answer is model-generated and is not from team documentation. \
先用一句话标明「模型 · 非团队知识」，再用中文简短回答。";

    let messages = vec![
        ChatMessage {
            role: "system".into(),
            content: system.into(),
        },
        ChatMessage {
            role: "user".into(),
            content: format!("我们怎么{cleaned}？（无团队知识命中，请模型兜底）"),
        },
    ];

    let reply = chat_completions_with_key(&settings, &key, &messages).expect("model fallback chat");
    eprintln!("model fallback reply:\n{reply}");
    assert!(!reply.trim().is_empty());
    let lower = reply.to_lowercase();
    assert!(
        reply.contains("非团队")
            || reply.contains("模型")
            || lower.contains("model")
            || lower.contains("not team"),
        "reply should label non-team/model source: {reply}"
    );
}

#[test]
fn live_ask_hit_prefers_team_snippet_without_calling_model() {
    let mut team = FragmentStats::new(
        "live-team-clean".into(),
        "清理日志标准流程".into(),
        "find /var/log -name '*.log' -mtime +7 -delete".into(),
        "ops".into(),
    );
    team.tags = vec!["log".into(), "prod".into()];

    let hits = retrieve_team_knowledge(
        "问：我们怎么清理日志",
        &[team],
        &[],
        &[],
        None,
        3,
    );
    assert!(!hits.is_empty());
    assert_eq!(hits[0].anchor, "fragment:live-team-clean");
    eprintln!(
        "OK: team hit title={} body={}",
        hits[0].title, hits[0].body
    );
}
