use std::ffi::CStr;
use std::os::raw::c_short;
use std::sync::Arc;
use std::{mem, ptr};

use x11_dl::xlib::{XIMCallback, XIMPreeditCaretCallbackStruct, XIMPreeditDrawCallbackStruct};

use super::{ffi, util, XConnection, XError};
use crate::platform_impl::platform::x11::ime::input_method::{InputMethod, Style, XIMStyle};
use crate::platform_impl::platform::x11::ime::{ImeEvent, ImeEventSender};

/// IME creation error.
#[derive(Debug)]
pub enum ImeContextCreationError {
    /// Got the error from Xlib.
    XError(XError),

    /// Got null pointer from Xlib but without exact reason.
    Null,
}

/// The callback used by XIM preedit functions.
type XIMProcNonnull = unsafe extern "C" fn(ffi::XIM, ffi::XPointer, ffi::XPointer);

/// Wrapper for creating XIM callbacks.
#[inline]
fn create_xim_callback(client_data: ffi::XPointer, callback: XIMProcNonnull) -> ffi::XIMCallback {
    XIMCallback { client_data, callback: Some(callback) }
}

/// The server started preedit.
extern "C" fn preedit_start_callback(
    _xim: ffi::XIM,
    client_data: ffi::XPointer,
    _call_data: ffi::XPointer,
) -> i32 {
    let client_data = unsafe { &mut *(client_data as *mut ImeContextClientData) };

    if ime_debug() {
        eprintln!("[ime-xim] PREEDIT-START win={:#x}", client_data.window);
    }
    client_data.text.clear();
    client_data.cursor_pos = 0;
    client_data
        .event_sender
        .send((client_data.window, ImeEvent::Start))
        .expect("failed to send preedit start event");
    -1
}

/// Done callback is used when the preedit should be hidden.
extern "C" fn preedit_done_callback(
    _xim: ffi::XIM,
    client_data: ffi::XPointer,
    _call_data: ffi::XPointer,
) {
    let client_data = unsafe { &mut *(client_data as *mut ImeContextClientData) };

    if ime_debug() {
        eprintln!("[ime-xim] PREEDIT-END win={:#x} buffered_chars={}", client_data.window, client_data.text.len());
    }
    // Drop text buffer and reset cursor position on done.
    client_data.text = Vec::new();
    client_data.cursor_pos = 0;

    client_data
        .event_sender
        .send((client_data.window, ImeEvent::End))
        .expect("failed to send preedit end event");
}

fn calc_byte_position(text: &[char], pos: usize) -> usize {
    text.iter().take(pos).fold(0, |byte_pos, text| byte_pos + text.len_utf8())
}

