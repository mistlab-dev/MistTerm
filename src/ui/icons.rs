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

/// 图集格子 ID(行列 = index / COLS, index % COLS)
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
    Settings,
}

impl IconId {
    pub const COUNT: usize = 48;

    pub fn index(self) -> u32 {
        self as u32
    }

    pub fn label_zh(self) -> Option<&'static str> {
        match self {
            IconId::Fragment => Some("命令片段"),
            IconId::Upload => Some("上传"),
            IconId::Search => Some("搜索"),
            IconId::Monitor => Some("系统监控"),
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

/// 在矩形内居中绘制图标(`logical_px` 为 egui 逻辑点；图集格已在加载时按 `pixels_per_point` 生成)
pub fn paint_icon(ui: &Ui, rect: Rect, id: IconId, tint: Color32, logical_px: f32) {
    let icons = UiIcons::get(ui.ctx());
    let side = logical_px.min(rect.width()).min(rect.height());
    let r = Rect::from_center_size(rect.center(), Vec2::splat(side));
    icons.paint(ui, r, id, tint);
}

/// 方形可点击图标区(`hit` / `icon_px` 为 egui 逻辑点)
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

/// 同 [`icon_hit_button`]，但 `revealed == false` 时仅保留点击区不绘制(避免显隐切换导致 hover 抖动)。
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

/// 图标 + 文字(水平)
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
        IconId::Settings,
    ]
}

// --- 图集绘制(0..1 单元格内坐标)---

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
            // 代码括号 <>，避免与 Server 矩形同轮廓
            let w = 2.2_f32;
            p.segs(
                &[
                    (0.38, 0.22, 0.20, 0.50),
                    (0.20, 0.50, 0.38, 0.78),
                    (0.62, 0.22, 0.80, 0.50),
                    (0.80, 0.50, 0.62, 0.78),
                ],
                w,
            );
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
            let w = 2.2_f32;
            p.rect(0.16, 0.18, 0.84, 0.64, w);
            p.segs(&[(0.36, 0.78, 0.64, 0.78), (0.50, 0.64, 0.50, 0.78)], w);
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
        // 开口文件夹：主体 + 清晰页签
        IconId::Folder => {
            let w = 2.2_f32;
            p.rect(0.18, 0.38, 0.82, 0.80, w);
            p.segs(
                &[
                    (0.18, 0.38, 0.42, 0.38),
                    (0.42, 0.38, 0.50, 0.24),
                    (0.50, 0.24, 0.78, 0.24),
                    (0.78, 0.24, 0.78, 0.38),
                ],
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
        IconId::Package => {
            // 包装盒(压缩包 / 包类条目)
            let w = 2.0_f32;
            p.rect(0.22, 0.32, 0.78, 0.80, w);
            p.segs(
                &[
                    (0.22, 0.32, 0.50, 0.18),
                    (0.78, 0.32, 0.50, 0.18),
                    (0.50, 0.18, 0.50, 0.32),
                    (0.50, 0.32, 0.50, 0.80),
                ],
                w,
            );
        }
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
        IconId::Network => {
            // 三节点拓扑(避免旧「弓形」缩成 ^)
            let w = 2.0_f32;
            p.fill_circle(0.50, 0.22, 0.09);
            p.fill_circle(0.20, 0.74, 0.09);
            p.fill_circle(0.80, 0.74, 0.09);
            p.segs(
                &[
                    (0.50, 0.30, 0.24, 0.66),
                    (0.50, 0.30, 0.76, 0.66),
                    (0.28, 0.74, 0.72, 0.74),
                ],
                w,
            );
        }
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
            // 机架：外框 + 三层插槽(与 Fragment 括号区分)
            let w = 2.2_f32;
            p.rect(0.22, 0.16, 0.78, 0.84, w);
            for y in [0.34, 0.50, 0.66] {
                p.segs(&[(0.32, y, 0.68, y)], 2.0);
            }
            p.fill_circle(0.70, 0.34, 0.045);
            p.fill_circle(0.70, 0.50, 0.045);
            p.fill_circle(0.70, 0.66, 0.045);
        }
        IconId::Database => {
            p.segs(
                &[(0.25, 0.32, 0.75, 0.32), (0.25, 0.68, 0.75, 0.68), (0.25, 0.32, 0.25, 0.68), (0.75, 0.32, 0.75, 0.68)],
                w,
            );
            p.segs(&[(0.3, 0.22, 0.7, 0.22), (0.3, 0.78, 0.7, 0.78)], w);
        }
        IconId::Api => {
            // AI：聊天气泡 + 三点(比折线 N 更易认)
            let w = 2.2_f32;
            p.rect(0.16, 0.16, 0.84, 0.58, w);
            p.segs(&[(0.32, 0.58, 0.26, 0.80), (0.26, 0.80, 0.48, 0.58)], w);
            p.fill_circle(0.34, 0.37, 0.055);
            p.fill_circle(0.50, 0.37, 0.055);
            p.fill_circle(0.66, 0.37, 0.055);
        }
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
        IconId::Settings => {
            // 偏好设置：齿轮
            let w = 2.0_f32;
            p.circle(0.5, 0.5, 0.22, w);
            p.fill_circle(0.5, 0.5, 0.08);
            for i in 0..6 {
                let a = std::f32::consts::TAU * i as f32 / 6.0;
                let (c, s) = (a.cos(), a.sin());
                p.segs(
                    &[(0.5 + 0.28 * c, 0.5 + 0.28 * s, 0.5 + 0.42 * c, 0.5 + 0.42 * s)],
                    w,
                );
            }
        }
    }
}

