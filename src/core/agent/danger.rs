//! AI 面板「用到终端」危险命令检测（issue #249）。
//!
//! # 产品钉死初版规则表（命中任一条即弹红框）
//!
//! | 规则 | 说明 |
//! |------|------|
//! | `rm` | 含 `rm -rf` / `rm -r` 等任意 `rm` |
//! | `dd` | 磁盘写入 |
//! | `mkfs` / `mkfs.*` | 格式化 |
//! | `kill -9` / `kill -KILL` | 强制杀进程 |
//! | `chmod -R` / `chown -R` | 递归改权限/属主 |
//! | `>` / `>\|` | 重定向覆盖写文件（`>>` 追加不拦） |
//! | `shutdown` / `reboot` / `halt` / `poweroff` | 关机重启 |
//! | `drop table` / `truncate` | SQL，大小写不敏感 |
//!
//! 只读命令（`ls`/`df`/`free` 等）不命中。实现为词法/启发式，后续可扩规则表。

/// 一条危险命中：规则 id、给人看的原因、能解析出的影响对象。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DangerHit {
    /// 规则表中的短 id（如 `rm`、`redirect_overwrite`）。
    pub rule_id: &'static str,
    /// 中文原因（UI 再按语言本地化展示时可用 label_*）。
    pub reason_zh: &'static str,
    pub reason_en: &'static str,
    /// 解析到的路径 / PID / 表名等；解析不出则为空。
    pub targets: Vec<String>,
}

impl DangerHit {
    pub fn reason(&self, zh: bool) -> &'static str {
        if zh {
            self.reason_zh
        } else {
            self.reason_en
        }
    }
}

/// 检测「用到终端」是否应弹危险确认。命中返回 `Some`，只读/无害返回 `None`。
pub fn assess_ai_terminal_danger(command: &str) -> Option<DangerHit> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return None;
    }

    // SQL 可写在任意位置（含管道/注释外），整串扫描。
    if let Some(hit) = assess_sql_danger(trimmed) {
        return Some(hit);
    }

    // 按 shell 连接符拆段，任一段命中即危险。
    for segment in split_shell_segments(trimmed) {
        if let Some(hit) = assess_segment(segment) {
            return Some(hit);
        }
    }
    None
}

fn assess_sql_danger(cmd: &str) -> Option<DangerHit> {
    let lower = cmd.to_ascii_lowercase();
    if let Some(rest) = find_keyword_rest(&lower, "drop table") {
        let table = first_sql_ident(rest);
        return Some(DangerHit {
            rule_id: "drop_table",
            reason_zh: "SQL DROP TABLE 会删除整张表",
            reason_en: "SQL DROP TABLE deletes an entire table",
            targets: table.into_iter().collect(),
        });
    }
    // truncate [table] name — 避免误伤 truncate 命令本身以外的英文词时要求空白后跟标识
    if let Some(rest) = find_keyword_rest(&lower, "truncate table") {
        let table = first_sql_ident(rest);
        return Some(DangerHit {
            rule_id: "truncate",
            reason_zh: "SQL TRUNCATE 会清空表数据",
            reason_en: "SQL TRUNCATE clears table data",
            targets: table.into_iter().collect(),
        });
    }
    if let Some(rest) = find_keyword_rest(&lower, "truncate") {
        // SQL `TRUNCATE name` 或 GNU `truncate <file>`：一并确认。
        let target = first_sql_ident(rest).or_else(|| first_shell_arg(rest));
        return Some(DangerHit {
            rule_id: "truncate",
            reason_zh: "TRUNCATE 会清空或截断目标",
            reason_en: "TRUNCATE clears or truncates the target",
            targets: target.into_iter().collect(),
        });
    }
    None
}

fn find_keyword_rest<'a>(hay: &'a str, keyword: &str) -> Option<&'a str> {
    let mut start = 0;
    while let Some(rel) = hay[start..].find(keyword) {
        let abs = start + rel;
        let before_ok = abs == 0
            || hay
                .as_bytes()
                .get(abs - 1)
                .is_some_and(|b| !b.is_ascii_alphanumeric() && *b != b'_');
        let after = abs + keyword.len();
        let after_ok = hay
            .as_bytes()
            .get(after)
            .map(|b| !b.is_ascii_alphanumeric() && *b != b'_')
            .unwrap_or(true);
        if before_ok && after_ok {
            return Some(hay[after..].trim_start());
        }
        start = abs + 1;
    }
    None
}