/// Preedit text information to be drawn inline by the client.
extern "C" fn preedit_draw_callback(
    _xim: ffi::XIM,
    client_data: ffi::XPointer,
    call_data: ffi::XPointer,
) {
    let client_data = unsafe { &mut *(client_data as *mut ImeContextClientData) };
    let call_data = unsafe { &mut *(call_data as *mut XIMPreeditDrawCallbackStruct) };
    client_data.cursor_pos = call_data.caret as usize;

    let chg_range =
        call_data.chg_first as usize..(call_data.chg_first + call_data.chg_length) as usize;
    if !chg_range_valid(client_data.text.len(), &chg_range) {
        // MISTTERM (#431/#430): `tracing::warn!` is invisible in GUI runs where
        // RUST_LOG is unset; escalate this diagnostic to stderr. A dropped Draw
        // leaves the preedit buffer empty ("Ime Preedit {\"text: \"\"}") and,
        // on ibus/XIM, correlates with the commit never being delivered to the
        // app — exactly the observed failure signature.
        if ime_debug() {
            eprintln!(
                "[ime-xim] DRAW-DROP invalid-chg-range buf_len={} chg_first={} chg_length={}",
                client_data.text.len(),
                call_data.chg_first,
                call_data.chg_length
            );
        }
        return;
    }

    // NULL indicate text deletion
    let mut new_chars = if call_data.text.is_null() {
        Vec::new()
    } else {
        let xim_text = unsafe { &mut *(call_data.text) };
        if xim_text.encoding_is_wchar > 0 {
            // MISTTERM (#431/#430): wchar-encoded Draw payloads were silently
            // dropped (silent `return` + no log). ibus/XIM is not known to use
            // wchar here, but if a server ever does, this is where the preedit
            // would vanish without a trace. Reconstruct the text in Rust,
            // store it, and emit the update instead of dropping it.
            let chars: Vec<char> = if xim_text.length == 0 {
                Vec::new()
            } else {
                let wide = unsafe {
                    std::slice::from_raw_parts(xim_text.string.wide_char, xim_text.length as usize)
                };
                match String::from_utf16(&wide.iter().map(|w| *w as u16).collect::<Vec<_>>()) {
                    Ok(s) => s.chars().collect(),
                    Err(err) => {
                        if ime_debug() {
                            eprintln!("[ime-xim] DRAW-DROP wchar-decode-failed err={err}");
                        }
                        Vec::new()
                    },
                }
            };
            if ime_debug() {
                eprintln!("[ime-xim] DRAW wchar path len={}", chars.len());
            }
            apply_draw(
                client_data,
                &chg_range,
                chars,
                call_data.caret as usize,
            );
            return;
        }

        let new_text = unsafe { xim_text.string.multi_byte };

        if new_text.is_null() {
            if ime_debug() {
                eprintln!("[ime-xim] DRAW null-text win={:#x}", client_data.window);
            }
            return;
        }

        let new_text = unsafe { CStr::from_ptr(new_text) };

        String::from(new_text.to_str().expect("Invalid UTF-8 String from IME")).chars().collect()
    };
    apply_draw(client_data, &chg_range, new_chars, call_data.caret as usize);
}

/// MISTTERM (#431/#430): shared tail of `preedit_draw_callback` — splice
/// `new_chars` into the buffered preedit at `chg_range` and emit
/// `ImeEvent::Update`. Also logs the draw payload when IME debugging is on.
fn apply_draw(
    client_data: &mut ImeContextClientData,
    chg_range: &std::ops::Range<usize>,
    new_chars: Vec<char>,
    caret: usize,
) {
    if ime_debug() {
        let text: String = new_chars.iter().collect();
        eprintln!(
            "[ime-xim] DRAW win={:#x} buf_len={} chg_first={} chg_length={} caret={} text={:?}",
            client_data.window,
            client_data.text.len(),
            chg_range.start,
            chg_range.end - chg_range.start,
            caret,
            text
        );
    }
    let mut old_text_tail = client_data.text.split_off(chg_range.end);
    client_data.text.truncate(chg_range.start);
    client_data.text.append(&mut new_chars.clone());
    client_data.text.append(&mut old_text_tail);
    client_data.cursor_pos = caret;
    let cursor_byte_pos = calc_byte_position(&client_data.text, client_data.cursor_pos);

    client_data
        .event_sender
        .send((
            client_data.window,
            ImeEvent::Update(client_data.text.iter().collect(), cursor_byte_pos),
        ))
        .expect("failed to send preedit update event");
}

/// MISTTERM (#431/#430): stderr-only IME diagnostics gate (matches app-side
/// `MISTTERM_IME_DEBUG`). Kept as a helper so probe sites stay one-liners.
fn ime_debug() -> bool {
    std::env::var_os("MISTTERM_IME_DEBUG").is_some()
}

/// Handling of cursor movements in preedit text.
extern "C" fn preedit_caret_callback(
    _xim: ffi::XIM,
    client_data: ffi::XPointer,
    call_data: ffi::XPointer,
) {
    let client_data = unsafe { &mut *(client_data as *mut ImeContextClientData) };
    let call_data = unsafe { &mut *(call_data as *mut XIMPreeditCaretCallbackStruct) };

    if call_data.direction == ffi::XIMCaretDirection::XIMAbsolutePosition {
        client_data.cursor_pos = call_data.position as usize;
        let cursor_byte_pos = calc_byte_position(&client_data.text, client_data.cursor_pos);

        client_data
            .event_sender
            .send((
                client_data.window,
                ImeEvent::Update(client_data.text.iter().collect(), cursor_byte_pos),
            ))
            .expect("failed to send preedit update event");
    }
}

