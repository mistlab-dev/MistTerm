//! 会话与命令执行结构化日志（P3：AI Ops 数据供给）。
//!
//! 统一数据契约（JSONL）：记录命令、退出码、耗时、时序、目标主机与标准输出摘要，
//! 为 AI 提炼排错 SOP / 团队知识提供结构化原材料。

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

/// 单条结构化执行记录契约
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecLogRecord {
    /// 唯一标识（UUID 或时间戳前缀）
    pub id: String,
    /// 会话或批次 ID
    pub session_id: String,
    /// 时间戳（ISO8601 格式或毫秒）
    pub timestamp: String,
    /// 目标主机标识（如 user@host:port 或 session_name）
    pub target: String,
    /// 完整执行命令
    pub command: String,
    /// 退出状态码（0 表示成功）
    pub exit_code: Option<i32>,
    /// 是否执行成功
    pub ok: bool,
    /// 执行耗时（毫秒）
    pub duration_ms: u64,
    /// 标准输出与错误摘要（截取前 4KB，避免文件无限膨胀）
    pub output_summary: String,
    /// 标签分类（如 "single", "batch", "interactive", "sop_candidate"）
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
        let id = format!("{}-{}", chrono::Utc::now().timestamp_millis(), &target.replace(['@', ':', '.'], "-"));

        // 截取最多 4096 字符的输出摘要，清洗过长的输出
        let output_summary = if output.len() > 4096 {
            let mut s = output[..4096].to_string();
            s.push_str("\n... [truncated]");
            s
        } else {
            output.to_string()
        };

        Self {
            id,
            session_id: session_id.to_string(),
            timestamp: now,
            target: target.to_string(),
            command: command.to_string(),
            exit_code,
            ok,
            duration_ms,
            output_summary,
            tags: vec![tag.to_string()],
        }
    }
}

/// 获取日志落盘路径：~/.mist/logs/exec-history.jsonl
pub fn get_log_file_path() -> PathBuf {
    let base_dir = dirs::home_dir()
        .map(|h| h.join(".mist").join("logs"))
        .unwrap_or_else(|| PathBuf::from(".mist").join("logs"));
    base_dir.join("exec-history.jsonl")
}

/// 追加写入结构化日志（静默降级，不阻断主流程）
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

/// 根据记录生成 Markdown 格式的排障 SOP 草稿
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
        let code = r.exit_code.map(|c| c.to_string()).unwrap_or_else(|| "-".to_string());
        md.push_str(&format!(
            "| `{}` | `{}` | {} ({}) | {}ms |\n",
            r.target, r.command, status, code, r.duration_ms
        ));
    }

    md.push_str("\n## 2. 步骤详情与执行回显\n\n");
    for (i, r) in records.iter().enumerate() {
        let icon = if r.ok { "✅" } else { "⚠️" };
        md.push_str(&format!("### 步骤 {}：{} `{}`\n\n", i + 1, icon, r.command));
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