fn first_sql_ident(rest: &str) -> Option<String> {
    let rest = rest.trim_start();
    let mut out = String::new();
    for ch in rest.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' || ch == '`' || ch == '"' {
            out.push(ch);
        } else if !out.is_empty() {
            break;
        } else if ch.is_whitespace() {
            continue;
        } else {
            break;
        }
    }
    let cleaned = out.trim_matches(|c| c == '`' || c == '"');
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned.to_string())
    }
}

fn split_shell_segments(cmd: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = cmd.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        // 粗略跳过引号内（不处理转义嵌套，够用）。
        if b == b'\'' || b == b'"' {
            let quote = b;
            i += 1;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            i += 1;
            continue;
        }
        if b == b'#' {
            // 行注释：本段到行末忽略
            let seg = cmd[start..i].trim();
            if !seg.is_empty() {
                out.push(seg);
            }
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            start = i;
            continue;
        }
        let is_sep = match b {
            b';' | b'\n' => true,
            b'|' if bytes.get(i + 1) != Some(&b'|') => true,
            b'|' if bytes.get(i + 1) == Some(&b'|') => {
                let seg = cmd[start..i].trim();
                if !seg.is_empty() {
                    out.push(seg);
                }
                i += 2;
                start = i;
                continue;
            }
            b'&' if bytes.get(i + 1) == Some(&b'&') => {
                let seg = cmd[start..i].trim();
                if !seg.is_empty() {
                    out.push(seg);
                }
                i += 2;
                start = i;
                continue;
            }
            _ => false,
        };
        if is_sep {
            let seg = cmd[start..i].trim();
            if !seg.is_empty() {
                out.push(seg);
            }
            i += 1;
            start = i;
            continue;
        }
        i += 1;
    }
    let seg = cmd[start..].trim();
    if !seg.is_empty() {
        out.push(seg);
    }
    out
}

fn assess_segment(segment: &str) -> Option<DangerHit> {
    let stripped = strip_leading_noise(segment);
    if stripped.is_empty() {
        return None;
    }

    // 重定向覆盖：段内任意 `>` / `>|`，但排除 `>>`。
    if let Some(targets) = redirect_overwrite_targets(&stripped) {
        return Some(DangerHit {
            rule_id: "redirect_overwrite",
            reason_zh: "重定向覆盖会改写目标文件",
            reason_en: "Redirect overwrite will replace the target file",
            targets,
        });
    }

    let tokens = tokenize_args(&stripped);
    if tokens.is_empty() {
        return None;
    }
    let cmd = tokens[0].to_ascii_lowercase();
    let cmd = cmd.as_str();

    match cmd {
        "rm" | "rmdir" => {
            let targets = collect_path_args(&tokens[1..]);
            Some(DangerHit {
                rule_id: "rm",
                reason_zh: "删除命令可能不可恢复",
                reason_en: "Delete command may be irreversible",
                targets,
            })
        }
        "dd" => {
            let mut targets = Vec::new();
            for t in &tokens[1..] {
                if let Some(of) = t.strip_prefix("of=") {
                    if !of.is_empty() {
                        targets.push(of.to_string());
                    }
                }
            }
            Some(DangerHit {
                rule_id: "dd",
                reason_zh: "dd 可能覆盖磁盘或设备",
                reason_en: "dd may overwrite a disk or device",
                targets,
            })
        }
        c if c == "mkfs" || c.starts_with("mkfs.") => {
            let targets = collect_path_args(&tokens[1..]);
            Some(DangerHit {
                rule_id: "mkfs",
                reason_zh: "格式化会清空文件系统",
                reason_en: "Formatting will wipe the filesystem",
                targets,
            })
        }
        "kill" | "killall" => {
            if kill_is_forced(&tokens[1..]) {
                let targets = collect_kill_targets(&tokens[1..]);
                Some(DangerHit {
                    rule_id: "kill_force",
                    reason_zh: "强制杀进程可能中断关键服务",
                    reason_en: "Forced kill may interrupt critical services",
                    targets,
                })
            } else {
                None
            }
        }
        "chmod" | "chown" => {
            if has_recursive_flag(&tokens[1..]) {
                let targets = collect_path_args(&tokens[1..]);
                Some(DangerHit {
                    rule_id: if cmd == "chmod" {
                        "chmod_r"
                    } else {
                        "chown_r"
                    },
                    reason_zh: "递归改权限/属主影响面大",
                    reason_en: "Recursive permission/ownership change has wide impact",
                    targets,
                })
            } else {
                None
            }
        }
        "shutdown" | "reboot" | "halt" | "poweroff" => Some(DangerHit {
            rule_id: "power",
            reason_zh: "关机/重启会影响整机可用性",
            reason_en: "Shutdown/reboot affects whole-host availability",
            targets: Vec::new(),
        }),
        _ => None,
    }
}

