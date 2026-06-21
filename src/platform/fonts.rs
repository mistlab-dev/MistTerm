//! egui 中文字体：内置 Noto 优先，各平台系统字体回退；终端等宽字体预设。

use eframe::egui;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

/// 新建终端与偏好设置中的默认字号（px）。
pub const DEFAULT_TERMINAL_FONT_SIZE: f32 = 13.0;
pub const TERMINAL_FONT_SIZE_MIN: f32 = 10.0;
pub const TERMINAL_FONT_SIZE_MAX: f32 = 24.0;

/// 限制终端字号在可渲染范围内。
pub fn clamp_terminal_font_size(size: f32) -> f32 {
    size.clamp(TERMINAL_FONT_SIZE_MIN, TERMINAL_FONT_SIZE_MAX)
}

/// 终端等宽字体预设（插入 `FontFamily::Monospace` 族首位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalFontPreset {
    #[default]
    Default,
    Consolas,
    CascadiaMono,
    JetBrainsMono,
}

impl TerminalFontPreset {
    pub const ALL: [Self; 4] = [
        Self::Default,
        Self::Consolas,
        Self::CascadiaMono,
        Self::JetBrainsMono,
    ];
}

static CJK_FONT_LOADED: AtomicBool = AtomicBool::new(false);

/// 最近一次 `configure_egui_fonts` 是否成功注册 CJK 字体。
pub fn cjk_font_loaded() -> bool {
    CJK_FONT_LOADED.load(Ordering::Relaxed)
}

/// 为 egui 注册终端等宽字体与 CJK 回退（拉丁 UI 仍用 egui 自带字体）。成功加载 CJK 返回 `true`。
pub fn configure_egui_fonts(ctx: &egui::Context, terminal_preset: TerminalFontPreset) -> bool {
    let ppp = ctx.pixels_per_point();
    log::info!("egui pixels_per_point = {ppp:.2} (HiDPI reference)");

    let mut fonts = egui::FontDefinitions::default();
    if let Some(bytes) = load_terminal_preset_font(terminal_preset) {
        let mono_name = "mistterm-terminal-mono".to_string();
        fonts
            .font_data
            .insert(mono_name.clone(), egui::FontData::from_owned(bytes));
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .insert(0, mono_name);
    }

    let loaded = if let Some(cjk_font) = load_cjk_font() {
        let cjk_name = "mistterm-cjk".to_string();
        fonts.font_data.insert(cjk_name.clone(), cjk_font);

        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .push(cjk_name.clone());
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .push(cjk_name);
        true
    } else {
        false
    };

    CJK_FONT_LOADED.store(loaded, Ordering::Relaxed);
    ctx.set_fonts(fonts);
    if !loaded {
        log::warn!("CJK font not loaded; Chinese UI text may show as tofu");
        ctx.data_mut(|d| {
            d.insert_temp(egui::Id::new("mist_cjk_font_missing"), ());
        });
    }
    loaded
}

fn load_terminal_preset_font(preset: TerminalFontPreset) -> Option<Vec<u8>> {
    if preset == TerminalFontPreset::Default {
        return None;
    }
    for path in terminal_preset_candidates(preset) {
        match std::fs::read(&path) {
            Ok(bytes) => {
                log::info!(
                    "Loaded terminal font preset {:?}: {}",
                    preset,
                    path.display()
                );
                return Some(bytes);
            }
            Err(e) => log::debug!("Skipped terminal font {}: {e}", path.display()),
        }
    }
    log::warn!(
        "Terminal font preset {:?} not found on this system; using egui default monospace",
        preset
    );
    None
}

fn terminal_preset_candidates(preset: TerminalFontPreset) -> Vec<std::path::PathBuf> {
    match preset {
        TerminalFontPreset::Default => Vec::new(),
        TerminalFontPreset::Consolas => consolas_font_candidates(),
        TerminalFontPreset::CascadiaMono => cascadia_mono_font_candidates(),
        TerminalFontPreset::JetBrainsMono => jetbrains_mono_font_candidates(),
    }
}

fn consolas_font_candidates() -> Vec<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    {
        return system_fonts_dir()
            .into_iter()
            .flat_map(|dir| [dir.join("consola.ttf"), dir.join("consolab.ttf")])
            .collect();
    }
    #[cfg(target_os = "macos")]
    {
        return [
            "/System/Library/Fonts/Menlo.ttc",
            "/System/Library/Fonts/SFNSMono.ttf",
            "/Library/Fonts/Consolas.ttf",
        ]
        .into_iter()
        .map(std::path::PathBuf::from)
        .collect();
    }
    #[cfg(target_os = "linux")]
    {
        return [
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
            "/usr/share/fonts/truetype/liberation2/LiberationMono-Regular.ttf",
        ]
        .into_iter()
        .map(std::path::PathBuf::from)
        .collect();
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        Vec::new()
    }
}

