//! 把 Mist 登记为浏览器里 `ssh://` 链接的打开方式（Windows、Linux；macOS 暂不做）。
//!
//! - Windows：写当前用户的注册表 `HKCU\Software\Classes\ssh`（安装版的安装程序也会写同样的键，便携版用设置里的按钮）。
//! - Linux：在 `~/.local/share/applications` 放一个 `mistterm-ssh.desktop`，再用 `xdg-mime` 设为 `x-scheme-handler/ssh` 的默认程序。
//!
//! 只动当前用户的设置，不需要管理员权限。

use std::path::{Path, PathBuf};

/// 当前登记状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandlerStatus {
    /// 已经由这个 Mist 打开。
    ThisApp,
    /// 由别的程序打开（附带能看到的程序信息）。
    Other(String),
    /// 没有任何程序登记。
    None,
    /// 这个系统不支持 / 查不到。
    Unsupported,
}

/// 这个平台是否支持登记。
pub fn supported() -> bool {
    cfg!(any(windows, target_os = "linux"))
}

fn current_exe() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|e| format!("找不到 Mist 程序位置：{e}"))
}

// ---------------------------------------------------------------- Linux

/// Linux `.desktop` 文件名。
pub const LINUX_DESKTOP_FILE: &str = "mistterm-ssh.desktop";

/// 按桌面规范给 Exec 里的程序路径加引号。
pub fn desktop_exec_quote(path: &str) -> String {
    let needs = path.chars().any(|c| c.is_whitespace() || "\"'\\><~|&;$*?#()`".contains(c));
    if !needs {
        return path.to_string();
    }
    let mut out = String::from("\"");
    for c in path.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// 从 `.desktop` 内容里取出 Exec 的程序路径（去掉引号和转义）。
pub fn desktop_exec_program(entry: &str) -> Option<PathBuf> {
    let exec = entry.lines().find_map(|l| l.trim().strip_prefix("Exec="))?.trim();
    let prog = if let Some(rest) = exec.strip_prefix('"') {
        let mut out = String::new();
        let mut chars = rest.chars();
        loop {
            match chars.next()? {
                '\\' => out.push(chars.next()?),
                '"' => break,
                c => out.push(c),
            }
        }
        out
    } else {
        exec.split_whitespace().next()?.to_string()
    };
    (!prog.is_empty()).then(|| PathBuf::from(prog))
}

/// 生成 Linux `.desktop` 内容。
pub fn linux_desktop_entry(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=MistTerm\nComment=用 MistTerm 打开 ssh:// 链接\nExec={} %u\nTerminal=false\nNoDisplay=true\nMimeType=x-scheme-handler/ssh;\nCategories=Network;RemoteAccess;\n",
        desktop_exec_quote(&exe.to_string_lossy())
    )
}

#[cfg(target_os = "linux")]
fn linux_applications_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| crate::platform::home_dir().map(|h| h.join(".local/share")))
        .ok_or_else(|| "找不到用户目录".to_string())?;
    Ok(base.join("applications"))
}

