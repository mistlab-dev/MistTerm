//! Unit tests for fragment model

use mistterm::core::fragment::*;
use mistterm::core::session::SessionConfig;
use std::collections::HashMap;

fn make_fragment_stats(id: &str, title: &str, command: &str, category: &str) -> FragmentStats {
    FragmentStats {
        id: id.into(),
        title: title.into(),
        command: command.into(),
        category: category.into(),
        tags: vec![],
        variables: vec![],
        usage_count: 1,
        success_count: 1,
        total_time_ms: 100,
        last_used: None,
    }
}

#[test]
fn fragment_variable_new() {
    let var = FragmentVariable::new("host", "Server hostname");
    assert_eq!(var.name, "host");
    assert_eq!(var.description, "Server hostname");
    assert!(var.default_value.is_none());
}

#[test]
fn fragment_variable_with_default() {
    let var = FragmentVariable::with_default("port", "Port number", "22");
    assert_eq!(var.name, "port");
    assert_eq!(var.default_value, Some("22".into()));
}

#[test]
fn fragment_stats_extract_placeholders() {
    let stats = make_fragment_stats(
        "id1",
        "Test",
        "ssh <user>@<host> -p <port>",
        "ssh",
    );

    let placeholders = stats.extract_placeholders();
    assert_eq!(placeholders.len(), 3);
    assert!(placeholders.contains(&"user".into()));
    assert!(placeholders.contains(&"host".into()));
    assert!(placeholders.contains(&"port".into()));
}

#[test]
fn fragment_stats_extract_placeholders_no_duplicates() {
    let stats = make_fragment_stats(
        "id1",
        "Test",
        "ssh <user>@<host> -p <port> and user:<user>",
        "ssh",
    );

    let placeholders = stats.extract_placeholders();
    assert_eq!(placeholders.len(), 3);
}

#[test]
fn fragment_stats_variable_defaults() {
    let mut stats = make_fragment_stats("id1", "Test", "ssh <host>", "ssh");
    stats.variables.push(FragmentVariable::new("host", "Hostname"));
    stats.variables.push(FragmentVariable::with_default("port", "Port", "22"));

    let defaults = stats.variable_defaults();
    assert!(!defaults.contains_key("host"));
    assert_eq!(defaults.get("port"), Some(&"22".into()));
}

#[test]
fn fragment_stats_apply_variables() {
    let mut stats = make_fragment_stats(
        "id1",
        "Test",
        "ssh <user>@<host> -p <port>",
        "ssh",
    );
    stats.variables.push(FragmentVariable::new("user", "Username"));
    stats.variables.push(FragmentVariable::new("host", "Hostname"));
    stats.variables.push(FragmentVariable::new("port", "Port"));

    let mut values = HashMap::new();
    values.insert("user".into(), "alice".into());
    values.insert("host".into(), "192.168.1.1".into());
    values.insert("port".into(), "22".into());

    let result = stats.apply_variables(&values);
    assert_eq!(result, "ssh alice@192.168.1.1 -p 22");
}

#[test]
fn fragment_stats_apply_variables_missing() {
    let mut stats = make_fragment_stats(
        "id1",
        "Test",
        "ssh <user>@<host>",
        "ssh",
    );
    stats.variables.push(FragmentVariable::new("user", "Username"));
    stats.variables.push(FragmentVariable::new("host", "Hostname"));

    let mut values = HashMap::new();
    values.insert("user".into(), "alice".into());

    let result = stats.apply_variables(&values);
    assert_eq!(result, "ssh alice@<host>");
}

#[test]
fn expand_command_template_no_session() {
    let template = "ls -la";
    let extras: HashMap<String, String> = HashMap::new();

    let result = expand_command_template(template, None, &extras);
    assert_eq!(result, "ls -la");
}

#[test]
fn expand_command_template_with_session() {
    let session = SessionConfig {
        name: "prod".into(),
        host: "10.0.0.1".into(),
        username: "bob".into(),
        port: 22,
        ..Default::default()
    };
    let extras: HashMap<String, String> = HashMap::new();

    let result = expand_command_template("ssh <user>@<host>", Some(&session), &extras);
    assert_eq!(result, "ssh bob@10.0.0.1");
}