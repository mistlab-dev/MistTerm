//! AI 排障时只读召回本团队 MistDocs Runbook / 故障 SOP。
//!
//! 只调用 `GET /v1/teams/{id}/docs/search`（[`TeamClient::search_team_docs`]）。
//! 未登录、无团队、404/501、超时和其它错误都变成空列表，由界面静默降级。
//! 不调用 `update_doc` 或任何写入端点。读权限过滤在服务端
//! （`TeamDocParagraphSearch`）；客户端只展示接口返回的段落。

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::knowledge::clean_ask_intent;
use super::team::{
    ensure_access_token, TeamClient, TeamDocSearchItem, TeamTokenStore, DEFAULT_MISTDOCS_WEB_ORIGIN,
};

/// 召回请求使用的团队上下文（不含 token；后台线程里再取 JWT）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunbookSearchContext {
    pub api_base: String,
    pub team_id: String,
}

/// 一条可展示的 Runbook 段落。`open_url` 已限制为 http(s)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunbookRecall {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub excerpt: String,
    /// `doc:{id}` 或 `doc:{id}#{slug}`。
    #[serde(default)]
    pub anchor: String,
    #[serde(default)]
    pub open_url: String,
}

const SEARCH_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_CARDS: usize = 5;
const MAX_QUERY_CHARS: usize = 160;
const MAX_EXCERPT_CHARS: usize = 500;

/// 从用户问题和附带终端文本抽出检索词。空串表示不必请求。
pub fn runbook_search_query(question: &str, attached_excerpt: &str) -> String {
    let cleaned = clean_ask_intent(question);
    let primary = if cleaned.is_empty() {
        question.trim()
    } else {
        cleaned.as_str()
    };
    let source = if primary.is_empty() {
        attached_excerpt.trim()
    } else {
        primary
    };
    let collapsed = source.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(MAX_QUERY_CHARS).collect()
}

/// 只读检索。任何失败都返回空，不把错误抛回排障流程。
pub fn recall_team_runbooks(ctx: &RunbookSearchContext, query: &str) -> Vec<RunbookRecall> {
    let query = query.trim();
    if query.is_empty() || !is_safe_team_id(&ctx.team_id) {
        return Vec::new();
    }
    let Ok(token) = ensure_access_token(&ctx.api_base, &TeamTokenStore::default()) else {
        return Vec::new();
    };
    let Ok(client) = TeamClient::with_timeout(&ctx.api_base, SEARCH_TIMEOUT) else {
        return Vec::new();
    };
    match client.search_team_docs(&token, &ctx.team_id, query, MAX_CARDS as u32) {
        Ok(Some(resp)) => runbooks_from_items(&resp.items),
        _ => Vec::new(),
    }
}

pub fn runbooks_from_items(items: &[TeamDocSearchItem]) -> Vec<RunbookRecall> {
    let mut out = Vec::new();
    for item in items {
        let id = item.id.trim();
        if !is_safe_doc_id(id) {
            continue;
        }
        let title = item.title.trim();
        let excerpt = plain_excerpt(&item.excerpt);
        if title.is_empty() && excerpt.is_empty() {
            continue;
        }
        let slug = item.slug.trim();
        let anchor = team_doc_anchor(id, slug);
        let Some(open_url) = mistdocs_paragraph_url(id, slug, &item.url) else {
            continue;
        };
        if out.iter().any(|c: &RunbookRecall| c.anchor == anchor) {
            continue;
        }
        out.push(RunbookRecall {
            title: title.chars().take(120).collect(),
            excerpt,
            anchor,
            open_url,
        });
        if out.len() >= MAX_CARDS {
            break;
        }
    }
    out
}

