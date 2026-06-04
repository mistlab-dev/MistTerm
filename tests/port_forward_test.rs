//! 端口转发核心逻辑与 session 文本辅助测试

use mistterm::core::{
    append_dynamic_forward_line, append_local_forward_line, format_local_forward_line,
    parse_dynamic_forwards_text, parse_forward_form, parse_local_forwards_text,
    ForwardFormInput, ForwardFormKind, PortForwardKind,
};
use mistterm::ssh::LocalPortForward;

#[test]
fn session_local_forward_roundtrip() {
    let mut text = String::new();
    let fwd = LocalPortForward {
        local_port: 8080,
        remote_host: "127.0.0.1".into(),
        remote_port: 80,
        bind_address: "127.0.0.1".into(),
    };
    append_local_forward_line(&mut text, &fwd);
    let parsed = parse_local_forwards_text(&text);
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].local_port, 8080);
    assert_eq!(format_local_forward_line(&fwd), "8080:127.0.0.1:80");
}

#[test]
fn dynamic_form_parses() {
    let input = ForwardFormInput {
        bind_address: "127.0.0.1".into(),
        local_port: "1080".into(),
        ..Default::default()
    };
    let k = parse_forward_form(ForwardFormKind::Dynamic, &input).unwrap();
    assert!(matches!(k, PortForwardKind::Dynamic(_)));
    if let PortForwardKind::Dynamic(d) = k {
        assert_eq!(d.local_port, 1080);
    }
}

#[test]
fn append_dynamic_dedup() {
    let mut text = String::new();
    let fwd = parse_dynamic_forwards_text("1080")[0].clone();
    append_dynamic_forward_line(&mut text, &fwd);
    append_dynamic_forward_line(&mut text, &fwd);
    assert_eq!(parse_dynamic_forwards_text(&text).len(), 1);
}

#[test]
fn audit_detail_contains_ports() {
    let k = PortForwardKind::Local(LocalPortForward {
        local_port: 9000,
        remote_host: "db".into(),
        remote_port: 5432,
        bind_address: "127.0.0.1".into(),
    });
    let d = k.audit_detail();
    assert_eq!(d["local_port"], 9000);
    assert_eq!(d["type"], "local");
}
