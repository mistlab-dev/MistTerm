//! 结构化命令执行历史（JSONL），供 CLI / GUI / AI 失败上下文共用。

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use crate::core::batch_exec::BatchExecRow;

const OUTPUT_SUMMARY_MAX_CHARS: usize = 4096;

/// 单条结构化执行记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecLogRecord {
    pub id: String,
    pub session_id: String,
    pub timestamp: String,
    pub target: String,
    pub command: String,
    pub exit_code: Option<i32>,
    pub ok: bool,
    pub duration_ms: u64,
    pub output_summary: String,
    pub tags: Vec<String>,
}

impl ExecLogRecord {
    pub fn new(
        session_id: &str,
        target: &str,
        command: &str,
        exit_code: Option<i32>,
        ok: bool,
        duration_ms: u64,
        output: &str,
        tag: &str,
    ) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        let id = format!(
            "{}-{}",
            chrono::Utc::now().timestamp_millis(),
            &target.replace(['@', ':', '.'], "-")
        );

        Self {
            id,
            session_id: session_id.to_string(),
            timestamp: now,
            target: target.to_string(),
            command: command.to_string(),
            exit_code,
            ok,
            duration_ms,
            output_summary: truncate_output_summary(output),
            tags: vec![tag.to_string()],
        }
    }
}

/// 按 Unicode 字符截断输出摘要，避免切在 UTF-8 码点中间。
pub fn truncate_output_summary(output: &str) -> String {
    let count = output.chars().count();
    if count <= OUTPUT_SUMMARY_MAX_CHARS {
        return output.to_string();
    }
    let head: String = output.chars().take(OUTPUT_SUMMARY_MAX_CHARS).collect();
    format!("{head}\n... [truncated]")
}

/// 日志路径：`~/.mist/logs/exec-history.jsonl`
pub fn get_log_file_path() -> PathBuf {
    let base_dir = dirs::home_dir()
        .map(|h| h.join(".mist").join("logs"))
        .unwrap_or_else(|| PathBuf::from(".mist").join("logs"));
    base_dir.join("exec-history.jsonl")
}

/// 追加写入（静默降级，不阻断主流程）
pub fn append_record(record: &ExecLogRecord) {
    let path = get_log_file_path();
    if let Some(parent) = path.parent() {
        let _ = create_dir_all(parent);
    }

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        if let Ok(json_line) = serde_json::to_string(record) {
            let _ = writeln!(file, "{}", json_line);
        }
    }
}

/// 将批量执行结果写入结构化日志
pub fn record_batch_rows(command: &str, rows: &[BatchExecRow], tag: &str) {
    let session_batch_id = format!("batch-{}", chrono::Utc::now().timestamp_millis());
    for r in rows {
        let record = ExecLogRecord::new(
            &session_batch_id,
            &r.label,
            command,
            r.exit_code,
            r.ok,
            r.duration_ms,
            &r.output,
            tag,
        );
        append_record(&record);
    }
}

/// 读取最近的 N 条记录
pub fn read_recent_records(limit: usize) -> Result<Vec<ExecLogRecord>> {
    let path = get_log_file_path();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(path)?;
    let mut records: Vec<ExecLogRecord> = content
        .lines()
        .filter_map(|line| serde_json::from_str::<ExecLogRecord>(line).ok())
        .collect();

    if records.len() > limit {
        let skip = records.len() - limit;
        records = records.split_off(skip);
    }
    Ok(records)
}

/// 根据记录生成 Markdown 排障 SOP 草稿
pub fn extract_sop_markdown(records: &[ExecLogRecord], title: Option<&str>) -> String {
    let title = title.unwrap_or("排错排查 SOP 草稿");
    let mut md = String::new();
    md.push_str(&format!("# {}\n\n", title));
    md.push_str("> 生成自 MistTerm 结构化执行日志，保留关键命令与输出证据。\n\n");
    md.push_str("## 1. 操作概要\n\n");
    md.push_str("| 目标 | 命令 | 状态码 | 耗时 |\n");
    md.push_str("|---|---|---|---|\n");

    for r in records {
        let status = if r.ok { "✅ 成功" } else { "❌ 失败" };
        let code = r
            .exit_code
            .map(|c| c.to_string())
            .unwrap_or_else(|| "-".to_string());
        md.push_str(&format!(
            "| `{}` | `{}` | {} ({}) | {}ms |\n",
            r.target, r.command, status, code, r.duration_ms
        ));
    }

    md.push_str("\n## 2. 步骤详情与执行回显\n\n");
    for (i, r) in records.iter().enumerate() {
        let icon = if r.ok { "✅" } else { "⚠️" };
        md.push_str(&format!(
            "### 步骤 {}：{} `{}`\n\n",
            i + 1,
            icon,
            r.command
        ));
        md.push_str(&format!("- **执行目标**：`{}`\n", r.target));
        md.push_str(&format!("- **执行时间**：{}\n", r.timestamp));
        md.push_str(&format!("- **耗时**：{}ms\n\n", r.duration_ms));
        if !r.output_summary.trim().is_empty() {
            md.push_str("```bash\n");
            md.push_str(r.output_summary.trim());
            md.push_str("\n```\n\n");
        }
    }

    md.push_str("## 3. 结论与复盘\n\n- 直接原因：\n- 处置方案：\n- 改进建议：\n");
    md
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_respects_utf8_char_boundary() {
        // 每个汉字 3 字节；按字节切 4096 会 panic，按字符则安全。
        let s: String = "测".repeat(5000);
        let out = truncate_output_summary(&s);
        assert!(out.ends_with("\n... [truncated]"));
        assert!(out.is_char_boundary(out.find('\n').unwrap()));
        assert_eq!(out.chars().count(), OUTPUT_SUMMARY_MAX_CHARS + "\n... [truncated]".chars().count());
    }

    #[test]
    fn truncate_short_passthrough() {
        assert_eq!(truncate_output_summary("hello"), "hello");
    }
}
