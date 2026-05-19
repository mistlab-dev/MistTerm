//! UI 图标图集：启动时生成一张纹理，各平台用 UV 切片绘制，不依赖系统 emoji/符号字体。

use eframe::egui::{self, Color32, CursorIcon, Rect, Response, Sense, TextureHandle, Ui, Vec2};
use image::{Rgba, RgbaImage};
use std::sync::Arc;

const COLS: u32 = 8;
const BASE_CELL: u32 = 32;
const MAX_CELL: u32 = 64;

fn atlas_cell_size(ppp: f32) -> u32 {
    (BASE_CELL as f32 * ppp)
        .round()
        .clamp(BASE_CELL as f32, MAX_CELL as f32) as u32
}

/// 图集格子 ID（行列 = index / COLS, index % COLS）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum IconId {
    Close = 0,
    SidebarCollapse,
    Plus,
    ChevronRight,
    Fragment,
    Upload,
    Search,
    Monitor,
    Alert,
    Brand,
    Refresh,
    Trash,
    Folder,
    File,
    GitBranch,
    GitPull,
    GitPush,
    GitCommit,
    Package,
    Key,
    Cloud,
    Warning,
    Cpu,
    Memory,
    Disk,
    Network,
    Chart,
    Timer,
    Plug,
    Rocket,
    Server,
    Database,
    Api,
    Attachment,
    Check,
    Cross,
    SortUsage,
    SortSuccess,
    SortRecent,
    SortName,
    TerminalPrompt,
    Dot,
    Zmodem,
    ChevronLeft,
}

impl IconId {
    pub const COUNT: usize = 44;

    pub fn index(self) -> u32 {
        self as u32
    }

    pub fn label_zh(self) -> Option<&'static str> {
        match self {
            IconId::Fragment => Some("命令片段"),
            IconId::Upload => Some("上传"),
            IconId::Search => Some("搜索"),
            IconId::Monitor => Some("监控"),
            IconId::Folder => Some("SFTP 文件"),
            _ => None,
        }
    }
}

/// 凭证分类 → 图集图标
pub fn credential_category_icon(cat: crate::core::credential::CredentialCategory) -> IconId {
    use crate::core::credential::CredentialCategory;
    match cat {
        CredentialCategory::Server => IconId::Server,
        CredentialCategory::Database => IconId::Database,
        CredentialCategory::SshKey => IconId::Key,
        CredentialCategory::Api => IconId::Api,
        CredentialCategory::Other => IconId::Attachment,
    }
}

/// 片段排序 → 图集图标
pub fn fragment_sort_icon(sort: crate::core::fragment::SortBy) -> IconId {
    use crate::core::fragment::SortBy;
    match sort {
        SortBy::UsageCount => IconId::SortUsage,
        SortBy::SuccessRate => IconId::SortSuccess,
        SortBy::LastUsed => IconId::SortRecent,
        SortBy::Name => IconId::SortName,
    }
}

pub struct UiIcons {
    texture: TextureHandle,
    size: Vec2,
    cell: u32,
}

fn icons_store_id() -> egui::Id {
    egui::Id::new("mist_ui_icons")
}

fn icons_ppp_id() -> egui::Id {
    egui::Id::new("mist_ui_icons_ppp")
}

impl UiIcons {
    pub fn install(ctx: &egui::Context) {
        Self::reload_if_ppp_changed(ctx);
    }

    /// 显示器缩放 / 跨屏 DPI 变化时按新 `pixels_per_point` 重建图集。
    pub fn reload_if_ppp_changed(ctx: &egui::Context) {
        let ppp = ctx.pixels_per_point();
        let store_id = icons_store_id();
        let ppp_id = icons_ppp_id();
        let needs_reload = ctx.data(|d| {
            d.get_temp::<f32>(ppp_id)
                .map(|prev| (prev - ppp).abs() > 0.01)
                .unwrap_or(true)
        });
        if !needs_reload && ctx.data(|d| d.get_temp::<Arc<UiIcons>>(store_id).is_some()) {
            return;
        }
        let icons = Arc::new(Self::load(ctx));
        ctx.data_mut(|d| {
            d.insert_temp(store_id, icons);
            d.insert_temp(ppp_id, ppp);
        });
    }

