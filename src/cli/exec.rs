//! `mist exec` — 单机 / 批量远程执行。

use anyhow::Result;
use crate::core::batch_exec::{
    run_batch_parallel, run_batch_serial_fail_fast, BatchExecJob, BatchExecRow,
};
use crate::core::session::SessionConfig;
use crate::ssh::SshClient;
use serde::Serialize;

use super::context::record_history;
use super::CliContext;

#[derive(Serialize)]
struct ExecJsonRow {
    target: String,
    ok: bool,
    exit_code: Option<i32>,
    output: String,
    error: Option<String>,
    duration_ms: u64,
}

fn row_to_json(r: &BatchExecRow) -> ExecJsonRow {
    ExecJsonRow {
        target: r.label.clone(),
        ok: r.ok,
        exit_code: r.exit_code,
        output: r.output.clone(),
        error: r.error.clone(),
        duration_ms: r.duration_ms,
    }
}

fn to_job(ctx: &CliContext, s: &SessionConfig) -> Result<BatchExecJob> {
    let config = ctx.ssh_config(s)?;
    Ok(BatchExecJob {
        target_id: s.id.clone(),
        label: format!("{}@{}:{}", s.username, s.host, s.port),
        config,
    })
}

fn print_rows(rows: &[BatchExecRow], json: bool) {
    if json {
        let v: Vec<ExecJsonRow> = rows.iter().map(row_to_json).collect();
        println!("{}", serde_json::to_string_pretty(&v).unwrap());
        return;
    }
    let multi = rows.len() > 1;
    for r in rows {
        if multi {
            println!("===== {} ({}ms) =====", r.label, r.duration_ms);
        }
        if !r.output.is_empty() {
            print!("{}", r.output);
            if !r.output.ends_with('\n') {
                println!();
            }
        }
        if let Some(e) = &r.error {
            eprintln!("[{}] {}", r.label, e);
        }
        if multi && !r.ok {
            println!("----- exit: {:?} -----", r.exit_code);
        }
    }
}

fn worst_exit_code(rows: &[BatchExecRow]) -> i32 {
    rows.iter()
        .filter_map(|r| r.exit_code)
        .filter(|c| *c != 0)
        .max()
        .unwrap_or(if rows.iter().all(|r| r.ok) { 0 } else { 1 })
}

/// 单机 exec：返回进程退出码。
pub fn run_single(
    ctx: &mut CliContext,
    target: &str,
    command: &str,
    json: bool,
) -> Result<i32> {
    let session = ctx.resolve_target(target)?;
    let config = ctx.ssh_config(&session)?;
    let label = format!("{}@{}:{}", session.username, session.host, session.port);

    let mut client = SshClient::new(config);
    client
        .connect()
        .map_err(|e| anyhow::anyhow!("连接失败 {label}: {e}"))?;
    ctx.mark_connected(&session);

    let result = client.exec_command(command);
    client.disconnect();

    let (output, code) = match result {
        Ok((out, c)) => (out, c),
        Err(e) => {
            record_history(command, Some(&session.id), Some(&session.name), false);
            anyhow::bail!("exec 失败 {label}: {e}");
        }
    };
    let ok = code == 0;
    record_history(command, Some(&session.id), Some(&session.name), ok);

    if json {
        let row = ExecJsonRow {
            target: label,
            ok,
            exit_code: Some(code),
            output,
            error: if ok {
                None
            } else {
                Some(format!("exit code {code}"))
            },
            duration_ms: 0,
        };
        println!("{}", serde_json::to_string_pretty(&row)?);
    } else {
        print!("{output}");
        if !output.ends_with('\n') && !output.is_empty() {
            println!();
        }
    }
    Ok(code)
}

/// 批量 exec（--group / --all）。
pub fn run_batch(
    ctx: &mut CliContext,
    group: Option<&str>,
    all: bool,
    serial: bool,
    parallel: usize,
    command: &str,
    json: bool,
) -> Result<i32> {
    let targets: Vec<SessionConfig> = ctx
        .sessions
        .get_sessions()
        .iter()
        .filter(|s| all || group.map(|g| s.group == g).unwrap_or(false))
        .cloned()
        .collect();

    if targets.is_empty() {
        anyhow::bail!("没有匹配的目标会话");
    }

    let mut jobs = Vec::with_capacity(targets.len());
    for s in &targets {
        match to_job(ctx, s) {
            Ok(j) => jobs.push(j),
            Err(e) => {
                eprintln!("跳过 {}: {e}", s.name);
            }
        }
    }
    if jobs.is_empty() {
        anyhow::bail!("所有目标都无法构造连接配置");
    }

    let rows = if serial {
        run_batch_serial_fail_fast(jobs, command.to_string())
    } else {
        run_batch_parallel(jobs, command.to_string(), parallel)
    };

    // 记录历史：每台一条（成功与否都记）
    for (s, r) in targets.iter().zip(rows.iter()) {
        record_history(command, Some(&s.id), Some(&s.name), r.ok);
        if r.ok {
            ctx.mark_connected(s);
        }
    }

    print_rows(&rows, json);
    Ok(worst_exit_code(&rows))
}
