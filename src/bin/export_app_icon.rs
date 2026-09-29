//! 导出应用图标 PNG：`cargo run --bin export_app_icon`
//! - `assets/app-icon-preview.png`：256 窗口图标预览
//! - `assets/app-icon-1024.png`：macOS 打包源图(`scripts/bundle-macos.sh` 生成 .icns)
//!
//! 仅预览、不改动 assets：`cargo run --bin export_app_icon -- --preview <DIR>`

use image::{Rgba, RgbaImage, imageops};
use mistterm::ui::icons;
use std::path::Path;

type Exporter = fn(&Path) -> Result<(), image::ImageError>;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [flag, dir] if flag == "--preview" => export_preview(Path::new(dir)),        _ => export_to(Path::new("assets")),
    };
    if let Err(e) = result {
        eprintln!("导出失败: {e}");
        std::process::exit(1);
    }
}

fn export_to(dir: &Path) -> Result<(), image::ImageError> {
    let _ = std::fs::create_dir_all(dir);
    let targets: [(&str, Exporter); 2] = [
        ("app-icon-preview.png", icons::export_app_icon_png),
        ("app-icon-1024.png", icons::export_app_icon_bundle_png),
    ];
    for (name, export) in targets {
        let path = dir.join(name);
        export(&path)?;
        println!("已写入 {}", path.display());
    }
    Ok(())
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