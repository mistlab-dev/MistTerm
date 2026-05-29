//! 批量 SSH 执行（独立连接 + exec，不占用终端 Tab）。

pub const TEAM_TARGET_PREFIX: &str = "team:";

use std::thread;
use std::time::Instant;

use crate::ssh::{SshClient, SshConfig};

pub const MAX_OUTPUT_CHARS: usize = 24_000;

#[derive(Debug, Clone)]
pub struct BatchExecJob {
    pub target_id: String,
    pub label: String,
    pub config: SshConfig,
}

#[derive(Debug, Clone)]
pub struct BatchExecRow {
    pub target_id: String,
    pub label: String,
    pub ok: bool,
    pub exit_code: Option<i32>,
    pub output: String,
    pub error: Option<String>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone)]
pub struct BatchTarget {
    pub id: String,
    pub label: String,
    pub group: String,
}

pub fn truncate_output(s: String) -> String {
    if s.chars().count() <= MAX_OUTPUT_CHARS {
        return s;
    }
    let head: String = s.chars().take(MAX_OUTPUT_CHARS).collect();
    format!("{head}\n…")
}

fn run_one(job: BatchExecJob, command: &str) -> BatchExecRow {
    let start = Instant::now();
    let mut client = SshClient::new(job.config);
    let connect = client.connect();
    let (ok, exit_code, output, error) = match connect {
        Err(e) => (false, None, String::new(), Some(e)),
        Ok(()) => match client.exec_command(command) {
            Ok((out, code)) => (
                code == 0,
                Some(code),
                truncate_output(out),
                if code == 0 {
                    None
                } else {
                    Some(format!("exit code {code}"))
                },
            ),
            Err(e) => (false, None, String::new(), Some(e)),
        },
    };
    client.disconnect();
    BatchExecRow {
        target_id: job.target_id,
        label: job.label,
        ok,
        exit_code,
        output,
        error,
        duration_ms: start.elapsed().as_millis() as u64,
    }
}

/// 按 `max_parallel` 分批并行执行；在后台线程调用。
pub fn run_batch_parallel(
    jobs: Vec<BatchExecJob>,
    command: String,
    max_parallel: usize,
) -> Vec<BatchExecRow> {
    if jobs.is_empty() {
        return Vec::new();
    }
    let parallel = max_parallel.clamp(1, 16);
    let mut all = Vec::with_capacity(jobs.len());
    for chunk in jobs.chunks(parallel) {
        let handles: Vec<_> = chunk
            .iter()
            .map(|job| {
                let job = job.clone();
                let cmd = command.clone();
                thread::spawn(move || run_one(job, &cmd))
            })
            .collect();
        for h in handles {
            if let Ok(row) = h.join() {
                all.push(row);
            }
        }
    }
    all
}

pub fn format_batch_results_for_clipboard(rows: &[BatchExecRow]) -> String {
    rows.iter()
        .map(|r| {
            let mut block = format!("=== {} ===\n", r.label);
            if let Some(c) = r.exit_code {
                block.push_str(&format!("exit: {c}\n"));
            }
            if let Some(e) = &r.error {
                block.push_str(&format!("error: {e}\n"));
            }
            if !r.output.is_empty() {
                block.push_str(&r.output);
                if !r.output.ends_with('\n') {
                    block.push('\n');
                }
            }
            block
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_long_output() {
        let s = "x".repeat(MAX_OUTPUT_CHARS + 10);
        let t = truncate_output(s);
        assert!(t.contains('…'));
        assert!(t.chars().count() <= MAX_OUTPUT_CHARS + 4);
    }
}
