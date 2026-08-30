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
        command,
        category: "c".into(),
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
        last_connected_at: None,
        created_at: None,
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
}
