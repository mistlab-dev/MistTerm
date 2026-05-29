//! ai_client 单元测试
//!
//! 测试 AI 客户端的脱敏和命令提取功能。

use mistterm::core::ai_client::{redact_for_ai, extract_shell_commands};

#[test]
fn redact_preserves_clean_text() {
    let clean = "Hello, how are you?";
    let result = redact_for_ai(clean);
    assert_eq!(result, clean);
}

#[test]
fn redact_bearer_token() {
    let text = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9";
    let result = redact_for_ai(text);
    assert!(result.contains("[REDACTED]"));
    assert!(!result.contains("Bearer"));
}

#[test]
fn redact_private_key() {
    let text = "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAA=\n-----END OPENSSH PRIVATE KEY-----";
    let result = redact_for_ai(text);
    assert!(result.contains("[REDACTED]"));
    assert!(!result.contains("BEGIN"));
}

#[test]
fn redact_private_key_with_label() {
    let text = "PRIVATE KEY hidden";
    let result = redact_for_ai(text);
    assert!(result.contains("[REDACTED]"));
    assert!(!result.contains("PRIVATE KEY"));
}

#[test]
fn redact_password_equal() {
    let text = "password=secret123";
    let result = redact_for_ai(text);
    assert!(result.contains("[REDACTED]"));
    assert!(!result.contains("secret123"));
}

#[test]
fn redact_password_uppercase() {
    let text = "PASSWORD=secret456";
    let result = redact_for_ai(text);
    assert!(result.contains("[REDACTED]"));
    assert!(!result.contains("secret456"));
}

#[test]
fn redact_api_key() {
    let text = "api_key=sk-1234567890abcdef";
    let result = redact_for_ai(text);
    assert!(result.contains("[REDACTED]"));
    assert!(!result.contains("sk-1234567890abcdef"));
}

#[test]
fn redact_api_key_uppercase() {
    let text = "API_KEY=sk-abcdef123456";
    let result = redact_for_ai(text);
    assert!(result.contains("[REDACTED]"));
    assert!(!result.contains("sk-abcdef123456"));
}

#[test]
fn redact_token() {
    let text = "token=abc123def456";
    let result = redact_for_ai(text);
    assert!(result.contains("[REDACTED]"));
    assert!(!result.contains("abc123def456"));
}

#[test]
fn redact_token_uppercase() {
    let text = "TOKEN=xyz789";
    let result = redact_for_ai(text);
    assert!(result.contains("[REDACTED]"));
    assert!(!result.contains("xyz789"));
}

#[test]
fn redact_multiple_secrets() {
    let text = "api_key=sk-123 password=pass123 token=tok456";
    let result = redact_for_ai(text);
    assert_eq!(result.matches("[REDACTED]").count(), 3);
}

#[test]
fn extract_no_commands_from_plain_text() {
    let text = "Hello, this is just a plain response without any commands.";
    let cmds = extract_shell_commands(text);
    assert!(cmds.is_empty());
}

#[test]
fn extract_commands_from_fence_block() {
    let text = r#"
Here are some commands:

```bash
ls -la
pwd
echo "hello"
```
"#;
    let cmds = extract_shell_commands(text);
    assert!(!cmds.is_empty());
    assert!(cmds.iter().any(|c| c.contains("ls")));
    assert!(cmds.iter().any(|c| c.contains("pwd")));
}

#[test]
fn extract_deduplicates_commands() {
    let text = r#"
```bash
ls
ls
echo "a"
echo "a"
```
"#;
    let cmds = extract_shell_commands(text);
    assert_eq!(cmds.len(), 2);
}

#[test]
fn extract_commands_skips_whole_script_block() {
    let text = r#"
```bash
#!/bin/bash
for i in {1..10}; do
    echo $i
done
```
"#;
    let cmds = extract_shell_commands(text);
    assert!(cmds.is_empty());
}

#[test]
fn extract_shell_commands_handles_empty_input() {
    let cmds = extract_shell_commands("");
    assert!(cmds.is_empty());
}