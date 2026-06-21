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
    ChevronUp,
    ArrowEnter,
    Copy,
}

impl IconId {
    pub const COUNT: usize = 47;

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
    icon_hit_button_revealed(
        ui, id, hit, icon_px, idle, hover, hover_fill, pressed_fill, rounding, true,
    )
}

/// 同 [`icon_hit_button`]，但 `revealed == false` 时仅保留点击区不绘制（避免显隐切换导致 hover 抖动）。
pub fn icon_hit_button_revealed(
    ui: &mut Ui,
    id: IconId,
    hit: f32,
    icon_px: f32,
    idle: Color32,
    hover: Color32,
    hover_fill: Color32,
    pressed_fill: Color32,
    rounding: f32,
    revealed: bool,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(hit), Sense::click());
    let active = revealed && (response.hovered() || response.is_pointer_button_down_on());
    if active {
        ui.ctx().request_repaint();
    }
    if revealed && (response.hovered() || response.is_pointer_button_down_on()) {
        let fill = if response.is_pointer_button_down_on() {
            pressed_fill
        } else {
            hover_fill
        };
        ui.painter().rect_filled(rect, rounding, fill);
    }
    if revealed {
        let color = if active { hover } else { idle };
        paint_icon(ui, rect, id, color, icon_px);
    }
    if revealed && response.hovered() {
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
        IconId::ChevronUp,
        IconId::ArrowEnter,
        IconId::Copy,
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
        IconId::Brand => draw_m_letter_cell(&mut p, w),
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
        IconId::ChevronUp => p.segs(&[(0.22, 0.65, 0.5, 0.32), (0.78, 0.65, 0.5, 0.32)], w),
        IconId::ArrowEnter => p.segs(
            &[(0.28, 0.72, 0.72, 0.72), (0.28, 0.72, 0.28, 0.32), (0.18, 0.42, 0.28, 0.32), (0.38, 0.42, 0.28, 0.32)],
            w,
        ),
        IconId::Copy => {
            p.rect(0.24, 0.26, 0.56, 0.66, w);
            p.rect(0.44, 0.36, 0.76, 0.76, w);
        }
    }
}

/// Mist 字母 M 笔画（0..1 归一化坐标）
const M_LETTER_SEGS: &[Seg] = &[
    (0.24, 0.78, 0.24, 0.22),
    (0.24, 0.22, 0.5, 0.58),
    (0.76, 0.22, 0.5, 0.58),
    (0.76, 0.78, 0.76, 0.22),
];

fn draw_m_letter_cell(p: &mut CellPainter<'_>, stroke_w: f32) {
    p.segs(M_LETTER_SEGS, stroke_w);
}

const APP_ICON_FONT: &[u8] = include_bytes!("../../assets/fonts/NotoSansSC-Regular.otf");

/// 霓虹青（参考图地平面 / 外发光）
const APP_ICON_CYAN: [u8; 3] = [55, 175, 255];
/// 字标核心高光白
const APP_ICON_TEXT_CORE: [u8; 4] = [238, 246, 255, 255];

/// 图标透明外圈比例：macOS Dock squircle 需留白；Windows 任务栏为方角缩放，留白会显得更小。
fn app_icon_outer_pad_frac() -> f32 {
    if cfg!(windows) {
        0.02
    } else if cfg!(target_os = "macos") {
        0.08
    } else {
        0.05
    }
}

/// 窗口 / Dock / 任务栏图标（霓虹 Mist 字标 + 圆角底板）。
pub fn app_window_icon_data() -> eframe::IconData {
    const SIZE: u32 = 256;
    let pad = (SIZE as f32 * app_icon_outer_pad_frac()).round() as u32;
    let mut img = RgbaImage::from_pixel(SIZE, SIZE, Rgba([0, 0, 0, 0]));
    let edge = SIZE - pad;
    paint_mist_app_icon(&mut img, pad, edge, edge);
    eframe::IconData {
        rgba: img.into_raw(),
        width: SIZE,
        height: SIZE,
    }
}

