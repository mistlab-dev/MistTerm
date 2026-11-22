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
        let resp = paint_modal_primary_button(ui, self.theme, self.label, self.can_activate);
        with_modal_primary_key_activation(ui.ctx(), resp, self.can_activate)
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

/// #463：确认框里的蓝色主按钮响应 Enter / Space（不必鼠标点）。
///
/// - 有真实文本输入框聚焦时让位（重命名弹窗的 Space 仍输入空格）；
/// - 仅弹窗底栏内的主按钮响应键盘，面板/菜单中的同款按钮不被全局回车误触；
/// - 每帧只激活一个主按钮：消费按键事件后闩住帧号；
/// - 同时吞掉 Enter/Space 的 Key 与 Text 事件，防止再喂给当前终端会话。
const MODAL_FOOTER_ACTIVE: &str = "mist_modal_footer_active_frame";

/// #463：从输入事件里移除主按钮已消费的 Enter/Space，避免再进终端。
fn consume_modal_primary_keys(i: &mut egui::InputState) -> bool {
    let pressed = i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space);
    if !pressed {
        return false;
    }
    i.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
    i.consume_key(egui::Modifiers::NONE, egui::Key::Space);
    i.events.retain(|e| !matches!(e, egui::Event::Text(t) if t == "\r" || t == "\n" || t == " "));
    true
}

fn modal_primary_key_activated(ctx: &egui::Context, can_activate: bool) -> bool {
    if !can_activate {
        return false;
    }
    // 仅弹窗底栏内的主按钮响应键盘；面板中的同款按钮不被全局 Enter/Space 误触。
    if ctx.data(|d| d.get_temp::<u64>(egui::Id::new(MODAL_FOOTER_ACTIVE)))
        != Some(ctx.cumulative_frame_nr())
    {
        return false;
    }
    // 有真实文本输入框(如「重命名标签」)聚焦时，Space 必须继续输入空格。
    // 终端 IME 隐藏框在确认框弹出后可能仍持有键盘焦点，但它不会走
    // `should_capture_pty_keyboard`（confirm 弹窗打开时该分支直接返回 false），
    // 所以这里直接以 egui 的文本输入焦点为准即可。
    if ctx.egui_wants_keyboard_input() {
        return false;
    }
    let plain_keys = ctx.input(|i| {
        !i.modifiers.ctrl && !i.modifiers.command && !i.modifiers.alt
    });
    if !plain_keys || !ctx.input_mut(consume_modal_primary_keys) {
        return false;
    }
    // 每帧只让第一个主按钮拿到激活（consume 已保证后续调用读不到按键，
    // 闩再兜一层，防止同帧其它代码重放按键）。
    let latch = egui::Id::new("mist_modal_primary_key_activation");
    let frame = ctx.cumulative_frame_nr();
    if ctx.data(|d| d.get_temp::<u64>(latch)) == Some(frame) {
        return false;
    }
    ctx.data_mut(|d| d.insert_temp(latch, frame));
    true
}

/// 把键盘激活并入主按钮响应，调用方仍只需 `.clicked()`。
fn with_modal_primary_key_activation(
    ctx: &egui::Context,
    mut resp: Response,
    can_activate: bool,
) -> Response {
    if modal_primary_key_activated(ctx, can_activate) {
        // egui 0.37：`clicked` 是方法而非字段，键盘激活需通过 Response.flags
        // 的 FAKE_PRIMARY_CLICKED 位并入 `.clicked()`（与 egui 内部对焦点
        // Enter/Space 的处理一致）。
        resp.flags.set(egui::response::Flags::FAKE_PRIMARY_CLICKED, true);
    }
    resp
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
    let resp = paint_modal_primary_button(ui, theme, label, true);
    with_modal_primary_key_activation(ui.ctx(), resp, true)
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
    with_modal_primary_key_activation(ui.ctx(), response, can_activate).on_hover_text(tooltip)
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
    let resp = paint_control_button(
        ui,
        theme,
        label,
        Some(icon),
        ControlButtonVariant::Primary,
        theme.size_modal_footer_btn_min_w_primary(),
        can_activate,
    );
    with_modal_primary_key_activation(ui.ctx(), resp, can_activate)
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
    let frame = ui.ctx().cumulative_frame_nr();
    ui.ctx()
        .data_mut(|d| d.insert_temp(egui::Id::new(MODAL_FOOTER_ACTIVE), frame));
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            add_buttons(ui, theme);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input_with_key(key: egui::Key, with_text: &str) -> egui::InputState {
        let mut i = egui::InputState::default();
        i.events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        if !with_text.is_empty() {
            i.events.push(egui::Event::Text(with_text.to_string()));
        }
        i
    }

    // #463：Enter / Space 触发主按钮后，Key 与 Text 事件都要吞掉，
    // 否则文本回车/空格会同时落到当前终端会话。
    #[test]
    fn modal_primary_consumes_enter_key_and_text() {
        let mut i = input_with_key(egui::Key::Enter, "\r");
        assert!(consume_modal_primary_keys(&mut i));
        assert!(!i.key_pressed(egui::Key::Enter));
        assert!(!i.events.iter().any(|e| matches!(e, egui::Event::Text(_))));
        // 第二次调用读不到按键，保证同帧只激活一个主按钮。
        assert!(!consume_modal_primary_keys(&mut i));
    }

    #[test]
    fn modal_primary_consumes_space_key_and_text() {
        let mut i = input_with_key(egui::Key::Space, " ");
        assert!(consume_modal_primary_keys(&mut i));
        assert!(!i.key_pressed(egui::Key::Space));
        assert!(!i.events.iter().any(|e| matches!(e, egui::Event::Text(_))));
    }

    #[test]
    fn modal_primary_ignores_unrelated_text() {
        let mut i = input_with_key(egui::Key::A, "a");
        assert!(!consume_modal_primary_keys(&mut i));
        assert!(i.events.iter().any(|e| matches!(e, egui::Event::Text(t) if t == "a")));
    }
}
