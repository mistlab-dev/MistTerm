//! macOS：通过 Carbon Text Input Source 选择「ABC」键盘布局（系统级当前输入法）。

use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::TCFType;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::string::{CFString, CFStringRef};
use std::os::raw::c_void;

type TISInputSourceRef = *const c_void;

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    static kTISPropertyInputSourceID: CFStringRef;
    static kTISPropertyInputSourceType: CFStringRef;
    static kTISTypeKeyboardLayout: CFStringRef;
    fn TISCopyCurrentKeyboardInputSource() -> TISInputSourceRef;
    fn TISGetInputSourceProperty(input_source: TISInputSourceRef, key: CFStringRef) -> *const c_void;
    fn TISCreateInputSourceList(properties: CFDictionaryRef, include_all_installed: u8) -> CFArrayRef;
    fn TISSelectInputSource(input_source: TISInputSourceRef) -> i32;
}

pub fn select_abc_keyboard_layout() {
    let layout_id = CFString::new("com.apple.keylayout.ABC");
    let key = unsafe { CFString::wrap_under_get_rule(kTISPropertyInputSourceID) };
    let dict = CFDictionary::from_CFType_pairs(&[(key, layout_id)]);

    let array_raw = unsafe { TISCreateInputSourceList(dict.as_concrete_TypeRef(), 0) };
    if array_raw.is_null() {
        log::debug!("TISCreateInputSourceList(ABC) returned empty; ABC may be disabled in System Settings");
        return;
    }

    let array = unsafe { CFArray::<*const c_void>::wrap_under_create_rule(array_raw) };
    let vals = array.get_all_values();
    let Some(&src) = vals.first() else {
        log::debug!("Input source com.apple.keylayout.ABC not found");
        return;
    };

    let status = unsafe { TISSelectInputSource(src as TISInputSourceRef) };
    if status != 0 {
        log::debug!("TISSelectInputSource(ABC) returned {}", status);
    } else {
        log::info!("Switched input source to com.apple.keylayout.ABC (English)");
    }
}

/// 当前输入源是不是普通键盘布局（ABC/US 等，不是拼音这类输入法）。
/// 普通布局下不存在真正的「组字」，winit 报来的 Preedit 应该当成已上屏的字直接发出去。
pub fn current_source_is_keyboard_layout() -> bool {
    unsafe {
        let src = TISCopyCurrentKeyboardInputSource();
        if src.is_null() {
            return false;
        }
        let ty = TISGetInputSourceProperty(src, kTISPropertyInputSourceType) as CFStringRef;
        let is_layout = !ty.is_null()
            && CFString::wrap_under_get_rule(ty) == CFString::wrap_under_get_rule(kTISTypeKeyboardLayout);
        core_foundation::base::CFRelease(src);
        is_layout
    }
}
