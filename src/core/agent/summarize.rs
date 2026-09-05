//! 将批量执行结果整理成对话可读摘要（不绑死单一指标）。

use crate::core::batch_exec::BatchExecRow;

pub fn summarize_batch_rows(command: &str, rows: &[BatchExecRow]) -> String {
    let ok_n = rows.iter().filter(|r| r.ok).count();
    let fail_n = rows.len().saturating_sub(ok_n);
    let mut out = String::new();
    out.push_str(&format!(
        "### 多机执行结果\n\n命令：`{command}`\n\n成功 {ok_n} / 共 {} · 失败 {fail_n}\n\n",
        rows.len()
    ));
    out.push_str("| 主机 | 状态 | 摘要 |\n|------|------|------|\n");
    for r in rows {
        let status = if r.ok {
            "OK".to_string()
        } else if let Some(e) = &r.error {
            format!("ERR: {e}")
        } else {
            "ERR".into()
        };
        let summary = first_useful_line(&r.output).unwrap_or_else(|| {
            if r.output.is_empty() {
                "—".into()
            } else {
                truncate_chars(&r.output.replace('\n', " "), 80)
            }
        });
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            escape_cell(&r.label),
            escape_cell(&status),
            escape_cell(&summary)
        ));
    }
    if fail_n > 0 {
        out.push_str("\n<details><summary>失败详情</summary>\n\n");
        for r in rows.iter().filter(|r| !r.ok) {
            out.push_str(&format!(
                "**{}**\n```\n{}\n```\n\n",
                r.label,
                r.error.as_deref().unwrap_or("(no error text)")
            ));
        }
        out.push_str("</details>\n");
    }
    out.push_str("\n<details><summary>按主机原始输出</summary>\n\n");
    for r in rows {
        out.push_str(&format!("**{}** ({} ms)\n```\n", r.label, r.duration_ms));
        if r.output.is_empty() {
            out.push_str("(empty)\n");
        } else {
            out.push_str(&truncate_chars(&r.output, 4000));
            if !r.output.ends_with('\n') {
                out.push('\n');
            }
        }
        out.push_str("```\n\n");
    }
    out.push_str("</details>\n");
    out
}

fn first_useful_line(output: &str) -> Option<String> {
    for line in output.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t.to_lowercase().starts_with("filesystem") || t.starts_with("total") {
            continue;
        }
        return Some(truncate_chars(t, 80));
    }
    None
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max).collect();
    format!("{head}…")
}

fn escape_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::batch_exec::BatchExecRow;

    #[test]
    fn summarizes_ok_row() {
        let rows = vec![BatchExecRow {
            target_id: "a".into(),
            label: "web-1".into(),
            ok: true,
            exit_code: Some(0),
            output: "Filesystem Size Used\n/dev/sda1 100G 40G\n".into(),
            error: None,
            duration_ms: 12,
        }];
        let s = summarize_batch_rows("df -h", &rows);
        assert!(s.contains("web-1"));
        assert!(s.contains("成功 1"));
    }
}
