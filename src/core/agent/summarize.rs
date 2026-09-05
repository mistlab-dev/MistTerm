//! 将批量执行结果整理成对话可读摘要（纯文本，供复制；UI 另有结构化卡片）。

use crate::core::batch_exec::BatchExecRow;

/// 短摘要（复制/持久化）；不含 HTML，不做 markdown 表。
pub fn summarize_batch_rows(command: &str, rows: &[BatchExecRow]) -> String {
    let ok_n = rows.iter().filter(|r| r.ok).count();
    let fail_n = rows.len().saturating_sub(ok_n);
    let mut out = String::new();
    out.push_str(&format!(
        "多机执行 · `{command}`\n成功 {ok_n} / 共 {} · 失败 {fail_n}\n\n",
        rows.len()
    ));

    // 失败优先，一眼能看出问题机
    let mut ordered: Vec<&BatchExecRow> = rows.iter().collect();
    ordered.sort_by_key(|r| r.ok);

    for r in ordered {
        let mark = if r.ok { "OK" } else { "FAIL" };
        let detail = if r.ok {
            first_useful_line(&r.output).unwrap_or_else(|| {
                if r.output.trim().is_empty() {
                    "—".into()
                } else {
                    truncate_chars(&r.output.replace('\n', " "), 72)
                }
            })
        } else {
            r.error
                .clone()
                .unwrap_or_else(|| "failed".into())
        };
        out.push_str(&format!(
            "[{mark}] {}  ·  {detail}  ({} ms)\n",
            r.label, r.duration_ms
        ));
    }
    out
}

pub fn first_useful_line(output: &str) -> Option<String> {
    for line in output.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let low = t.to_lowercase();
        if low.starts_with("filesystem") || low.starts_with("total") {
            continue;
        }
        return Some(truncate_chars(t, 96));
    }
    None
}

pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max).collect();
    format!("{head}…")
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
        assert!(s.contains("[OK]"));
        assert!(!s.contains("<details>"));
    }

    #[test]
    fn fails_listed_first() {
        let rows = vec![
            BatchExecRow {
                target_id: "a".into(),
                label: "ok-host".into(),
                ok: true,
                exit_code: Some(0),
                output: "hello\n".into(),
                error: None,
                duration_ms: 1,
            },
            BatchExecRow {
                target_id: "b".into(),
                label: "bad-host".into(),
                ok: false,
                exit_code: None,
                output: String::new(),
                error: Some("timeout".into()),
                duration_ms: 2,
            },
        ];
        let s = summarize_batch_rows("uptime", &rows);
        let bad = s.find("[FAIL]").unwrap();
        let good = s.find("[OK]").unwrap();
        assert!(bad < good);
    }
}
