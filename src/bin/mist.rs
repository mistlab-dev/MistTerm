//! `mist` — MistTerm 命令行入口（P1：ls / exec / get / put / rls）。

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use mistterm::cli::{exec, ls, sftp_cmds, CliContext};

#[derive(Parser)]
#[command(name = "mist", version = mistterm::platform::APP_VERSION, about = "MistTerm CLI — 复用 GUI 会话配置的命令行 SSH 工具")]
struct Cli {
    /// 输出 JSON（ls/exec/rls/update 支持）
    #[arg(long, global = true)]
    json: bool,

    /// 提高日志详细度（-v info，-vv debug）
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    cmd: Option<Cmd>,
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

        /// 人已确认：会改动服务器、或看不出是否只读的命令也执行（团队策略拦截的仍不执行）。
        /// 要写在目标前面：mist exec --yes web-01 -- systemctl restart nginx
        #[arg(long, short = 'y')]
        yes: bool,
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

    /// 从 Xshell 或 FinalShell 导入会话（主机、分组、端口、用户名；能读出的密码一起导入）
    ///
    /// Xshell：选 Sessions 文件夹、单个 .xsh，或 Xshell「文件 → 导出」得到的 .xts。
    /// FinalShell：选数据目录（里面有 conn 文件夹）。
    /// Xshell 设了主密码时，用环境变量 MIST_XSHELL_MASTER_PASSWORD 提供。
    Import {
        /// 来源：xshell、finalshell；不写则自动判断
        #[arg(value_parser = ["xshell", "finalshell", "auto"])]
        source: String,

        /// 文件或文件夹路径
        path: std::path::PathBuf,

        /// 只预览，不保存
        #[arg(long)]
        dry_run: bool,

        /// 原电脑的 Windows 账号名（解 Xshell 5.1 以后的密码用；在原电脑上运行 `whoami /user` 可查）
        #[arg(long)]
        windows_user: Option<String>,

        /// 原电脑 Windows 账号的 SID（形如 S-1-5-21-…）
        #[arg(long)]
        windows_sid: Option<String>,
    },