/// 在 `mimeapps.list` 的 `[Default Applications]` 里设置 / 替换一项（没有 `xdg-mime` 时用）。
pub fn mimeapps_set_default(content: &str, mime: &str, desktop: &str) -> String {
    let mut lines: Vec<String> = content.lines().map(str::to_string).collect();
    let header = "[Default Applications]";
    let entry = format!("{mime}={desktop}");
    match lines.iter().position(|l| l.trim() == header) {
        Some(start) => {
            let end = lines[start + 1..].iter().position(|l| l.trim_start().starts_with('[')).map(|i| start + 1 + i).unwrap_or(lines.len());
            if let Some(i) = (start + 1..end).find(|&i| lines[i].trim_start().starts_with(&format!("{mime}="))) {
                lines[i] = entry;
            } else {
                lines.insert(start + 1, entry);
            }
        }
        None => {
            if !lines.is_empty() && !lines.last().is_some_and(|l| l.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push(header.to_string());
            lines.push(entry);
        }
    }
    let mut s = lines.join("\n");
    s.push('\n');
    s
}

#[cfg(target_os = "linux")]
pub fn register() -> Result<(), String> {
    let exe = current_exe()?;
    let dir = linux_applications_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("建不了 {}：{e}", dir.display()))?;
    let file = dir.join(LINUX_DESKTOP_FILE);
    // 程序路径里有空格等字符时，不少系统的 xdg-open 读不懂带引号的 Exec，
    // 所以放一个不带空格的链接指向 Mist，Exec 里写这个链接。
    let exec_target = if desktop_exec_quote(&exe.to_string_lossy()) == exe.to_string_lossy() {
        exe.clone()
    } else {
        let link_dir = dir.parent().unwrap_or(&dir).join("mistterm");
        std::fs::create_dir_all(&link_dir).map_err(|e| format!("建不了 {}：{e}", link_dir.display()))?;
        let link = link_dir.join("mist-open-ssh-url");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&exe, &link).map_err(|e| format!("建不了 {}：{e}", link.display()))?;
        if desktop_exec_quote(&link.to_string_lossy()) == link.to_string_lossy() {
            link
        } else {
            exe.clone()
        }
    };
    std::fs::write(&file, linux_desktop_entry(&exec_target)).map_err(|e| format!("写不了 {}：{e}", file.display()))?;
    // xdg-mime 在 ~/.config 不存在时会失败但仍返回 0，所以先建好目录，事后再查一遍
    let cfg = linux_config_dir()?;
    std::fs::create_dir_all(&cfg).map_err(|e| format!("建不了 {}：{e}", cfg.display()))?;
    let _ = quiet("xdg-mime", &["default", LINUX_DESKTOP_FILE, "x-scheme-handler/ssh"]);
    let list = cfg.join("mimeapps.list");
    let old = std::fs::read_to_string(&list).unwrap_or_default();
    let set_in_list = old.lines().any(|l| {
        l.trim().strip_prefix("x-scheme-handler/ssh=").is_some_and(|v| v.split(';').next() == Some(LINUX_DESKTOP_FILE))
    });
    if !set_in_list {
        // 没有 xdg-mime 或它没写成：直接改 mimeapps.list
        std::fs::write(&list, mimeapps_set_default(&old, "x-scheme-handler/ssh", LINUX_DESKTOP_FILE))
            .map_err(|e| format!("写不了 {}：{e}", list.display()))?;
    }
    let _ = quiet("update-desktop-database", &[&dir.to_string_lossy()]);
    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_config_dir() -> Result<PathBuf, String> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| crate::platform::home_dir().map(|h| h.join(".config")))
        .ok_or_else(|| "找不到用户目录".to_string())
}