    pub fn get(ctx: &egui::Context) -> Arc<UiIcons> {
        Self::reload_if_ppp_changed(ctx);
        let id = icons_store_id();
        ctx.data(|d| d.get_temp(id))
            .unwrap_or_else(|| {
                let icons = Arc::new(Self::load(ctx));
                ctx.data_mut(|d| d.insert_temp(id, Arc::clone(&icons)));
                icons
            })
    }

    fn load(ctx: &egui::Context) -> Self {
        let cell = atlas_cell_size(ctx.pixels_per_point());
        let rows = (IconId::COUNT as u32 + COLS - 1) / COLS;
        let w = COLS * cell;
        let h = rows * cell;
        let mut img = RgbaImage::new(w, h);
        for id in all_icon_ids() {
            draw_icon_cell(&mut img, id, cell);
        }
        let pixels: Vec<egui::Color32> = img
            .pixels()
            .map(|p| {
                if p[3] == 0 {
                    egui::Color32::TRANSPARENT
                } else {
                    egui::Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3])
                }
            })
            .collect();
        let color_image =
            egui::ColorImage { size: [w as usize, h as usize], pixels };
        let texture = ctx.load_texture(
            "mist_ui_atlas",
            color_image,
            egui::TextureOptions {
                magnification: egui::TextureFilter::Linear,
                minification: egui::TextureFilter::Linear,
                ..Default::default()
            },
        );
        Self {
            texture,
            size: Vec2::new(w as f32, h as f32),
            cell,
        }
    }

    pub fn texture_id(&self) -> egui::TextureId {
        self.texture.id()
    }

    pub fn uv(&self, id: IconId) -> Rect {
        let idx = id.index();
        let col = idx % COLS;
        let row = idx / COLS;
        let s = self.cell as f32;
        let w = self.size.x;
        let h = self.size.y;
        Rect::from_min_max(
            egui::pos2(col as f32 * s / w, row as f32 * s / h),
            egui::pos2((col + 1) as f32 * s / w, (row + 1) as f32 * s / h),
        )
    }

    pub fn paint(&self, ui: &Ui, rect: Rect, id: IconId, tint: Color32) {
        ui.painter()
            .image(self.texture_id(), rect, self.uv(id), tint);
    }
}

/// 在矩形内居中绘制图标（`logical_px` 为 egui 逻辑点；图集格已在加载时按 `pixels_per_point` 生成）
pub fn paint_icon(ui: &Ui, rect: Rect, id: IconId, tint: Color32, logical_px: f32) {
    let icons = UiIcons::get(ui.ctx());
    let side = logical_px.min(rect.width()).min(rect.height());
    let r = Rect::from_center_size(rect.center(), Vec2::splat(side));
    icons.paint(ui, r, id, tint);
}

/// 方形可点击图标区（`hit` / `icon_px` 为 egui 逻辑点）
pub fn icon_hit_button(
    ui: &mut Ui,
    id: IconId,
    hit: f32,
    icon_px: f32,
    idle: Color32,
    hover: Color32,
    hover_fill: Color32,
    pressed_fill: Color32,
    rounding: f32,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(hit), Sense::click());
    let active = response.hovered() || response.is_pointer_button_down_on();
    if active {
        ui.ctx().request_repaint();
    }
    if response.hovered() || response.is_pointer_button_down_on() {
        let fill = if response.is_pointer_button_down_on() {
            pressed_fill
        } else {
            hover_fill
        };
        ui.painter().rect_filled(rect, rounding, fill);
    }
    let color = if active { hover } else { idle };
    paint_icon(ui, rect, id, color, icon_px);
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

