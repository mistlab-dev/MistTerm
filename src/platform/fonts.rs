//! egui 中文字体：内置 Noto 优先，各平台系统字体回退；终端等宽字体预设与本机枚举。

use eframe::egui;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

/// 新建终端与偏好设置中的默认字号（px）。
pub const DEFAULT_TERMINAL_FONT_SIZE: f32 = 13.0;
pub const TERMINAL_FONT_SIZE_MIN: f32 = 10.0;
pub const TERMINAL_FONT_SIZE_MAX: f32 = 24.0;

/// 选中的字体文件超过这个大小就不再 `read` 进内存。
const MAX_SELECTED_FONT_BYTES: u64 = 32 * 1024 * 1024;

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

/// 偏好里选中的终端字体。`family` 非空时覆盖预设，身份是家族名（跨机器可移植）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalFontSelection {
    pub preset: TerminalFontPreset,
    family: String,
}

impl Default for TerminalFontSelection {
    fn default() -> Self {
        Self::preset(TerminalFontPreset::Default)
    }
}

impl TerminalFontSelection {
    pub fn preset(preset: TerminalFontPreset) -> Self {
        Self {
            preset,
            family: String::new(),
        }
    }

    pub fn family(family: impl Into<String>) -> Self {
        Self {
            preset: TerminalFontPreset::Default,
            family: family.into(),
        }
    }

    /// 用户选中的本机家族名。空表示走 [`TerminalFontPreset`]。
    pub fn family_name(&self) -> Option<&str> {
        let name = self.family.trim();
        if name.is_empty() {
            None
        } else {
            Some(name)
        }
    }
}

/// 预设在下拉里的家族名。`Default` 没有固定家族。
pub fn terminal_preset_family_name(preset: TerminalFontPreset) -> Option<&'static str> {
    match preset {
        TerminalFontPreset::Default => None,
        TerminalFontPreset::Consolas => Some("Consolas"),
        TerminalFontPreset::CascadiaMono => Some("Cascadia Mono"),
        TerminalFontPreset::JetBrainsMono => Some("JetBrains Mono"),
    }
}

/// 预设是否能在本机点到。系统默认始终可选；其余看枚举到的家族名，
/// 再用严格文件名兜底（不含 DejaVu / Menlo 这类回退）。
pub fn terminal_preset_installed(preset: TerminalFontPreset) -> bool {
    match terminal_preset_family_name(preset) {
        None => true,
        Some(name) => {
            let from_scan = super::font_enum::system_mono_fonts_if_ready().is_some_and(|fonts| {
                fonts
                    .iter()
                    .any(|f| super::font_enum::family_eq(&f.family, name))
            });
            from_scan || preset_strict_paths(preset).iter().any(|p| p.is_file())
        }
    }
}

/// 本机等宽家族（已去重、已排序）。
///
/// 阻塞到扫描结束。偏好下拉用 [`system_mono_fonts_if_ready`]。
pub fn list_system_mono_fonts() -> &'static [super::font_enum::SystemMonoFont] {
    super::font_enum::list_system_mono_fonts()
}

/// 在后台开始枚举。已经在扫、或已经扫完，则直接返回。
pub fn start_system_mono_font_scan() {
    super::font_enum::start_system_mono_font_scan();
}

/// 枚举完成则返回缓存；否则 `None`，并确保后台扫描已启动。
pub fn system_mono_fonts_if_ready() -> Option<&'static [SystemMonoFont]> {
    super::font_enum::system_mono_fonts_if_ready()
}

/// 已保存的家族名还在等后台枚举，启动时先用了系统默认。
pub fn terminal_font_apply_deferred() -> bool {
    DEFERRED_FAMILY_APPLY.load(Ordering::Acquire)
}

/// 后台枚举完成后返回 `true` 一次。调用方应再跑 [`configure_terminal_font`]。
pub fn take_deferred_terminal_font_apply() -> bool {
    if !terminal_font_apply_deferred() || !super::font_enum::system_mono_fonts_ready() {
        return false;
    }
    DEFERRED_FAMILY_APPLY.swap(false, Ordering::AcqRel)
}

/// 家族名已选定，但枚举还没结束，先画系统默认，扫完再换。
static DEFERRED_FAMILY_APPLY: AtomicBool = AtomicBool::new(false);

pub use super::font_enum::{family_eq as font_family_eq, SystemMonoFont};

