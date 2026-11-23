//! 命令片段展开与校验（无 UI）
//!
//! 将 Rhai、`<占位符>`、会话字段替换集中在此，供弹窗预览与发送前校验共用。

use std::collections::HashMap;

use crate::core::fragment::{
    expand_command_template, expand_fragment_command_stages, FragmentStats,
};
use crate::core::fragment_expr::{expand_rhai_blocks, merge_rhai_context};
use crate::core::session::SessionConfig;

/// 变量填写弹窗中的命令预览
pub fn build_fragment_command_preview(
    fragment: &FragmentStats,
    session: Option<&SessionConfig>,
    values: &HashMap<String, String>,
) -> String {
    expand_fragment_command_stages(&fragment.command, session, values).unwrap_or_else(|_| {
        let after = fragment.apply_variables(values);
        let ctx = merge_rhai_context(session, values);
        expand_rhai_blocks(&after, &ctx)
            .map(|rh| expand_command_template(&rh, session, values))
            .unwrap_or_else(|_| expand_command_template(&after, session, values))
    })
}

/// 「填写变量」弹窗里一行变量是否真的会改写下方命令（#466）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceholderUsage {
    /// 片段模板里有 `<name>`：填值后下方命令会变。
    Used,
    /// 片段模板里没有 `<name>`（例如直接写死 IP，或只用了会话字段 `<host>`）：
    /// 填了也不会改变当前命令，UI 必须给出可操作提示，不能静默无变化。
    Missing,
}

/// 判断变量名在片段命令模板中是否会参与替换。
///
/// 传入片段**原始模板**（而不是已展开的命令），才能区分「片段里定义的
/// `<宿主机IP>`」与「会话自动带入的 `<host>`」——后者填自定义变量当然不会变。
pub fn placeholder_usage(template: &str, name: &str) -> PlaceholderUsage {
    let name = name.trim();
    if name.is_empty() {
        return PlaceholderUsage::Missing;
    }
    let token = format!("<{name}>").to_ascii_lowercase();
    if template.to_ascii_lowercase().contains(&token) {
        PlaceholderUsage::Used
    } else {
        PlaceholderUsage::Missing
    }
}

/// 模板中未被任何 `<占位符>` 引用的变量名（去重、保序）。用于「不会改变命令」提示。
pub fn unmatched_variable_names<'a>(template: &str, names: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in names {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        if placeholder_usage(template, name) == PlaceholderUsage::Missing
            && !out.iter().any(|n| n.eq_ignore_ascii_case(name))
        {
            out.push(name.to_string());
        }
    }
    out
}

/// 发送前最终展开（含 Rhai 块内 `<user>` 等）
pub fn finalize_fragment_command_text(
    text: &str,
    session: Option<&SessionConfig>,
    values: &HashMap<String, String>,
) -> Result<String, String> {
    expand_fragment_command_stages(text, session, values)
}

// ---- helpers used only by tests (build small fragments & sessions)

#[cfg(test)]
fn make_fragment(command: impl Into<String>, vars: &[&str]) -> FragmentStats {
    use crate::core::fragment::FragmentVariable;
    let command = command.into();
    FragmentStats {
        id: "t".into(),
        title: "t".into(),
        title_en: None,
        command,
        category: "c".into(),
        category_en: None,
        variables: vars
            .iter()
            .map(|n| FragmentVariable::new(n, n))
            .collect(),
        tags: vec![],
        usage_count: 0,
        success_count: 0,
        total_time_ms: 0,
        last_used: None,
        source_status: String::new(),
        linked_docs: Vec::new(),
    }
}