/// 图标 + 文字（水平）
pub fn icon_label_row(
    ui: &mut Ui,
    id: IconId,
    label: &str,
    icon_px: f32,
    gap: f32,
    rich: impl FnOnce(egui::RichText) -> egui::RichText,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        let (r, _) = ui.allocate_exact_size(Vec2::splat(icon_px), Sense::hover());
        paint_icon(ui, r, id, ui.visuals().text_color(), icon_px);
        ui.label(rich(egui::RichText::new(label)));
    });
}

fn all_icon_ids() -> [IconId; IconId::COUNT] {
    [
        IconId::Close,
        IconId::SidebarCollapse,
        IconId::Plus,
        IconId::ChevronRight,
        IconId::Fragment,
        IconId::Upload,
        IconId::Search,
        IconId::Monitor,
        IconId::Alert,
        IconId::Brand,
        IconId::Refresh,
        IconId::Trash,
        IconId::Folder,
        IconId::File,
        IconId::GitBranch,
        IconId::GitPull,
        IconId::GitPush,
        IconId::GitCommit,
        IconId::Package,
        IconId::Key,
        IconId::Cloud,
        IconId::Warning,
        IconId::Cpu,
        IconId::Memory,
        IconId::Disk,
        IconId::Network,
        IconId::Chart,
        IconId::Timer,
        IconId::Plug,
        IconId::Rocket,
        IconId::Server,
        IconId::Database,
        IconId::Api,
        IconId::Attachment,
        IconId::Check,
        IconId::Cross,
        IconId::SortUsage,
        IconId::SortSuccess,
        IconId::SortRecent,
        IconId::SortName,
        IconId::TerminalPrompt,
        IconId::Dot,
        IconId::Zmodem,
        IconId::ChevronLeft,
    ]
}

// --- 图集绘制（0..1 单元格内坐标）---

type Seg = (f32, f32, f32, f32);

struct CellPainter<'a> {
    img: &'a mut RgbaImage,
    ox: u32,
    oy: u32,
    cell: u32,
}

impl<'a> CellPainter<'a> {
    fn new(img: &'a mut RgbaImage, id: IconId, cell: u32) -> Self {
        let idx = id.index();
        Self {
            img,
            ox: (idx % COLS) * cell,
            oy: (idx / COLS) * cell,
            cell,
        }
    }

    fn map(&self, x: f32, y: f32) -> (i32, i32) {
        let m = self.cell as f32 * 0.16;
        let s = self.cell as f32 * 0.69;
        (
            (self.ox as f32 + m + x * s).round() as i32,
            (self.oy as f32 + m + y * s).round() as i32,
        )
    }

    fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, w: f32) {
        let (px0, py0) = self.map(x0, y0);
        let (px1, py1) = self.map(x1, y1);
        draw_line_aa(self.img, px0, py0, px1, py1, w, 255);
    }

    fn circle(&mut self, cx: f32, cy: f32, r: f32, w: f32) {
        let steps = 24;
        for i in 0..steps {
            let a0 = std::f32::consts::TAU * i as f32 / steps as f32;
            let a1 = std::f32::consts::TAU * (i + 1) as f32 / steps as f32;
            self.line(
                cx + r * a0.cos(),
                cy + r * a0.sin(),
                cx + r * a1.cos(),
                cy + r * a1.sin(),
                w,
            );
        }
    }

    fn rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, w: f32) {
        self.line(x0, y0, x1, y0, w);
        self.line(x1, y0, x1, y1, w);
        self.line(x1, y1, x0, y1, w);
        self.line(x0, y1, x0, y0, w);
    }

    fn segs(&mut self, segs: &[Seg], w: f32) {
        for &(a, b, c, d) in segs {
            self.line(a, b, c, d, w);
        }
    }

    fn fill_circle(&mut self, cx: f32, cy: f32, r: f32) {
        let (pcx, pcy) = self.map(cx, cy);
        let pr = (r * self.cell as f32 * 0.69).round() as i32;
        for dy in -pr..=pr {
            for dx in -pr..=pr {
                if dx * dx + dy * dy <= pr * pr {
                    put_px(self.img, pcx + dx, pcy + dy, 255);
                }
            }
        }
    }
}

