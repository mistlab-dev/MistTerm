use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/fonts/NotoSansSC-Regular.otf");
    println!("cargo:rerun-if-changed=assets/fonts/NotoSansSC-Regular.ttf");
    println!("cargo:rerun-if-changed=build.rs");
    embed_windows_icon();
}

/// 用 MSVC `rc.exe` 把 `assets/app-icon.ico` 编进各 exe 的资源（资源管理器 / 快捷方式 / 任务栏图标）。
/// CI 上缺 `rc.exe` 直接失败，避免发布无图标的 exe；本地仅警告。
fn embed_windows_icon() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
    {
        return;
    }
    println!("cargo:rerun-if-changed=assets/app-icon.ico");
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let ico = manifest_dir.join("assets").join("app-icon.ico");
    let rc = out_dir.join("app-icon.rc");
    let res = out_dir.join("app-icon.res");
    let ico_literal = ico.display().to_string().replace('\\', "\\\\");
    std::fs::write(&rc, format!("1 ICON \"{ico_literal}\"\n")).expect("write app-icon.rc");

    let status = Command::new("rc.exe")
        .arg("/nologo")
        .arg("/fo")
        .arg(&res)
        .arg(&rc)
        .status();
    match status {
        Ok(s) if s.success() => println!("cargo:rustc-link-arg-bins={}", res.display()),
        other => {
            let msg = format!("app icon not embedded: rc.exe failed or not on PATH ({other:?})");
            if env::var_os("CI").is_some() {
                panic!("{msg}");
            }
            println!("cargo:warning={msg}");
        }
    }
}