/// Mist 字母 M 笔画(0..1 归一化坐标)
const M_LETTER_SEGS: &[Seg] = &[
    (0.24, 0.78, 0.24, 0.22),
    (0.24, 0.22, 0.5, 0.58),
    (0.76, 0.22, 0.5, 0.58),
    (0.76, 0.78, 0.76, 0.22),
];

fn draw_m_letter_cell(p: &mut CellPainter<'_>, stroke_w: f32) {
    p.segs(M_LETTER_SEGS, stroke_w);
}

/// 霓虹青(提示符外发光 / 雾气)
const APP_ICON_CYAN: [u8; 3] = [55, 175, 255];
/// 提示符核心高光白
const APP_ICON_TEXT_CORE: [u8; 4] = [238, 246, 255, 255];
/// macOS 打包源图规格(`scripts/bundle-macos.sh` 由它生成 .icns)。
/// 对齐 Apple 1024 模板：本体约 824×824、四周各约 100px 透明边、圆角约 185px。
const APP_ICON_BUNDLE_SIZE: u32 = 1024;
const APP_ICON_BUNDLE_PAD_FRAC: f32 = 100.0 / 1024.0;
/// 相对底板边长的圆角比例（185 / 824）。
const APP_ICON_BUNDLE_CORNER_FRAC: f32 = 185.0 / 824.0;

/// Windows 任务栏 / exe 资源图标规格(`assets/app-icon.ico` 同样使用)
pub const APP_ICON_WINDOWS_PAD_FRAC: f32 = 0.02;
pub const APP_ICON_WINDOWS_CORNER_FRAC: f32 = 0.10;

/// 图标透明外圈比例：macOS 与打包 .icns 同一套规格；Windows 任务栏留白宜小。
fn app_icon_outer_pad_frac() -> f32 {
    if cfg!(windows) {
        APP_ICON_WINDOWS_PAD_FRAC
    } else if cfg!(target_os = "macos") {
        APP_ICON_BUNDLE_PAD_FRAC
    } else {
        0.05
    }
}

/// 圆角比例：macOS 与打包源图一致；Windows 任务栏再套方角缩放，圆角过大会吃掉有效面积。
fn app_icon_corner_frac() -> f32 {
    if cfg!(windows) {
        APP_ICON_WINDOWS_CORNER_FRAC
    } else if cfg!(target_os = "macos") {
        APP_ICON_BUNDLE_CORNER_FRAC
    } else {
        0.165
    }
}

/// 窗口 / Dock / 任务栏图标(霓虹 `>_` + 底部烟雾)。
pub fn app_window_icon_data() -> eframe::IconData {
    const SIZE: u32 = 256;
    let img = render_app_icon(SIZE, app_icon_outer_pad_frac(), app_icon_corner_frac());
    eframe::IconData {
        rgba: img.into_raw(),
        width: SIZE,
        height: SIZE,
    }
}

/// 按任意尺寸原生绘制应用图标(外圈透明)。
pub fn render_app_icon(size: u32, pad_frac: f32, corner_frac: f32) -> RgbaImage {
    let pad = (size as f32 * pad_frac).round() as u32;
    let mut img = RgbaImage::from_pixel(size, size, Rgba([0, 0, 0, 0]));
    let edge = size - pad;
    paint_mist_app_icon(&mut img, pad, edge, edge, corner_frac);
    img
}

/// 按 macOS 打包规格渲染(预览不同尺寸用)。
pub fn render_app_icon_bundle(size: u32) -> RgbaImage {
    render_app_icon(size, APP_ICON_BUNDLE_PAD_FRAC, APP_ICON_BUNDLE_CORNER_FRAC)
}