/// 拼进系统提示的只读摘录。空列表返回空串（不追加说明）。
pub fn format_runbook_prompt(items: &[RunbookRecall]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let mut s = String::from(
        "\n\n[TEAM RUNBOOKS]\nThese excerpts are read-only MistDocs runbooks or fault SOPs this user can already read. When you use one, cite its source anchor. Do not create, edit, or update any document.\n",
    );
    for item in items.iter().take(MAX_CARDS) {
        s.push_str("- ");
        s.push_str(&item.title);
        s.push_str(" (");
        s.push_str(&item.anchor);
        s.push_str(")\n");
        if !item.excerpt.is_empty() {
            s.push_str("  ");
            s.push_str(&item.excerpt.chars().take(280).collect::<String>());
            s.push('\n');
        }
    }
    s
}

pub fn team_doc_anchor(doc_id: &str, slug: &str) -> String {
    if slug.is_empty() {
        format!("doc:{doc_id}")
    } else {
        format!("doc:{doc_id}#{slug}")
    }
}

/// 段落在 MistDocs 网页上的地址。服务端给了绝对 http(s) 链接就用它，否则落到文档页。
pub fn mistdocs_paragraph_url(doc_id: &str, slug: &str, server_url: &str) -> Option<String> {
    let slug = slug.trim();
    if let Some(url) = sanitize_runbook_url(server_url) {
        return sanitize_runbook_url(&with_fragment(url, slug));
    }
    let id = doc_id.trim();
    if !is_safe_doc_id(id) {
        return None;
    }
    // id 已限制为字母数字、`-`、`_`，可以直接放进路径。
    let url = format!("{DEFAULT_MISTDOCS_WEB_ORIGIN}/docs/{id}");
    sanitize_runbook_url(&with_fragment(url, slug))
}

/// 只接受没有用户信息、没有空白的 http(s) 绝对地址。
pub fn sanitize_runbook_url(raw: &str) -> Option<String> {
    let url = raw.trim();
    if url.is_empty()
        || url.len() > 2000
        || url.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return None;
    }
    let lower = url.to_ascii_lowercase();
    let rest = if lower.starts_with("https://") {
        &url["https://".len()..]
    } else if lower.starts_with("http://") {
        &url["http://".len()..]
    } else {
        return None;
    };
    if rest.is_empty() || rest.starts_with('/') {
        return None;
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() || host.contains('@') || host.contains('\\') {
        return None;
    }
    Some(url.to_string())
}

fn with_fragment(url: String, slug: &str) -> String {
    if slug.is_empty() || url.contains('#') || !is_safe_fragment(slug) {
        return url;
    }
    format!("{url}#{slug}")
}

fn is_safe_team_id(id: &str) -> bool {
    is_safe_token(id, 64)
}

fn is_safe_doc_id(id: &str) -> bool {
    is_safe_token(id, 128)
}