    /// 检查并安装 MistTerm 新版本（也可用 `mist self-update`）
    ///
    /// 退出码：0 已是最新或已更新；10 有新版本但没有安装；1 出错。
    #[command(visible_alias = "self-update")]
    Update {
        /// 只检查，不安装（0 已是最新，10 有新版本，1 出错）
        #[arg(long)]
        check: bool,

        /// 不询问，直接安装（用于脚本）
        #[arg(short, long)]
        yes: bool,

        /// 退回到上一个版本（更新时保留了一份备份）
        #[arg(long, conflicts_with = "check")]
        rollback: bool,

        /// 允许安装比当前更低的版本（排查问题时手动回退用）
        #[arg(long, hide = true)]
        allow_downgrade: bool,
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

        /// 人已确认：会改动服务器、或看不出是否只读的命令也执行（团队策略拦截的仍不执行）。
        /// 要写在目标前面：mist frag run --yes 重启nginx web-01
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

/// Chinese help only when LC_ALL / LC_MESSAGES / LANG (first non-empty) starts with "zh".
fn cli_locale_is_chinese() -> bool {
    for k in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = std::env::var(k) {
            if !v.is_empty() {
                return v.to_ascii_lowercase().starts_with("zh");
            }
        }
    }
    false
}

/// English help text, keyed by subcommand path ("" = root) and arg id. Chinese stays in the doc comments.
const EN_ABOUT: &[(&str, &str)] = &[
    ("", "MistTerm CLI — command-line SSH that reuses your GUI session settings"),
    ("ls", "List saved sessions"),
    ("ssh", "Connect and open an interactive shell: mist ssh <target>"),
    ("exec", "Run a command on a target"),
    ("rls", "List a remote directory: mist rls <target>:<path>"),
    ("get", "Download a file: mist get <target>:<remote> <local>"),
    ("put", "Upload a file: mist put <local> <target>:<remote>"),
    ("fwd", "Port forwarding: start the given rules or the session's saved rules (foreground, Ctrl+C to stop)"),
    ("frag", "Manage and run command fragments"),
    ("frag list", "List all saved command fragments"),
    ("frag run", "Run a fragment"),
    ("import-ssh-config", "Import sessions from OpenSSH ~/.ssh/config"),
    ("import", "Import sessions from Xshell or FinalShell (host, group, port, user; passwords too when readable)"),
    ("update", "Check for and install a new MistTerm version (also `mist self-update`)"),
    ("sop", "View and extract troubleshooting SOPs (session structured logs)"),
    ("sop extract", "Export recent runs as a Markdown troubleshooting SOP draft"),
];

const EN_LONG_ABOUT: &[(&str, &str)] = &[
    ("import", "Import sessions from Xshell or FinalShell (host, group, port, user; passwords too when readable)\n\nXshell: pick the Sessions folder, a single .xsh, or an .xts from Xshell File → Export.\nFinalShell: pick the data folder (the one containing conn).\nIf Xshell has a master password, provide it via MIST_XSHELL_MASTER_PASSWORD."),
    ("update", "Check for and install a new MistTerm version (also `mist self-update`)\n\nExit codes: 0 up to date or updated; 10 update available but not installed; 1 error."),
];

const EN_ARG: &[(&str, &str, &str)] = &[
    ("", "json", "Output JSON (supported by ls/exec/rls/update)"),
    ("", "verbose", "More log detail (-v info, -vv debug)"),
    ("ls", "group", "Filter by group"),
    ("ssh", "target", "Target: session name/id/host, or user@host[:port]"),
    ("exec", "target", "Target: session name/id/host, or user@host[:port]"),
    ("exec", "command", "Command to run"),
    ("exec", "group", "Run on every session in a group"),
    ("exec", "all_targets", "Run on all saved sessions"),
    ("exec", "serial", "Run one at a time and stop at the first failure (default: parallel)"),
    ("exec", "parallel", "Parallelism (1-16, default 8)"),
    ("exec", "yes", "Confirmed by a human: also run commands that change the server or aren't clearly read-only (team-policy blocks still apply). Put it before the target: mist exec --yes web-01 -- systemctl restart nginx"),
    ("rls", "spec", "<target>:<remote-path>"),
    ("get", "spec", "<target>:<remote-path>"),
    ("get", "local", "Local path (may be a directory)"),
    ("put", "local", "Local file"),
    ("put", "spec", "<target>:<remote-path> (ending with / keeps the file name)"),
    ("fwd", "target", "Target: session name/id/host, or user@host[:port]"),
    ("fwd", "locals", "Local forward: [bind_addr:]local_port:remote_host:remote_port (repeatable)"),
    ("fwd", "remotes", "Remote forward: [bind_addr:]remote_port:target_host:target_port (repeatable)"),
    ("fwd", "dynamics", "Dynamic SOCKS5 forward: [bind_addr:]local_port (repeatable)"),
    ("import-ssh-config", "file", "Custom ssh config path (default ~/.ssh/config)"),
    ("import-ssh-config", "dry_run", "Only print the sessions that would be imported; don't save"),
    ("import-ssh-config", "overwrite", "Overwrite existing sessions with the same name (default: skip)"),
    ("import", "source", "Source: xshell, finalshell; auto-detect if omitted"),
    ("import", "path", "File or folder path"),
    ("import", "dry_run", "Preview only; don't save"),
    ("import", "windows_user", "Windows account name on the original PC (for Xshell 5.1+ passwords; run `whoami /user` there)"),
    ("import", "windows_sid", "SID of that Windows account (like S-1-5-21-…)"),
    ("update", "check", "Only check, don't install (0 up to date, 10 update available, 1 error)"),
    ("update", "yes", "Install without asking (for scripts)"),
    ("update", "rollback", "Roll back to the previous version (a backup is kept on update)"),
    ("sop extract", "last", "Read the last N records (default 10)"),
    ("sop extract", "title", "SOP title"),
    ("frag run", "name", "Fragment title or ID"),
    ("frag run", "target", "Target session (single-host mode)"),
    ("frag run", "group", "Run on every session in a group"),
    ("frag run", "all_targets", "Run on all saved sessions"),
    ("frag run", "serial", "Run one at a time and stop at the first failure (default: parallel)"),
    ("frag run", "parallel", "Parallelism (1-16, default 8)"),
    ("frag run", "vars", "Template variable: key=value (repeatable)"),
    ("frag run", "yes", "Confirmed by a human: also run commands that change the server or aren't clearly read-only (team-policy blocks still apply). Put it before the target: mist frag run --yes restart-nginx web-01"),
];

fn localize_help_en(cmd: clap::Command) -> clap::Command {
    fn walk(mut c: clap::Command, path: &str) -> clap::Command {
        if let Some((_, a)) = EN_ABOUT.iter().find(|(p, _)| *p == path) {
            c = c.about(*a);
        }
        if let Some((_, a)) = EN_LONG_ABOUT.iter().find(|(p, _)| *p == path) {
            c = c.long_about(*a);
        } else {
            c = c.long_about(None::<&str>);
        }
        for (p, id, h) in EN_ARG {
            if *p == path {
                let (id, h) = (*id, *h);
                c = c.mut_arg(id, |a| a.help(h).long_help(None::<&str>));
            }
        }
        let names: Vec<String> = c.get_subcommands().map(|s| s.get_name().to_string()).collect();
        for n in names {
            let sub_path = if path.is_empty() { n.clone() } else { format!("{path} {n}") };
            c = c.mut_subcommand(n, |s| walk(s, &sub_path));
        }
        c
    }
    walk(cmd, "")
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
/// 把命令参数拼成远端要执行的一行。
///
/// 只有一个参数时原样使用（和 `ssh host 'a | b'` 一样）：`mist exec web -- "df -h | tail -1"`
/// 里的管道、分号要交给远端 shell。多个参数时逐个加引号（保持原来的行为）。
fn shell_join(argv: &[String]) -> String {
    if let [one] = argv {
        return one.trim().to_string();
    }
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

/// `--yes` 写在别处时直接报错（见 `exec_gate::yes_flag_right_after`）。
fn require_yes_position(yes: bool, sub: &[&str], usage: &str) {
    if !yes {
        return;
    }
    let args: Vec<String> = std::env::args().collect();
    if !mistterm::cli::exec_gate::yes_flag_right_after(&args, sub) {
        eprintln!("错误: --yes 要紧跟在 {} 后面，例如：{usage}", sub.join(" "));
        std::process::exit(2);
    }
}

fn main() {
    let mut cmd = Cli::command();
    if !cli_locale_is_chinese() {
        cmd = localize_help_en(cmd);
    }
    let cli = Cli::from_arg_matches(&cmd.get_matches()).unwrap_or_else(|e| e.exit());
    init_logging(cli.verbose);

    // 更新命令不需要会话和设置，在加载它们之前处理。
    if let Some(Cmd::Update {
        check,
        yes,
        rollback,
        allow_downgrade,
    }) = &cli.cmd
    {
        let args = mistterm::cli::update::UpdateArgs {
            check: *check,
            json: cli.json,
            yes: *yes,
            rollback: *rollback,
            allow_downgrade: *allow_downgrade,
        };
        std::process::exit(mistterm::cli::update::run(&args));
    }

    let mut ctx = CliContext::load();
    ctx.json = cli.json;
    let cmd = match &cli.cmd {
        Some(c) => c,
        None => {
            // 无子命令时默认拉起 GUI 桌面界面
            mistterm::platform::run_gui();
            // GUI 正常退出视为成功；不要落到下方 Ok(0) 以外的路径。
            std::process::exit(0);
        }
    };
    let code = match cmd {
        Cmd::Ls { group } => ls::run(&ctx, group.as_deref(), cli.json).map(|_| 0),
        Cmd::Ssh { target } => mistterm::cli::ssh_cmd::run_ssh(&mut ctx, target),
        Cmd::Exec {
            target,
            command,
            group,
            all_targets,
            serial,
            parallel,
            yes,
        } => {
            require_yes_position(*yes, &["exec"], "mist exec --yes <目标> -- <命令>");
            ctx.exec_yes = *yes;
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
                yes,
            } => {
                require_yes_position(*yes, &["frag", "run"], "mist frag run --yes <片段> <目标>");
                ctx.exec_yes = *yes;
                mistterm::cli::frag::run_run(
                &mut ctx,
                name,
                target.as_deref(),
                group.as_deref(),
                *all_targets,
                *serial,
                *parallel,
                vars,
                cli.json,
            )
            }
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
        Cmd::Import {
            source,
            path,
            dry_run,
            windows_user,
            windows_sid,
        } => mistterm::cli::import_foreign::run_import(
            &mut ctx,
            mistterm::cli::import_foreign::ImportArgs {
                source: match source.as_str() {
                    "xshell" => Some(mistterm::core::foreign_import::ForeignSource::Xshell),
                    "finalshell" => Some(mistterm::core::foreign_import::ForeignSource::FinalShell),
                    _ => None,
                },
                path,
                dry_run: *dry_run,
                windows_user: windows_user.clone(),
                windows_sid: windows_sid.clone(),
            },
        ),
        Cmd::Update { .. } => unreachable!("handled before loading the CLI context"),
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

    // 约定：连接/执行/IO 等业务错误 → 1；仅 clap 用法错误由 clap 自行非 0 退出。
    // 子命令返回的 Ok(code) 透传远端/进程退出码（可为 0）。
    let exit = match code {
        Ok(c) => c,
        Err(e) => {
            eprintln!("mist: {e:#}");
            1
        }
    };
    std::process::exit(exit);
}

#[cfg(test)]
mod help_i18n_tests {
    use super::*;

    fn has_han(s: &str) -> bool {
        s.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
    }

    #[test]
    fn english_help_has_no_chinese() {
        fn check(c: &mut clap::Command, path: String) {
            let help = c.render_long_help().to_string();
            assert!(!has_han(&help), "Chinese left in `mist {path} --help`:\n{help}");
            let names: Vec<String> = c.get_subcommands().map(|s| s.get_name().to_string()).collect();
            for n in names {
                let mut sub = c.find_subcommand_mut(&n).unwrap().clone();
                check(&mut sub, format!("{path} {n}"));
            }
        }
        // Pre-existing: global -v (verbose) and `frag run -v` (var) clash, which only debug asserts catch.
        // Drop the short flag here so the help text itself can be checked.
        let mut c = localize_help_en(Cli::command()).mut_subcommand("frag", |f| {
            f.mut_subcommand("run", |r| r.mut_arg("vars", |a| a.short(None)))
        });
        c.build();
        check(&mut c, String::new());
    }

    #[test]
    fn chinese_help_unchanged() {
        let help = Cli::command().render_help().to_string();
        assert!(help.contains("列出已保存会话"));
    }
}
