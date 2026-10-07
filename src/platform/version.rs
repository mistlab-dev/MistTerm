//! 程序版本号：GUI「关于」、`--version`、问题反馈、审计日志与更新检查共用同一来源。

/// 当前程序的版本号（发布时与 `Cargo.toml`、`Info.plist` 和 Git 标签一致，CI 会检查）。
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 打印 `<程序名> <版本号>`，与 `mist --version` 的格式一致。
pub fn print_version_line(bin_name: &str) {
    println!("{bin_name} {APP_VERSION}");
}