#[cfg(test)]
fn make_session(host: &str, user: &str, port: u16, name: &str) -> SessionConfig {
    use crate::core::credential::SecretBackend;
    SessionConfig {
        id: "s".into(),
        name: name.into(),
        group: "g".into(),
        host: host.into(),
        port,
        username: user.into(),
        password: String::new(),
        private_key_path: String::new(),
        use_ssh_agent: false,
        auth_mode: crate::core::session::SessionAuthMode::Standard,
        last_connected_at: None,
        created_at: None,
        updated_at: None,
        ssh_config_marker: None,
        proxy_jump: String::new(),
        proxy_command: String::new(),
        color_tag: String::new(),
        keepalive_enabled: true,
        keepalive_interval_secs: 30,
        keepalive_count_max: 3,
        keepalive_auto_reconnect: true,
        secret_backend: SecretBackend::default(),
        local_forwards_text: String::new(),
        remote_forwards_text: String::new(),
        dynamic_forwards_text: String::new(),
        post_connect_command: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------- finalize_fragment_command_text

    #[test]
    fn finalize_plain_string_passthrough() {
        let out = finalize_fragment_command_text("ls -la", None, &HashMap::new()).unwrap();
        assert_eq!(out, "ls -la");
    }

    #[test]
    fn finalize_expands_placeholder_from_extras() {
        let mut v = HashMap::new();
        v.insert("path".into(), "/var/log".into());
        let out = finalize_fragment_command_text("tail -f <path>/*.log", None, &v).unwrap();
        assert_eq!(out, "tail -f /var/log/*.log");
    }

    #[test]
    fn finalize_expands_session_field_placeholders() {
        let s = make_session("db.internal", "dba", 2222, "Prod DB");
        let out = finalize_fragment_command_text(
            "ssh -p <port> <user>@<host> # <session>",
            Some(&s),
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(out, "ssh -p 2222 dba@db.internal # Prod DB");
    }

    #[test]
    fn finalize_missing_placeholder_leaves_original() {
        // No value provided for <foo>; the replacement key doesn't match -> keep literal.
        let out = finalize_fragment_command_text("echo <foo>", None, &HashMap::new()).unwrap();
        assert_eq!(out, "echo <foo>");
    }

    #[test]
    fn finalize_rhai_block_evaluates_to_result() {
        let mut v = HashMap::new();
        v.insert("a".into(), "3".into());
        v.insert("b".into(), "4".into());
        // `concat` in Rhai context returns strings joined.
        let out = finalize_fragment_command_text("v={{ concat(a, b) }}", None, &v).unwrap();
        assert_eq!(out, "v=34");
    }

    #[test]
    fn finalize_broken_rhai_block_returns_err() {
        // Unclosed `{{` triggers parse error from fragment_expr::expand_rhai_blocks.
        let r = finalize_fragment_command_text("a={{ unterminated", None, &HashMap::new());
        assert!(r.is_err());
    }

    // -------------------------------------------------------------- build_fragment_command_preview
    // (This wraps a three-layer fallback: stages -> rhai -> apply_variables -> template)

    #[test]
    fn preview_stages_success_path_plain_literal() {
        let f = make_fragment("echo hello", &[]);
        let out = build_fragment_command_preview(&f, None, &HashMap::new());
        assert_eq!(out, "echo hello");
    }

    #[test]
    fn preview_uses_session_fields_via_stages() {
        let f = make_fragment("ssh <user>@<host>", &[]);
        let s = make_session("h", "u", 22, "n");
        let out = build_fragment_command_preview(&f, Some(&s), &HashMap::new());
        assert_eq!(out, "ssh u@h");
    }

    #[test]
    fn preview_applies_fragment_variables_even_when_stages_ok() {
        let f = make_fragment("run <script>", &["script"]);
        let mut v = HashMap::new();
        v.insert("script".into(), "deploy.sh".into());
        let out = build_fragment_command_preview(&f, None, &v);
        assert_eq!(out, "run deploy.sh");
    }

    #[test]
    fn preview_broken_rhai_falls_back_to_apply_variables_plus_template() {
        // Intentionally broken (unclosed) Rhai block so stages returns Err.
        // `build_fragment_command_preview` must then fall back to `apply_variables` +
        // `expand_rhai_blocks` and finally `expand_command_template`.
        let f = make_fragment("echo {{ oops", &[]);
        let out = build_fragment_command_preview(&f, None, &HashMap::new());
        // The fallback path tries `expand_rhai_blocks` again on the broken string,
        // which will also fail, so it lands on the last fallback:
        // `expand_command_template(&after, ...)` where `after` == `apply_variables(result)`.
        // With no variables the literal broken-template text is what survives,
        // preserving debug-ability of the preview.
        assert_eq!(out, "echo {{ oops");
    }

    #[test]
    fn preview_unclosed_angle_bracket_reconstructed_in_fallback() {
        // `<` without `>` survives `apply_variables` and `expand_command_template` as literal.
        let f = make_fragment("awk '{ print $1 < $2 }'", &[]);
        let out = build_fragment_command_preview(&f, None, &HashMap::new());
        assert!(out.contains("< $2"));
    }

    #[test]
    fn preview_applies_variables_before_rhai_evaluation_in_fallback_chain() {
        // Use a simple Rhai expression that we know works.
        let f = make_fragment("hello {{ upper(name) }}", &["name"]);
        let mut v = HashMap::new();
        v.insert("name".into(), "world".into());
        let out = build_fragment_command_preview(&f, None, &v);
        // Either direct stages path or fallback chain resolves to "hello WORLD".
        assert!(out.contains("WORLD"), "got: {out}");
    }

    // -------------------------------------------------------------- #466 guidance helpers

    #[test]
    fn placeholder_usage_detects_custom_placeholder_in_template() {
        let t = "ping -c 3 <宿主机IP>";
        assert_eq!(placeholder_usage(t, "宿主机IP"), PlaceholderUsage::Used);
        assert_eq!(placeholder_usage(t, "宿主机ip"), PlaceholderUsage::Used);
        assert_eq!(placeholder_usage(t, " other "), PlaceholderUsage::Missing);
        assert_eq!(placeholder_usage(t, ""), PlaceholderUsage::Missing);
    }

    #[test]
    fn placeholder_usage_literal_ip_or_session_host_is_missing() {
        // 模板里直接写死 IP（没有自定义占位符）：填 <宿主机IP> 不会改变命令。
        let t = "ping -c 3 10.0.0.9";
        assert_eq!(placeholder_usage(t, "宿主机IP"), PlaceholderUsage::Missing);
        // 模板只用会话字段 <host>：自定义变量同样不会被替换。
        let t2 = "ssh <user>@<host>";
        assert_eq!(placeholder_usage(t2, "宿主机IP"), PlaceholderUsage::Missing);
        assert_eq!(placeholder_usage(t2, "host"), PlaceholderUsage::Used);
    }

    #[test]
    fn unmatched_variable_names_lists_only_unused() {
        let t = "curl <url>";
        let names = unmatched_variable_names(t, ["url", "token", "url", "", " "]);
        assert_eq!(names, vec!["token".to_string()]);
        assert!(unmatched_variable_names("curl <url> <token>", ["url", "token"]).is_empty());
    }

    /// 验收 1：模板含 `<自定义占位符>`，填值后预览出现该值；未填时保持占位符。
    #[test]
    fn preview_shows_custom_placeholder_value_after_fill() {
        let mut f = make_fragment("ping -c 3 <宿主机IP>", &["宿主机IP"]);
        f.variables[0].description = "host".into(); // 描述故意写得像会话字段
        let empty = build_fragment_command_preview(&f, None, &HashMap::new());
        assert_eq!(empty, "ping -c 3 <宿主机IP>");
        let mut v = HashMap::new();
        v.insert("宿主机IP".into(), "10.0.0.9".into());
        let out = build_fragment_command_preview(&f, None, &v);
        assert_eq!(out, "ping -c 3 10.0.0.9");
        // 决定是否「会变」看的是占位符名，不是描述。
        assert_eq!(placeholder_usage(&f.command, "宿主机IP"), PlaceholderUsage::Used);
        assert_eq!(placeholder_usage(&f.command, "host"), PlaceholderUsage::Missing);
    }

    /// 验收 2：模板不含该占位符时，帮助函数必须报 Missing（UI 据此给出下一步提示），
    /// 且预览与填值前后的真实命令一致——即「不会静默地假装变了」。
    #[test]
    fn unmatched_variable_reports_no_effect_instead_of_silent_change() {
        let f = make_fragment("ssh <user>@<host>", &["宿主机IP"]);
        let mut v = HashMap::new();
        v.insert("宿主机IP".into(), "10.0.0.9".into());
        let out = build_fragment_command_preview(&f, None, &v);
        // 会话缺失时 <user>/<host> 保持字面；关键是没有注入 10.0.0.9。
        assert!(!out.contains("10.0.0.9"), "got: {out}");
        assert_eq!(placeholder_usage(&f.command, "宿主机IP"), PlaceholderUsage::Missing);
        assert_eq!(
            unmatched_variable_names(&f.command, f.variables.iter().map(|x| x.name.as_str())),
            vec!["宿主机IP".to_string()]
        );
    }
}
