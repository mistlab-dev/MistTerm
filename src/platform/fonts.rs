//! egui 中文字体：内置 Noto 优先，各平台系统字体回退。

use eframe::egui;
use std::sync::atomic::{AtomicBool, Ordering};

static CJK_FONT_LOADED: AtomicBool = AtomicBool::new(false);

/// 最近一次 `configure_egui_fonts` 是否成功注册 CJK 字体。
pub fn cjk_font_loaded() -> bool {
    CJK_FONT_LOADED.load(Ordering::Relaxed)
}

/// 为 egui 注册 CJK 回退字体（拉丁仍用 egui 自带字体）。成功返回 `true`。
pub fn configure_egui_fonts(ctx: &egui::Context) -> bool {
    let ppp = ctx.pixels_per_point();
    log::info!("egui pixels_per_point = {ppp:.2} (HiDPI reference)");

    let mut fonts = egui::FontDefinitions::default();
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