fn cascadia_mono_font_candidates() -> Vec<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    {
        return system_fonts_dir()
            .into_iter()
            .flat_map(|dir| {
                [
                    dir.join("CascadiaMono.ttf"),
                    dir.join("CascadiaCode.ttf"),
                    dir.join("CascadiaMonoPL.ttf"),
                    dir.join("CascadiaCodePL.ttf"),
                ]
            })
            .collect();
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        return [
            format!("{home}/Library/Fonts/CascadiaMono.ttf"),
            format!("{home}/Library/Fonts/Cascadia Code.ttf"),
            "/Library/Fonts/CascadiaMono.ttf".to_string(),
        ]
        .into_iter()
        .map(std::path::PathBuf::from)
        .collect();
    }
    #[cfg(target_os = "linux")]
    {
        return [
            "/usr/share/fonts/truetype/cascadia/CascadiaMono.ttf",
            "/usr/share/fonts/truetype/cascadia-code/CascadiaMono.ttf",
            "/usr/share/fonts/opentype/cascadia-code/CascadiaMono.ttf",
        ]
        .into_iter()
        .map(std::path::PathBuf::from)
        .collect();
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        Vec::new()
    }
}

fn jetbrains_mono_font_candidates() -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    #[cfg(target_os = "windows")]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            paths.push(
                std::path::PathBuf::from(local)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Fonts")
                    .join("JetBrainsMono-Regular.ttf"),
            );
        }
        if let Some(dir) = system_fonts_dir() {
            paths.push(dir.join("JetBrainsMono-Regular.ttf"));
            paths.push(dir.join("JetBrainsMono.ttf"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            paths.push(
                std::path::PathBuf::from(&home)
                    .join("Library/Fonts/JetBrainsMono-Regular.ttf"),
            );
            paths.push(
                std::path::PathBuf::from(&home)
                    .join("Library/Fonts/JetBrains Mono Regular.ttf"),
            );
        }
        paths.push(std::path::PathBuf::from(
            "/Library/Fonts/JetBrainsMono-Regular.ttf",
        ));
    }
    #[cfg(target_os = "linux")]
    {
        paths.extend(
            [
                "/usr/share/fonts/truetype/jetbrains-mono/JetBrainsMono-Regular.ttf",
                "/usr/share/fonts/truetype/jetbrains/JetBrainsMono-Regular.ttf",
                "/usr/local/share/fonts/JetBrainsMono-Regular.ttf",
            ]
            .into_iter()
            .map(std::path::PathBuf::from),
        );
        if let Ok(home) = std::env::var("HOME") {
            paths.push(
                std::path::PathBuf::from(home)
                    .join(".local/share/fonts/JetBrainsMono-Regular.ttf"),
            );
        }
    }
    paths
}

#[cfg(target_os = "windows")]
fn system_fonts_dir() -> Option<std::path::PathBuf> {
    std::env::var("WINDIR")
        .ok()
        .map(|w| std::path::PathBuf::from(w).join("Fonts"))
        .or_else(|| Some(std::path::PathBuf::from(r"C:\Windows\Fonts")))
}

fn load_cjk_font() -> Option<egui::FontData> {
    if let Some(data) = load_embedded_cjk_font() {
        return Some(data);
    }
    for path in cjk_font_candidates() {
        match std::fs::read(&path) {
            Ok(bytes) => {
                log::info!("Loaded system CJK font: {}", path.display());
                return Some(egui::FontData::from_owned(bytes));
            }
            Err(e) => log::debug!("Skipped CJK font {}: {e}", path.display()),
        }
    }
    None
}

fn load_embedded_cjk_font() -> Option<egui::FontData> {
    const BYTES: &[u8] = include_bytes!("../../assets/fonts/NotoSansSC-Regular.otf");
    if BYTES.is_empty() {
        return None;
    }
    log::info!("Loaded embedded CJK font NotoSansSC-Regular.otf ({} bytes)", BYTES.len());
    Some(egui::FontData::from_static(BYTES))
}

/// 各平台系统自带中文字体路径（按优先级，内置失败时使用）。
pub fn cjk_font_candidates() -> Vec<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let fonts_dir = std::env::var("WINDIR")
            .map(|w| std::path::PathBuf::from(w).join("Fonts"))
            .unwrap_or_else(|_| std::path::PathBuf::from(r"C:\Windows\Fonts"));
        return [
            "msyh.ttc",
            "msyhbd.ttc",
            "msyhl.ttc",
            "simhei.ttf",
            "simsun.ttc",
            "simsunb.ttf",
            "Deng.ttf",
            "Dengb.ttf",
            "NotoSansSC-VF.ttf",
        ]
        .into_iter()
        .map(|name| fonts_dir.join(name))
        .collect();
    }

    #[cfg(target_os = "macos")]
    {
        return [
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/System/Library/Fonts/Supplemental/Songti.ttc",
        ]
        .into_iter()
        .map(std::path::PathBuf::from)
        .collect();
    }

    #[cfg(target_os = "linux")]
    {
        return [
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJKSC-Regular.otf",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/wenquanyi/wqy-microhei.ttc",
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        ]
        .into_iter()
        .map(std::path::PathBuf::from)
        .collect();
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_font_preset_defaults_to_default() {
        assert_eq!(
            TerminalFontPreset::default(),
            TerminalFontPreset::Default
        );
    }

    #[test]
    fn clamp_terminal_font_size_bounds() {
        assert_eq!(clamp_terminal_font_size(8.0), TERMINAL_FONT_SIZE_MIN);
        assert_eq!(clamp_terminal_font_size(13.0), 13.0);
        assert_eq!(clamp_terminal_font_size(30.0), TERMINAL_FONT_SIZE_MAX);
    }

    #[test]
    fn terminal_font_preset_serde_roundtrip() {
        let json = serde_json::to_string(&TerminalFontPreset::CascadiaMono).unwrap();
        let parsed: TerminalFontPreset = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, TerminalFontPreset::CascadiaMono);
    }
}
