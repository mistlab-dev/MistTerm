//! 导出应用图标：`cargo run --bin export_app_icon`
//! - `assets/app-icon-preview.png`：256 窗口图标预览
//! - `assets/app-icon-1024.png`：macOS 打包源图(`scripts/bundle-macos.sh` 生成 .icns)
//! - `assets/app-icon.ico`：Windows exe 资源图标(`build.rs` 嵌入)与安装包图标(`scripts/MistTerm.iss`)
//! - `assets/social-preview.png`：GitHub 仓库 Social preview(1280×640，需在仓库 Settings 手动上传)
//!
//! 仅预览、不改动 assets：`cargo run --bin export_app_icon -- --preview <DIR>`

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::{ColorType, Rgba, RgbaImage, imageops};
use mistterm::ui::icons;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

type Exporter = fn(&Path) -> Result<(), image::ImageError>;

const SOCIAL_FONT: &[u8] = include_bytes!("../../assets/fonts/Geist-Regular.ttf");

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [flag, dir] if flag == "--preview" => export_preview(Path::new(dir)),
        _ => export_to(Path::new("assets")),
    };
    if let Err(e) = result {
        eprintln!("导出失败: {e}");
        std::process::exit(1);
    }
}

fn export_to(dir: &Path) -> Result<(), image::ImageError> {
    let _ = std::fs::create_dir_all(dir);
    let targets: [(&str, Exporter); 4] = [
        ("app-icon-preview.png", icons::export_app_icon_png),
        ("app-icon-1024.png", icons::export_app_icon_bundle_png),
        ("app-icon.ico", export_windows_ico),
        ("social-preview.png", export_social_preview),
    ];
    for (name, export) in targets {
        let path = dir.join(name);
        export(&path)?;
        println!("已写入 {}", path.display());
    }
    Ok(())
}

/// Windows 多尺寸 ICO：各尺寸原生渲染(小尺寸不靠缩放)，PNG 帧。
fn export_windows_ico(path: &Path) -> Result<(), image::ImageError> {
    const SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];
    let frames = SIZES
        .iter()
        .map(|&size| {
            let img = icons::render_app_icon(
                size,
                icons::APP_ICON_WINDOWS_PAD_FRAC,
                icons::APP_ICON_WINDOWS_CORNER_FRAC,
            );
            IcoFrame::as_png(img.as_raw(), size, size, ColorType::Rgba8)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let file = File::create(path).map_err(image::ImageError::IoError)?;
    IcoEncoder::new(BufWriter::new(file)).encode_images(&frames)
}

/// GitHub Social preview：左侧图标，右侧名称与简介。
fn export_social_preview(path: &Path) -> Result<(), image::ImageError> {
    const W: u32 = 1280;
    const H: u32 = 640;
    const ICON: u32 = 300;
    const ICON_X: u32 = 130;
    const TEXT_X: f32 = 500.0;
    let top = [14.0, 22.0, 44.0];
    let bottom = [7.0, 11.0, 22.0];
    let mut img = RgbaImage::from_fn(W, H, |_, y| {
        let t = y as f32 / (H - 1) as f32;
        let c = |i: usize| (top[i] + (bottom[i] - top[i]) * t) as u8;
        Rgba([c(0), c(1), c(2), 255])
    });

    let icon_y = (H - ICON) / 2;
    let (cx, cy) = ((ICON_X + ICON / 2) as f32, (icon_y + ICON / 2) as f32);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt() / 360.0;
        blend(px, [55, 175, 255], (1.0 - d).max(0.0).powi(2) * 0.22);
    }
    imageops::overlay(&mut img, &icons::render_app_icon_bundle(ICON), ICON_X as i64, icon_y as i64);

    let font = FontRef::try_from_slice(SOCIAL_FONT).expect("bundled Geist font");
    let lines: [(&str, f32, f32, [u8; 3]); 4] = [
        ("MistTerm", 118.0, 270.0, [238, 246, 255]),
        ("Modern SSH terminal for DevOps", 38.0, 342.0, [178, 196, 224]),
        ("and backend developers", 38.0, 390.0, [178, 196, 224]),
        ("Rust  ·  GPU UI  ·  Multi-tab  ·  SFTP  ·  ZMODEM", 26.0, 462.0, [55, 175, 255]),
    ];
    for (text, px, baseline, color) in lines {
        draw_text(&mut img, &font, px, TEXT_X, baseline, text, color);
    }
    img.save(path)
}

fn draw_text(img: &mut RgbaImage, font: &FontRef<'_>, px: f32, x: f32, baseline: f32, text: &str, color: [u8; 3]) {
    let scaled = font.as_scaled(PxScale::from(px));
    let mut caret = x;
    let mut prev = None;
    for ch in text.chars() {
        let id = scaled.glyph_id(ch);
        if let Some(p) = prev {
            caret += scaled.kern(p, id);
        }
        prev = Some(id);
        let glyph = id.with_scale_and_position(px, point(caret, baseline));
        caret += scaled.h_advance(id);
        let Some(outline) = font.outline_glyph(glyph) else {
            continue;
        };
        let bounds = outline.px_bounds();
        outline.draw(|gx, gy, coverage| {
            let (ix, iy) = (bounds.min.x as i32 + gx as i32, bounds.min.y as i32 + gy as i32);
            if ix >= 0 && iy >= 0 && (ix as u32) < img.width() && (iy as u32) < img.height() {
                blend(img.get_pixel_mut(ix as u32, iy as u32), color, coverage);
            }
        });
    }
}

fn blend(px: &mut Rgba<u8>, color: [u8; 3], alpha: f32) {
    let a = alpha.clamp(0.0, 1.0);
    for i in 0..3 {
        px.0[i] = (px.0[i] as f32 * (1.0 - a) + color[i] as f32 * a).round() as u8;
    }
}

/// 导出同名文件到 `dir`，另拼一张多尺寸对比图(各尺寸原生渲染，深浅两种底色)
fn export_preview(dir: &Path) -> Result<(), image::ImageError> {
    export_to(dir)?;
    const SIZES: [u32; 5] = [320, 128, 64, 32, 16];
    const GAP: u32 = 32;
    let row_h = SIZES[0] + GAP * 2;
    let sheet_w = GAP + SIZES.iter().map(|s| s + GAP).sum::<u32>();
    let mut sheet = RgbaImage::new(sheet_w, row_h * 2);
    for (row, bg) in [[46, 48, 54, 255], [236, 236, 240, 255]].into_iter().enumerate() {
        let y0 = row as u32 * row_h;
        imageops::replace(&mut sheet, &RgbaImage::from_pixel(sheet_w, row_h, Rgba(bg)), 0, y0 as i64);
        let mut x = GAP;
        for size in SIZES {
            let y = y0 + GAP + (SIZES[0] - size) / 2;
            imageops::overlay(&mut sheet, &icons::render_app_icon_bundle(size), x as i64, y as i64);
            x += size + GAP;
        }
    }
    let path = dir.join("sizes.png");
    sheet.save(&path)?;
    println!("已写入 {}", path.display());
    Ok(())
}