/// Struct to simplify callback creation and latter passing into Xlib XIM.
struct PreeditCallbacks {
    start_callback: ffi::XIMCallback,
    done_callback: ffi::XIMCallback,
    draw_callback: ffi::XIMCallback,
    caret_callback: ffi::XIMCallback,
}

impl PreeditCallbacks {
    pub fn new(client_data: ffi::XPointer) -> PreeditCallbacks {
        let start_callback = create_xim_callback(client_data, unsafe {
            mem::transmute::<usize, unsafe extern "C" fn(ffi::XIM, ffi::XPointer, ffi::XPointer)>(
                preedit_start_callback as *const () as usize,
            )
        });
        let done_callback = create_xim_callback(client_data, preedit_done_callback);
        let caret_callback = create_xim_callback(client_data, preedit_caret_callback);
        let draw_callback = create_xim_callback(client_data, preedit_draw_callback);

        PreeditCallbacks { start_callback, done_callback, caret_callback, draw_callback }
    }
}

struct ImeContextClientData {
    window: ffi::Window,
    event_sender: ImeEventSender,
    text: Vec<char>,
    cursor_pos: usize,
}

// XXX: this struct doesn't destroy its XIC resource when dropped.
// This is intentional, as it doesn't have enough information to know whether or not the context
// still exists on the server. Since `ImeInner` has that awareness, destruction must be handled
// through `ImeInner`.
pub struct ImeContext {
    pub(crate) ic: ffi::XIC,
    pub(crate) ic_spot: ffi::XPoint,
    pub(crate) allowed: bool,
    // Since the data is passed shared between X11 XIM callbacks, but couldn't be directly free
    // from there we keep the pointer to automatically deallocate it.
    _client_data: Box<ImeContextClientData>,
}

impl ImeContext {
    pub(crate) unsafe fn new(
        xconn: &Arc<XConnection>,
        im: &InputMethod,
        window: ffi::Window,
        ic_spot: Option<ffi::XPoint>,
        event_sender: ImeEventSender,
        allowed: bool,
    ) -> Result<Self, ImeContextCreationError> {
        let client_data = Box::into_raw(Box::new(ImeContextClientData {
            window,
            event_sender,
            text: Vec::new(),
            cursor_pos: 0,
        }));

        let style = if allowed { im.preedit_style } else { im.none_style };

        let ic = match style as _ {
            Style::Preedit(style) => unsafe {
                ImeContext::create_preedit_ic(
                    xconn,
                    im.im,
                    style,
                    window,
                    client_data as ffi::XPointer,
                )
            },
            Style::Nothing(style) => unsafe {
                ImeContext::create_nothing_ic(xconn, im.im, style, window)
            },
            Style::None(style) => unsafe {
                ImeContext::create_none_ic(xconn, im.im, style, window)
            },
        }
        .ok_or(ImeContextCreationError::Null)?;

        xconn.check_errors().map_err(ImeContextCreationError::XError)?;

        if std::env::var_os("MISTTERM_IME_DEBUG").is_some() {
            eprintln!("[ime-vendor] IC created: allowed={}", allowed);
        }
        let mut context = ImeContext {
            ic,
            ic_spot: ffi::XPoint { x: 0, y: 0 },
            allowed,
            _client_data: unsafe { Box::from_raw(client_data) },
        };

        // Set the spot location, if it's present.
        if let Some(ic_spot) = ic_spot {
            context.set_spot(xconn, ic_spot.x, ic_spot.y)
        }

        Ok(context)
    }

    unsafe fn create_none_ic(
        xconn: &Arc<XConnection>,
        im: ffi::XIM,
        style: XIMStyle,
        window: ffi::Window,
    ) -> Option<ffi::XIC> {
        let ic = unsafe {
            (xconn.xlib.XCreateIC)(
                im,
                ffi::XNInputStyle_0.as_ptr() as *const _,
                style,
                ffi::XNClientWindow_0.as_ptr() as *const _,
                window,
                ptr::null_mut::<()>(),
            )
        };

        (!ic.is_null()).then_some(ic)
    }

