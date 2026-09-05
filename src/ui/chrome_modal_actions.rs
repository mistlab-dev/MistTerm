use super::*;

/// 弹窗主按钮(自绘三态；勿 `add_enabled` 灰化，否则悬停不可见)
pub struct ModalPrimaryButton<'a> {
    theme: &'a Theme,
    label: &'a str,
    /// `false` 时仍可悬停高亮，点击由调用方忽略
    can_activate: bool,
}

impl ModalPrimaryButton<'_> {
    pub fn can_activate(mut self, can: bool) -> Self {
        self.can_activate = can;
        self
    }
}

impl Widget for ModalPrimaryButton<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        paint_modal_primary_button(ui, self.theme, self.label, self.can_activate)
    }
}

pub fn modal_primary_button_widget<'a>(theme: &'a Theme, label: &'a str) -> ModalPrimaryButton<'a> {
    ModalPrimaryButton {
        theme,
        label,
        can_activate: true,
    }
}

fn paint_modal_primary_button(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    can_activate: bool,
) -> Response {
    paint_control_button(
        ui,
        theme,
        label,
        None,
        ControlButtonVariant::Primary,
        theme.size_modal_footer_btn_min_w_primary(),
        can_activate,
    )
}

fn paint_modal_secondary_button(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    paint_control_button(
        ui,
        theme,
        label,
        None,
        ControlButtonVariant::Secondary,
        theme.size_modal_footer_btn_min_w_secondary(),
        true,
    )
}

fn paint_modal_danger_button(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    let size = theme.vec2_modal_footer_secondary();
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let rounding = theme.radius_list_item();
    let hovered = response.hovered();
    let pressed = response.is_pointer_button_down_on();
    if hovered || pressed {
        ui.ctx().request_repaint();
    }
    if hovered || pressed {
        ui.painter().rect_filled(
            rect,
            rounding,
            theme
                .red_color()
                .gamma_multiply(if pressed { 0.22 } else { 0.14 }),
        );
    }
    let text_color = if hovered || pressed {
        theme.red_color()
    } else {
        theme.red_color().gamma_multiply(0.85)
    };
    paint_caption_in_rect_center(ui, rect, label, theme.font_size_normal(), text_color);
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

pub fn modal_secondary_button(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    paint_modal_secondary_button(ui, theme, label)
}

pub fn modal_primary_button(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    paint_modal_primary_button(ui, theme, label, true)
}

pub fn modal_danger_button(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    paint_modal_danger_button(ui, theme, label)
}

pub fn modal_secondary_icon_button(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    tooltip: &str,
) -> Response {
    let label_size = control_button_size(
        ui,
        theme,
        tooltip,
        true,
        theme.size_modal_footer_btn_min_w_secondary(),
    );
    let response = if ui.available_width() >= label_size.x {
        paint_control_button(
            ui,
            theme,
            tooltip,
            Some(icon),
            ControlButtonVariant::Secondary,
            theme.size_modal_footer_btn_min_w_secondary(),
            true,
        )
    } else {
        paint_icon_only_button(
            ui,
            theme,
            icon,
            ControlButtonVariant::Secondary,
            theme.size_modal_footer_btn_min_w_secondary(),
            true,
        )
    };
    response.on_hover_text(tooltip)
}

pub fn modal_primary_icon_button(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    tooltip: &str,
) -> Response {
    modal_primary_icon_button_ex(ui, theme, icon, tooltip, true)
}

pub fn modal_primary_icon_button_ex(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    tooltip: &str,
    can_activate: bool,
) -> Response {
    let label_size = control_button_size(
        ui,
        theme,
        tooltip,
        true,
        theme.size_modal_footer_btn_min_w_primary(),
    );
    let response = if ui.available_width() >= label_size.x {
        paint_control_button(
            ui,
            theme,
            tooltip,
            Some(icon),
            ControlButtonVariant::Primary,
            theme.size_modal_footer_btn_min_w_primary(),
            can_activate,
        )
    } else {
        paint_icon_only_button(
            ui,
            theme,
            icon,
            ControlButtonVariant::Primary,
            theme.size_modal_footer_btn_min_w_primary(),
            can_activate,
        )
    };
    response.on_hover_text(tooltip)
}

/// 弹窗底栏主操作(纯图标)，用于 `ui.add(...)`。
pub struct ModalPrimaryIconButton<'a> {
    theme: &'a Theme,
    icon: IconId,
    tooltip: &'a str,
    can_activate: bool,
}

impl Widget for ModalPrimaryIconButton<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        modal_primary_icon_button_ex(ui, self.theme, self.icon, self.tooltip, self.can_activate)
    }
}

impl ModalPrimaryIconButton<'_> {
    pub fn can_activate(mut self, can: bool) -> Self {
        self.can_activate = can;
        self
    }
}

pub fn modal_primary_icon_button_widget<'a>(
    theme: &'a Theme,
    icon: IconId,
    tooltip: &'a str,
) -> ModalPrimaryIconButton<'a> {
    ModalPrimaryIconButton {
        theme,
        icon,
        tooltip,
        can_activate: true,
    }
}

/// 弹窗底栏主操作：图标 + 可见文字。
pub fn modal_primary_button_with_icon_ex(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    label: &str,
    can_activate: bool,
) -> Response {
    paint_control_button(
        ui,
        theme,
        label,
        Some(icon),
        ControlButtonVariant::Primary,
        theme.size_modal_footer_btn_min_w_primary(),
        can_activate,
    )
}

pub fn modal_primary_button_with_icon(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    label: &str,
) -> Response {
    modal_primary_button_with_icon_ex(ui, theme, icon, label, true)
}

/// 弹窗底栏主操作(图标 + 文字)，用于 `ui.add(...)`。
pub struct ModalPrimaryButtonWithIcon<'a> {
    theme: &'a Theme,
    icon: IconId,
    label: &'a str,
    can_activate: bool,
}

impl Widget for ModalPrimaryButtonWithIcon<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        modal_primary_button_with_icon_ex(ui, self.theme, self.icon, self.label, self.can_activate)
    }
}

impl ModalPrimaryButtonWithIcon<'_> {
    pub fn can_activate(mut self, can: bool) -> Self {
        self.can_activate = can;
        self
    }
}

pub fn modal_primary_button_with_icon_widget<'a>(
    theme: &'a Theme,
    icon: IconId,
    label: &'a str,
) -> ModalPrimaryButtonWithIcon<'a> {
    ModalPrimaryButtonWithIcon {
        theme,
        icon,
        label,
        can_activate: true,
    }
}

pub fn modal_danger_icon_button(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    tooltip: &str,
) -> Response {
    let label_size = control_button_size(
        ui,
        theme,
        tooltip,
        false,
        theme.size_modal_footer_btn_min_w_secondary(),
    );
    let response = if ui.available_width() >= label_size.x {
        paint_modal_danger_button(ui, theme, tooltip)
    } else {
        paint_icon_only_button(
            ui,
            theme,
            icon,
            ControlButtonVariant::Danger,
            theme.size_modal_footer_btn_min_w_secondary(),
            true,
        )
    };
    response.on_hover_text(tooltip)
}

/// 右对齐底栏：先 add 主操作，再 add 次操作(`right_to_left` 布局)。
pub fn modal_footer_actions<F>(ui: &mut Ui, theme: &Theme, add_buttons: F)
where
    F: FnOnce(&mut Ui, &Theme),
{
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            add_buttons(ui, theme);
        });
    });
}
