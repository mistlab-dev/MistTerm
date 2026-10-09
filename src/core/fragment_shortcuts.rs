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

    /// 按平台统一修饰键：Windows/Linux 上 egui 会把 Ctrl 同时报成 `ctrl` 和 `command`，
    /// 这里合并成 `ctrl`（`command = false`）；macOS 上 `command` 就是 ⌘，原样保留。
    pub fn normalized_for(&self, platform: KeyPlatform) -> Self {
        let mut out = self.clone();
        if platform == KeyPlatform::Other {
            out.ctrl = self.ctrl || self.command;
            out.command = false;
        }
        out
    }

    /// 按当前平台统一修饰键（录制、保存、匹配都走这里）。
    pub fn normalized(&self) -> Self {
        self.normalized_for(KeyPlatform::current())
    }

    fn same_combo(&self, other: &Self, platform: KeyPlatform) -> bool {
        let a = self.normalized_for(platform);
        let b = other.normalized_for(platform);
        a.key.eq_ignore_ascii_case(&b.key)
            && a.ctrl == b.ctrl
            && a.shift == b.shift
            && a.alt == b.alt
            && a.command == b.command
    }

    pub fn matches(
        &self,
        key: &str,
        ctrl: bool,
        shift: bool,
        alt: bool,
        command: bool,
    ) -> bool {
        let pressed = FragmentShortcut::new(key, ctrl, shift, alt, command);
        self.same_combo(&pressed, KeyPlatform::current())
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

/// 快捷键规则按哪种键盘来判断（测试里可以指定，平时用 [`KeyPlatform::current`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyPlatform {
    Mac,
    /// Windows / Linux
    Other,
}

