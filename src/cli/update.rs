//! `mist update` / `mist self-update`：检查并安装新版本。
//!
//! 退出码：
//! - `--check`：0 已是最新，10 有新版本，1 出错。
//! - 安装模式：0 已更新（或已是最新），10 有新版本但没有安装（取消、需要手动更新），1 出错。
//!
//! CLI 不会在后台自己联网检查；只有执行这个命令时才会请求更新服务器。

use std::io::{IsTerminal, Write};
use std::sync::atomic::AtomicBool;

use crate::core::updater::apply::{self, ApplyOutcome, ApplyStage};
use crate::core::updater::{
    check_for_update, CheckOptions, CheckOutcome, InstallPlan, UpdateChannel, UpdateError, UpdateInfo,
    UpdateState,
};

pub const EXIT_UPDATE_AVAILABLE: i32 = 10;

#[derive(Debug, Clone, Default)]
pub struct UpdateArgs {
    pub check: bool,
    pub json: bool,
    pub yes: bool,
    pub rollback: bool,
    pub allow_downgrade: bool,
}

pub fn run(args: &UpdateArgs) -> i32 {
    if args.rollback {
        return run_rollback(args);
    }
    let mut state = UpdateState::load();
    let mut opts = CheckOptions::for_current_build(UpdateChannel::Stable);
    opts.allow_downgrade = args.allow_downgrade;
    if !args.json {
        eprintln!("正在检查更新…");
    }
    let outcome = match check_for_update(&opts, &mut state) {
        Ok(o) => {
            let _ = state.save();
            o
        }
        Err(e) => return report_error(args, &e),
    };
    match outcome {
        CheckOutcome::UpToDate { current, latest } => {
            if args.json {
                print_json(serde_json::json!({
                    "current": current.to_string(),
                    "latest": latest.to_string(),
                    "update_available": false,
                }));
            } else {
                println!("已是最新版本 {current}。");
            }
            0
        }
        CheckOutcome::Available(info) => {
            if args.check {
                if args.json {
                    print_json(info_json(&info));
                } else {
                    print_available(&info, true);
                }
                return EXIT_UPDATE_AVAILABLE;
            }
            install(args, &info)
        }
    }
}

fn info_json(info: &UpdateInfo) -> serde_json::Value {
    let (auto, reason, download_url) = match &info.plan {
        InstallPlan::Auto { .. } => (true, None, None),
        InstallPlan::Manual {
            reason, download_url, ..
        } => (false, Some(reason.code()), Some(download_url.clone())),
    };
    serde_json::json!({
        "current": info.current.to_string(),
        "latest": info.manifest.version,
        "update_available": true,
        "pub_date": info.manifest.pub_date,
        "notes_url": info.manifest.notes_url,
        "download_size": info.plan.download_size(),
        "auto_install": auto,
        "manual_reason": reason,
        "download_url": download_url,
    })
}

fn print_available(info: &UpdateInfo, show_install_hint: bool) {
    println!("发现新版本 {}（当前 {}）。", info.manifest.version, info.current);
    if let Some(size) = info.plan.download_size() {
        println!("下载大小：{}", human_size(size));
    }
    if let Some(url) = &info.manifest.notes_url {
        println!("更新说明：{url}");
    }
    match &info.plan {
        InstallPlan::Auto { .. } => {
            if show_install_hint {
                println!("运行 `mist update` 安装。");
            }
        }
        InstallPlan::Manual {
            reason, download_url, ..
        } => {
            println!("{}", reason.user_message(true));
            println!("下载页：{download_url}");
        }
    }
}

