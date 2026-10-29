//! MistTerm - 异步 SSH 终端
//!
//! Windows 使用 GUI 子系统，避免启动时额外弹出控制台窗口（见 `windows_subsystem`）。
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

//! 架构分层见 `mistterm` 库（`src/lib.rs`）；本文件仅为 GUI 入口。

fn main() -> eframe::Result<()> {
    // #150：panic hook 尽早安装——panic 信息 + 栈落到 logs/crash-reports/，
    // 解决「Rust panic 不生成 .ips、会话日志无栈」的可观测性缺口。
    // #189 的下次启动提示在 GUI `begin_run` / `on_exit` 里处理，不在这里调用，
    // 避免 `--version`、`update` 等无界面路径清掉干净退出戳。
    mistterm::core::crash_reporter::install();

    // `Mist update`：桌面发行包不再附带命令行 `mist`（Linux #127）。无界面，和 `mist update` 同一套逻辑。
    dispatch_gui_update_command();

    // `Mist --version`：不启动界面，只打印版本号（更新器用它做冒烟验证，安装脚本也可用）。
    if std::env::args().skip(1).any(|a| a == "--version" || a == "-V") {
        mistterm::platform::print_version_line("Mist");
        return Ok(());
    }

    // `Mist --register-ssh-url`：不启动界面，把浏览器里的 ssh:// 链接交给 Mist 打开（Windows、Linux）。
    // 安装脚本 / 便携版可以用；界面里在「设置 → 连接」也有同样的按钮。
    if std::env::args().skip(1).any(|a| a == "--register-ssh-url") {
        match mistterm::platform::url_handler::register() {
            Ok(()) => {
                let st = mistterm::platform::url_handler::status();
                if st == mistterm::platform::url_handler::HandlerStatus::ThisApp {
                    println!("ssh:// links will open in MistTerm");
                    return Ok(());
                }
                eprintln!("registered, but the system still reports: {st:?}");
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("register ssh:// handler failed: {e}");
                std::process::exit(1);
            }
        }
    }

    // 测试用：设置了 MIST_SSH_URL_PROBE=<文件> 时，被 ssh:// 链接打开就把收到的参数写进这个文件后退出，
    // 不启动界面（CI 用来确认系统确实把链接交给了 Mist；平时没有这个环境变量，不影响使用）。
    if let Some(probe) = std::env::var_os("MIST_SSH_URL_PROBE") {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.iter().any(|a| a.to_ascii_lowercase().starts_with("ssh://")) {
            let _ = std::fs::write(probe, args.join("\n"));
            return Ok(());
        }
    }

    // macOS：嵌入 Info.plist，使菜单栏/Dock 显示 MistTerm（CFBundleDisplayName），而非裸二进制名
    #[cfg(target_os = "macos")]
    embed_plist::embed_info_plist!("../Info.plist");

    mistterm::platform::init_runtime_logging();

    mistterm::platform::run_gui();
    Ok(())
}

/// `Mist update` / `Mist self-update`。不是这条命令就直接返回，继续走 GUI。
fn dispatch_gui_update_command() {
    let mut args = std::env::args().skip(1);
    let Some(head) = args.next() else {
        return;
    };
    if head != "update" && head != "self-update" {
        return;
    }
    let mut parsed = mistterm::cli::update::UpdateArgs::default();
    for a in args {
        match a.as_str() {
            "--check" => parsed.check = true,
            "--json" => parsed.json = true,
            "--yes" | "-y" => parsed.yes = true,
            "--rollback" => parsed.rollback = true,
            "--allow-downgrade" => parsed.allow_downgrade = true,
            "--help" | "-h" => {
                eprintln!("Usage: Mist update [--check] [--json] [--yes] [--rollback]");
                std::process::exit(0);
            }
            other => {
                eprintln!("unknown argument for Mist update: {other}");
                eprintln!("Usage: Mist update [--check] [--json] [--yes] [--rollback]");
                std::process::exit(2);
            }
        }
    }
    std::process::exit(mistterm::cli::update::run(&parsed));
}
