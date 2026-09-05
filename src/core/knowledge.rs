//! 团队知识检索与来源锚点（v1.1.3）。
//!
//! - 主检索：团队/个人命令片段（关键词意图打分，非 embedding）
//! - 可选：MistDocs `GET /v1/teams/{id}/docs/search`（404 软失败）
//! - 统一 [`KnowledgeHit`]，供拦截 Toast、「问：我们怎么」与 Model 兜底标明来源

use crate::core::fragment_recommendations::{
    query_topic_keywords, score_fragment_against_keywords, SuggestionEnvContext,
};
use crate::core::FragmentStats;

/// 知识命中来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnowledgeSource {
    TeamFragment,
    PersonalFragment,
    TeamDoc,
    Model,
}

impl KnowledgeSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TeamFragment => "team",
            Self::PersonalFragment => "personal",
            Self::TeamDoc => "doc",
            Self::Model => "model",
        }
    }

    pub fn label_en(self) -> &'static str {
        match self {
            Self::TeamFragment => "Team snippet",
            Self::PersonalFragment => "Personal snippet",
            Self::TeamDoc => "Team doc",
            Self::Model => "Model (not team knowledge)",
        }
    }

    pub fn label_zh(self) -> &'static str {
        match self {
            Self::TeamFragment => "团队片段",
            Self::PersonalFragment => "个人片段",
            Self::TeamDoc => "团队文档",
            Self::Model => "模型（非团队知识）",
        }
    }
}

/// 一条可展示的知识命中（片段或文档段落）。
#[derive(Debug, Clone)]
pub struct KnowledgeHit {
    pub source: KnowledgeSource,
    pub title: String,
    /// 可插入终端的命令，或文档段落正文。
    pub body: String,
    /// 来源锚点：`fragment:{id}` / `doc:{id}#{slug}` / `model:fallback`
    pub anchor: String,
    /// 排序分（越大越靠前）。
    pub score: i64,
    /// 若来自片段，保留完整统计以便插入/沉底。
    pub fragment: Option<FragmentStats>,
}

impl KnowledgeHit {
    pub fn from_fragment(source: KnowledgeSource, fragment: FragmentStats, score: i64) -> Self {
        let anchor = format!("fragment:{}", fragment.id);
        Self {
            source,
            title: fragment.title.clone(),
            body: fragment.command.clone(),
            anchor,
            score,
            fragment: Some(fragment),
        }
    }

    pub fn from_doc(title: String, body: String, doc_id: &str, slug: &str, score: i64) -> Self {
        let anchor = if slug.is_empty() {
            format!("doc:{doc_id}")
        } else {
            format!("doc:{doc_id}#{slug}")
        };
        Self {
            source: KnowledgeSource::TeamDoc,
            title,
            body,
            anchor,
            score,
            fragment: None,
        }
    }

    pub fn model_fallback_placeholder(question: &str) -> Self {
        Self {
            source: KnowledgeSource::Model,
            title: question.trim().to_string(),
            body: String::new(),
            anchor: "model:fallback".into(),
            score: 0,
            fragment: None,
        }
    }
}

/// MistDocs 搜索结果条目（与 Team API 契约对齐；服务端未实现时客户端得到空列表）。
#[derive(Debug, Clone, Default)]
pub struct DocSearchHit {
    pub id: String,
    pub title: String,
    pub excerpt: String,
    pub slug: String,
    pub score: i64,
}

/// 清洗「问：我们怎么…」类自然语言，抽出检索关键词。
pub fn clean_ask_intent(query: &str) -> String {
    let mut q = query.trim().to_string();
    for prefix in [
        "问：",
        "问:",
        "请问",
        "帮我",
        "如何",
        "怎么",
        "怎样",
        "怎么样",
        "我们怎么",
        "我们如何",
        "how do we",
        "how to",
        "how can i",
        "how can we",
    ] {
        let lower = q.to_lowercase();
        let p = prefix.to_lowercase();
        if lower.starts_with(&p) {
            q = q[prefix.len()..].trim().to_string();
        }
    }
    q.trim_matches(|c: char| c == '?' || c == '？' || c == '：' || c == ':')
        .trim()
        .to_string()
}

