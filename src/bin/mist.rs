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

    /// 交互式连接并打开 Shell：mist ssh <target>
    Ssh {
        /// 目标：会话名/id/host，或 user@host[:port]
        target: String,
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

    /// 端口转发：启动指定规则或会话中已配置的转发规则（前台运行，Ctrl+C 停止）
    Fwd {
        /// 目标：会话名/id/host，或 user@host[:port]
        target: String,

        /// 本地端口转发：[bind_addr:]local_port:remote_host:remote_port，可指定多个
        #[arg(short = 'L', long = "local")]
        locals: Vec<String>,

        /// 远程端口转发：[bind_addr:]remote_port:target_host:target_port，可指定多个
        #[arg(short = 'R', long = "remote")]
        remotes: Vec<String>,

        /// 动态 SOCKS5 转发：[bind_addr:]local_port，可指定多个
        #[arg(short = 'D', long = "dynamic")]
        dynamics: Vec<String>,
    },

    /// 命令片段管理与执行
    Frag {
        #[command(subcommand)]
        sub: FragCmd,
    },

    /// 从 OpenSSH ~/.ssh/config 导入会话
    ImportSshConfig {
        /// 自定义 ssh config 文件路径（默认 ~/.ssh/config）
        #[arg(short, long)]
        file: Option<std::path::PathBuf>,

        /// 仅打印将导入的会话，不写入存储
        #[arg(long)]
        dry_run: bool,

        /// 覆盖已存在的同名会话（默认跳过）
        #[arg(long)]
        overwrite: bool,
    },

    /// 查看与提炼排错 SOP（P3：会话结构化日志）
    Sop {
        #[command(subcommand)]
        sub: SopCmd,
    },
}

#[derive(Subcommand)]
enum SopCmd {
    /// 导出最近执行记录为 Markdown 格式的排错 SOP 草稿
    Extract {
        /// 读取最近 N 条记录（默认 10）
        #[arg(short = 'n', long, default_value_t = 10)]
        last: usize,

        /// 指定 SOP 标题
        #[arg(short, long)]
        title: Option<String>,
    },
}

#[derive(Subcommand)]
enum FragCmd {
    /// 列出所有保存的命令片段
    List,

    /// 运行指定片段
    Run {
        /// 片段标题或 ID
        name: String,

        /// 目标会话（单机执行模式）
        target: Option<String>,

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

        /// 模板变量赋值：key=value，可传多次
        #[arg(short = 'v', long = "var")]
        vars: Vec<String>,
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

/// 把本地 argv 按 POSIX shell 规则重组为单条远端命令。
/// 每个参数用单引号包裹，内嵌单引号转 '\''——保证
/// `mist exec t -- bash -c 'echo hi; exit 42'` 原样到达远端 shell。
fn shell_join(argv: &[String]) -> String {
    argv.iter()
        .map(|a| {
            if a.chars().all(|c| c.is_ascii_alphanumeric() || "_+-=./:@%,".contains(c))
                && !a.is_empty()
            {
                a.clone()
            } else {
                format!("'{}'", a.replace('\'', "'\\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn main() {
    let cli = Cli::parse();
    init_logging(cli.verbose);

    let mut ctx = CliContext::load();
    let code = match &cli.cmd {
        Cmd::Ls { group } => ls::run(&ctx, group.as_deref(), cli.json).map(|_| 0),
        Cmd::Ssh { target } => mistterm::cli::ssh_cmd::run_ssh(&mut ctx, target),
        Cmd::Exec {
            target,
            command,
            group,
            all_targets,
            serial,
            parallel,
            ..
        } => {
            if *all_targets || group.is_some() {
                // 批量模式下没有 target 位置参数，如果用户没写 -- 分隔，
                // 第一个词可能被 clap 误解析进了 target，需要拼回 command
                let mut full_cmd = Vec::new();
                if let Some(t) = target {
                    full_cmd.push(t.clone());
                }
                full_cmd.extend_from_slice(command);
                let cmd_str = shell_join(&full_cmd);
                if cmd_str.is_empty() {
                    eprintln!("错误: 缺少要执行的命令");
                    std::process::exit(1);
                }
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
                let cmd_str = shell_join(command);
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
        Cmd::Fwd {
            target,
            locals,
            remotes,
            dynamics,
        } => mistterm::cli::fwd::run_fwd(&mut ctx, target, locals, remotes, dynamics),
        Cmd::Frag { sub } => match sub {
            FragCmd::List => mistterm::cli::frag::run_list(cli.json),
            FragCmd::Run {
                name,
                target,
                group,
                all_targets,
                serial,
                parallel,
                vars,
            } => mistterm::cli::frag::run_run(
                &mut ctx,
                name,
                target.as_deref(),
                group.as_deref(),
                *all_targets,
                *serial,
                *parallel,
                vars,
                cli.json,
            ),
        },
        Cmd::ImportSshConfig {
            file,
            dry_run,
            overwrite,
        } => mistterm::cli::import_ssh::run_import(
            &mut ctx,
            file.clone(),
            *dry_run,
            *overwrite,
        ),
        Cmd::Sop { sub } => match sub {
            SopCmd::Extract { last, title } => {
                match mistterm::core::exec_history::read_recent_records(*last) {
                    Ok(records) => {
                        if records.is_empty() {
                            eprintln!("未找到执行记录（~/.mist/logs/exec-history.jsonl 为空）");
                        } else {
                            let md = mistterm::core::exec_history::extract_sop_markdown(
                                &records,
                                title.as_deref(),
                            );
                            println!("{md}");
                        }
                        Ok(0)
                    }
                    Err(e) => Err(e),
                }
            }
        },
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