fn install(args: &UpdateArgs, info: &UpdateInfo) -> i32 {
    print_available(info, false);
    if !info.plan.is_auto() {
        return EXIT_UPDATE_AVAILABLE;
    }
    if info.plan.uses_installer() {
        println!("安装程序会关闭所有正在运行的 Mist 窗口（SSH 会话会断开），然后完成更新。");
    }
    if !args.yes {
        if !std::io::stdin().is_terminal() {
            eprintln!("当前不是交互式终端；确认安装请加 --yes。");
            return EXIT_UPDATE_AVAILABLE;
        }
        if !confirm(&format!("现在安装 {} 吗？[y/N] ", info.manifest.version)) {
            println!("已取消。");
            return EXIT_UPDATE_AVAILABLE;
        }
    }
    let Some(exe) = crate::core::updater::paths::current_exe() else {
        return report_error(args, &UpdateError::Install("cannot locate the running program".into()));
    };
    let cancel = AtomicBool::new(false);
    let mut printer = ProgressPrinter::new();
    let result = apply::apply_update(info, &exe, false, &mut |s| printer.update(s), &cancel);
    printer.finish();
    match result {
        Ok(ApplyOutcome::Installed { version, .. }) => {
            if args.json {
                print_json(serde_json::json!({ "installed": version, "restart_required": true }));
            } else {
                println!("已更新到 {version}。正在运行的 Mist 窗口重启后才会用上新版本。");
                println!("如需退回上一个版本：mist update --rollback");
            }
            0
        }
        Ok(ApplyOutcome::InstallerStarted { version }) => {
            if args.json {
                print_json(serde_json::json!({ "installer_started": version }));
            } else {
                println!("安装程序已在后台启动，会关闭 Mist 并安装 {version}。");
            }
            0
        }
        Err(e) => report_error(args, &e),
    }
}

fn run_rollback(args: &UpdateArgs) -> i32 {
    let Some(exe) = crate::core::updater::paths::current_exe() else {
        return report_error(args, &UpdateError::Install("cannot locate the running program".into()));
    };
    let Some(dir) = exe.parent() else {
        return report_error(args, &UpdateError::NoBackup);
    };
    let Some(prev) = apply::backup_version(dir) else {
        return report_error(args, &UpdateError::NoBackup);
    };
    if !args.yes {
        if !std::io::stdin().is_terminal() {
            eprintln!("当前不是交互式终端；确认回退请加 --yes。");
            return 1;
        }
        if !confirm(&format!(
            "退回到上一个版本 {prev}（当前 {}）吗？[y/N] ",
            crate::core::updater::APP_VERSION
        )) {
            println!("已取消。");
            return 1;
        }
    }
    match apply::rollback(&exe) {
        Ok(v) => {
            if args.json {
                print_json(serde_json::json!({ "rolled_back_to": v }));
            } else {
                println!("已退回到 {v}。正在运行的 Mist 窗口重启后生效。");
            }
            0
        }
        Err(e) => report_error(args, &e),
    }
}

fn confirm(prompt: &str) -> bool {
    print!("{prompt}");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes" | "是")
}

fn report_error(args: &UpdateArgs, e: &UpdateError) -> i32 {
    if args.json {
        print_json(serde_json::json!({ "error": e.to_string(), "message": e.user_message(true) }));
    } else {
        eprintln!("mist: {}", e.user_message(true));
    }
    1
}

fn print_json(v: serde_json::Value) {
    println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
}

pub fn human_size(bytes: u64) -> String {
    let mb = bytes as f64 / (1024.0 * 1024.0);
    if mb >= 1.0 {
        format!("{mb:.1} MB")
    } else {
        format!("{:.0} KB", (bytes as f64 / 1024.0).max(1.0))
    }
}

struct ProgressPrinter {
    tty: bool,
    last_pct: i64,
}

impl ProgressPrinter {
    fn new() -> Self {
        Self {
            tty: std::io::stderr().is_terminal(),
            last_pct: -1,
        }
    }

    fn update(&mut self, stage: ApplyStage) {
        match stage {
            ApplyStage::Downloading { done, total } => {
                let pct = if total == 0 { 100 } else { (done * 100 / total) as i64 };
                let step = if self.tty { 1 } else { 25 };
                if pct / step != self.last_pct / step || self.last_pct < 0 {
                    self.last_pct = pct;
                    if self.tty {
                        eprint!("\r下载中… {pct:>3}%（{} / {}）", human_size(done), human_size(total));
                    } else {
                        eprintln!("下载中… {pct}%");
                    }
                }
            }
            ApplyStage::Verifying => {
                self.finish();
                eprintln!("正在校验…");
            }
            ApplyStage::Installing => {
                self.finish();
                eprintln!("正在安装…");
            }
        }
    }

    fn finish(&mut self) {
        if self.tty && self.last_pct >= 0 {
            eprintln!();
            self.last_pct = -2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(human_size(60_385_277), "57.6 MB");
        assert_eq!(human_size(2048), "2 KB");
        assert_eq!(human_size(10), "1 KB");
    }
}