/// 运行外部命令，不让它的输出跑到终端里；返回 stdout（失败为 None）。
#[cfg(target_os = "linux")]
fn quiet(prog: &str, args: &[&str]) -> Option<String> {
    let o = std::process::Command::new(prog)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

#[cfg(target_os = "linux")]
pub fn status() -> HandlerStatus {
    // 以 mimeapps.list 为准（xdg-mime query 在没设默认时会退回到「唯一能打开的程序」，不准）
    let from_list = linux_config_dir()
        .ok()
        .and_then(|c| std::fs::read_to_string(c.join("mimeapps.list")).ok())
        .and_then(|text| {
            text.lines().find_map(|l| {
                l.trim().strip_prefix("x-scheme-handler/ssh=").map(|v| v.split(';').next().unwrap_or("").trim().to_string())
            })
        })
        .filter(|v| !v.is_empty());
    let current = from_list
        .or_else(|| quiet("xdg-mime", &["query", "default", "x-scheme-handler/ssh"]).map(|s| s.trim().to_string()))
        .unwrap_or_default();
    if current.is_empty() {
        return HandlerStatus::None;
    }
    if current != LINUX_DESKTOP_FILE {
        return HandlerStatus::Other(current);
    }
    // 是我们的文件：再确认里面指向的就是正在运行的这个 Mist（可能经过一个链接）
    let ours = linux_applications_dir()
        .ok()
        .and_then(|d| std::fs::read_to_string(d.join(LINUX_DESKTOP_FILE)).ok())
        .and_then(|text| desktop_exec_program(&text))
        .zip(current_exe().ok())
        .is_some_and(|(prog, exe)| {
            let a = std::fs::canonicalize(&prog).unwrap_or(prog);
            let b = std::fs::canonicalize(&exe).unwrap_or(exe);
            a == b
        });
    if ours {
        HandlerStatus::ThisApp
    } else {
        HandlerStatus::Other(format!("{LINUX_DESKTOP_FILE}（指向另一个位置的 Mist）"))
    }
}

// ---------------------------------------------------------------- Windows

/// 注册表里 `shell\open\command` 的值。
pub fn windows_open_command(exe: &Path) -> String {
    format!("\"{}\" \"%1\"", exe.display())
}

/// 直接读写当前用户的注册表（不用 reg.exe：它的输出按系统代码页编码，中文路径会对不上）。
#[cfg(windows)]
mod winreg_util {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use winapi::shared::minwindef::HKEY;
    use winapi::um::winnt::{KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ};
    use winapi::um::winreg::{
        RegCloseKey, RegCreateKeyExW, RegGetValueW, RegSetValueExW, HKEY_CURRENT_USER, RRF_RT_REG_SZ,
    };

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    /// 在 HKCU 下写一个字符串值；`name` 为 None 时写默认值。
    pub fn set(subkey: &str, name: Option<&str>, value: &str) -> Result<(), String> {
        let sk = wide(subkey);
        let data = wide(value);
        let name_w = name.map(wide);
        unsafe {
            let mut hkey: HKEY = std::ptr::null_mut();
            let rc = RegCreateKeyExW(
                HKEY_CURRENT_USER,
                sk.as_ptr(),
                0,
                std::ptr::null_mut(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                std::ptr::null_mut(),
                &mut hkey,
                std::ptr::null_mut(),
            );
            if rc != 0 {
                return Err(format!("写不了注册表 HKCU\\{subkey}（错误 {rc}）"));
            }
            let rc = RegSetValueExW(
                hkey,
                name_w.as_ref().map_or(std::ptr::null(), |v| v.as_ptr()),
                0,
                REG_SZ,
                data.as_ptr() as *const u8,
                (data.len() * 2) as u32,
            );
            RegCloseKey(hkey);
            if rc != 0 {
                return Err(format!("写不了注册表 HKCU\\{subkey}（错误 {rc}）"));
            }
        }
        Ok(())
    }

    /// 读 HKCU 下的字符串值；`name` 为 None 时读默认值。
    pub fn get(subkey: &str, name: Option<&str>) -> Option<String> {
        let sk = wide(subkey);
        let name_w = name.map(wide);
        let name_ptr = name_w.as_ref().map_or(std::ptr::null(), |v| v.as_ptr());
        unsafe {
            let mut size: u32 = 0;
            let rc = RegGetValueW(
                HKEY_CURRENT_USER,
                sk.as_ptr(),
                name_ptr,
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            );
            if rc != 0 || size == 0 {
                return None;
            }
            let mut buf = vec![0u16; (size as usize + 1) / 2 + 1];
            let mut size2 = (buf.len() * 2) as u32;
            let rc = RegGetValueW(
                HKEY_CURRENT_USER,
                sk.as_ptr(),
                name_ptr,
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr() as *mut _,
                &mut size2,
            );
            if rc != 0 {
                return None;
            }
            let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            Some(String::from_utf16_lossy(&buf[..len]))
        }
    }
}

#[cfg(windows)]
const WIN_KEY: &str = r"Software\Classes\ssh";
/// 用户在「Windows 设置 → 默认应用」里选过的程序记在这里，优先于上面的键，而且程序没法替用户改。
#[cfg(windows)]
const WIN_USER_CHOICE: &str =
    r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\ssh\UserChoice";

/// 用户在 Windows 设置里另选了程序时返回那个程序的标识。
#[cfg(windows)]
fn windows_user_choice_other() -> Option<String> {
    winreg_util::get(WIN_USER_CHOICE, Some("ProgId"))
        .filter(|p| !p.trim().is_empty() && !p.eq_ignore_ascii_case("ssh"))
}

#[cfg(windows)]
pub fn register() -> Result<(), String> {
    let exe = current_exe()?;
    let cmd = windows_open_command(&exe);
    let icon = format!("\"{}\",0", exe.display());
    winreg_util::set(WIN_KEY, None, "URL:SSH Protocol")?;
    winreg_util::set(WIN_KEY, Some("URL Protocol"), "")?;
    winreg_util::set(&format!(r"{WIN_KEY}\DefaultIcon"), None, &icon)?;
    winreg_util::set(&format!(r"{WIN_KEY}\shell\open\command"), None, &cmd)?;
    if let Some(other) = windows_user_choice_other() {
        return Err(format!(
            "Windows 设置里已经选了别的程序（{other}）打开 ssh:// 链接。请到「Windows 设置 → 应用 → 默认应用」搜 ssh，改成 MistTerm。"
        ));
    }
    Ok(())
}

#[cfg(windows)]
pub fn status() -> HandlerStatus {
    if let Some(other) = windows_user_choice_other() {
        return HandlerStatus::Other(other);
    }
    let value = winreg_util::get(&format!(r"{WIN_KEY}\shell\open\command"), None).unwrap_or_default();
    match current_exe() {
        Ok(exe) if value.eq_ignore_ascii_case(&windows_open_command(&exe)) => HandlerStatus::ThisApp,
        _ if value.trim().is_empty() => HandlerStatus::None,
        _ => HandlerStatus::Other(value),
    }
}

// ---------------------------------------------------------------- 其它平台

#[cfg(not(any(windows, target_os = "linux")))]
pub fn register() -> Result<(), String> {
    Err("这个系统暂不支持".into())
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn status() -> HandlerStatus {
    HandlerStatus::Unsupported
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entry_quotes_paths() {
        assert_eq!(desktop_exec_quote("/opt/mist/Mist"), "/opt/mist/Mist");
        assert_eq!(desktop_exec_quote("/home/a b/Mist"), "\"/home/a b/Mist\"");
        assert_eq!(desktop_exec_quote("/x/$y\"z"), "\"/x/\\$y\\\"z\"");
        let e = linux_desktop_entry(Path::new("/home/tian/.local/bin/Mist"));
        assert!(e.contains("Exec=/home/tian/.local/bin/Mist %u\n"));
        assert!(e.contains("MimeType=x-scheme-handler/ssh;\n"));
        assert!(e.contains("NoDisplay=true"));
    }

    #[test]
    fn exec_program_round_trip() {
        for p in ["/opt/mist/Mist", "/home/a b/My \"Apps\"/Mist", "/tmp/x$y/Mist"] {
            let entry = linux_desktop_entry(Path::new(p));
            assert_eq!(desktop_exec_program(&entry), Some(PathBuf::from(p)), "{entry}");
        }
    }

    #[test]
    fn mimeapps_list_editing() {
        assert_eq!(
            mimeapps_set_default("", "x-scheme-handler/ssh", "m.desktop"),
            "[Default Applications]\nx-scheme-handler/ssh=m.desktop\n"
        );
        let old = "[Added Associations]\ntext/plain=gedit.desktop;\n\n[Default Applications]\nx-scheme-handler/ssh=putty.desktop\ntext/html=firefox.desktop\n";
        let new = mimeapps_set_default(old, "x-scheme-handler/ssh", "m.desktop");
        assert!(new.contains("x-scheme-handler/ssh=m.desktop\n"));
        assert!(!new.contains("putty"));
        assert!(new.contains("text/html=firefox.desktop"));
        assert!(new.contains("text/plain=gedit.desktop;"));
        let only_other = "[Added Associations]\ntext/plain=gedit.desktop;\n";
        let n2 = mimeapps_set_default(only_other, "x-scheme-handler/ssh", "m.desktop");
        assert!(n2.ends_with("[Default Applications]\nx-scheme-handler/ssh=m.desktop\n"));
    }

    #[test]
    fn windows_command_value() {
        assert_eq!(
            windows_open_command(Path::new(r"C:\Program Files\MistTerm\Mist.exe")),
            r#""C:\Program Files\MistTerm\Mist.exe" "%1""#
        );
    }
}
