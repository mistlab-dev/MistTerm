use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/fonts/NotoSansSC-Regular.otf");
    println!("cargo:rerun-if-changed=assets/fonts/NotoSansSC-Regular.ttf");
    println!("cargo:rerun-if-changed=build.rs");
    emit_app_version();
    check_update_signing_config();
    embed_windows_icon();
}

/// 程序版本号：默认取 `Cargo.toml`；只有 `--features update-test` 时才允许用
/// `MIST_BUILD_VERSION` 覆盖（端到端测试要构建出 1.90.0 / 1.91.0 两个版本）。
fn emit_app_version() {
    println!("cargo:rerun-if-env-changed=MIST_BUILD_VERSION");
    let pkg = env::var("CARGO_PKG_VERSION").unwrap();
    let test_build = env::var_os("CARGO_FEATURE_UPDATE_TEST").is_some();
    let version = match env::var("MIST_BUILD_VERSION") {
        Ok(v) if !v.trim().is_empty() => {
            if !test_build {
                panic!(
                    "MIST_BUILD_VERSION is only allowed together with --features update-test \
                     (release versions come from Cargo.toml)"
                );
            }
            v.trim().to_string()
        }
        _ => pkg,
    };
    println!("cargo:rustc-env=MIST_APP_VERSION={version}");
}

/// 官方发布构建（`MIST_DIST_CHANNEL=github-release`）必须内置真实的更新签名公钥，
/// 且绝不能带测试 feature。否则直接失败，避免发出一个更新功能失效或信任测试钥匙的正式版。
fn check_update_signing_config() {
    println!("cargo:rerun-if-env-changed=MIST_DIST_CHANNEL");
    println!("cargo:rerun-if-env-changed=MIST_UPDATE_TEST_PUBKEY");
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let key_files = [
        manifest_dir.join("resources/update/minisign-primary.pub"),
        manifest_dir.join("resources/update/minisign-backup.pub"),
    ];
    for f in &key_files {
        println!("cargo:rerun-if-changed={}", f.display());
    }
    let channel = env::var("MIST_DIST_CHANNEL").unwrap_or_default();
    if channel != "github-release" {
        return;
    }
    if env::var_os("CARGO_FEATURE_UPDATE_TEST").is_some() {
        panic!("the update-test feature must never be enabled in an official release build");
    }
    for f in &key_files {
        let content = std::fs::read_to_string(f).unwrap_or_default();
        if let Err(why) = validate_pubkey_file(&content) {
            panic!(
                "official release build needs the real update signing public key in {}: {why}. \
                 See docs/release/AUTO_UPDATE.md (Tian generates the key pair on his own computer).",
                f.display()
            );
        }
    }
}

/// minisign 公钥文件：一行 `untrusted comment:`，一行 base64（42 字节 → 56 个字符，以 `RW` 开头）。
fn validate_pubkey_file(content: &str) -> Result<(), &'static str> {
    if content.contains("PLACEHOLDER") {
        return Err("file still contains the PLACEHOLDER key");
    }
    let key = content
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("untrusted comment:"))
        .ok_or("no key line")?;
    let ok = key.len() == 56
        && key.starts_with("RW")
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=');
    if ok {
        Ok(())
    } else {
        Err("key line is not a minisign Ed25519 public key")
    }
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