    unsafe fn create_preedit_ic(
        xconn: &Arc<XConnection>,
        im: ffi::XIM,
        style: XIMStyle,
        window: ffi::Window,
        client_data: ffi::XPointer,
    ) -> Option<ffi::XIC> {
        let preedit_callbacks = PreeditCallbacks::new(client_data);
        let preedit_attr = util::memory::XSmartPointer::new(xconn, unsafe {
            (xconn.xlib.XVaCreateNestedList)(
                0,
                ffi::XNPreeditStartCallback_0.as_ptr() as *const _,
                &(preedit_callbacks.start_callback) as *const _,
                ffi::XNPreeditDoneCallback_0.as_ptr() as *const _,
                &(preedit_callbacks.done_callback) as *const _,
                ffi::XNPreeditCaretCallback_0.as_ptr() as *const _,
                &(preedit_callbacks.caret_callback) as *const _,
                ffi::XNPreeditDrawCallback_0.as_ptr() as *const _,
                &(preedit_callbacks.draw_callback) as *const _,
                ptr::null_mut::<()>(),
            )
        })
        .expect("XVaCreateNestedList returned NULL");

        let ic = unsafe {
            (xconn.xlib.XCreateIC)(
                im,
                ffi::XNInputStyle_0.as_ptr() as *const _,
                style,
                ffi::XNClientWindow_0.as_ptr() as *const _,
                window,
                ffi::XNPreeditAttributes_0.as_ptr() as *const _,
                preedit_attr.ptr,
                ptr::null_mut::<()>(),
            )
        };

        (!ic.is_null()).then_some(ic)
    }

    unsafe fn create_nothing_ic(
        xconn: &Arc<XConnection>,
        im: ffi::XIM,
        style: XIMStyle,
        window: ffi::Window,
    ) -> Option<ffi::XIC> {
        let ic = unsafe {
            (xconn.xlib.XCreateIC)(
                im,
                ffi::XNInputStyle_0.as_ptr() as *const _,
                style,
                ffi::XNClientWindow_0.as_ptr() as *const _,
                window,
                ptr::null_mut::<()>(),
            )
        };

        (!ic.is_null()).then_some(ic)
    }

    pub(crate) fn focus(&self, xconn: &Arc<XConnection>) -> Result<(), XError> {
        if std::env::var_os("MISTTERM_IME_DEBUG").is_some() {
            eprintln!("[ime-vendor] XSetICFocus called");
        }
        unsafe {
            (xconn.xlib.XSetICFocus)(self.ic);
        }
        xconn.check_errors()
    }

    pub(crate) fn unfocus(&self, xconn: &Arc<XConnection>) -> Result<(), XError> {
        if std::env::var_os("MISTTERM_IME_DEBUG").is_some() {
            eprintln!("[ime-vendor] XUnsetICFocus called");
        }
        unsafe {
            (xconn.xlib.XUnsetICFocus)(self.ic);
        }
        xconn.check_errors()
    }

    pub fn is_allowed(&self) -> bool {
        self.allowed
    }

    // Set the spot for preedit text. Setting spot isn't working with libX11 when preedit callbacks
    // are being used. Certain IMEs do show selection window, but it's placed in bottom left of the
    // window and couldn't be changed.
    //
    // For me see: https://bugs.freedesktop.org/show_bug.cgi?id=1580.
    pub(crate) fn set_spot(&mut self, xconn: &Arc<XConnection>, x: c_short, y: c_short) {
        if !self.is_allowed() || self.ic_spot.x == x && self.ic_spot.y == y {
            return;
        }

        self.ic_spot = ffi::XPoint { x, y };

        unsafe {
            let preedit_attr = util::memory::XSmartPointer::new(
                xconn,
                (xconn.xlib.XVaCreateNestedList)(
                    0,
                    ffi::XNSpotLocation_0.as_ptr(),
                    &self.ic_spot,
                    ptr::null_mut::<()>(),
                ),
            )
            .expect("XVaCreateNestedList returned NULL");

            (xconn.xlib.XSetICValues)(
                self.ic,
                ffi::XNPreeditAttributes_0.as_ptr() as *const _,
                preedit_attr.ptr,
                ptr::null_mut::<()>(),
            );
        }

        // MISTTERM (#430): XSetICValues(XNSpotLocation) can disturb the
        // server-side IC focus state on ibus/XIM during an active composition.
        // egui-winit re-reports the IME cursor area on every frame that carries
        // any input event — including preedit/commit Ime events — so this fires
        // between the user's last composition key and the commit. If the IC
        // loses focus there, the final commit key bypasses XIM lookup and the
        // composed string is never delivered (MistTerm #430: only the first
        // Latin letter reached the PTY). Re-apply XSetICFocus afterwards so the
        // IC stays focused while it exists.
        let _ = self.focus(xconn);
    }
}