/// 导出 PNG 预览（`cargo run --bin export_app_icon`）。
pub fn export_app_icon_png(path: &std::path::Path) -> Result<(), image::ImageError> {
    let icon = app_window_icon_data();
    let img = image::RgbaImage::from_raw(icon.width, icon.height, icon.rgba)
        .expect("app icon buffer size mismatch");
    img.save(path)
}

/// 在 `[x0,x1)×[y0,y1)` 内绘制 Mist 品牌图标（参考霓虹字标 + 底部地光）。
fn paint_mist_app_icon(img: &mut RgbaImage, x0: u32, x1: u32, y1: u32) {
    let y0 = x0;
    let w = (x1 - x0) as f32;
    let h = (y1 - y0) as f32;
    let ox = x0 as f32;
    let oy = y0 as f32;
    let cx = ox + w * 0.5;
    let cy = oy + h * 0.35;
    let tw = if cfg!(windows) { w * 0.80 } else { w * 0.72 };
    let text_bottom = wordmark_metrics("Mist", cx, cy, tw)
        .map(|m| m.text_bottom)
        .unwrap_or(cy + 18.0);
    // 镜面线：落在正文与倒影之间的中部
    const TEXT_MIRROR_GAP: f32 = 24.0;
    let mirror_y = text_bottom + TEXT_MIRROR_GAP * 0.86;

    fill_vertical_gradient(img, x0, y0, x1, y1, [10, 14, 32], [3, 5, 16]);
    paint_bottom_floor_glow(img, ox, oy, w, h);
    draw_wordmark_reflection(img, "Mist", cx, cy, mirror_y, tw);
    paint_mirror_surface_line(img, ox + w * 0.12, ox + w * 0.88, mirror_y);
    draw_neon_wordmark(img, "Mist", cx, cy, tw);

    // 圆角遮罩：macOS 连续圆角；Windows 任务栏再套方角缩放，圆角过大会吃掉有效面积
    let radius = w.min(h) * if cfg!(windows) { 0.10 } else { 0.165 };
    apply_rounded_alpha_mask(img, ox, oy, ox + w, oy + h, radius);
}

