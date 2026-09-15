//! `mist` — MistTerm 命令行入口（P1：ls / exec / get / put / rls）。

use clap::{Parser, Subcommand};
use mistterm::cli::{exec, ls, sftp_cmds, CliContext};

#[derive(Parser)]
#[command(name = "mist", version, about = "MistTerm CLI — 复用 GUI 会话配置的命令行 SSH 工具")]
struct Cli {
    /// 输出 JSON（ls/exec/rls 支持）
    #[arg(long, global = true)]
    json: bool,

    /// 提高日志详细度（-v info，-vv debug）
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 列出已保存会话
    Ls {
        /// 按分组过滤
        #[arg(long)]
        group: Option<String>,
    },

    /// 在目标上执行命令
    Exec {
        /// 目标：会话名/id/host，或 user@host[:port]
        target: Option<String>,

        /// 要执行的命令
        #[arg(trailing_var_arg = true)]
        command: Vec<String>,

        /// 按分组批量执行
        #[arg(long, conflicts_with = "all_targets")]
        group: Option<String>,

        /// 对所有已保存会话批量执行
        #[arg(long = "all")]
        all_targets: bool,

        /// 串行执行，遇失败熔断（默认并行）
        #[arg(long)]
        serial: bool,

        /// 并行度（1-16，默认 8）
        #[arg(long, default_value_t = 8)]
        parallel: usize,
    },

    /// 列出远端目录：mist rls <target>:<path>
    Rls {
        /// <target>:<remote-path>
        spec: String,
    },

    /// 下载文件：mist get <target>:<remote> <local>
    Get {
        /// <target>:<remote-path>
        spec: String,
        /// 本地路径（可为目录）
        local: String,
    },

    /// 上传文件：mist put <local> <target>:<remote>
    Put {
        /// 本地文件
        local: String,
        /// <target>:<remote-path>（以 / 结尾则保留文件名）
        spec: String,
    },
}

fn init_logging(verbose: u8) {
    let level = match verbose {
        0 => tracing::Level::WARN,
        1 => tracing::Level::INFO,
        _ => tracing::Level::DEBUG,
    };
    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_writer(std::io::stderr)
        .init();
}

fn main() {
    let cli = Cli::parse();
    init_logging(cli.verbose);

    let mut ctx = CliContext::load();
    let code = match &cli.cmd {
        Cmd::Ls { group } => ls::run(&ctx, group.as_deref(), cli.json).map(|_| 0),
        Cmd::Exec {
            target,
            command,
            group,
            all_targets,
            serial,
            parallel,
            ..
        } => {
            let cmd_str = command.join(" ");
            if *all_targets || group.is_some() {
                exec::run_batch(
                    &mut ctx,
                    group.as_deref(),
                    *all_targets,
                    *serial,
                    *parallel,
                    &cmd_str,
                    cli.json,
                )
            } else {
                let cmd_str = command.join(" ");
                if cmd_str.is_empty() {
                    eprintln!("错误: 缺少要执行的命令");
                    std::process::exit(1);
                }
                let t = target.as_deref().unwrap_or_else(|| {
                    eprintln!("错误: 缺少 target（或使用 --group/--all）");
                    std::process::exit(1);
                });
                exec::run_single(&mut ctx, t, &cmd_str, cli.json)
            }
        }
        Cmd::Rls { spec } => sftp_cmds::run_rls(&mut ctx, spec, cli.json),
        Cmd::Get { spec, local } => sftp_cmds::run_get(&mut ctx, spec, local),
        Cmd::Put { local, spec } => sftp_cmds::run_put(&mut ctx, local, spec),
    };

    let exit = match code {
        Ok(c) => c,
        Err(e) => {
            eprintln!("mist: {e:#}");
            2
        }
    };
    std::process::exit(exit);
}
