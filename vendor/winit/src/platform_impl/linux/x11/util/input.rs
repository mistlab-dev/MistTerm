use std::{slice, str};
use x11rb::protocol::xinput::{self, ConnectionExt as _};
use x11rb::protocol::xkb;

use super::*;

pub const VIRTUAL_CORE_POINTER: u16 = 2;
pub const VIRTUAL_CORE_KEYBOARD: u16 = 3;

// A base buffer size of 1kB uses a negligible amount of RAM while preventing us from having to
// re-allocate (and make another round-trip) in the *vast* majority of cases.
// To test if `lookup_utf8` works correctly, set this to 1.
const TEXT_BUFFER_SIZE: usize = 1024;

impl XConnection {
    pub fn select_xinput_events(
        &self,
        window: xproto::Window,
        device_id: u16,
        mask: xinput::XIEventMask,
    ) -> Result<VoidCookie<'_>, X11Error> {
        self.xcb_connection()
            .xinput_xi_select_events(window, &[xinput::EventMask {
                deviceid: device_id,
                mask: vec![mask],
            }])
            .map_err(Into::into)
    }

    pub fn select_xkb_events(
        &self,
        device_id: xkb::DeviceSpec,
        mask: xkb::EventType,
    ) -> Result<bool, X11Error> {
        let mask = u16::from(mask) as _;
        let status =
            unsafe { (self.xlib.XkbSelectEvents)(self.display, device_id as _, mask, mask) };

        if status == ffi::True {
            self.flush_requests()?;
            Ok(true)
        } else {
            tracing::error!("Could not select XKB events: The XKB extension is not initialized!");
            Ok(false)
        }
    }

    pub fn query_pointer(
        &self,
        window: xproto::Window,
        device_id: u16,
    ) -> Result<xinput::XIQueryPointerReply, X11Error> {
        self.xcb_connection()
            .xinput_xi_query_pointer(window, device_id)?
            .reply()
            .map_err(Into::into)
    }

    fn lookup_utf8_inner(
        &self,
        ic: ffi::XIC,
        key_event: &mut ffi::XKeyEvent,
        buffer: *mut u8,
        size: usize,
    ) -> (ffi::KeySym, ffi::Status, c_int) {
        let mut keysym: ffi::KeySym = 0;
        let mut status: ffi::Status = 0;
        let count = unsafe {
            (self.xlib.Xutf8LookupString)(
                ic,
                key_event,
                buffer as *mut c_char,
                size as c_int,
                &mut keysym,
                &mut status,
            )
        };
        (keysym, status, count)
    }

    pub fn lookup_utf8(&self, ic: ffi::XIC, key_event: &mut ffi::XKeyEvent) -> String {
        // `assume_init` is safe here because the array consists of `MaybeUninit` values,
        // which do not require initialization.
        let mut buffer: [MaybeUninit<u8>; TEXT_BUFFER_SIZE] =
            unsafe { MaybeUninit::uninit().assume_init() };
        // If the buffer overflows, we'll make a new one on the heap.
        let mut vec;

        let (keysym, mut status, count) =
            self.lookup_utf8_inner(ic, key_event, buffer.as_mut_ptr() as *mut u8, buffer.len());

        let bytes = if status == ffi::XBufferOverflow {
            vec = Vec::with_capacity(count as usize);
            let (_, new_status, new_count) =
                self.lookup_utf8_inner(ic, key_event, vec.as_mut_ptr(), vec.capacity());
            debug_assert_eq!(count, new_count);
            status = new_status;

            unsafe { vec.set_len(count as usize) };
            &vec[..count as usize]
        } else {
            unsafe { slice::from_raw_parts(buffer.as_ptr() as *const u8, count as usize) }
        };

        let decoded = str::from_utf8(bytes);
        if std::env::var_os("MISTTERM_IME_DEBUG").is_some() {
            eprintln!(
                "[ime-xlib] lookup ic={:p} window={:#x} serial={} keycode={} status={} count={} utf8={} bytes={:02x?}",
                ic,
                key_event.window,
                key_event.serial,
                key_event.keycode,
                status,
                count,
                decoded.is_ok(),
                bytes,
            );
        }

        let mut written = decoded.unwrap_or("").to_string();
        // XIM_COMMIT may carry XimLookupKeySym only (or conversion of the
        // COMPOUND_TEXT payload may fail). Recover a single character from
        // the keysym when the byte buffer is empty.
        if written.is_empty() {
            if let Some(from_keysym) = text_from_x11_keysym(keysym) {
                if std::env::var_os("MISTTERM_IME_DEBUG").is_some() {
                    eprintln!(
                        "[ime-xlib] lookup empty bytes; recovered keysym={:#x} -> {:?}",
                        keysym, from_keysym
                    );
                }
                written = from_keysym;
            }
        }
        written
    }
}

/// Decode text from an X11 keysym when `Xutf8LookupString` returned no bytes.
///
/// Unicode keysyms use the 0x01000000 base from XKBlib / X11. Latin-1 keysyms
/// occupy 0x20..=0xff and map 1:1 to Unicode. This is only used for the
/// synthetic keycode-0 XIM commit path, not for ordinary key repeats.
pub(crate) fn text_from_x11_keysym(keysym: ffi::KeySym) -> Option<String> {
    const UNICODE_KEYSYM_BASE: u64 = 0x0100_0000;
    let keysym = keysym as u64;
    let codepoint = if (UNICODE_KEYSYM_BASE..=UNICODE_KEYSYM_BASE + 0x10_FFFF).contains(&keysym) {
        (keysym - UNICODE_KEYSYM_BASE) as u32
    } else if (0x20..=0xff).contains(&keysym) {
        keysym as u32
    } else {
        return None;
    };
    char::from_u32(codepoint).filter(|ch| *ch != '\0').map(String::from)
}

#[cfg(test)]
mod xim_commit_keysym_tests {
    use super::text_from_x11_keysym;

    #[test]
    fn unicode_keysym_decodes_cjk() {
        // U+4F60 你 = 0x01004F60
        assert_eq!(text_from_x11_keysym(0x0100_4F60), Some("你".to_string()));
        // U+597D 好
        assert_eq!(text_from_x11_keysym(0x0100_597D), Some("好".to_string()));
        // U+4E2D 中, U+6587 文
        assert_eq!(text_from_x11_keysym(0x0100_4E2D), Some("中".to_string()));
        assert_eq!(text_from_x11_keysym(0x0100_6587), Some("文".to_string()));
    }

    #[test]
    fn latin1_and_ascii_keysyms() {
        assert_eq!(text_from_x11_keysym(0x74), Some("t".to_string()));
        assert_eq!(text_from_x11_keysym(0xe9), Some("é".to_string()));
        assert_eq!(text_from_x11_keysym(0x0d), None, "control chars are not commits");
        assert_eq!(text_from_x11_keysym(0x0), None);
        assert_eq!(text_from_x11_keysym(0xff0d), None, "XK_Return is not a unicode keysym");
    }
}