/// 导出窗口图标 PNG 预览(`cargo run --bin export_app_icon`)。
pub fn export_app_icon_png(path: &std::path::Path) -> Result<(), image::ImageError> {
    render_app_icon(256, app_icon_outer_pad_frac(), app_icon_corner_frac()).save(path)
}

/// 导出 macOS 打包用 1024 源图(`cargo run --bin export_app_icon`)。
pub fn export_app_icon_bundle_png(path: &std::path::Path) -> Result<(), image::ImageError> {
    render_app_icon_bundle(APP_ICON_BUNDLE_SIZE).save(path)
}

/// `>_` 相对底板宽度的缩放
const APP_ICON_PROMPT_SCALE: f32 = 0.62;

/// 在 `[x0,x1)×[y0,y1)` 内绘制 Mist 品牌图标：居中霓虹 `>_`，底部烟雾。
fn paint_mist_app_icon(img: &mut RgbaImage, x0: u32, x1: u32, y1: u32, corner_frac: f32) {
    let y0 = x0;
    let w = (x1 - x0) as f32;
    let h = (y1 - y0) as f32;
    let ox = x0 as f32;
    let oy = y0 as f32;

    fill_vertical_gradient(img, x0, y0, x1, y1, [19, 27, 52], [8, 11, 26]);
    paint_mist_smoke(img, x0, y0, x1, y1);

    let mut neon = NeonMask::new(img.width(), img.height());
    add_prompt_centered(&mut neon, ox + w * 0.5, oy + h * 0.45, w * APP_ICON_PROMPT_SCALE);
    neon.composite(img, w);

    let radius = w.min(h) * corner_frac;
    paint_window_border(img, ox, oy, ox + w, oy + h, radius, (w * 0.008).max(1.0));
    apply_rounded_alpha_mask(img, ox, oy, ox + w, oy + h, radius);
}

/// `>` 几何(以 `scale` = 1 时的底板宽度为单位)
const CHEVRON_W: f32 = 0.20;
const CHEVRON_HALF_H: f32 = 0.155;
const PROMPT_HALF_STROKE: f32 = 0.042;
const CURSOR_GAP: f32 = 0.08;
const CURSOR_W: f32 = 0.27;

/// `>` 描边：左端点 x = `left`，垂直中心 = `cy`
fn add_chevron(neon: &mut NeonMask, left: f32, cy: f32, scale: f32) {
    let hw = scale * PROMPT_HALF_STROKE;
    let a = (left, cy - scale * CHEVRON_HALF_H);
    let b = (left + scale * CHEVRON_W, cy);
    let c = (left, cy + scale * CHEVRON_HALF_H);
    neon.add_sdf((a.0 - hw, a.1 - hw, b.0 + hw, c.1 + hw), |px, py| {
        dist_to_segment(px, py, a, b).min(dist_to_segment(px, py, b, c)) - hw
    });
}

/// 下划线光标：底边与 `>` 下端对齐
fn add_cursor(neon: &mut NeonMask, x0: f32, x1: f32, bottom: f32, half_stroke: f32) {
    let (y0, r) = (bottom - half_stroke * 2.0, half_stroke * 0.35);
    neon.add_sdf((x0, y0, x1, bottom), |px, py| sdf_rounded_rect(px, py, x0, y0, x1, bottom, r));
}

/// `>_`：`left` 为 `>` 左端点，`cy` 为垂直中心
fn add_prompt(neon: &mut NeonMask, left: f32, cy: f32, scale: f32) {
    let hw = scale * PROMPT_HALF_STROKE;
    add_chevron(neon, left, cy, scale);
    let x0 = left + scale * (CHEVRON_W + CURSOR_GAP);
    add_cursor(neon, x0, x0 + scale * CURSOR_W, cy + scale * CHEVRON_HALF_H + hw, hw);
}

/// `>_` 以可见外框中心定位
fn add_prompt_centered(neon: &mut NeonMask, cx: f32, cy: f32, scale: f32) {
    let hw = scale * PROMPT_HALF_STROKE;
    let span = scale * (CHEVRON_W + CURSOR_GAP + CURSOR_W) + hw;
    add_prompt(neon, cx - span * 0.5 + hw, cy, scale);
}

/// 霓虹层覆盖率遮罩：图形先合成，再统一模糊出外发光
struct NeonMask {
    w: usize,
    h: usize,
    cov: Vec<f32>,
}

