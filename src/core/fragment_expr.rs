//! 片段命令中的 `{{ ... }}` Rhai 表达式：在 `<占位符>` 与会话字段展开后求值，用于 md5/base64 等组合。
//!
//! 未引入完整 Lua，以降低依赖与安全面；Rhai 为 Rust 生态常见嵌入式脚本，默认无文件/网络 API。
//!
//! 内置函数含 **`unix_timestamp()`**（秒）、**`unix_timestamp_ms()`**（毫秒），UTC。
//!
//! 同一条命令里若多处要用**同一时间**的戳，用一次展开内固定的变量 **`unix_ts`**、**`unix_ts_ms`**
//!（字符串，在整条模板第一次做 `{{ … }}` 展开前注入；若片段变量里已同名则**不覆盖**）。
//!
//! **`{{ … }}` 可嵌套**：先按括号深度匹配闭合的 `}}`，再对块内表达式递归展开内层 `{{ … }}`，最后交给 Rhai。
//! 拼常量与戳请用引号与 `concat`（`unix_ts` 已为字符串变量），例如  
//! `{{ md5(concat("appinfo73ea…", unix_ts)) }}` 或 `concat(concat("a", "b"), unix_ts)`。  
//! 勿写 `md5(abcd{{ unix_ts }})`：`abcd` 会被当成未定义标识符。

use crate::core::session::SessionConfig;
use chrono::Utc;
use base64::Engine;
use rhai::{Dynamic, Engine as RhaiEngine, Scope};
use std::collections::HashMap;

use sha2::{Digest, Sha256};

fn truncate_dbg(s: &str, max_chars: usize) -> String {
    let mut it = s.chars();
    let head: String = it.by_ref().take(max_chars).collect();
    if it.next().is_some() {
        format!("{}…", head)
    } else {
        head
    }
}

fn md5_hex(s: &str) -> String {
    format!("{:x}", md5::compute(s.as_bytes()))
}

fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    Digest::update(&mut h, s.as_bytes());
    format!("{:x}", Digest::finalize(h))
}

fn b64_enc(s: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(s.as_bytes())
}

fn b64_dec(s: &str) -> String {
    match base64::engine::general_purpose::STANDARD.decode(s.as_bytes()) {
        Ok(v) => String::from_utf8_lossy(&v).into_owned(),
        Err(_) => String::new(),
    }
}

fn dynamic_to_fragment_string(d: Dynamic) -> String {
    if let Some(s) = d.clone().try_cast::<rhai::ImmutableString>() {
        return s.as_str().to_string();
    }
    d.to_string()
}

fn make_engine() -> RhaiEngine {
    let mut e = RhaiEngine::new();
    e.register_fn("md5", md5_hex);
    e.register_fn("sha256", sha256_hex);
    e.register_fn("base64_encode", b64_enc);
    e.register_fn("base64_dec", b64_dec);
    e.register_fn("base64_decode", b64_dec);
    e.register_fn("concat", |a: &str, b: &str| -> String { format!("{a}{b}") });
    e.register_fn("lower", |s: &str| -> String { s.to_lowercase() });
    e.register_fn("upper", |s: &str| -> String { s.to_uppercase() });
    e.register_fn("unix_timestamp", || -> i64 { Utc::now().timestamp() });
    e.register_fn("unix_timestamp_ms", || -> i64 { Utc::now().timestamp_millis() });
    e
}

/// `{{ … }}` 内可直接写 `<host>`、`<user>`：在 Rhai 求值前替换为带引号的字符串字面量，
/// 避免出现 `md5(alice)` 被当成未定义变量（alice）的问题。
fn substitute_angle_placeholders_in_expr(expr: &str, ctx: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(expr.len() + 16);
    let mut rest = expr;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        rest = &rest[open + 1..];
        let Some(close) = rest.find('>') else {
            out.push('<');
            out.push_str(rest);
            return out;
        };
        let key = rest[..close].trim();
        rest = &rest[close + 1..];
        if key.is_empty() {
            out.push('<');
            out.push('>');
            continue;
        }
        if let Some(val) = ctx.get(key) {
            let escaped = val.replace('\\', "\\\\").replace('"', "\\\"");
            out.push('"');
            out.push_str(&escaped);
            out.push('"');
        } else {
            out.push('<');
            out.push_str(key);
            out.push('>');
        }
    }
    out.push_str(rest);
    out
}