/// 圆角矩形 SDF（负值 = 内侧）
fn sdf_rounded_rect(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> f32 {
    let cx = (x0 + x1) * 0.5;
    let cy = (y0 + y1) * 0.5;
    let hx = (x1 - x0) * 0.5 - r;
    let hy = (y1 - y0) * 0.5 - r;
    let qx = (px - cx).abs() - hx;
    let qy = (py - cy).abs() - hy;
    let ax = qx.max(0.0);
    let ay = qy.max(0.0);
    (ax * ax + ay * ay).sqrt() - r + qx.min(qy).min(0.0)
}

fn rounded_rect_coverage(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> f32 {
    let d = sdf_rounded_rect(px, py, x0, y0, x1, y1, r);
    (0.5 - d).clamp(0.0, 1.0)
}

/// 将图标内容裁切为圆角方形（外侧透明）
fn apply_rounded_alpha_mask(img: &mut RgbaImage, x0: f32, y0: f32, x1: f32, y1: f32, radius: f32) {
    let w = img.width();
    let h = img.height();
    for y in 0..h {
        for x in 0..w {
            let cov = rounded_rect_coverage(x as f32 + 0.5, y as f32 + 0.5, x0, y0, x1, y1, radius);
            if cov <= 0.0 {
                img.put_pixel(x, y, Rgba([0, 0, 0, 0]));
            } else if cov < 1.0 {
                let p = img.get_pixel(x, y);
                let a = (p[3] as f32 * cov).round() as u8;
                img.put_pixel(x, y, Rgba([p[0], p[1], p[2], a]));
            }
        }
    }
}

/// 底部径向地光（参考图蓝色光池）
fn paint_bottom_floor_glow(img: &mut RgbaImage, ox: f32, oy: f32, w: f32, h: f32) {
    let c = APP_ICON_CYAN;
    draw_soft_ellipse(img, ox + w * 0.5, oy + h * 0.92, w * 0.62, h * 0.28, [c[0], c[1], c[2], 48]);
    draw_soft_ellipse(img, ox + w * 0.5, oy + h * 0.82, w * 0.48, h * 0.20, [c[0], c[1], c[2], 100]);
    draw_soft_ellipse(img, ox + w * 0.5, oy + h * 0.72, w * 0.36, h * 0.14, [c[0], c[1], c[2], 130]);
    draw_soft_ellipse(img, ox + w * 0.5, oy + h * 0.64, w * 0.22, h * 0.08, [200, 230, 255, 40]);
}

/// 霓虹「Mist」：外发光 + 下半青 + 核心白
fn draw_neon_wordmark(img: &mut RgbaImage, text: &str, cx: f32, cy: f32, target_width: f32) {
    const GLOW: &[(f32, f32, u8)] = &[
        (0.0, 0.0, 42),
        (-2.0, 0.0, 28),
        (2.0, 0.0, 28),
        (0.0, -2.0, 28),
        (0.0, 2.0, 28),
        (-3.0, -1.0, 18),
        (3.0, 1.0, 18),
        (-1.0, 2.0, 16),
        (1.0, -2.0, 16),
        (-4.0, 0.0, 10),
        (4.0, 0.0, 10),
        (0.0, -4.0, 10),
        (0.0, 4.0, 10),
    ];
    let glow_color = [APP_ICON_CYAN[0], APP_ICON_CYAN[1], APP_ICON_CYAN[2]];
    for &(dx, dy, a) in GLOW {
        let _ = draw_wordmark(
            img,
            text,
            cx + dx,
            cy + dy,
            target_width,
            [glow_color[0], glow_color[1], glow_color[2], a],
            WordmarkDrawOpts::default(),
        );
    }
    let _ = draw_wordmark(
        img,
        text,
        cx,
        cy + 2.0,
        target_width,
        [APP_ICON_CYAN[0], APP_ICON_CYAN[1], APP_ICON_CYAN[2], 210],
        WordmarkDrawOpts::default(),
    );
    let _ = draw_wordmark(img, text, cx, cy, target_width, APP_ICON_TEXT_CORE, WordmarkDrawOpts::default());
}

/// 镜面地平面亮线（在字标下方，作为反射分界）
fn paint_mirror_surface_line(img: &mut RgbaImage, x0: f32, x1: f32, y: f32) {
    let core = [140, 230, 255, 155];
    let glow = [APP_ICON_CYAN[0], APP_ICON_CYAN[1], APP_ICON_CYAN[2]];
    for dy in -3i32..=2 {
        let t = dy.unsigned_abs();
        let a = match t {
            0 => 155u8,
            1 => 95,
            2 => 48,
            _ => 22,
        };
        let row = (y + dy as f32).round() as i32;
        let x_start = x0.round() as i32;
        let x_end = x1.round() as i32;
        for x in x_start..=x_end {
            let c = if t == 0 { core } else { [glow[0], glow[1], glow[2], a] };
            blend_pixel(img, x, row, c);
        }
    }
}

/// 字标在镜面下方的倒影（随深度衰减）
fn draw_wordmark_reflection(
    img: &mut RgbaImage,
    text: &str,
    center_x: f32,
    center_y: f32,
    mirror_y: f32,
    target_width: f32,
) {
    let color = [APP_ICON_CYAN[0], APP_ICON_CYAN[1], APP_ICON_CYAN[2], 140];
    let _ = draw_wordmark(
        img,
        text,
        center_x,
        center_y,
        target_width,
        color,
        WordmarkDrawOpts {
            mirror_y: Some(mirror_y),
            mirror_fade_depth: REFLECT_DEPTH,
            mirror_strength: 0.26,
            mirror_peak_boost: 0.09,
            ..WordmarkDrawOpts::default()
        },
    );
}

const REFLECT_DEPTH: f32 = 46.0;

struct WordmarkMetrics {
    text_bottom: f32,
}

struct WordmarkDrawOpts {
    mirror_y: Option<f32>,
    mirror_fade_depth: f32,
    mirror_strength: f32,
    /// 贴近视平线处略提亮，镜面更清晰
    mirror_peak_boost: f32,
    /// 首字母 M 相对其余字号的放大倍率
    cap_m_scale: f32,
}

impl Default for WordmarkDrawOpts {
    fn default() -> Self {
        Self {
            mirror_y: None,
            mirror_fade_depth: 36.0,
            mirror_strength: 0.4,
            mirror_peak_boost: 0.0,
            cap_m_scale: 1.14,
        }
    }
}

#[inline]
fn wordmark_char_size(ch: char, base: f32, cap_m_scale: f32) -> f32 {
    if ch == 'M' {
        base * cap_m_scale
    } else {
        base
    }
}

fn wordmark_metrics(text: &str, _center_x: f32, center_y: f32, target_width: f32) -> Option<WordmarkMetrics> {
    use ab_glyph::{Font, FontRef, PxScale, ScaleFont};

    let font = FontRef::try_from_slice(APP_ICON_FONT).ok()?;
    let cap_m = WordmarkDrawOpts::default().cap_m_scale;
    let mut size = target_width / measure_text_width(&font, text, 1.0, cap_m);
    size = size.clamp(18.0, 128.0);
    let scaled = font.as_scaled(PxScale::from(size));
    let ascent = scaled.ascent();
    let descent = scaled.descent();
    let baseline_y = center_y + (ascent + descent) * 0.5 - descent;
    let m_extra = size * (cap_m - 1.0) * 0.35;
    Some(WordmarkMetrics {
        text_bottom: baseline_y + descent + m_extra,
    })
}

fn fill_vertical_gradient(
    img: &mut RgbaImage,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    top: [u8; 3],
    bottom: [u8; 3],
) {
    let h = (y1 - y0).max(1) as f32;
    for y in y0..y1 {
        let t = (y - y0) as f32 / h;
        let r = lerp_u8(top[0], bottom[0], t);
        let g = lerp_u8(top[1], bottom[1], t);
        let b = lerp_u8(top[2], bottom[2], t);
        for x in x0..x1 {
            img.put_pixel(x, y, Rgba([r, g, b, 255]));
        }
    }
}

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t).round() as u8
}