impl NeonMask {
    fn new(w: u32, h: u32) -> Self {
        let (w, h) = (w as usize, h as usize);
        Self { w, h, cov: vec![0.0; w * h] }
    }

    fn max_at(&mut self, x: i32, y: i32, c: f32) {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h {
            return;
        }
        let i = y as usize * self.w + x as usize;
        self.cov[i] = self.cov[i].max(c);
    }

    fn add_sdf(&mut self, bounds: (f32, f32, f32, f32), sdf: impl Fn(f32, f32) -> f32) {
        let x0 = (bounds.0 - 2.0).floor().max(0.0) as i32;
        let y0 = (bounds.1 - 2.0).floor().max(0.0) as i32;
        let x1 = (bounds.2 + 2.0).ceil().min(self.w as f32 - 1.0) as i32;
        let y1 = (bounds.3 + 2.0).ceil().min(self.h as f32 - 1.0) as i32;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let c = (0.5 - sdf(x as f32 + 0.5, y as f32 + 0.5)).clamp(0.0, 1.0);
                self.max_at(x, y, c);
            }
        }
    }

    /// 三次盒式模糊近似高斯
    fn blurred(&self, radius: f32) -> Vec<f32> {
        let r = (radius / 1.7).round().max(1.0) as usize;
        let mut a = self.cov.clone();
        let mut tmp = vec![0.0; a.len()];
        for _ in 0..3 {
            box_blur_pass(&a, &mut tmp, self.w, self.h, r, self.w, 1);
            box_blur_pass(&tmp, &mut a, self.h, self.w, r, 1, self.w);
        }
        a
    }

    /// 宽外发光 + 窄外发光 + 白色核心
    fn composite(&self, img: &mut RgbaImage, plate_w: f32) {
        let wide = self.blurred(plate_w * 0.06);
        let tight = self.blurred(plate_w * 0.012);
        let c = APP_ICON_CYAN;
        let core = APP_ICON_TEXT_CORE;
        for (i, &cov) in self.cov.iter().enumerate() {
            let (x, y) = ((i % self.w) as i32, (i / self.w) as i32);
            let a1 = ((wide[i] * 1.6).min(1.0) * 110.0).round() as u8;
            let a2 = ((tight[i] * 1.5).min(1.0) * 200.0).round() as u8;
            let a3 = (cov * core[3] as f32).round() as u8;
            blend_pixel(img, x, y, [c[0], c[1], c[2], a1]);
            blend_pixel(img, x, y, [c[0], c[1], c[2], a2]);
            blend_pixel(img, x, y, [core[0], core[1], core[2], a3]);
        }
    }
}

/// 一维盒式模糊：沿 `len` 方向(步长 `step`)处理 `lines` 条线(线间步长 `line_step`)，界外视为 0
fn box_blur_pass(
    src: &[f32],
    dst: &mut [f32],
    len: usize,
    lines: usize,
    r: usize,
    line_step: usize,
    step: usize,
) {
    let norm = 1.0 / (2 * r + 1) as f32;
    for line in 0..lines {
        let base = line * line_step;
        let at = |k: usize| src[base + k * step];
        let mut sum: f32 = (0..=r.min(len - 1)).map(at).sum();
        for k in 0..len {
            dst[base + k * step] = sum * norm;
            if k + r + 1 < len {
                sum += at(k + r + 1);
            }
            if k >= r {
                sum -= at(k - r);
            }
        }
    }
}

/// 底部烟雾：波浪上沿 + 扭曲噪声丝缕，坐标按底板归一化，各尺寸形态一致
fn paint_mist_smoke(img: &mut RgbaImage, x0: u32, y0: u32, x1: u32, y1: u32) {
    const DEEP: [f32; 3] = [28.0, 78.0, 140.0];
    const BRIGHT: [f32; 3] = [80.0, 175.0, 240.0];
    let w = (x1 - x0) as f32;
    let h = (y1 - y0) as f32;
    let v_start = 0.55;
    for y in ((y0 as f32 + h * v_start) as u32)..y1 {
        let v = (y - y0) as f32 / h;
        for x in x0..x1 {
            let u = (x - x0) as f32 / w;
            let edge = 0.70 + 0.045 * (u * 5.3 + 0.7).sin() + 0.06 * (fbm(u * 2.5, 3.1) - 0.5);
            let body = smoothstep(edge - 0.08, edge + 0.26, v);
            if body <= 0.0 {
                continue;
            }
            let wx = u * 2.2 + 1.0 * fbm(u * 1.5 + 4.1, v * 2.5 + 1.7);
            let wy = v * 4.0 + 1.0 * fbm(u * 1.5 + 8.3, v * 2.5 + 2.9);
            let n = fbm(wx, wy);
            let wisp = (1.0 - (2.0 * n - 1.0).abs()).powi(2);
            let density = (body * (0.35 + 0.50 * n + 0.35 * wisp)).clamp(0.0, 1.0);
            let t = (n * 0.5 + wisp * 0.3 + body * 0.2).clamp(0.0, 1.0);
            let c = |i: usize| (DEEP[i] + (BRIGHT[i] - DEEP[i]) * t).round() as u8;
            let a = (density * 150.0).round() as u8;
            blend_pixel(img, x as i32, y as i32, [c(0), c(1), c(2), a]);
        }
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn hash_noise(ix: i32, iy: i32) -> f32 {
    let mut h = (ix as u32).wrapping_mul(374_761_393) ^ (iy as u32).wrapping_mul(668_265_263);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h ^ (h >> 16)) & 0x00ff_ffff) as f32 / 16_777_215.0
}

