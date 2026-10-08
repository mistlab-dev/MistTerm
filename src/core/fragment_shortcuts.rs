//! 命令片段本地快捷键（个人/团队共用本地映射；不随团队同步）。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;

/// 一条可持久化的快捷键（修饰键 + egui Key 名）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FragmentShortcut {
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub command: bool,
    /// egui `Key` 的 Debug/序列化名，如 `"J"`、`"Num1"`、`"F5"`。
    pub key: String,
}

impl FragmentShortcut {
    pub fn new(key: impl Into<String>, ctrl: bool, shift: bool, alt: bool, command: bool) -> Self {
        Self {
            ctrl,
            shift,
            alt,
            command,
            key: key.into(),
        }
    }

    pub fn matches(
        &self,
        key: &str,
        ctrl: bool,
        shift: bool,
        alt: bool,
        command: bool,
    ) -> bool {
        self.key.eq_ignore_ascii_case(key)
            && self.ctrl == ctrl
            && self.shift == shift
            && self.alt == alt
            && self.command == command
    }

    /// 人类可读标签（⌘⇧J / Ctrl+Shift+J）。
    pub fn display_label(&self) -> String {
        #[cfg(target_os = "macos")]
        {
            let mut parts = Vec::new();
            if self.command {
                parts.push("⌘".to_string());
            }
            if self.ctrl {
                parts.push("⌃".to_string());
            }
            if self.alt {
                parts.push("⌥".to_string());
            }
            if self.shift {
                parts.push("⇧".to_string());
            }
            parts.push(pretty_key(&self.key));
            parts.join("")
        }
        #[cfg(not(target_os = "macos"))]
        {
            let mut parts: Vec<String> = Vec::new();
            if self.ctrl || self.command {
                parts.push("Ctrl".into());
            }
            if self.alt {
                parts.push("Alt".into());
            }
            if self.shift {
                parts.push("Shift".into());
            }
            parts.push(pretty_key(&self.key));
            parts.join("+")
        }
    }
}