static CJK_FONT_LOADED: AtomicBool = AtomicBool::new(false);

/// 嵌入/系统 CJK 字体在 egui 中的注册名（可作 `FontFamily::Name`）。
pub const CJK_UI_FONT_NAME: &str = "mistterm-cjk";

/// 最近一次 `configure_egui_fonts` 是否成功注册 CJK 字体。
pub fn cjk_font_loaded() -> bool {
    CJK_FONT_LOADED.load(Ordering::Relaxed)
}

/// 界面统一正文字体（中英同一套；CJK 未加载时回退 egui 默认比例字体）。
pub fn ui_font_id(size: f32) -> egui::FontId {
    if cjk_font_loaded() {
        egui::FontId::new(size, egui::FontFamily::Name(CJK_UI_FONT_NAME.into()))
    } else {
        egui::FontId::proportional(size)
    }
}

/// 说明链接等与 [`ui_font_id`] 相同（保留别名，避免旧调用点遗漏）。
#[inline]
pub fn ui_caption_font_id(size: f32) -> egui::FontId {
    ui_font_id(size)
}

/// 为 egui 注册字体：界面比例字体统一为 CJK（含拉丁）；终端等宽仍用预设 + CJK 回退。
pub fn configure_egui_fonts(ctx: &egui::Context, terminal_preset: TerminalFontPreset) -> bool {
    configure_terminal_font(ctx, &TerminalFontSelection::preset(terminal_preset))
}

