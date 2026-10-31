//! 跨平台桌面通知（无额外 crate：走系统自带工具）。
//!
//! - Windows：PowerShell WinRT Toast（尽力）
//! - macOS：osascript `display notification`
//! - Linux：`notify-send`（尽力）
//!
//! 权限被拒 / 工具缺失时返回明确结果，由上层降级到面板内告警。

use std::process::Command;

/// 桌面通知投递结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopNotifyResult {
    /// 已成功发起投递（系统是否真正弹出取决于 OS / 权限）。
    Sent,
    /// 系统拒绝或通知服务不可用（应降级到面板内）。
    DeniedOrUnavailable,
}

/// 发出一条桌面通知。`_action_open_monitor` 预留动作位（多数平台仅能展示文案；
/// 真正「打开监控」仍靠应用内 Toast 按钮）。
pub fn send_desktop_notification(
    title: &str,
    body: &str,
    _action_open_monitor: bool,
) -> DesktopNotifyResult {
    let title = title.trim();
    let body = body.trim();
    if title.is_empty() && body.is_empty() {
        return DesktopNotifyResult::DeniedOrUnavailable;
    }
    let title = if title.is_empty() { "MistTerm" } else { title };
    let body = if body.is_empty() { title } else { body };

    #[cfg(target_os = "windows")]
    {
        return send_windows(title, body);
    }
    #[cfg(target_os = "macos")]
    {
        return send_macos(title, body);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        return send_linux(title, body);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
    {
        let _ = (title, body);
        DesktopNotifyResult::DeniedOrUnavailable
    }
}

/// 按偏好播放告警提示音（尽力；失败静默）。
pub fn play_alert_sound() {
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "[Console]::Beep(880, 220)",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("afplay")
            .arg("/System/Library/Sounds/Funk.aiff")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let candidates: &[(&str, &[&str])] = &[
            (
                "paplay",
                &["/usr/share/sounds/freedesktop/stereo/dialog-warning.oga"],
            ),
            ("canberra-gtk-play", &["--id=dialog-warning"]),
        ];
        for (bin, args) in candidates {
            if Command::new(bin)
                .args(*args)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .is_ok()
            {
                break;
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn send_windows(title: &str, body: &str) -> DesktopNotifyResult {
    let title_ps = title.replace('\'', "''");
    let body_ps = body.replace('\'', "''");
    let script = format!(
        r#"
$ErrorActionPreference = 'Stop'
try {{
  [Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null
  [Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime] | Out-Null
  $t = '{title}'
  $b = '{body}'
  $xml = New-Object Windows.Data.Xml.Dom.XmlDocument
  $escT = [System.Security.SecurityElement]::Escape($t)
  $escB = [System.Security.SecurityElement]::Escape($b)
  $payload = "<toast><visual><binding template=`"ToastGeneric`"><text>$escT</text><text>$escB</text></binding></visual></toast>"
  $xml.LoadXml($payload)
  $toast = [Windows.UI.Notifications.ToastNotification]::new($xml)
  [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('MistTerm').Show($toast)
  exit 0
}} catch {{
  exit 1
}}
"#,
        title = title_ps,
        body = body_ps
    );
    match Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(st) if st.success() => DesktopNotifyResult::Sent,
        _ => DesktopNotifyResult::DeniedOrUnavailable,
    }
}

#[cfg(target_os = "macos")]
fn send_macos(title: &str, body: &str) -> DesktopNotifyResult {
    let title_esc = title.replace('\\', "\\\\").replace('"', "\\\"");
    let body_esc = body.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(
        r#"display notification "{body}" with title "{title}""#,
        title = title_esc,
        body = body_esc
    );
    match Command::new("osascript")
        .args(["-e", &script])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(st) if st.success() => DesktopNotifyResult::Sent,
        _ => DesktopNotifyResult::DeniedOrUnavailable,
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn send_linux(title: &str, body: &str) -> DesktopNotifyResult {
    match Command::new("notify-send")
        .args([
            "--app-name=MistTerm",
            "--urgency=critical",
            "--expire-time=8000",
            title,
            body,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(st) if st.success() => DesktopNotifyResult::Sent,
        _ => DesktopNotifyResult::DeniedOrUnavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_payload_is_unavailable() {
        assert_eq!(
            send_desktop_notification("", "", false),
            DesktopNotifyResult::DeniedOrUnavailable
        );
    }
}