/// MISTTERM (#431/#430): validity predicate for an XIM PreeditDraw chg range
/// against the current buffered preedit length, extracted so it can be unit
/// tested without a live X server.
fn chg_range_valid(buffer_len: usize, chg_range: &std::ops::Range<usize>) -> bool {
    chg_range.start <= buffer_len && chg_range.end <= buffer_len
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn client_data() -> (ImeContextClientData, mpsc::Receiver<(ffi::Window, ImeEvent)>) {
        let (tx, rx) = mpsc::channel();
        (
            ImeContextClientData { window: 42, event_sender: tx, text: Vec::new(), cursor_pos: 0 },
            rx,
        )
    }

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn chg_range_valid_rejects_out_of_bounds() {
        assert!(chg_range_valid(6, &(0..6)));
        assert!(chg_range_valid(6, &(6..6)));
        assert!(!chg_range_valid(6, &(0..7)));
        assert!(!chg_range_valid(6, &(7..7)));
        // Empty preedit buffer: only a full-replace 0..0 draw is valid.
        assert!(chg_range_valid(0, &(0..0)));
        assert!(!chg_range_valid(0, &(0..1)));
    }

    #[test]
    fn apply_draw_replaces_ascii_preedit_with_cjk_commit_shape() {
        // ibus/XIM draws "ni hao" first, then replaces the whole buffer with
        // "你好" before the commit; the splice must not lose or corrupt text.
        let (mut cd, rx) = client_data();
        cd.text = chars("ni hao");
        apply_draw(&mut cd, &(0..6), chars("你好"), 2);
        assert_eq!(cd.text.iter().collect::<String>(), "你好");
        assert_eq!(cd.cursor_pos, 2);
        let (win, ev) = rx.try_recv().expect("update event emitted");
        assert_eq!(win, 42);
        match ev {
            ImeEvent::Update(text, cursor_byte_pos) => {
                assert_eq!(text, "你好");
                assert_eq!(cursor_byte_pos, 6); // 2 CJK chars = 6 UTF-8 bytes
            },
            other => panic!("expected Update, got {other:?}"),
        }
    }

    #[test]
    fn apply_draw_preserves_tail_outside_chg_range() {
        let (mut cd, rx) = client_data();
        cd.text = chars("zhong wen123");
        apply_draw(&mut cd, &(0..9), chars("中文"), 2);
        assert_eq!(cd.text.iter().collect::<String>(), "中文123");
        drop(rx);
    }

    #[test]
    fn apply_draw_null_text_deletes_range() {
        let (mut cd, _rx) = client_data();
        cd.text = chars("n hao");
        apply_draw(&mut cd, &(0..5), Vec::new(), 0);
        assert!(cd.text.is_empty());
    }

    #[test]
    fn calc_byte_position_counts_utf8_bytes_of_cjk() {
        assert_eq!(calc_byte_position(&chars("你好"), 0), 0);
        assert_eq!(calc_byte_position(&chars("你好"), 1), 3);
        assert_eq!(calc_byte_position(&chars("你好"), 2), 6);
        assert_eq!(calc_byte_position(&chars("ni 你"), 3), 3); // "ni " = 3 bytes
    }

    #[test]
    fn wchar_decode_from_utf16_round_trips_cjk() {
        // Mirrors the wchar Draw path: UTF-16 code units -> String.
        let wide: [u16; 2] = [0x4f60, 0x597d]; // 你好
        let s = String::from_utf16(&wide).expect("CJK decodes from UTF-16");
        assert_eq!(s, "你好");
    }
}