fn strip_leading_noise(segment: &str) -> String {
    let mut s = segment.trim().to_string();
    // 去掉前导 env 赋值与 sudo / nice / time 等包装。
    loop {
        let before = s.clone();
        if let Some(rest) = s.strip_prefix("sudo ") {
            let mut rest = rest.trim_start().to_string();
            while rest.starts_with('-') {
                let mut parts = rest.splitn(2, char::is_whitespace);
                let _flag = parts.next();
                rest = parts.next().unwrap_or("").trim_start().to_string();
            }
            s = rest;
            continue;
        }
        let mut stripped_wrap = false;
        for wrap in ["nice ", "time ", "nohup ", "command ", "env "] {
            if let Some(rest) = s.strip_prefix(wrap) {
                s = rest.trim_start().to_string();
                stripped_wrap = true;
                break;
            }
        }
        if stripped_wrap {
            continue;
        }
        // FOO=bar BAZ=1 cmd …
        let mut changed = false;
        while let Some((key, rest)) = s.split_once('=') {
            let key = key.trim();
            if key.is_empty()
                || key.contains(' ')
                || key.contains('/')
                || !key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                break;
            }
            let rest = rest.trim_start();
            if rest.starts_with('\'') || rest.starts_with('"') {
                break;
            }
            if let Some((_val, after)) = rest.split_once(char::is_whitespace) {
                s = after.trim_start().to_string();
                changed = true;
            } else {
                s.clear();
                changed = true;
                break;
            }
        }
        if !changed && s == before {
            break;
        }
        if !changed {
            break;
        }
    }
    s
}

fn tokenize_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\'' => {
                while let Some(c) = chars.next() {
                    if c == '\'' {
                        break;
                    }
                    cur.push(c);
                }
            }
            '"' => {
                while let Some(c) = chars.next() {
                    if c == '"' {
                        break;
                    }
                    if c == '\\' {
                        if let Some(n) = chars.next() {
                            cur.push(n);
                        }
                    } else {
                        cur.push(c);
                    }
                }
            }
            c if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn collect_path_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip_next = false;
    for a in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if a.starts_with('-') {
            // chmod 模式、kill 信号已在别处处理；这里跳过纯短选项，吃掉带参长选项粗略处理
            if a == "--" {
                continue;
            }
            // -u user / --reference=file 等：简单跳过下一参数若选项不像 cluster
            if a.contains('=') {
                continue;
            }
            // 单字母簇如 -rf 不带值
            if a.chars().skip(1).all(|c| c.is_ascii_alphanumeric()) && a.len() <= 5 {
                continue;
            }
            skip_next = true;
            continue;
        }
        out.push(a.clone());
    }
    out
}

fn has_recursive_flag(args: &[String]) -> bool {
    for a in args {
        if a == "--recursive" || a == "-R" || a == "-r" {
            return true;
        }
        if a.starts_with('-') && !a.starts_with("--") {
            // -Rf / -fR 等
            if a.contains('R') || a.contains('r') {
                // 排除 chmod 模式像 -rwx（含字母但不是选项簇？chmod 模式不以 -R 单独出现时常是 -rwx）
                // 保守：仅当选项串全是短选项字母且含 R/r
                let body = &a[1..];
                if body.chars().all(|c| c.is_ascii_alphabetic()) && (body.contains('R') || body.contains('r'))
                {
                    // chmod -rwxr-xr-x 含 r 但那是模式；chmod 模式通常更长且含 -
                    if body.contains('-') {
                        continue;
                    }
                    // -rwx 三字符权限写法：含 r/w/x 但不是 recursive。递归一般是单独的 R 或与 f 组合 -Rf
                    if body.chars().all(|c| matches!(c, 'r' | 'w' | 'x' | 's' | 't')) {
                        continue;
                    }
                    return true;
                }
            }
        }
    }
    false
}