fn value_noise(x: f32, y: f32) -> f32 {
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - x.floor(), y - y.floor());
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let top = hash_noise(ix, iy) + (hash_noise(ix + 1, iy) - hash_noise(ix, iy)) * sx;
    let bottom = hash_noise(ix, iy + 1) + (hash_noise(ix + 1, iy + 1) - hash_noise(ix, iy + 1)) * sx;
    top + (bottom - top) * sy
}

/// 分形噪声，输出约 0..1
fn fbm(x: f32, y: f32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 0.5, 1.0, 0.0);
    for _ in 0..4 {
        sum += value_noise(x * freq, y * freq) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

/// 沿底板内侧的细描边，深色 Dock / 任务栏上也能看清窗口轮廓
fn paint_window_border(img: &mut RgbaImage, x0: f32, y0: f32, x1: f32, y1: f32, r: f32, width: f32) {
    paint_sdf(img, (x0, y0, x1, y1), [120, 200, 255, 70], 0.0, |px, py| {
        (sdf_rounded_rect(px, py, x0, y0, x1, y1, r) + width * 0.5).abs() - width * 0.5
    });
}

fn dist_to_segment(px: f32, py: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = (dx * dx + dy * dy).max(1e-6);
    let t = (((px - a.0) * dx + (py - a.1) * dy) / len2).clamp(0.0, 1.0);
    let (qx, qy) = (a.0 + dx * t - px, a.1 + dy * t - py);
    (qx * qx + qy * qy).sqrt()
}

/// 按 SDF(负值 = 内侧)着色：`glow == 0` 时抗锯齿实心；否则内侧满色、外侧 `glow` 像素内平方衰减。
fn paint_sdf(
    img: &mut RgbaImage,
    bounds: (f32, f32, f32, f32),
    color: [u8; 4],
    glow: f32,
    sdf: impl Fn(f32, f32) -> f32,
) {
    let m = glow + 2.0;
    let x0 = (bounds.0 - m).floor().max(0.0) as i32;
    let y0 = (bounds.1 - m).floor().max(0.0) as i32;
    let x1 = (bounds.2 + m).ceil().min(img.width() as f32 - 1.0) as i32;
    let y1 = (bounds.3 + m).ceil().min(img.height() as f32 - 1.0) as i32;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let d = sdf(x as f32 + 0.5, y as f32 + 0.5);
            let cov = if glow > 0.0 {
                if d <= 0.0 { 1.0 } else { (1.0 - d / glow).max(0.0).powi(2) }
            } else {
                (0.5 - d).clamp(0.0, 1.0)
            };
            let a = (color[3] as f32 * cov).round() as u8;
            if a > 0 {
                blend_pixel(img, x, y, [color[0], color[1], color[2], a]);
            }
        }
    }
}

/// 圆角矩形 SDF(负值 = 内侧)
fn sdf_rounded_rect(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> f32 {
    let cx = (x0 + x1) * 0.5;
    let cy = (y0 + y1) * 0.5;
    let hx = (x1 - x0) * 0.5 - r;
    let hy = (y1 - y0) * 0.5 - r;
    let qx = (px - cx).abs() - hx;
    let qy = (py - cy).abs() - hy;
    let ax = qx.max(0.0);
    let ay = qy.max(0.0);
    (ax * ax + ay * ay).sqrt() - r + qx.max(qy).min(0.0)
}

fn rounded_rect_coverage(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> f32 {
    let d = sdf_rounded_rect(px, py, x0, y0, x1, y1, r);
    (0.5 - d).clamp(0.0, 1.0)
}

/// 将图标内容裁切为圆角方形(外侧透明)
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