fn valid_rhai_binding_key(k: &str) -> bool {
    let mut it = k.chars();
    let Some(first) = it.next() else {
        return false;
    };
    if !(first.is_alphabetic() || first == '_') {
        return false;
    }
    k.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// 合并会话字段与用户提供变量，供 Rhai 中 `host`、`a` 等标识符使用；`user` 覆盖同名会话键。
pub fn merge_rhai_context(
    session: Option<&SessionConfig>,
    user: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut m = HashMap::new();
    if let Some(s) = session {
        m.insert("host".into(), s.host.clone());
        m.insert("hostname".into(), s.host.clone());
        m.insert("user".into(), s.username.clone());
        m.insert("username".into(), s.username.clone());
        m.insert("port".into(), s.port.to_string());
        m.insert("session".into(), s.name.clone());
        m.insert("session_name".into(), s.name.clone());
        m.insert("name".into(), s.name.clone());
    }
    for (k, v) in user {
        m.insert(k.clone(), v.clone());
    }
    m
}

/// 为本次整条命令的 Rhai 展开准备上下文：复制 `ctx`，并一次性写入 `unix_ts` / `unix_ts_ms`（同一 `Utc::now()`），
/// 便于多处 `{{ unix_ts }}` 得到相同字符串；用户或片段里已提供的键不覆盖。
pub fn snapshot_rhai_context(ctx: &HashMap<String, String>) -> HashMap<String, String> {
    let mut eff = ctx.clone();
    let now = Utc::now();
    eff.entry("unix_ts".into())
        .or_insert_with(|| now.timestamp().to_string());
    eff.entry("unix_ts_ms".into())
        .or_insert_with(|| now.timestamp_millis().to_string());
    eff
}

/// `open_idx` 必须为 `{{` 的起始下标；返回闭合 `}}` 的第一个 `}` 的下标。
pub(crate) fn find_closing_double_brace(s: &str, open_idx: usize) -> Option<usize> {
    if open_idx + 2 > s.len() || !s[open_idx..].starts_with("{{") {
        return None;
    }
    let bytes = s.as_bytes();
    let mut i = open_idx + 2;
    let mut nest = 1u32;
    while i + 1 < bytes.len() {
        if bytes[i] == b'{' && bytes[i + 1] == b'{' {
            nest += 1;
            i += 2;
            continue;
        }
        if bytes[i] == b'}' && bytes[i + 1] == b'}' {
            nest = nest.checked_sub(1)?;
            if nest == 0 {
                return Some(i);
            }
            i += 2;
            continue;
        }
        i += 1;
    }
    None
}

fn expand_rhai_blocks_with_eff(
    template: &str,
    eff: &HashMap<String, String>,
    eng: &RhaiEngine,
) -> Result<String, String> {
    let mut s = template.to_string();
    loop {
        let Some(start) = s.find("{{") else {
            break;
        };
        let Some(close_idx) = find_closing_double_brace(&s, start) else {
            return Err("未闭合的 {{ … }}（缺少 }}）".to_string());
        };
        let expr = s[start + 2..close_idx].trim();
        if expr.is_empty() {
            return Err("{{ }} 内表达式不能为空".to_string());
        }
        let tail_start = close_idx + 2;

        let expr_ready = if expr.contains("{{") {
            expand_rhai_blocks_with_eff(expr, eff, eng)?
        } else {
            expr.to_string()
        };

        let mut scope = Scope::new();
        for (k, v) in eff {
            if valid_rhai_binding_key(k) {
                let _ = scope.push(k.clone(), v.clone());
            }
        }

        let expr_for_rhai = substitute_angle_placeholders_in_expr(&expr_ready, eff);
        let evaluated: Dynamic = eng
            .eval_with_scope(&mut scope, &expr_for_rhai)
            .map_err(|e| {
                log::warn!(
                    "Rhai 片段表达式失败: expr_preview={} … eval_expr={} err={}",
                    truncate_dbg(&expr_ready, 80),
                    truncate_dbg(&expr_for_rhai, 120),
                    e
                );
                format!(
                    "表达式错误：{}（完整表达式已写入日志）",
                    truncate_dbg(&e.to_string(), 72)
                )
            })?;
        let replacement = dynamic_to_fragment_string(evaluated);

        s = format!("{}{}{}", &s[..start], replacement, &s[tail_start..]);
    }
    Ok(s)
}

/// 将 `{{ expr }}` 替换为 Rhai 求值结果。`ctx` 为已解析占位符名 → 字符串（如片段变量、用户填写值）。
pub fn expand_rhai_blocks(template: &str, ctx: &HashMap<String, String>) -> Result<String, String> {
    let eff = snapshot_rhai_context(ctx);
    let eng = make_engine();
    expand_rhai_blocks_with_eff(template, &eff, &eng)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_concat() {
        let mut m = HashMap::new();
        m.insert("a".into(), "hello".into());
        m.insert("b".into(), "world".into());
        let out = expand_rhai_blocks("prefix {{ md5(concat(a, b)) }}", &m).unwrap();
        assert!(out.starts_with("prefix "));
        assert!(!out.contains("{{"));
    }

    #[test]
    fn md5_angle_user_becomes_string_literal() {
        let mut m = HashMap::new();
        m.insert("user".into(), "alice".into());
        let out = expand_rhai_blocks("{{ md5(<user>) }}", &m).unwrap();
        assert_eq!(out.len(), 32);
        assert!(!out.contains("{{"));
    }

    #[test]
    fn rhai_int_becomes_decimal_string() {
        assert_eq!(
            expand_rhai_blocks("n={{ 40+2 }},flag={{ true }}", &HashMap::new()).unwrap(),
            "n=42,flag=true"
        );
    }

    #[test]
    fn unix_timestamp_numeric() {
        let out = expand_rhai_blocks("{{ unix_timestamp() }}", &HashMap::new()).unwrap();
        assert!(out.parse::<i64>().is_ok());
        let ms = expand_rhai_blocks("{{ unix_timestamp_ms() }}", &HashMap::new()).unwrap();
        assert!(ms.parse::<i64>().is_ok());
    }

    #[test]
    fn unix_ts_same_in_multiple_blocks() {
        let out = expand_rhai_blocks("{{ unix_ts }}-{{ unix_ts }}", &HashMap::new()).unwrap();
        let (a, b) = out.split_once('-').unwrap();
        assert_eq!(a, b);
    }

    /// 内层 `}}` 不得截断外层：以前用 `find("}}")` 会误把 `{{ unix_timestamp() }}` 的 `}}` 当成整块结束。
    #[test]
    fn nested_double_curly_arithmetic() {
        let out = expand_rhai_blocks("{{ 10 * {{ 2 + 3 }} }}", &HashMap::new()).unwrap();
        assert_eq!(out, "50");
    }

    #[test]
    fn unix_ts_user_override() {
        let mut m = HashMap::new();
        m.insert("unix_ts".into(), "fixed".into());
        assert_eq!(
            expand_rhai_blocks("{{ unix_ts }}", &m).unwrap(),
            "fixed"
        );
    }
}