fn kill_is_forced(args: &[String]) -> bool {
    for a in args {
        let lower = a.to_ascii_lowercase();
        if lower == "-9" || lower == "-kill" || lower == "--signal=kill" || lower == "--signal=9" {
            return true;
        }
        if lower.starts_with("-s") && args.windows(2).any(|w| {
            w[0].eq_ignore_ascii_case("-s")
                && (w[1].eq_ignore_ascii_case("kill") || w[1] == "9")
        }) {
            return true;
        }
        if lower == "-s" {
            // handled by windows check
        }
    }
    // `kill -s KILL pid`
    let mut i = 0;
    while i + 1 < args.len() {
        if args[i].eq_ignore_ascii_case("-s")
            && (args[i + 1].eq_ignore_ascii_case("kill") || args[i + 1] == "9")
        {
            return true;
        }
        i += 1;
    }
    false
}

fn collect_kill_targets(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "-s" || a.eq_ignore_ascii_case("--signal") {
            i += 2;
            continue;
        }
        if a.starts_with('-') {
            i += 1;
            continue;
        }
        out.push(a.clone());
        i += 1;
    }
    out
}

fn first_shell_arg(rest: &str) -> Option<String> {
    let tokens = tokenize_args(rest);
    tokens.into_iter().find(|t| !t.starts_with('-'))
}