fn draw_soft_ellipse(
    img: &mut RgbaImage,
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
    color: [u8; 4],
) {
    if rx < 1.0 || ry < 1.0 || color[3] == 0 {
        return;
    }
    let x0 = (cx - rx - 2.0).floor().max(0.0) as i32;
    let x1 = (cx + rx + 2.0).ceil().min(img.width() as f32 - 1.0) as i32;
    let y0 = (cy - ry - 2.0).floor().max(0.0) as i32;
    let y1 = (cy + ry + 2.0).ceil().min(img.height() as f32 - 1.0) as i32;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let nx = (x as f32 + 0.5 - cx) / rx;
            let ny = (y as f32 + 0.5 - cy) / ry;
            let d2 = nx * nx + ny * ny;
            if d2 <= 1.0 {
                let edge = (1.0 - d2).powf(1.35);
                let a = ((color[3] as f32) * edge).round() as u8;
                if a > 0 {
                    blend_pixel(img, x, y, [color[0], color[1], color[2], a]);
                }
            }
        }
    }
}

fn blend_pixel(img: &mut RgbaImage, x: i32, y: i32, fg: [u8; 4]) {
    if fg[3] == 0 || x < 0 || y < 0 {
        return;
    }
    let (w, h) = (img.width() as i32, img.height() as i32);
    if x >= w || y >= h {
        return;
    }
    let p = img.get_pixel_mut(x as u32, y as u32);
    let fa = fg[3] as f32 / 255.0;
    let ba = p[3] as f32 / 255.0;
    let out_a = fa + ba * (1.0 - fa);
    if out_a < 1.0 / 255.0 {
        return;
    }
    let blend = |fc: u8, bc: u8| -> u8 {
        ((fc as f32 * fa + bc as f32 * ba * (1.0 - fa)) / out_a).round() as u8
    };
    *p = Rgba([
        blend(fg[0], p[0]),
        blend(fg[1], p[1]),
        blend(fg[2], p[2]),
        (out_a * 255.0).round() as u8,
    ]);
}

