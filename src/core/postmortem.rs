//! 排障复盘草稿（#67）：AI 生成 Markdown 草稿 → 人确认 → 用户 JWT + `create_doc` 入库。
//!
//! 边界（issue #67 口径）：
//! - 草稿只在本地生成/编辑，不自动写回、不调 `update_doc`
//! - 保存走用户自己的 access token（`POST /v1/teams/{team_id}/documents`），
//!   权限与手动建文档完全一致；服务端 audit 现有 `create_doc` 已覆盖
//! - 生成失败/为空 = 明确报错，不产生半截文档

use serde::{Deserialize, Serialize};

/// 复盘草稿（现象 / 原因 / 处置 / 建议），Markdown 正文。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostmortemDraft {
    /// 文档标题，入库时使用（服务端限 200 rune）。
    pub title: String,
    /// Markdown 正文。
    pub markdown: String,
    /// 本次排障引用的 Runbook 来源（#66 召回锚点），草稿「参考」小节用。
    pub references: Vec<String>,
}

impl PostmortemDraft {
    pub fn is_empty(&self) -> bool {
        self.markdown.trim().is_empty()
    }

    /// 服务端标题硬限 200 rune（mist-docs TeamCreateDocument）。
    pub fn title_valid(&self) -> bool {
        let t = self.title.trim();
        !t.is_empty() && t.chars().count() <= 200
    }

    /// 「保存到 MistDocs」前的人类可读校验信息；`None` = 可保存。
    pub fn validation_error(&self) -> Option<&'static str> {
        if self.is_empty() {
            Some("Draft is empty")
        } else if !self.title_valid() {
            Some("Title is empty or too long")
        } else {
            None
        }
    }
}

/// 复盘草稿的系统提示词：要求 AI 只输出 Markdown 草稿本体。
pub fn postmortem_system_prompt() -> String {
    "你是运维排障复盘助手。根据用户提供的终端排障会话记录，输出一篇 Markdown 复盘草稿，\
包含且仅包含以下四个二级标题小节：\n\
## 现象\n## 原因定位\n## 处置步骤\n## 后续建议\n\n\
要求：只输出 Markdown 本体，不要额外开场白或解释；命令一律用代码块；\
内容严格来自会话记录，不要编造未出现的事实。"
    .to_string()
}

/// 草稿标题：`复盘：{主机} {日期}`（issue 口径；事件摘要由 AI 正文体现）。
pub fn postmortem_title(host: &str, date: &str) -> String {
    let host = host.trim();
    let host = if host.is_empty() { "会话" } else { host };
    format!("复盘：{host} {date}")
}

/// 把召回引用渲染进草稿尾部（已有「参考」节则不重复包裹）。
pub fn with_references(markdown: &str, references: &[String]) -> String {
    if references.is_empty() {
        return markdown.to_string();
    }
    let mut out = markdown.trim_end().to_string();
    out.push_str("\n\n## 参考\n");
    for r in references {
        out.push_str(&format!("- {r}\n"));
    }
    out
}

/// `POST /v1/teams/{team_id}/documents` 的路径。
pub fn team_create_document_path(team_id: &str) -> String {
    format!("/v1/teams/{team_id}/documents")
}

/// create_doc 请求体（mist-docs TeamCreateDocument：title 必填，type 默认 doc）。
#[derive(Debug, Serialize)]
pub struct CreateDocumentRequest {
    pub title: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub r#type: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub folder_id: String,
    pub content: String,
}

/// create_doc 响应（`{"data": {"id": ..., "title": ...}}`）。
#[derive(Debug, Deserialize)]
pub struct CreateDocumentResponse {
    pub data: Option<CreatedDocument>,
}

#[derive(Debug, Deserialize)]
pub struct CreatedDocument {
    pub id: String,
    #[serde(default)]
    pub title: String,
}

/// 用用户 JWT 创建文档。只调 create_doc，不做任何其它写操作。
pub fn create_document_blocking(
    api_base: &str,
    access_token: &str,
    team_id: &str,
    req: &CreateDocumentRequest,
) -> Result<CreatedDocument, String> {
    let url = format!(
        "{}{}",
        api_base.trim_end_matches('/'),
        team_create_document_path(team_id)
    );
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post(&url)
        .bearer_auth(access_token)
        .json(req)
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body = resp.text().unwrap_or_default();
    if !status.is_success() {
        return Err(format!("create_doc failed: HTTP {status} {body}"));
    }
    let parsed: CreateDocumentResponse =
        serde_json::from_str(&body).map_err(|e| format!("create_doc response: {e}"))?;
    parsed.data.ok_or_else(|| "create_doc: no data".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_draft_rejected() {
        let d = PostmortemDraft {
            title: "复盘：x 2026-10-23".into(),
            markdown: "   ".into(),
            references: vec![],
        };
        assert!(d.is_empty());
        assert!(d.validation_error().is_some());
    }

    #[test]
    fn oversize_title_rejected() {
        let d = PostmortemDraft {
            title: "复".repeat(201),
            markdown: "内容".into(),
            references: vec![],
        };
        assert_eq!(d.validation_error(), Some("Title is empty or too long"));
    }

    #[test]
    fn valid_draft_passes() {
        let d = PostmortemDraft {
            title: postmortem_title("web-01", "2026-10-23"),
            markdown: "## 现象\n502\n".into(),
            references: vec!["doc:abc#disk".into()],
        };
        assert_eq!(d.title, "复盘：web-01 2026-10-23");
        assert_eq!(d.validation_error(), None);
    }

    #[test]
    fn references_appended_once() {
        let md = with_references("## 现象\nx", &["doc:a#b".into()]);
        assert!(md.ends_with("## 参考\n- doc:a#b\n"));
        let md2 = with_references(&md, &["doc:c".into()]);
        assert_eq!(md2.matches("## 参考").count(), 2); // 旧的保留，新的追加
    }

    #[test]
    fn blank_host_falls_back() {
        assert_eq!(postmortem_title("  ", "d"), "复盘：会话 d");
    }

    #[test]
    fn create_path_shape() {
        assert_eq!(
            team_create_document_path("t1"),
            "/v1/teams/t1/documents"
        );
    }
}