fn is_safe_token(id: &str, max: usize) -> bool {
    !id.is_empty()
        && id.len() <= max
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn is_safe_fragment(slug: &str) -> bool {
    let n = slug.chars().count();
    (1..=128).contains(&n)
        && slug.chars().all(|c| {
            !c.is_whitespace() && !c.is_control() && !matches!(c, '#' | '?' | '&' | '/' | '\\')
        })
}

fn plain_excerpt(raw: &str) -> String {
    let mut stripped = String::new();
    let mut in_tag = false;
    for c in raw.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            c if !in_tag => stripped.push(c),
            _ => {}
        }
    }
    let text = stripped
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > MAX_EXCERPT_CHARS {
        let head: String = collapsed.chars().take(MAX_EXCERPT_CHARS).collect();
        format!("{head}…")
    } else {
        collapsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, title: &str, excerpt: &str, slug: &str, url: &str) -> TeamDocSearchItem {
        TeamDocSearchItem {
            id: id.into(),
            title: title.into(),
            excerpt: excerpt.into(),
            slug: slug.into(),
            score: 1,
            url: url.into(),
        }
    }

    #[test]
    fn search_query_strips_ask_prefix_and_falls_back_to_attachment() {
        assert_eq!(
            runbook_search_query("问：我们怎么清理日志？", ""),
            "清理日志"
        );
        assert_eq!(
            runbook_search_query("", "  disk full\nno space  "),
            "disk full no space"
        );
        assert!(runbook_search_query("   ", "  ").is_empty());
        let long = "日".repeat(200);
        assert_eq!(
            runbook_search_query(&long, "").chars().count(),
            MAX_QUERY_CHARS
        );
    }

    #[test]
    fn paragraph_url_uses_slug_or_server_url() {
        assert_eq!(
            mistdocs_paragraph_url("abc-1", "disk", ""),
            Some("https://docs.mistlab.dev/docs/abc-1#disk".into())
        );
        assert_eq!(
            mistdocs_paragraph_url("abc-1", "", ""),
            Some("https://docs.mistlab.dev/docs/abc-1".into())
        );
        assert_eq!(
            mistdocs_paragraph_url("abc-1", "disk", "https://docs.example.com/docs/abc-1"),
            Some("https://docs.example.com/docs/abc-1#disk".into())
        );
        assert_eq!(
            mistdocs_paragraph_url("abc-1", "other", "https://docs.example.com/docs/abc-1#kept"),
            Some("https://docs.example.com/docs/abc-1#kept".into())
        );
    }

    #[test]
    fn paragraph_url_rejects_unsafe_targets() {
        assert_eq!(
            mistdocs_paragraph_url("abc", "", "javascript:alert(1)"),
            Some("https://docs.mistlab.dev/docs/abc".into())
        );
        assert_eq!(mistdocs_paragraph_url("../etc", "", ""), None);
        assert!(sanitize_runbook_url("javascript:alert(1)").is_none());
        assert!(sanitize_runbook_url("https://user:pw@docs.mistlab.dev/docs/a").is_none());
        assert!(sanitize_runbook_url("file:///etc/passwd").is_none());
        assert!(sanitize_runbook_url("https://docs.mistlab.dev/docs/a b").is_none());
    }

    #[test]
    fn items_become_linked_cards_without_html() {
        let cards = runbooks_from_items(&[
            item(
                "abc-1",
                "磁盘 SOP",
                "<p>先看 df &amp; du</p>",
                "disk",
                "javascript:alert(1)",
            ),
            item("abc-1", "重复", "另一段", "disk", ""),
            item("no/slash", "坏 id", "x", "", ""),
            item("", "无 id", "x", "", "https://docs.mistlab.dev/docs/z"),
        ]);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].title, "磁盘 SOP");
        assert_eq!(cards[0].excerpt, "先看 df & du");
        assert_eq!(cards[0].anchor, "doc:abc-1#disk");
        assert_eq!(
            cards[0].open_url,
            "https://docs.mistlab.dev/docs/abc-1#disk"
        );
    }

    #[test]
    fn prompt_is_read_only_and_empty_when_no_hits() {
        assert!(format_runbook_prompt(&[]).is_empty());
        let text = format_runbook_prompt(&[RunbookRecall {
            title: "磁盘 SOP".into(),
            excerpt: "先看 df".into(),
            anchor: "doc:abc-1#disk".into(),
            open_url: "https://docs.mistlab.dev/docs/abc-1#disk".into(),
        }]);
        assert!(text.contains("doc:abc-1#disk"));
        assert!(text.contains("read-only"));
        assert!(text.contains("Do not create, edit, or update"));
        assert!(!text.contains("update_doc"));
    }

    #[test]
    fn search_item_accepts_paragraph_url_alias() {
        let item: TeamDocSearchItem = serde_json::from_str(
            r#"{"id":"abc","title":"SOP","excerpt":"do x","slug":"step-1","anchor_url":"https://docs.mistlab.dev/docs/abc#step-1"}"#,
        )
        .unwrap();
        assert_eq!(item.url, "https://docs.mistlab.dev/docs/abc#step-1");
        let href: TeamDocSearchItem =
            serde_json::from_str(r#"{"id":"abc","href":"https://docs.mistlab.dev/docs/abc"}"#)
                .unwrap();
        assert_eq!(href.url, "https://docs.mistlab.dev/docs/abc");
    }
}