fn draw_wordmark(
    img: &mut RgbaImage,
    text: &str,
    center_x: f32,
    center_y: f32,
    target_width: f32,
    color: [u8; 4],
    opts: WordmarkDrawOpts,
) -> bool {
    use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};

    let font = match FontRef::try_from_slice(APP_ICON_FONT) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let cap_m = opts.cap_m_scale;
    let mut size = target_width / measure_text_width(&font, text, 1.0, cap_m);
    size = size.clamp(18.0, 128.0);
    let base_scale = PxScale::from(size);
    let base_scaled = font.as_scaled(base_scale);
    let width = measure_text_width(&font, text, size, cap_m);
    let ascent = base_scaled.ascent();
    let descent = base_scaled.descent();
    let baseline_x = center_x - width * 0.5;
    let baseline_y = center_y + (ascent + descent) * 0.5 - descent;

    let mut pen_x = baseline_x;
    let mut prev: Option<(ab_glyph::GlyphId, f32)> = None;
    for ch in text.chars() {
        let ch_size = wordmark_char_size(ch, size, cap_m);
        let ch_scale = PxScale::from(ch_size);
        let ch_scaled = font.as_scaled(ch_scale);
        let gid = ch_scaled.glyph_id(ch);
        if let Some((p, _)) = prev {
            pen_x += base_scaled.kern(p, gid);
        }
        let glyph = gid.with_scale_and_position(ch_scale, point(pen_x, baseline_y));
        if let Some(outline) = font.outline_glyph(glyph) {
            let b = outline.px_bounds();
            outline.draw(|gx, gy, cov| {
                let px = (b.min.x + gx as f32).round() as i32;
                let py = (b.min.y + gy as f32).round() as i32;
                if let Some(mirror_y) = opts.mirror_y {
                    let py_ref = (2.0 * mirror_y - (b.min.y + gy as f32)).round() as i32;
                    if py_ref as f32 > mirror_y + 0.5 {
                        let depth = py_ref as f32 - mirror_y;
                        let t = (1.0 - depth / opts.mirror_fade_depth).clamp(0.0, 1.0);
                        let fade = t * t * opts.mirror_strength + t * opts.mirror_peak_boost;
                        let a = (cov * color[3] as f32 * fade).round() as u8;
                        if a > 0 {
                            blend_pixel(img, px, py_ref, [color[0], color[1], color[2], a]);
                        }
                    }
                    return;
                }
                let a = (cov * color[3] as f32).round() as u8;
                if a == 0 {
                    return;
                }
                blend_pixel(img, px, py, [color[0], color[1], color[2], a]);
            });
        }
        pen_x += ch_scaled.h_advance(gid);
        prev = Some((gid, ch_size));
    }
    true
}

fn measure_text_width(font: &impl ab_glyph::Font, text: &str, size: f32, cap_m_scale: f32) -> f32 {
    use ab_glyph::{PxScale, ScaleFont};
    let base_scaled = font.as_scaled(PxScale::from(size));
    let mut w = 0.0;
    let mut prev: Option<ab_glyph::GlyphId> = None;
    for ch in text.chars() {
        let ch_size = wordmark_char_size(ch, size, cap_m_scale);
        let ch_scaled = font.as_scaled(PxScale::from(ch_size));
        let gid = ch_scaled.glyph_id(ch);
        if let Some(p) = prev {
            w += base_scaled.kern(p, gid);
        }
        w += ch_scaled.h_advance(gid);
        prev = Some(gid);
    }
    w
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