/// 找出覆盖重定向的目标路径；若无覆盖重定向返回 None。
fn redirect_overwrite_targets(segment: &str) -> Option<Vec<String>> {
    let bytes = segment.as_bytes();
    let mut targets = Vec::new();
    let mut i = 0;
    let mut in_sq = false;
    let mut in_dq = false;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\'' && !in_dq {
            in_sq = !in_sq;
            i += 1;
            continue;
        }
        if b == b'"' && !in_sq {
            in_dq = !in_dq;
            i += 1;
            continue;
        }
        if in_sq || in_dq {
            i += 1;
            continue;
        }
        if b == b'>' {
            // >> 追加：跳过
            if bytes.get(i + 1) == Some(&b'>') {
                i += 2;
                continue;
            }
            // >| 强制覆盖，或单独 >
            let mut j = i + 1;
            if bytes.get(j) == Some(&b'|') {
                j += 1;
            }
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            let start = j;
            while j < bytes.len()
                && !bytes[j].is_ascii_whitespace()
                && bytes[j] != b';'
                && bytes[j] != b'|'
                && bytes[j] != b'&'
                && bytes[j] != b'<'
                && bytes[j] != b'>'
            {
                j += 1;
            }
            if j > start {
                let t = &segment[start..j];
                // /dev/null 等空设备覆盖通常无害，但仍属写；产品规则未豁免，列入目标即可。
                targets.push(t.to_string());
            } else {
                targets.push(String::new());
            }
            i = j;
            continue;
        }
        i += 1;
    }
    if targets.is_empty() {
        None
    } else {
        // 过滤空占位但保留至少一次命中
        let cleaned: Vec<String> = targets.into_iter().filter(|t| !t.is_empty()).collect();
        Some(if cleaned.is_empty() {
            Vec::new()
        } else {
            cleaned
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(cmd: &str) -> Option<&'static str> {
        assess_ai_terminal_danger(cmd).map(|h| h.rule_id)
    }

    #[test]
    fn readonly_commands_are_safe() {
        for cmd in [
            "ls -la",
            "df -h",
            "free -m",
            "ps aux",
            "top -bn1",
            "cat /var/log/syslog",
            "grep error /var/log/app.log",
            "echo hello",
            "pwd",
            "whoami",
            "uname -a",
        ] {
            assert!(assess_ai_terminal_danger(cmd).is_none(), "{cmd}");
        }
    }

    #[test]
    fn rm_any_form_is_dangerous() {
        for cmd in [
            "rm file.txt",
            "rm -rf /tmp/x",
            "rm -r /var/cache/old",
            "rm -v a b c",
            "sudo rm -rf /opt/app",
        ] {
            let hit = assess_ai_terminal_danger(cmd).expect(cmd);
            assert_eq!(hit.rule_id, "rm", "{cmd}");
        }
        let hit = assess_ai_terminal_danger("rm -rf /tmp/a /tmp/b").unwrap();
        assert!(hit.targets.iter().any(|t| t.contains("/tmp/a")));
        assert!(hit.targets.iter().any(|t| t.contains("/tmp/b")));
    }

    #[test]
    fn dd_mkfs_power_kill_chmod() {
        assert_eq!(ids("dd if=/dev/zero of=/dev/sda bs=1M"), Some("dd"));
        let dd = assess_ai_terminal_danger("dd if=/dev/zero of=/dev/sda").unwrap();
        assert_eq!(dd.targets, vec!["/dev/sda"]);

        assert_eq!(ids("mkfs.ext4 /dev/sdb1"), Some("mkfs"));
        assert_eq!(ids("mkfs -t xfs /dev/sdc"), Some("mkfs"));

        assert_eq!(ids("kill -9 1234"), Some("kill_force"));
        assert_eq!(ids("kill -KILL 99"), Some("kill_force"));
        assert_eq!(ids("kill -s KILL 99"), Some("kill_force"));
        assert!(assess_ai_terminal_danger("kill -15 1").is_none());

        assert_eq!(ids("chmod -R 777 /var/www"), Some("chmod_r"));
        assert_eq!(ids("chown -R www-data:www-data /var/www"), Some("chown_r"));
        assert!(assess_ai_terminal_danger("chmod 644 file.txt").is_none());

        for cmd in ["shutdown -h now", "reboot", "halt", "poweroff"] {
            assert_eq!(ids(cmd), Some("power"), "{cmd}");
        }
    }

    #[test]
    fn redirect_overwrite_but_not_append() {
        let hit = assess_ai_terminal_danger("echo hi > /tmp/out.txt").unwrap();
        assert_eq!(hit.rule_id, "redirect_overwrite");
        assert!(hit.targets.iter().any(|t| t.contains("/tmp/out.txt")));

        let hit2 = assess_ai_terminal_danger("cat a >| /tmp/forced").unwrap();
        assert_eq!(hit2.rule_id, "redirect_overwrite");

        assert!(assess_ai_terminal_danger("echo hi >> /tmp/out.txt").is_none());
        // 只读管道里的 grep 不写文件
        assert!(assess_ai_terminal_danger("ps aux | grep nginx").is_none());
    }

    #[test]
    fn sql_drop_and_truncate_case_insensitive() {
        let hit = assess_ai_terminal_danger("DROP TABLE users;").unwrap();
        assert_eq!(hit.rule_id, "drop_table");
        assert_eq!(hit.targets, vec!["users"]);

        let hit2 = assess_ai_terminal_danger("truncate table `orders`").unwrap();
        assert_eq!(hit2.rule_id, "truncate");
        assert!(hit2.targets.iter().any(|t| t.contains("orders")));
    }

    #[test]
    fn pipelines_and_comments() {
        assert_eq!(
            ids("ls /tmp | head; rm -rf /tmp/old"),
            Some("rm")
        );
        // 注释里的危险字不触发
        assert!(assess_ai_terminal_danger("df -h   # rm -rf / 只是注释").is_none());
        // grep 字面量
        assert!(assess_ai_terminal_danger("grep 'rm -rf' /var/log/a.log").is_none());
    }

    #[test]
    fn rule_table_ids_are_stable() {
        // 与文件头注释中的规则表对齐，防止无意改名。
        let expected = [
            ("rm -rf /x", "rm"),
            ("dd of=/dev/sda if=/dev/zero", "dd"),
            ("mkfs.xfs /dev/sdb", "mkfs"),
            ("kill -9 1", "kill_force"),
            ("chmod -R 700 /data", "chmod_r"),
            ("chown -R root /data", "chown_r"),
            ("echo x > /tmp/y", "redirect_overwrite"),
            ("reboot", "power"),
            ("drop table t", "drop_table"),
            ("TRUNCATE old_logs", "truncate"),
        ];
        for (cmd, id) in expected {
            assert_eq!(ids(cmd), Some(id), "{cmd}");
        }
    }
}
