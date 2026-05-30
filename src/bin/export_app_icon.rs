//! 导出应用图标预览 PNG：`cargo run --bin export_app_icon`

fn main() {
    let path = std::path::Path::new("assets/app-icon-preview.png");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match mistterm::ui::icons::export_app_icon_png(path) {
        Ok(()) => println!("已写入 {}", path.display()),
        Err(e) => {
            eprintln!("导出失败: {e}");
            std::process::exit(1);
        }
    }
}
