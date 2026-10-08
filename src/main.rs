//! MistTerm - 异步 SSH 终端
//!
//! Windows 使用 GUI 子系统，避免启动时额外弹出控制台窗口（见 `windows_subsystem`）。
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

//! 架构分层见 `mistterm` 库（`src/lib.rs`）；本文件仅为 GUI 入口。

fn main() -> eframe::Result<()> {
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
                    println!("ssh:// links will open in Mist");
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

    // macOS：嵌入 Info.plist，使菜单栏/Dock 显示 Mist 而非可执行文件名 mistterm
    #[cfg(target_os = "macos")]
    embed_plist::embed_info_plist!("../Info.plist");

    mistterm::platform::init_runtime_logging();

    mistterm::platform::run_gui();
    Ok(())
}
