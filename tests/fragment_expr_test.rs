//! fragment_expr 单元测试
//!
//! 测试 `{{ ... }}` Rhai 表达式展开、上下文合并、时间戳注入等功能。

use mistterm::core::fragment_expr::*;
use mistterm::core::session::SessionConfig;
use std::collections::HashMap;

fn make_session(host: &str, user: &str, port: u16, name: &str) -> SessionConfig {
    SessionConfig {
        name: name.into(),
        host: host.into(),
        username: user.into(),
        port,
        ..Default::default()
    }
}

#[test]
fn merge_rhai_context_empty() {
    let ctx: HashMap<String, String> = HashMap::new();
    let merged = merge_rhai_context(None, &ctx);
    assert!(merged.is_empty());
}

#[test]
fn merge_rhai_context_with_session() {
    let session = make_session("192.168.1.1", "alice", 22, "prod-server");
    let ctx: HashMap<String, String> = HashMap::new();
    let merged = merge_rhai_context(Some(&session), &ctx);

    assert_eq!(merged.get("host"), Some(&"192.168.1.1".into()));
    assert_eq!(merged.get("hostname"), Some(&"192.168.1.1".into()));
    assert_eq!(merged.get("user"), Some(&"alice".into()));
    assert_eq!(merged.get("username"), Some(&"alice".into()));
    assert_eq!(merged.get("port"), Some(&"22".into()));
    assert_eq!(merged.get("session"), Some(&"prod-server".into()));
    assert_eq!(merged.get("session_name"), Some(&"prod-server".into()));
    assert_eq!(merged.get("name"), Some(&"prod-server".into()));
}

#[test]
fn merge_rhai_context_user_overrides_session() {
    let session = make_session("192.168.1.1", "alice", 22, "prod-server");
    let mut user_vars: HashMap<String, String> = HashMap::new();
    user_vars.insert("user".into(), "bob".into());
    user_vars.insert("custom".into(), "value".into());

    let ctx = merge_rhai_context(Some(&session), &user_vars);

    assert_eq!(ctx.get("user"), Some(&"bob".into()));
    assert_eq!(ctx.get("username"), Some(&"alice".into()));
    assert_eq!(ctx.get("custom"), Some(&"value".into()));
}

#[test]
fn merge_rhai_context_multiple_user_vars() {
    let session = make_session("10.0.0.1", "root", 2222, "dev");
    let mut user_vars: HashMap<String, String> = HashMap::new();
    user_vars.insert("env".into(), "production".into());
    user_vars.insert("role".into(), "admin".into());

    let ctx = merge_rhai_context(Some(&session), &user_vars);

    assert!(ctx.len() >= 9);
    assert_eq!(ctx.get("env"), Some(&"production".into()));
    assert_eq!(ctx.get("role"), Some(&"admin".into()));
}

#[test]
fn snapshot_rhai_context_injects_timestamps() {
    let ctx: HashMap<String, String> = HashMap::new();
    let snap = snapshot_rhai_context(&ctx);

    assert!(snap.contains_key("unix_ts"));
    assert!(snap.contains_key("unix_ts_ms"));
    assert!(snap.get("unix_ts").unwrap().parse::<i64>().is_ok());
    assert!(snap.get("unix_ts_ms").unwrap().parse::<i64>().is_ok());
}

#[test]
fn snapshot_rhai_context_does_not_override_existing() {
    let mut ctx: HashMap<String, String> = HashMap::new();
    ctx.insert("unix_ts".into(), "fixed_value".into());
    ctx.insert("user_var".into(), "test".into());

    let snap = snapshot_rhai_context(&ctx);

    assert_eq!(snap.get("unix_ts"), Some(&"fixed_value".into()));
    assert_eq!(snap.get("user_var"), Some(&"test".into()));
}

#[test]
fn expand_rhai_blocks_simple_text_no_expression() {
    let ctx: HashMap<String, String> = HashMap::new();
    let result = expand_rhai_blocks("Hello World", &ctx).unwrap();
    assert_eq!(result, "Hello World");
}

#[test]
fn expand_rhai_blocks_md5_hash() {
    let mut ctx: HashMap<String, String> = HashMap::new();
    ctx.insert("data".into(), "hello".into());

    let result = expand_rhai_blocks("{{ md5(data) }}", &ctx).unwrap();
    assert_eq!(result, "5d41402abc4b2a76b9719d911017c592");
}