impl KeyPlatform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            KeyPlatform::Mac
        } else {
            KeyPlatform::Other
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
        let mut store: Self =
            crate::security::encrypted_file::load_encrypted_json(&Self::default_path());
        // 1.2.2 测试版在 Windows/Linux 上存下的组合键带着 `command`，读进来时统一一下。
        for sc in store.bindings.values_mut() {
            *sc = sc.normalized();
        }
        store
    }

    pub fn save(&self) -> io::Result<()> {
        crate::security::encrypted_file::save_encrypted_json(&Self::default_path(), self)
    }

    pub fn get(&self, fragment_id: &str) -> Option<&FragmentShortcut> {
        self.bindings.get(fragment_id)
    }

    pub fn set(&mut self, fragment_id: String, shortcut: FragmentShortcut) {
        self.bindings.insert(fragment_id, shortcut.normalized());
    }

    pub fn clear(&mut self, fragment_id: &str) -> bool {
        self.bindings.remove(fragment_id).is_some()
    }

    /// 去掉片段已经不存在的快捷键；返回去掉了几条。
    pub fn prune_missing(&mut self, exists: impl Fn(&str) -> bool) -> usize {
        let before = self.bindings.len();
        self.bindings.retain(|id, _| exists(id));
        before - self.bindings.len()
    }

    /// 查找占用同一组合键的其它片段 id。
    pub fn conflict_fragment_id(
        &self,
        shortcut: &FragmentShortcut,
        except_fragment_id: Option<&str>,
    ) -> Option<String> {
        self.conflict_fragment_id_for(shortcut, except_fragment_id, KeyPlatform::current())
    }

    fn conflict_fragment_id_for(
        &self,
        shortcut: &FragmentShortcut,
        except_fragment_id: Option<&str>,
        platform: KeyPlatform,
    ) -> Option<String> {
        self.bindings.iter().find_map(|(id, sc)| {
            if except_fragment_id == Some(id.as_str()) {
                return None;
            }
            if sc.same_combo(shortcut, platform) {
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
    /// 没有修饰键，或只按了 Shift
    NeedsModifier,
    /// Ctrl+键留给 shell（Ctrl+C、Ctrl+R……；Mac 上不带 ⌘ 的 Ctrl 组合也一样）
    ShellCtrlLetter,
    /// 组合不对：Windows/Linux 要 Ctrl+Shift，Mac 要带 ⌘（Alt/Option 组合在终端里会打字或按词移动）
    NeedsCombo,
    /// 与内置应用快捷键冲突
    ReservedApp(String),
    /// 与另一条片段快捷键冲突
    OtherFragment(String),
}

impl ShortcutConflict {
    /// 给用户看的提示 (英文, 中文)。
    pub fn message(&self) -> (String, String) {
        let rule_en = if cfg!(target_os = "macos") {
            "Use a ⌘ combination, for example ⌘⇧Y."
        } else {
            "Use Ctrl+Shift+key, for example Ctrl+Shift+Y."
        };
        let rule_zh = if cfg!(target_os = "macos") {
            "请用带 ⌘ 的组合，比如 ⌘⇧Y。"
        } else {
            "请用 Ctrl+Shift+键，比如 Ctrl+Shift+Y。"
        };
        match self {
            ShortcutConflict::NeedsModifier => (
                format!("A single key or Shift+key can't be a snippet shortcut. {rule_en}"),
                format!("单个键或 Shift+键不能当片段快捷键。{rule_zh}"),
            ),
            ShortcutConflict::ShellCtrlLetter => (
                format!("Ctrl+key belongs to the shell (Ctrl+C, Ctrl+R and so on). {rule_en}"),
                format!("Ctrl+键留给 shell 用（比如 Ctrl+C、Ctrl+R）。{rule_zh}"),
            ),
            ShortcutConflict::NeedsCombo => (
                format!(
                    "Alt/Option combinations type characters or move by word in the terminal. {rule_en}"
                ),
                format!("Alt/Option 组合在终端里会打出字符或按词移动。{rule_zh}"),
            ),
            ShortcutConflict::ReservedApp(label) => (
                format!("Conflicts with a built-in shortcut ({label})."),
                format!("和应用自带的快捷键冲突（{label}）。"),
            ),
            ShortcutConflict::OtherFragment(_) => (
                "This shortcut is already used by another snippet.".into(),
                "这个快捷键已经给另一条片段用了。".into(),
            ),
        }
    }

    /// 录制区下方的说明 (英文, 中文)。
    pub fn rule_hint() -> (&'static str, &'static str) {
        if cfg!(target_os = "macos") {
            (
                "Works in the terminal. Use a ⌘ combination (for example ⌘⇧Y); built-in shortcuts can't be used.",
                "在终端里也能用。请用带 ⌘ 的组合（比如 ⌘⇧Y），应用自带的快捷键不能用。",
            )
        } else {
            (
                "Works in the terminal. Use Ctrl+Shift+key (for example Ctrl+Shift+Y); built-in shortcuts can't be used.",
                "在终端里也能用。请用 Ctrl+Shift+键（比如 Ctrl+Shift+Y），应用自带的快捷键不能用。",
            )
        }
    }
}

/// 内置快捷键（与 `app.rs` / `keyboard_shortcuts.rs` / `terminal_keys.rs` 对齐），已按平台统一修饰键。
fn reserved_app_shortcuts(platform: KeyPlatform) -> Vec<FragmentShortcut> {
    let mut out = Vec::new();
    match platform {
        KeyPlatform::Mac => {
            let cmd = |k: &str| FragmentShortcut::new(k, false, false, false, true);
            let cmd_shift = |k: &str| FragmentShortcut::new(k, false, true, false, true);
            // 应用快捷键 + 系统编辑键（复制、粘贴、全选、撤销……）
            for k in [
                "N", "E", "J", "K", "B", "H", "F", "T", "W", "Q", "M", "C", "V", "X", "A", "Z",
                "Tab",
            ] {
                out.push(cmd(k));
            }
            for n in 1..=9 {
                out.push(cmd(&format!("Num{n}")));
            }
            // ⌘⇧：片段选择器、AI、分屏等；⌘⇧3/4/5 是系统截图
            for k in ["J", "A", "L", "D", "U", "N", "E", "H", "Z", "Tab", "Num3", "Num4", "Num5"] {
                out.push(cmd_shift(k));
            }
            out.push(FragmentShortcut::new("ArrowLeft", false, false, true, true));
            out.push(FragmentShortcut::new("ArrowRight", false, false, true, true));
            out.push(FragmentShortcut::new("F", true, false, false, true)); // 全屏 ⌃⌘F
        }
        KeyPlatform::Other => {
            let ctrl_shift = |k: &str| FragmentShortcut::new(k, true, true, false, false);
            // Ctrl+Shift：标签、片段选择器、AI、分屏、终端复制粘贴、SFTP 等
            for k in [
                "T", "W", "J", "A", "L", "D", "U", "S", "C", "V", "N", "E", "H", "Tab",
                "ArrowLeft", "ArrowRight", "F9", "F10", "Backspace", "Escape",
            ] {
                out.push(ctrl_shift(k));
            }
        }
    }
    out
}

fn is_letter_key(key: &str) -> bool {
    let k = key.to_ascii_uppercase();
    k.len() == 1 && k.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
}

/// 校验快捷键是否可保存（按当前平台）。
pub fn validate_shortcut(
    store: &FragmentShortcutStore,
    shortcut: &FragmentShortcut,
    except_fragment_id: Option<&str>,
) -> Result<(), ShortcutConflict> {
    validate_shortcut_for(store, shortcut, except_fragment_id, KeyPlatform::current())
}

/// 规则（尽量小，和内置快捷键一致）：
/// - Windows/Linux：必须同时按 Ctrl+Shift（可再加 Alt）。Ctrl+键留给 shell，Alt 组合在终端里按词移动。
/// - Mac：必须带 ⌘。Ctrl 组合留给 shell，Option 组合会打出字符。
/// - 单个键、只按 Shift 一律不行；内置快捷键和别的片段占用的也不行。
pub fn validate_shortcut_for(
    store: &FragmentShortcutStore,
    shortcut: &FragmentShortcut,
    except_fragment_id: Option<&str>,
    platform: KeyPlatform,
) -> Result<(), ShortcutConflict> {
    let sc = shortcut.normalized_for(platform);
    if !sc.ctrl && !sc.alt && !sc.command {
        return Err(ShortcutConflict::NeedsModifier);
    }

    let primary_ok = match platform {
        KeyPlatform::Mac => sc.command,
        KeyPlatform::Other => sc.ctrl && sc.shift,
    };
    if !primary_ok {
        if sc.ctrl && !sc.alt {
            // Ctrl+键 / Ctrl+数字（Win/Linux 上 Ctrl+1..9 也是切标签）
            if is_letter_key(&sc.key) || platform == KeyPlatform::Mac {
                return Err(ShortcutConflict::ShellCtrlLetter);
            }
            if let Some(n) = sc.key.strip_prefix("Num") {
                if !n.is_empty() {
                    return Err(ShortcutConflict::ReservedApp(sc.display_label()));
                }
            }
            return Err(ShortcutConflict::ShellCtrlLetter);
        }
        return Err(ShortcutConflict::NeedsCombo);
    }

    for reserved in reserved_app_shortcuts(platform) {
        if reserved.same_combo(&sc, platform) {
            return Err(ShortcutConflict::ReservedApp(reserved.display_label()));
        }
    }

    if let Some(other) = store.conflict_fragment_id_for(&sc, except_fragment_id, platform) {
        return Err(ShortcutConflict::OtherFragment(other));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN: KeyPlatform = KeyPlatform::Other;
    const MAC: KeyPlatform = KeyPlatform::Mac;

    /// Windows/Linux 上 egui 实际报上来的样子：按 Ctrl 时 ctrl 和 command 都是 true。
    fn win(key: &str, ctrl: bool, shift: bool, alt: bool) -> FragmentShortcut {
        FragmentShortcut::new(key, ctrl, shift, alt, ctrl)
    }

    fn check(sc: &FragmentShortcut, p: KeyPlatform) -> Result<(), ShortcutConflict> {
        validate_shortcut_for(&FragmentShortcutStore::default(), sc, None, p)
    }

    #[test]
    fn win_ctrl_letters_rejected_even_with_command_flag() {
        for k in ["A", "C", "R", "D", "Z", "L"] {
            assert_eq!(
                check(&win(k, true, false, false), WIN),
                Err(ShortcutConflict::ShellCtrlLetter),
                "Ctrl+{k}"
            );
        }
    }

    #[test]
    fn win_builtin_shortcuts_rejected() {
        assert!(matches!(
            check(&win("T", true, true, false), WIN),
            Err(ShortcutConflict::ReservedApp(_))
        ));
        assert!(matches!(
            check(&win("C", true, true, false), WIN),
            Err(ShortcutConflict::ReservedApp(_))
        ));
        assert!(matches!(
            check(&win("Num1", true, false, false), WIN),
            Err(ShortcutConflict::ReservedApp(_))
        ));
    }

    #[test]
    fn shift_only_and_bare_keys_rejected() {
        for p in [WIN, MAC] {
            assert_eq!(
                check(&FragmentShortcut::new("A", false, true, false, false), p),
                Err(ShortcutConflict::NeedsModifier)
            );
            assert_eq!(
                check(&FragmentShortcut::new("F5", false, false, false, false), p),
                Err(ShortcutConflict::NeedsModifier)
            );
        }
    }

    #[test]
    fn alt_and_option_only_rejected() {
        assert_eq!(
            check(&FragmentShortcut::new("A", false, false, true, false), MAC),
            Err(ShortcutConflict::NeedsCombo)
        );
        assert_eq!(
            check(&FragmentShortcut::new("A", false, true, true, false), MAC),
            Err(ShortcutConflict::NeedsCombo)
        );
        assert_eq!(
            check(&win("B", false, false, true), WIN),
            Err(ShortcutConflict::NeedsCombo)
        );
        // Mac 上不带 ⌘ 的 Ctrl 组合留给 shell
        assert_eq!(
            check(&FragmentShortcut::new("Y", true, true, false, false), MAC),
            Err(ShortcutConflict::ShellCtrlLetter)
        );
    }

    #[test]
    fn mac_builtin_and_edit_keys_rejected() {
        for k in ["C", "V", "A", "Z", "T", "Num1"] {
            assert!(
                matches!(
                    check(&FragmentShortcut::new(k, false, false, false, true), MAC),
                    Err(ShortcutConflict::ReservedApp(_))
                ),
                "⌘{k}"
            );
        }
    }

    #[test]
    fn valid_combos_accepted() {
        assert_eq!(check(&win("Y", true, true, false), WIN), Ok(()));
        assert_eq!(check(&win("Y", true, true, true), WIN), Ok(()));
        assert_eq!(
            check(&FragmentShortcut::new("Y", false, true, false, true), MAC),
            Ok(())
        );
        assert_eq!(
            check(&FragmentShortcut::new("Y", false, false, true, true), MAC),
            Ok(())
        );
    }

    #[test]
    fn other_fragment_conflict_ignores_command_flag() {
        let mut store = FragmentShortcutStore::default();
        // 旧版存下的（不带 command）与新录制的（带 command）是同一个组合
        store
            .bindings
            .insert("a".into(), FragmentShortcut::new("Y", true, true, false, false));
        assert_eq!(
            validate_shortcut_for(&store, &win("Y", true, true, false), Some("b"), WIN),
            Err(ShortcutConflict::OtherFragment("a".into()))
        );
        assert!(validate_shortcut_for(&store, &win("Y", true, true, false), Some("a"), WIN).is_ok());
    }

    #[test]
    fn normalized_merges_command_into_ctrl_on_win_linux() {
        let sc = win("Y", true, true, false).normalized_for(WIN);
        assert!(sc.ctrl && !sc.command);
        let mac = FragmentShortcut::new("Y", false, true, false, true).normalized_for(MAC);
        assert!(mac.command && !mac.ctrl);
    }

    #[test]
    fn prune_drops_missing_fragments() {
        let mut store = FragmentShortcutStore::default();
        store.set("keep".into(), win("Y", true, true, false));
        store.set("gone".into(), win("K", true, true, false));
        assert_eq!(store.prune_missing(|id| id == "keep"), 1);
        assert!(store.get("keep").is_some() && store.get("gone").is_none());
    }

    #[test]
    fn display_label_has_modifiers() {
        let sc = FragmentShortcut::new("J", true, true, false, false);
        let label = sc.display_label();
        assert!(label.contains('J') || label.contains('j'));
    }

    /// GUI E2E 落盘后：本机配置里应能读到 Ctrl+Shift+Y（或用户自测组合）。
    /// 默认忽略；跑完 `.tmp-gui-define-fragment-shortcut.py` 后用
    /// `cargo test --lib gui_e2e_fragment_shortcut_binding_persisted -- --ignored --nocapture`。
    #[test]
    #[ignore]
    fn gui_e2e_fragment_shortcut_binding_persisted() {
        let store = FragmentShortcutStore::load();
        eprintln!("bindings={:?}", store.bindings.len());
        for (id, sc) in &store.bindings {
            eprintln!("  {id} -> {}", sc.display_label());
        }
        assert!(
            store.bindings.values().any(|sc| {
                sc.key.eq_ignore_ascii_case("O") && sc.shift && (sc.ctrl || sc.command)
            }) || store.bindings.values().any(|sc| {
                sc.key.eq_ignore_ascii_case("Y") && sc.shift && (sc.ctrl || sc.command)
            }),
            "expected a Ctrl+Shift+O/Y fragment shortcut after GUI E2E"
        );
    }

    /// 与 `poll_fragment_shortcut` 一致：egui `Key` 的 `Debug` 名 + Win 上 ctrl/command 同开。
    #[test]
    fn find_matching_follows_egui_key_debug_and_win_modifiers() {
        let mut store = FragmentShortcutStore::default();
        store.set("frag1".into(), win("Y", true, true, false));
        assert_eq!(
            store.find_matching_fragment_id("Y", true, true, false, true),
            Some("frag1".into())
        );
        assert!(
            store
                .find_matching_fragment_id("Y", true, false, false, true)
                .is_none()
        );
        assert!(
            store
                .find_matching_fragment_id("Z", true, true, false, true)
                .is_none()
        );
    }
}