/// 同 [`configure_egui_fonts`]，但可以选本机枚举到的家族名。
pub fn configure_terminal_font(ctx: &egui::Context, selection: &TerminalFontSelection) -> bool {
    let ppp = ctx.pixels_per_point();
    log::info!("egui pixels_per_point = {ppp:.2} (HiDPI reference)");

    let mut fonts = egui::FontDefinitions::default();
    if let Some((bytes, index)) = load_selected_terminal_font(selection) {
        let mono_name = "mistterm-terminal-mono".to_string();
        // 行高已用 FACTOR 收紧，勿再大幅 y_offset，否则字下移而块光标仍贴格顶，看起来光标偏低。
        // 字节已在 load_selected_terminal_font 里确认有轮廓且 ab_glyph 能解析。
        // epaint 对解析失败的字体会在下一帧 Fonts::new 里 panic，set_fonts 本身只排队。
        let mut mono_data = egui::FontData::from_owned(bytes);
        mono_data.index = index;
        let mono_data = mono_data.tweak(egui::FontTweak {
            y_offset_factor: 0.02,
            baseline_offset_factor: -0.02,
            ..Default::default()
        });
        fonts.font_data.insert(mono_name.clone(), std::sync::Arc::new(mono_data));
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .insert(0, mono_name);
    }

    let loaded = if let Some(cjk_font) = load_cjk_font() {
        let cjk_name = CJK_UI_FONT_NAME.to_string();
        fonts.font_data.insert(cjk_name.clone(), std::sync::Arc::new(cjk_font));

        // 界面：CJK 置顶为唯一首选，中英混排同一字形/基线；egui 默认拉丁仅作缺字兜底。
        {
            let entry = fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default();
            entry.retain(|n| n != &cjk_name);
            entry.insert(0, cjk_name.clone());
        }
        // 终端等宽：拉丁/符号走 mono 预设，中文回退 CJK。
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .push(cjk_name.clone());
        fonts.families.insert(
            egui::FontFamily::Name(CJK_UI_FONT_NAME.into()),
            vec![cjk_name],
        );
        true
    } else {
        false
    };

    // UI 拉丁字体：Geist（对齐设计稿的现代无衬线），置 Proportional 首位；
    // CJK 仍作缺字兜底（中文回退 NotoSansSC，英文/数字用 Geist）。
    {
        const GEIST_UI: &[u8] = include_bytes!("../../assets/fonts/Geist-Regular.ttf");
        let name = "geist-ui".to_string();
        fonts
            .font_data
            .insert(name.clone(), std::sync::Arc::new(egui::FontData::from_static(GEIST_UI)));
        let entry = fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default();
        entry.retain(|n| n != &name);
        entry.insert(0, name);
    }

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

fn load_selected_terminal_font(selection: &TerminalFontSelection) -> Option<(Vec<u8>, u32)> {
    if let Some(family) = selection.family_name() {
        if super::font_enum::terminal_family_rejected(family) {
            log::warn!(
                "terminal font family {family:?} is unsupported (bitmap-only or non-outline); using system default"
            );
            DEFERRED_FAMILY_APPLY.store(false, Ordering::Release);
            return load_terminal_preset_font(TerminalFontPreset::Default).map(|bytes| (bytes, 0));
        }
        if !super::font_enum::system_mono_fonts_ready() {
            super::font_enum::start_system_mono_font_scan();
            DEFERRED_FAMILY_APPLY.store(true, Ordering::Release);
            log::info!(
                "terminal font family {family:?} waiting for monospace scan; using system default until then"
            );
            return load_terminal_preset_font(TerminalFontPreset::Default).map(|bytes| (bytes, 0));
        }
        DEFERRED_FAMILY_APPLY.store(false, Ordering::Release);
        if let Some(face) = super::font_enum::resolve_mono_family(family) {
            match read_font_file_capped(&face.path) {
                FontRead::Ok(bytes) => {
                    if let Some(loaded) = accept_terminal_font(bytes, face.index) {
                        log::info!(
                            "Loaded terminal font family {family:?}: {} (face {})",
                            face.path.display(),
                            face.index
                        );
                        return Some(loaded);
                    }
                    log::warn!(
                        "terminal font family {family:?} at {} cannot be rasterized by egui; using system default",
                        face.path.display()
                    );
                }
                FontRead::TooLarge(len) => {
                    log::warn!(
                        "terminal font family {family:?} at {} is {len} bytes, over the {MAX_SELECTED_FONT_BYTES} byte limit; using system default",
                        face.path.display()
                    );
                }
                FontRead::Failed(e) => {
                    log::warn!(
                        "Terminal font family {family:?} at {} unreadable: {e}",
                        face.path.display()
                    );
                }
            }
        } else {
            log::warn!("Terminal font family {family:?} is not installed; using system default");
        }
        return load_terminal_preset_font(TerminalFontPreset::Default).map(|bytes| (bytes, 0));
    }
    DEFERRED_FAMILY_APPLY.store(false, Ordering::Release);
    load_terminal_preset_font(selection.preset).map(|bytes| (bytes, 0))
}

#[derive(Debug)]
enum FontRead {
    Ok(Vec<u8>),
    TooLarge(u64),
    Failed(std::io::Error),
}

/// 有轮廓且 ab_glyph 能解析才采用。失败时调用方回退系统默认，避免 epaint panic。
fn accept_terminal_font(bytes: Vec<u8>, index: u32) -> Option<(Vec<u8>, u32)> {
    if terminal_font_bytes_usable(&bytes, index) {
        Some((bytes, index))
    } else {
        None
    }
}

fn terminal_font_bytes_usable(bytes: &[u8], index: u32) -> bool {
    if !super::font_enum::font_bytes_have_outline(bytes, index) {
        return false;
    }
    egui_can_parse_font(bytes, index)
}

/// 与 epaint `ab_glyph_font_from_font_data` 同一条解析路径。
/// 解析失败时 epaint 会 `panic!("Error parsing {name:?} TTF/OTF font file")`。
fn egui_can_parse_font(bytes: &[u8], index: u32) -> bool {
    use ab_glyph::Font as _;
    match ab_glyph::FontRef::try_from_slice_and_index(bytes, index) {
        Ok(font) => font.units_per_em().is_some(),
        Err(_) => false,
    }
}

fn read_font_file_capped(path: &std::path::Path) -> FontRead {
    match std::fs::metadata(path) {
        Ok(meta) => {
            let len = meta.len();
            if len > MAX_SELECTED_FONT_BYTES {
                return FontRead::TooLarge(len);
            }
        }
        Err(e) => return FontRead::Failed(e),
    }
    match std::fs::read(path) {
        Ok(bytes) if bytes.len() as u64 > MAX_SELECTED_FONT_BYTES => {
            FontRead::TooLarge(bytes.len() as u64)
        }
        Ok(bytes) => FontRead::Ok(bytes),
        Err(e) => FontRead::Failed(e),
    }
}

fn load_terminal_preset_font(preset: TerminalFontPreset) -> Option<Vec<u8>> {
    let candidates = match preset {
        // Default：优先系统 Regular 等宽（与 Windows Terminal / 常见终端观感接近），
        // 绝不回退到 Bold 字重文件。
        TerminalFontPreset::Default => default_mono_font_candidates(),
        other => terminal_preset_candidates(other),
    };
    for path in candidates {
        match std::fs::read(&path) {
            Ok(bytes) if terminal_font_bytes_usable(&bytes, 0) => {
                log::info!(
                    "Loaded terminal font preset {:?}: {}",
                    preset,
                    path.display()
                );
                return Some(bytes);
            }
            Ok(_) => log::warn!(
                "Skipped terminal font {} (no outline egui can rasterize)",
                path.display()
            ),
            Err(e) => log::debug!("Skipped terminal font {}: {e}", path.display()),
        }
    }
    log::warn!(
        "Terminal font preset {:?} not found on this system; using egui default monospace",
        preset
    );
    None
}

/// 跨平台默认等宽：仅 Regular。Windows 优先 Consolas（与 PowerShell 观感接近）。
fn default_mono_font_candidates() -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    #[cfg(target_os = "windows")]
    {
        paths.extend(consolas_font_candidates());
        paths.extend(cascadia_mono_font_candidates());
    }
    #[cfg(not(target_os = "windows"))]
    {
        paths.extend(cascadia_mono_font_candidates());
        paths.extend(consolas_font_candidates());
    }
    paths.extend(jetbrains_mono_font_candidates());
    paths
}