fn filter_fragments_for_env(
    fragments: &[FragmentStats],
    env: Option<&SuggestionEnvContext>,
) -> Vec<FragmentStats> {
    let Some(env) = env else {
        return fragments.to_vec();
    };
    let tags = env.effective_match_tags();
    if tags.is_empty() {
        return fragments.to_vec();
    }
    let filtered: Vec<FragmentStats> = fragments
        .iter()
        .filter(|f| env.fragment_matches(f))
        .cloned()
        .collect();
    if filtered.is_empty() {
        fragments.to_vec()
    } else {
        filtered
    }
}

/// 先团队片段，再个人片段，再文档；按分数取 TopN。
pub fn retrieve_team_knowledge(
    query: &str,
    team_fragments: &[FragmentStats],
    personal_fragments: &[FragmentStats],
    doc_hits: &[DocSearchHit],
    env: Option<&SuggestionEnvContext>,
    limit: usize,
) -> Vec<KnowledgeHit> {
    let cleaned = clean_ask_intent(query);
    let keywords = query_topic_keywords(&cleaned);
    if keywords.is_empty() && doc_hits.is_empty() {
        return Vec::new();
    }

    let team = filter_fragments_for_env(team_fragments, env);
    let personal = filter_fragments_for_env(personal_fragments, env);

    let mut hits: Vec<KnowledgeHit> = Vec::new();

    for f in &team {
        let score = score_fragment_against_keywords(f, &keywords);
        if score > 0 {
            hits.push(KnowledgeHit::from_fragment(
                KnowledgeSource::TeamFragment,
                f.clone(),
                score,
            ));
        }
    }
    for f in &personal {
        let score = score_fragment_against_keywords(f, &keywords);
        if score > 0 {
            hits.push(KnowledgeHit::from_fragment(
                KnowledgeSource::PersonalFragment,
                f.clone(),
                score,
            ));
        }
    }
    for d in doc_hits {
        let mut score = d.score.max(1);
        let hay = format!("{} {}", d.title, d.excerpt).to_lowercase();
        for k in &keywords {
            if !k.is_empty() && hay.contains(k) {
                score += 2;
            }
        }
        hits.push(KnowledgeHit::from_doc(
            d.title.clone(),
            d.excerpt.clone(),
            &d.id,
            &d.slug,
            score,
        ));
    }

    // 团队片段优先于同分数个人/文档：来源权重
    hits.sort_by(|a, b| {
        let aw = source_rank(a.source);
        let bw = source_rank(b.source);
        b.score
            .cmp(&a.score)
            .then_with(|| aw.cmp(&bw))
            .then_with(|| a.title.cmp(&b.title))
    });
    hits.truncate(limit.max(1));
    hits
}

fn source_rank(s: KnowledgeSource) -> u8 {
    match s {
        KnowledgeSource::TeamFragment => 0,
        KnowledgeSource::TeamDoc => 1,
        KnowledgeSource::PersonalFragment => 2,
        KnowledgeSource::Model => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::FragmentStats;

    #[test]
    fn clean_ask_strips_chinese_prefix() {
        assert_eq!(clean_ask_intent("问：我们怎么清理日志？"), "清理日志");
        assert_eq!(clean_ask_intent("如何重启 nginx"), "重启 nginx");
    }

    #[test]
    fn retrieve_prefers_team_cleanup() {
        let mut team = FragmentStats::new(
            "t1".into(),
            "清理日志标准流程".into(),
            "find /var/log -name '*.log' -mtime +7 -delete".into(),
            "ops".into(),
        );
        team.tags = vec!["log".into()];
        let personal = FragmentStats::new(
            "p1".into(),
            "其他".into(),
            "echo hi".into(),
            "misc".into(),
        );
        let hits = retrieve_team_knowledge(
            "我们怎么清理日志",
            &[team],
            &[personal],
            &[],
            None,
            5,
        );
        assert!(!hits.is_empty());
        assert_eq!(hits[0].source, KnowledgeSource::TeamFragment);
        assert_eq!(hits[0].anchor, "fragment:t1");
    }
}
