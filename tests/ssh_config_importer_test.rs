//! Unit tests for ssh_config_importer
//!
//! Tests OpenSSH config file parsing logic.

use mistterm::core::ssh_config_importer::*;

#[test]
fn parse_empty_config() {
    let result = parse_ssh_config_str("");
    assert!(result.candidates.is_empty());
    assert!(result.warnings.is_empty());
}

#[test]
fn parse_single_host() {
    let config = r#"
Host myserver
    HostName 192.168.1.100
    Port 22
    User admin
"#;
    let result = parse_ssh_config_str(config);
    assert_eq!(result.candidates.len(), 1);

    let host = &result.candidates[0];
    assert_eq!(host.host_alias, "myserver");
    assert_eq!(host.hostname, Some("192.168.1.100".into()));
    assert_eq!(host.port, 22);
    assert_eq!(host.username, "admin");
    assert!(host.importable());
}

#[test]
fn parse_host_with_identity_file() {
    let config = r#"
Host gitlab
    HostName gitlab.example.com
    User git
    IdentityFile ~/.ssh/id_ed25519
"#;
    let result = parse_ssh_config_str(config);
    assert_eq!(result.candidates.len(), 1);

    let host = &result.candidates[0];
    assert_eq!(host.host_alias, "gitlab");
    assert!(host.identity_file.contains("id_ed25519"));
}

#[test]
fn parse_host_with_proxy_jump() {
    let config = r#"
Host internal
    HostName 10.0.0.5
    ProxyJump bastion
"#;
    let result = parse_ssh_config_str(config);
    assert_eq!(result.candidates.len(), 1);

    let host = &result.candidates[0];
    assert_eq!(host.proxy_jump, "bastion");
}

#[test]
fn parse_host_with_proxy_command() {
    let config = r#"
Host internal
    HostName 10.0.0.5
    ProxyCommand ssh -W %h:%p jumphost
"#;
    let result = parse_ssh_config_str(config);
    assert_eq!(result.candidates.len(), 1);

    let host = &result.candidates[0];
    assert!(host.proxy_command.contains("ssh -W"));
}

#[test]
fn parse_multiple_hosts() {
    let config = r#"
Host server1
    HostName 192.168.1.1
    User root

Host server2
    HostName 192.168.1.2
    User admin
"#;
    let result = parse_ssh_config_str(config);
    assert_eq!(result.candidates.len(), 2);
    assert_eq!(result.candidates[0].host_alias, "server1");
    assert_eq!(result.candidates[1].host_alias, "server2");
}

#[test]
fn parse_host_without_hostname_is_not_importable() {
    let config = r#"
Host nohost
    User admin
"#;
    let result = parse_ssh_config_str(config);
    assert_eq!(result.candidates.len(), 1);

    let host = &result.candidates[0];
    assert!(!host.importable());
    assert!(host.skip_reason.is_some());
}

#[test]
fn parse_comment_lines_ignored() {
    let config = r#"
# This is a comment
Host myserver
    # Another comment
    HostName 192.168.1.100
    User admin
"#;
    let result = parse_ssh_config_str(config);
    assert_eq!(result.candidates.len(), 1);
}

#[test]
fn parse_empty_lines_ignored() {
    let config = r#"

Host myserver

    HostName 192.168.1.100

    User admin

"#;
    let result = parse_ssh_config_str(config);
    assert_eq!(result.candidates.len(), 1);
}

#[test]
fn parse_host_with_custom_port() {
    let config = r#"
Host custom
    HostName 192.168.1.100
    Port 2222
    User admin
"#;
    let result = parse_ssh_config_str(config);
    assert_eq!(result.candidates.len(), 1);

    let host = &result.candidates[0];
    assert_eq!(host.port, 2222);
}

#[test]
fn ssh_config_candidate_importable() {
    let candidate = SshConfigCandidate {
        host_alias: "test".into(),
        hostname: Some("127.0.0.1".into()),
        port: 22,
        username: "user".into(),
        identity_file: String::new(),
        proxy_jump: String::new(),
        proxy_command: String::new(),
        skip_reason: None,
    };
    assert!(candidate.importable());
}

#[test]
fn ssh_config_candidate_not_importable_with_skip_reason() {
    let candidate = SshConfigCandidate {
        host_alias: "test".into(),
        hostname: Some("127.0.0.1".into()),
        port: 22,
        username: "user".into(),
        identity_file: String::new(),
        proxy_jump: String::new(),
        proxy_command: String::new(),
        skip_reason: Some("HostName missing".into()),
    };
    assert!(!candidate.importable());
}

#[test]
fn ssh_config_candidate_not_importable_without_hostname() {
    let candidate = SshConfigCandidate {
        host_alias: "test".into(),
        hostname: None,
        port: 22,
        username: "user".into(),
        identity_file: String::new(),
        proxy_jump: String::new(),
        proxy_command: String::new(),
        skip_reason: None,
    };
    assert!(!candidate.importable());
}

#[test]
fn ssh_config_candidate_marker_key() {
    let candidate = SshConfigCandidate {
        host_alias: "myserver".into(),
        hostname: Some("192.168.1.1".into()),
        port: 22,
        username: "admin".into(),
        identity_file: String::new(),
        proxy_jump: String::new(),
        proxy_command: String::new(),
        skip_reason: None,
    };
    assert_eq!(candidate.marker_key(), "myserver|192.168.1.1|22");
}

#[test]
fn ssh_config_candidate_display_target_with_hostname() {
    let candidate = SshConfigCandidate {
        host_alias: "myserver".into(),
        hostname: Some("192.168.1.1".into()),
        port: 2222,
        username: "admin".into(),
        identity_file: String::new(),
        proxy_jump: "bastion".into(),
        proxy_command: String::new(),
        skip_reason: None,
    };
    assert_eq!(candidate.display_target(), "192.168.1.1:2222 (Jump bastion)");
}

#[test]
fn ssh_config_candidate_display_target_without_hostname() {
    let candidate = SshConfigCandidate {
        host_alias: "nohost".into(),
        hostname: None,
        port: 22,
        username: "admin".into(),
        identity_file: String::new(),
        proxy_jump: String::new(),
        proxy_command: String::new(),
        skip_reason: None,
    };
    assert_eq!(candidate.display_target(), "(HostName 缺失)");
}

#[test]
fn ssh_config_parse_result_default() {
    let result = SshConfigParseResult::default();
    assert!(result.candidates.is_empty());
    assert!(result.warnings.is_empty());
}

#[test]
fn parse_hostname_with_ipv6_address() {
    let config = r#"
Host ipv6server
    HostName fe80::1
    User admin
"#;
    let result = parse_ssh_config_str(config);
    assert_eq!(result.candidates.len(), 1);
    assert_eq!(result.candidates[0].hostname, Some("fe80::1".into()));
}