fn terminal_preset_candidates(preset: TerminalFontPreset) -> Vec<std::path::PathBuf> {
    match preset {
        TerminalFontPreset::Default => default_mono_font_candidates(),
        TerminalFontPreset::Consolas => consolas_font_candidates(),
        TerminalFontPreset::CascadiaMono => cascadia_mono_font_candidates(),
        TerminalFontPreset::JetBrainsMono => jetbrains_mono_font_candidates(),
    }
}

fn consolas_font_candidates() -> Vec<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    {
        // 仅 Regular（consola.ttf）；不要 consolab.ttf（Bold）
        return system_fonts_dir()
            .into_iter()
            .map(|dir| dir.join("consola.ttf"))
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
                std::path::PathBuf::from(&home).join("Library/Fonts/JetBrainsMono-Regular.ttf"),
            );
            paths.push(
                std::path::PathBuf::from(&home).join("Library/Fonts/JetBrains Mono Regular.ttf"),
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
                std::path::PathBuf::from(home).join(".local/share/fonts/JetBrainsMono-Regular.ttf"),
            );
        }
    }
    paths
}

/// 预设「真的装了」才算，不含 Default 那条 DejaVu / Menlo 回退链。
fn preset_strict_paths(preset: TerminalFontPreset) -> Vec<std::path::PathBuf> {
    match preset {
        TerminalFontPreset::Default => Vec::new(),
        TerminalFontPreset::Consolas => consolas_strict_paths(),
        TerminalFontPreset::CascadiaMono => cascadia_mono_font_candidates()
            .into_iter()
            .filter(|p| {
                p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                    n.eq_ignore_ascii_case("CascadiaMono.ttf")
                        || n.eq_ignore_ascii_case("CascadiaMono.otf")
                })
            })
            .collect(),
        TerminalFontPreset::JetBrainsMono => jetbrains_mono_font_candidates(),
    }
}