fn pretty_key(key: &str) -> String {
    match key {
        "Num0" => "0".into(),
        "Num1" => "1".into(),
        "Num2" => "2".into(),
        "Num3" => "3".into(),
        "Num4" => "4".into(),
        "Num5" => "5".into(),
        "Num6" => "6".into(),
        "Num7" => "7".into(),
        "Num8" => "8".into(),
        "Num9" => "9".into(),
        "ArrowLeft" => "←".into(),
        "ArrowRight" => "→".into(),
        "ArrowUp" => "↑".into(),
        "ArrowDown" => "↓".into(),
        "Escape" => "Esc".into(),
        "Backspace" => "⌫".into(),
        "Enter" => "Enter".into(),
        "Tab" => "Tab".into(),
        "Space" => "Space".into(),
        other => {
            if other.len() == 1 {
                other.to_uppercase()
            } else {
                other.to_string()
            }
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FragmentShortcutStore {
    /// fragment_id → shortcut
    #[serde(default)]
    pub bindings: HashMap<String, FragmentShortcut>,
}

impl FragmentShortcutStore {
    pub fn default_path() -> PathBuf {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("mistterm");
        let _ = std::fs::create_dir_all(&config_dir);
        config_dir.join("fragment_shortcuts.json")
    }

    pub fn load() -> Self {
        crate::security::encrypted_file::load_encrypted_json(&Self::default_path())
    }

    pub fn save(&self) -> io::Result<()> {
        crate::security::encrypted_file::save_encrypted_json(&Self::default_path(), self)
    }

    pub fn get(&self, fragment_id: &str) -> Option<&FragmentShortcut> {
        self.bindings.get(fragment_id)
    }

    pub fn set(&mut self, fragment_id: String, shortcut: FragmentShortcut) {
        self.bindings.insert(fragment_id, shortcut);
    }

    pub fn clear(&mut self, fragment_id: &str) {
        self.bindings.remove(fragment_id);
    }

    /// 查找占用同一组合键的其它片段 id。
    pub fn conflict_fragment_id(
        &self,
        shortcut: &FragmentShortcut,
        except_fragment_id: Option<&str>,
    ) -> Option<String> {
        self.bindings.iter().find_map(|(id, sc)| {
            if except_fragment_id == Some(id.as_str()) {
                return None;
            }
            if sc == shortcut {
                Some(id.clone())
            } else {
                None
            }
        })
    }

    pub fn find_matching_fragment_id(
        &self,
        key: &str,
        ctrl: bool,
        shift: bool,
        alt: bool,
        command: bool,
    ) -> Option<String> {
        self.bindings.iter().find_map(|(id, sc)| {
            if sc.matches(key, ctrl, shift, alt, command) {
                Some(id.clone())
            } else {
                None
            }
        })
    }
}

/// 校验失败原因（给 UI 提示）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutConflict {
    /// 需要至少一个修饰键
    NeedsModifier,
    /// Ctrl+字母留给 shell（无 Shift/Alt/⌘）
    ShellCtrlLetter,
    /// 与内置应用快捷键冲突
    ReservedApp(String),
    /// 与另一条片段快捷键冲突
    OtherFragment(String),
}

/// 内置应用快捷键（与 `app_shortcuts` / `keyboard_shortcuts` 对齐的简化集合）。
fn reserved_app_shortcuts() -> Vec<FragmentShortcut> {
    let mut out = Vec::new();
    let primary = |key: &str, shift: bool| -> FragmentShortcut {
        #[cfg(target_os = "macos")]
        {
            FragmentShortcut::new(key, false, shift, false, true)
        }
        #[cfg(not(target_os = "macos"))]
        {
            // Win/Linux：无 Shift 的主修饰键是 Ctrl；带 Shift 的应用键是 Ctrl+Shift。
            FragmentShortcut::new(key, true, shift, false, false)
        }
    };

    for k in ["N", "E", "J", "K", "B", "H", "F", "Comma"] {
        out.push(primary(k, false));
    }
    // 标签 1–9
    for n in 1..=9 {
        out.push(primary(&format!("Num{n}"), false));
    }
    out.push(primary("Tab", false));
    out.push(primary("Tab", true));

    #[cfg(target_os = "macos")]
    {
        out.push(FragmentShortcut::new("T", false, false, false, true));
        out.push(FragmentShortcut::new("W", false, false, false, true));
        out.push(FragmentShortcut::new("Q", false, false, false, true));
        out.push(FragmentShortcut::new("H", false, false, false, true));
        out.push(FragmentShortcut::new("M", false, false, false, true));
        out.push(FragmentShortcut::new("J", false, true, false, true)); // quick picker
        out.push(FragmentShortcut::new("A", false, true, false, true));
        out.push(FragmentShortcut::new("L", false, true, false, true));
        out.push(FragmentShortcut::new("D", false, true, false, true));
        out.push(FragmentShortcut::new("U", false, true, false, true));
        out.push(FragmentShortcut::new("ArrowLeft", false, false, true, true));
        out.push(FragmentShortcut::new("ArrowRight", false, false, true, true));
    }
    #[cfg(not(target_os = "macos"))]
    {
        out.push(FragmentShortcut::new("T", true, true, false, false));
        out.push(FragmentShortcut::new("W", true, true, false, false));
        out.push(FragmentShortcut::new("J", true, true, false, false));
        out.push(FragmentShortcut::new("A", true, true, false, false));
        out.push(FragmentShortcut::new("L", true, true, false, false));
        out.push(FragmentShortcut::new("D", true, true, false, false));
        out.push(FragmentShortcut::new("U", true, true, false, false));
        out.push(FragmentShortcut::new("S", true, true, false, false));
        out.push(FragmentShortcut::new("ArrowLeft", true, true, false, false));
        out.push(FragmentShortcut::new("ArrowRight", true, true, false, false));
        out.push(FragmentShortcut::new("F9", true, true, false, false));
        out.push(FragmentShortcut::new("F10", true, true, false, false));
    }
    out
}

fn is_letter_key(key: &str) -> bool {
    let k = key.to_ascii_uppercase();
    k.len() == 1 && k.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
}

/// 校验快捷键是否可保存。
pub fn validate_shortcut(
    store: &FragmentShortcutStore,
    shortcut: &FragmentShortcut,
    except_fragment_id: Option<&str>,
) -> Result<(), ShortcutConflict> {
    if !shortcut.ctrl && !shortcut.shift && !shortcut.alt && !shortcut.command {
        return Err(ShortcutConflict::NeedsModifier);
    }

    // Ctrl+字母（无 Shift/Alt/⌘）留给 shell
    if shortcut.ctrl
        && !shortcut.shift
        && !shortcut.alt
        && !shortcut.command
        && is_letter_key(&shortcut.key)
    {
        return Err(ShortcutConflict::ShellCtrlLetter);
    }

    for reserved in reserved_app_shortcuts() {
        if &reserved == shortcut {
            return Err(ShortcutConflict::ReservedApp(reserved.display_label()));
        }
    }

    if let Some(other) = store.conflict_fragment_id(shortcut, except_fragment_id) {
        return Err(ShortcutConflict::OtherFragment(other));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_ctrl_letter_rejected() {
        let store = FragmentShortcutStore::default();
        let sc = FragmentShortcut::new("A", true, false, false, false);
        assert_eq!(
            validate_shortcut(&store, &sc, None),
            Err(ShortcutConflict::ShellCtrlLetter)
        );
    }

    #[test]
    fn ctrl_shift_letter_ok_on_non_mac_style() {
        let store = FragmentShortcutStore::default();
        let sc = FragmentShortcut::new("Y", true, true, false, false);
        // May still hit reserved on some platforms; Y is free.
        assert!(validate_shortcut(&store, &sc, None).is_ok());
    }

    #[test]
    fn other_fragment_conflict() {
        let mut store = FragmentShortcutStore::default();
        let sc = FragmentShortcut::new("Y", true, true, false, false);
        store.set("a".into(), sc.clone());
        assert_eq!(
            validate_shortcut(&store, &sc, Some("b")),
            Err(ShortcutConflict::OtherFragment("a".into()))
        );
        assert!(validate_shortcut(&store, &sc, Some("a")).is_ok());
    }

    #[test]
    fn display_label_has_modifiers() {
        let sc = FragmentShortcut::new("J", true, true, false, false);
        let label = sc.display_label();
        assert!(label.contains('J') || label.contains('j'));
    }
}
