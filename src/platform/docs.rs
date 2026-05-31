//! 在线产品文档与 GitHub 链接（Release 不携带本地 `docs/` 目录）。

/// GitHub 文档索引页。
pub const DOCS_INDEX_URL: &str = "https://github.com/mistlab-dev/MistTerm/tree/main/docs";

/// GitHub Issues 列表。
pub const GITHUB_ISSUES_URL: &str = "https://github.com/mistlab-dev/MistTerm/issues";

const GITHUB_NEW_ISSUE_BASE: &str = "https://github.com/mistlab-dev/MistTerm/issues/new";

/// 打开 Bug 报告模板（`.github/ISSUE_TEMPLATE/bug_report.yml`），并预填标题中的版本与 OS。
pub fn github_new_issue_url(app_version: &str) -> String {
    let os = std::env::consts::OS;
    let title = format!("[Bug] v{app_version} ({os})");
    format!(
        "{GITHUB_NEW_ISSUE_BASE}?template=bug_report.yml&title={}",
        encode_query_component(&title)
    )
}

/// 打开功能建议模板。
pub fn github_feature_request_url() -> String {
    format!("{GITHUB_NEW_ISSUE_BASE}?template=feature_request.yml")
}

fn encode_query_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push_str("%20"),
            b'\n' => out.push_str("%0A"),
            b'\r' => out.push_str("%0D"),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_issue_url_uses_bug_template() {
        let url = github_new_issue_url("0.2.4");
        assert!(url.contains("template=bug_report.yml"));
        assert!(url.contains("0.2.4"));
    }

    #[test]
    fn feature_request_url_uses_template() {
        let url = github_feature_request_url();
        assert!(url.contains("template=feature_request.yml"));
    }
}