fn draw_icon_cell(img: &mut RgbaImage, id: IconId, cell: u32) {
    let mut p = CellPainter::new(img, id, cell);
    let w = 1.8_f32;
    match id {
        IconId::Close => p.segs(&[(0.22, 0.22, 0.78, 0.78), (0.78, 0.22, 0.22, 0.78)], w),
        IconId::SidebarCollapse => p.segs(&[(0.72, 0.2, 0.32, 0.5), (0.32, 0.5, 0.72, 0.8)], w),
        IconId::Plus => p.segs(&[(0.5, 0.2, 0.5, 0.8), (0.2, 0.5, 0.8, 0.5)], w),
        IconId::ChevronRight => p.segs(&[(0.35, 0.22, 0.68, 0.5), (0.35, 0.78, 0.68, 0.5)], w),
        IconId::Fragment => {
            p.rect(0.22, 0.2, 0.78, 0.8, w);
            p.segs(&[(0.32, 0.38, 0.68, 0.38), (0.32, 0.5, 0.6, 0.5), (0.32, 0.62, 0.68, 0.62)], 1.4);
        }
        IconId::Upload => p.segs(
            &[(0.5, 0.72, 0.5, 0.28), (0.35, 0.42, 0.5, 0.28), (0.65, 0.42, 0.5, 0.28), (0.22, 0.78, 0.78, 0.78)],
            w,
        ),
        IconId::Search => {
            p.circle(0.42, 0.42, 0.22, w);
            p.segs(&[(0.58, 0.58, 0.78, 0.78)], w);
        }
        IconId::Monitor => {
            p.rect(0.18, 0.22, 0.82, 0.68, w);
            p.segs(&[(0.4, 0.78, 0.6, 0.78), (0.5, 0.68, 0.5, 0.78)], w);
        }
        IconId::Alert => p.segs(
            &[(0.5, 0.18, 0.22, 0.78), (0.5, 0.18, 0.78, 0.78), (0.38, 0.62, 0.62, 0.62)],
            w,
        ),
        IconId::Brand => {
            for i in 0..4 {
                let a = std::f32::consts::FRAC_PI_2 * i as f32 - std::f32::consts::FRAC_PI_4;
                p.line(0.5 + 0.28 * a.cos(), 0.5 + 0.28 * a.sin(), 0.5, 0.5, w);
            }
        }
        IconId::Refresh => {
            p.circle(0.5, 0.5, 0.28, w);
            p.segs(&[(0.62, 0.28, 0.78, 0.18), (0.78, 0.18, 0.68, 0.38)], w);
        }
        IconId::Trash => {
            p.segs(&[(0.28, 0.32, 0.72, 0.32), (0.32, 0.32, 0.34, 0.78), (0.66, 0.32, 0.68, 0.78), (0.34, 0.78, 0.66, 0.78)], w);
            p.segs(&[(0.38, 0.22, 0.62, 0.22)], w);
        }
        // 开口文件夹：矩形主体 + 分段顶边（小尺寸底栏/SFTP 侧栏须可辨认）
        IconId::Folder => {
            p.rect(0.2, 0.36, 0.8, 0.78, w);
            p.segs(
                &[(0.2, 0.36, 0.46, 0.36), (0.46, 0.36, 0.54, 0.26), (0.76, 0.26, 0.76, 0.36)],
                w,
            );
        }
        IconId::File => {
            p.segs(&[(0.28, 0.2, 0.55, 0.2), (0.72, 0.35, 0.72, 0.8), (0.28, 0.8, 0.28, 0.2)], w);
            p.segs(&[(0.55, 0.2, 0.72, 0.35), (0.55, 0.2, 0.55, 0.35), (0.55, 0.35, 0.72, 0.35)], 1.4);
        }
        IconId::GitBranch => p.segs(
            &[(0.5, 0.2, 0.5, 0.45), (0.32, 0.55, 0.68, 0.55), (0.32, 0.55, 0.32, 0.78), (0.68, 0.55, 0.68, 0.78)],
            w,
        ),
        IconId::GitPull => p.segs(
            &[(0.5, 0.22, 0.5, 0.62), (0.38, 0.5, 0.5, 0.68), (0.62, 0.5, 0.5, 0.68), (0.28, 0.78, 0.72, 0.78)],
            w,
        ),
        IconId::GitPush => p.segs(
            &[(0.5, 0.78, 0.5, 0.38), (0.38, 0.5, 0.5, 0.32), (0.62, 0.5, 0.5, 0.32), (0.28, 0.22, 0.72, 0.22)],
            w,
        ),
        IconId::GitCommit => {
            p.circle(0.5, 0.5, 0.22, w);
            p.segs(&[(0.28, 0.5, 0.72, 0.5)], w);
        }
        IconId::Package => p.rect(0.25, 0.28, 0.75, 0.72, w),
        IconId::Key => {
            p.circle(0.35, 0.38, 0.14, w);
            p.segs(&[(0.45, 0.45, 0.78, 0.78), (0.65, 0.65, 0.78, 0.52), (0.65, 0.78, 0.78, 0.78)], w);
        }
        IconId::Cloud => {
            p.circle(0.38, 0.48, 0.16, w);
            p.circle(0.58, 0.48, 0.18, w);
            p.segs(&[(0.22, 0.55, 0.78, 0.55), (0.22, 0.55, 0.22, 0.62), (0.78, 0.55, 0.78, 0.62)], w);
        }
        IconId::Warning => p.segs(
            &[(0.5, 0.18, 0.22, 0.78), (0.5, 0.18, 0.78, 0.78), (0.42, 0.58, 0.58, 0.58)],
            w,
        ),
        IconId::Cpu => p.rect(0.28, 0.28, 0.72, 0.72, w),
        IconId::Memory => p.segs(
            &[(0.22, 0.35, 0.78, 0.35), (0.22, 0.65, 0.78, 0.65), (0.35, 0.28, 0.35, 0.72), (0.65, 0.28, 0.65, 0.72)],
            w,
        ),
        IconId::Disk => {
            p.circle(0.5, 0.5, 0.28, w);
            p.fill_circle(0.5, 0.5, 0.1);
        }
        IconId::Network => p.segs(
            &[(0.2, 0.5, 0.38, 0.5), (0.62, 0.5, 0.8, 0.5), (0.38, 0.5, 0.5, 0.32), (0.5, 0.32, 0.62, 0.5)],
            w,
        ),
        IconId::Chart => p.segs(
            &[(0.2, 0.75, 0.35, 0.45), (0.35, 0.45, 0.52, 0.62), (0.52, 0.62, 0.68, 0.35), (0.68, 0.35, 0.82, 0.55)],
            w,
        ),
        IconId::Timer => {
            p.circle(0.5, 0.5, 0.3, w);
            p.segs(&[(0.5, 0.5, 0.5, 0.32), (0.5, 0.5, 0.65, 0.5)], w);
        }
        IconId::Plug => p.segs(
            &[(0.35, 0.22, 0.35, 0.45), (0.65, 0.22, 0.65, 0.45), (0.28, 0.45, 0.72, 0.45), (0.5, 0.45, 0.5, 0.78)],
            w,
        ),
        IconId::Rocket => p.segs(
            &[(0.5, 0.18, 0.38, 0.55), (0.5, 0.18, 0.62, 0.55), (0.38, 0.55, 0.62, 0.55), (0.5, 0.55, 0.5, 0.82)],
            w,
        ),
        IconId::Server => {
            p.rect(0.22, 0.2, 0.78, 0.8, w);
            for y in [0.38, 0.55, 0.72] {
                p.segs(&[(0.32, y, 0.68, y)], 1.4);
            }
        }
        IconId::Database => {
            p.segs(
                &[(0.25, 0.32, 0.75, 0.32), (0.25, 0.68, 0.75, 0.68), (0.25, 0.32, 0.25, 0.68), (0.75, 0.32, 0.75, 0.68)],
                w,
            );
            p.segs(&[(0.3, 0.22, 0.7, 0.22), (0.3, 0.78, 0.7, 0.78)], w);
        }
        IconId::Api => p.segs(&[(0.22, 0.72, 0.42, 0.28), (0.42, 0.28, 0.62, 0.72), (0.62, 0.72, 0.82, 0.28)], w),
        IconId::Attachment => p.segs(
            &[(0.42, 0.18, 0.42, 0.62), (0.42, 0.35, 0.62, 0.55), (0.62, 0.55, 0.62, 0.35), (0.62, 0.35, 0.42, 0.18)],
            w,
        ),
        IconId::Check => p.segs(&[(0.22, 0.52, 0.42, 0.72), (0.42, 0.72, 0.78, 0.28)], w),
        IconId::Cross => p.segs(&[(0.25, 0.25, 0.75, 0.75), (0.75, 0.25, 0.25, 0.75)], w),
        IconId::SortUsage => p.segs(
            &[(0.28, 0.22, 0.28, 0.78), (0.42, 0.35, 0.55, 0.35), (0.42, 0.5, 0.62, 0.5), (0.42, 0.65, 0.7, 0.65)],
            w,
        ),
        IconId::SortSuccess => p.segs(&[(0.22, 0.52, 0.4, 0.72), (0.4, 0.72, 0.78, 0.28)], w),
        IconId::SortRecent => {
            p.circle(0.5, 0.5, 0.28, w);
            p.segs(&[(0.5, 0.5, 0.5, 0.32), (0.5, 0.5, 0.65, 0.55)], w);
        }
        IconId::SortName => p.segs(
            &[(0.25, 0.28, 0.25, 0.72), (0.45, 0.28, 0.45, 0.72), (0.65, 0.28, 0.65, 0.72)],
            w,
        ),
        IconId::TerminalPrompt => p.segs(&[(0.22, 0.28, 0.48, 0.5), (0.22, 0.72, 0.48, 0.5)], w),
        IconId::Dot => p.fill_circle(0.5, 0.5, 0.18),
        IconId::Zmodem => {
            p.rect(0.2, 0.25, 0.8, 0.75, w);
            p.segs(&[(0.35, 0.42, 0.65, 0.42), (0.35, 0.58, 0.65, 0.58)], 1.4);
        }
        IconId::ChevronLeft => p.segs(&[(0.65, 0.22, 0.32, 0.5), (0.65, 0.78, 0.32, 0.5)], w),
    }
}

fn put_px(img: &mut RgbaImage, x: i32, y: i32, a: u8) {
    if x < 0 || y < 0 {
        return;
    }
    let (w, h) = (img.width() as i32, img.height() as i32);
    if x >= w || y >= h {
        return;
    }
    let p = img.get_pixel_mut(x as u32, y as u32);
    let na = a.max(p[3]);
    if na > 0 {
        *p = Rgba([255, 255, 255, na]);
    }
}

fn draw_line_aa(img: &mut RgbaImage, x0: i32, y0: i32, x1: i32, y1: i32, width: f32, alpha: u8) {
    let dx = (x1 - x0) as f32;
    let dy = (y1 - y0) as f32;
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    let steps = (len * 2.0) as i32 + 1;
    let hw = (width * 0.5).max(1.0) as i32;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let cx = (x0 as f32 + dx * t).round() as i32;
        let cy = (y0 as f32 + dy * t).round() as i32;
        for oy in -hw..=hw {
            for ox in -hw..=hw {
                if ox * ox + oy * oy <= hw * hw {
                    put_px(img, cx + ox, cy + oy, alpha);
                }
            }
        }
    }
}
