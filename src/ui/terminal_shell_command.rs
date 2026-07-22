//! Shell 命令行词法辅助：识别 `rz`/`sz` 等 ZMODEM 相关提交。

fn first_shell_word(line: &str) -> &str {
    line.trim().split_whitespace().next().unwrap_or("")
}

fn shell_word_basename(word: &str) -> &str {
    word.rsplit(['\\', '/']).next().unwrap_or(word)
}

/// 判断用户在 shell 提交的是否为远端接收命令 `rz`/`lrz`（本机应对应 ZMODEM 发送）。
pub fn is_rz_shell_command(line: &str) -> bool {
    let base = shell_word_basename(first_shell_word(line));
    let lower = base.to_ascii_lowercase();
    matches!(lower.as_str(), "rz" | "lrz" | "rz.exe" | "lrz.exe")
}

pub fn is_sz_shell_command(line: &str) -> bool {
    let base = shell_word_basename(first_shell_word(line));
    let lower = base.to_ascii_lowercase();
    matches!(lower.as_str(), "sz" | "lsz" | "sz.exe" | "lsz.exe")
}

#[cfg(test)]
mod rz_shell_command_tests {
    use super::{is_rz_shell_command, is_sz_shell_command};

    #[test]
    fn detects_rz_variants() {
        assert!(is_rz_shell_command("rz -bye"));
        assert!(is_rz_shell_command("  lrz -y "));
        assert!(is_rz_shell_command(
            "C:\\ProgramData\\mistterm\\lrzsz\\rz.exe -bye"
        ));
        assert!(!is_rz_shell_command("sz -bye foo.txt"));
        assert!(!is_rz_shell_command("where rz"));
        assert!(!is_rz_shell_command("echo rz"));
        assert!(is_sz_shell_command("sz -bye foo.txt"));
        assert!(!is_sz_shell_command("rz -bye"));
    }
}