fn consolas_strict_paths() -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    #[cfg(target_os = "windows")]
    {
        paths.extend(consolas_font_candidates());
    }
    #[cfg(target_os = "macos")]
    {
        paths.push(std::path::PathBuf::from("/Library/Fonts/Consolas.ttf"));
        paths.push(std::path::PathBuf::from("/Library/Fonts/consola.ttf"));
        if let Ok(home) = std::env::var("HOME") {
            let home = std::path::PathBuf::from(home);
            paths.push(home.join("Library/Fonts/Consolas.ttf"));
            paths.push(home.join("Library/Fonts/consola.ttf"));
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        for dir in [
            "/usr/share/fonts/truetype/consolas",
            "/usr/local/share/fonts",
            "/usr/share/fonts",
        ] {
            let dir = std::path::PathBuf::from(dir);
            paths.push(dir.join("consola.ttf"));
            paths.push(dir.join("Consolas.ttf"));
        }
        if let Ok(home) = std::env::var("HOME") {
            let home = std::path::PathBuf::from(home);
            for rel in [".local/share/fonts", ".fonts"] {
                paths.push(home.join(rel).join("consola.ttf"));
                paths.push(home.join(rel).join("Consolas.ttf"));
            }
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

fn load_cjk_font() -> Option<std::sync::Arc<egui::FontData>> {
    if let Some(data) = load_embedded_cjk_font() {
        return Some(data);
    }
    for path in cjk_font_candidates() {
        match std::fs::read(&path) {
            Ok(bytes) => {
                log::info!("Loaded system CJK font: {}", path.display());
                return Some(std::sync::Arc::new(egui::FontData::from_owned(bytes)));
            }
            Err(e) => log::debug!("Skipped CJK font {}: {e}", path.display()),
        }
    }
    None
}

fn load_embedded_cjk_font() -> Option<std::sync::Arc<egui::FontData>> {
    const BYTES: &[u8] = include_bytes!("../../assets/fonts/NotoSansSC-Regular.otf");
    if BYTES.is_empty() {
        return None;
    }
    log::info!(
        "Loaded embedded CJK font NotoSansSC-Regular.otf ({} bytes)",
        BYTES.len()
    );
    Some(std::sync::Arc::new(egui::FontData::from_static(BYTES)))
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
        assert_eq!(TerminalFontPreset::default(), TerminalFontPreset::Default);
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

    #[test]
    fn family_selection_overrides_preset_identity() {
        let preset = TerminalFontSelection::preset(TerminalFontPreset::CascadiaMono);
        assert!(preset.family_name().is_none());
        assert_eq!(preset.preset, TerminalFontPreset::CascadiaMono);

        let family = TerminalFontSelection::family("  DejaVu Sans Mono  ");
        assert_eq!(family.family_name(), Some("DejaVu Sans Mono"));
        assert_eq!(family.preset, TerminalFontPreset::Default);
    }

    #[test]
    fn preset_family_names_are_exact() {
        assert_eq!(
            terminal_preset_family_name(TerminalFontPreset::JetBrainsMono),
            Some("JetBrains Mono")
        );
        assert_eq!(
            terminal_preset_family_name(TerminalFontPreset::Default),
            None
        );
    }

    #[test]
    fn oversized_font_file_is_rejected_before_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("huge.ttf");
        let len = MAX_SELECTED_FONT_BYTES + 1;
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(len).unwrap();
        drop(file);
        match read_font_file_capped(&path) {
            FontRead::TooLarge(n) => assert_eq!(n, len),
            other => panic!("expected size rejection, got {other:?}"),
        }
    }

    #[test]
    fn bitmap_only_font_is_not_applied_and_real_outline_is() {
        let bitmap = pack_sfnt(&[
            (b"bdat", vec![1, 2, 3, 4]),
            (b"bloc", vec![1, 2, 3, 4]),
            (b"bhed", vec![1, 2, 3, 4]),
        ]);
        assert!(!terminal_font_bytes_usable(&bitmap, 0));
        assert!(accept_terminal_font(bitmap, 0).is_none());

        let geist = include_bytes!("../../assets/fonts/Geist-Regular.ttf");
        assert!(terminal_font_bytes_usable(geist, 0));
        assert!(accept_terminal_font(geist.to_vec(), 0).is_some());
    }

    #[test]
    fn glyf_tag_without_a_parseable_face_falls_back() {
        let bogus = pack_sfnt(&[(b"glyf", vec![0, 0, 0, 0])]);
        assert!(super::super::font_enum::font_bytes_have_outline(&bogus, 0));
        assert!(!terminal_font_bytes_usable(&bogus, 0));
        assert!(accept_terminal_font(bogus, 0).is_none());
    }

    fn pack_sfnt(tables: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let n = tables.len() as u16;
        let mut out = Vec::new();
        out.extend_from_slice(&0x00010000u32.to_be_bytes());
        out.extend_from_slice(&n.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        let dir_at = out.len();
        out.resize(dir_at + tables.len() * 16, 0);
        for (i, (tag, data)) in tables.iter().enumerate() {
            while out.len() % 4 != 0 {
                out.push(0);
            }
            let offset = out.len() as u32;
            let rec = dir_at + i * 16;
            out[rec..rec + 4].copy_from_slice(tag.as_slice());
            out[rec + 8..rec + 12].copy_from_slice(&offset.to_be_bytes());
            out[rec + 12..rec + 16].copy_from_slice(&(data.len() as u32).to_be_bytes());
            out.extend_from_slice(data);
        }
        out
    }
}