#[test]
fn expand_rhai_blocks_concat() {
    let mut ctx: HashMap<String, String> = HashMap::new();
    ctx.insert("a".into(), "hello".into());
    ctx.insert("b".into(), "world".into());

    let result = expand_rhai_blocks("{{ concat(a, b) }}", &ctx).unwrap();
    assert_eq!(result, "helloworld");
}

#[test]
fn expand_rhai_blocks_case_conversion() {
    let mut ctx: HashMap<String, String> = HashMap::new();
    ctx.insert("text".into(), "HeLLo".into());

    let result_lower = expand_rhai_blocks("{{ lower(text) }}", &ctx).unwrap();
    assert_eq!(result_lower, "hello");

    let result_upper = expand_rhai_blocks("{{ upper(text) }}", &ctx).unwrap();
    assert_eq!(result_upper, "HELLO");
}

#[test]
fn expand_rhai_blocks_base64() {
    let mut ctx: HashMap<String, String> = HashMap::new();
    ctx.insert("data".into(), "Hello".into());

    let encoded = expand_rhai_blocks("{{ base64_encode(data) }}", &ctx).unwrap();
    assert_eq!(encoded, "SGVsbG8=");

    let decoded = expand_rhai_blocks("{{ base64_decode(\"SGVsbG8=\") }}", &ctx).unwrap();
    assert_eq!(decoded, "Hello");
}

#[test]
fn expand_rhai_blocks_sha256() {
    let mut ctx: HashMap<String, String> = HashMap::new();
    ctx.insert("data".into(), "hello".into());

    let result = expand_rhai_blocks("{{ sha256(data) }}", &ctx).unwrap();
    assert_eq!(result.len(), 64);
    assert!(result.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn expand_rhai_blocks_arithmetic() {
    let ctx: HashMap<String, String> = HashMap::new();

    let result = expand_rhai_blocks("{{ 10 + 20 }}", &ctx).unwrap();
    assert_eq!(result, "30");

    let result2 = expand_rhai_blocks("{{ 10 * 5 + 3 }}", &ctx).unwrap();
    assert_eq!(result2, "53");

    let result3 = expand_rhai_blocks("{{ (10 + 5) * 2 }}", &ctx).unwrap();
    assert_eq!(result3, "30");
}

#[test]
fn expand_rhai_blocks_comparison() {
    let ctx: HashMap<String, String> = HashMap::new();

    let result = expand_rhai_blocks("{{ 10 > 5 }}", &ctx).unwrap();
    assert_eq!(result, "true");

    let result2 = expand_rhai_blocks("{{ 3 == 3 }}", &ctx).unwrap();
    assert_eq!(result2, "true");

    let result3 = expand_rhai_blocks("{{ 5 < 3 }}", &ctx).unwrap();
    assert_eq!(result3, "false");
}

#[test]
fn expand_rhai_blocks_multiple_expressions_with_vars() {
    let mut ctx: HashMap<String, String> = HashMap::new();
    ctx.insert("host".into(), "server1".into());
    ctx.insert("user".into(), "admin".into());
    ctx.insert("port".into(), "22".into());

    let result = expand_rhai_blocks(
        "ssh {{ user }}@{{ host }} -p {{ port }}",
        &ctx,
    ).unwrap();
    assert!(result.contains("ssh"));
    assert!(result.contains("admin@server1"));
    assert!(!result.contains("{{"));
}

#[test]
fn expand_rhai_blocks_unclosed_brace_error() {
    let ctx: HashMap<String, String> = HashMap::new();
    let result = expand_rhai_blocks("{{ 10 +", &ctx);
    assert!(result.is_err());
}

#[test]
fn expand_rhai_blocks_empty_expression_error() {
    let ctx: HashMap<String, String> = HashMap::new();
    let result = expand_rhai_blocks("{{ }}", &ctx);
    assert!(result.is_err());
}

#[test]
fn expand_rhai_blocks_invalid_function_error() {
    let ctx: HashMap<String, String> = HashMap::new();
    let result = expand_rhai_blocks("{{ nonexistent_func() }}", &ctx);
    assert!(result.is_err());
}

#[test]
fn expand_rhai_blocks_timestamp_in_result() {
    let ctx: HashMap<String, String> = HashMap::new();
    let result = expand_rhai_blocks("ts={{ unix_timestamp() }}", &ctx).unwrap();
    assert!(result.starts_with("ts="));
    let ts_str = result.trim_start_matches("ts=");
    assert!(ts_str.parse::<i64>().is_ok());
}