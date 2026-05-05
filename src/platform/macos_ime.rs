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
    fn TISCreateInputSourceList(properties: CFDictionaryRef, include_all_installed: u8) -> CFArrayRef;
    fn TISSelectInputSource(input_source: TISInputSourceRef) -> i32;
}

pub fn select_abc_keyboard_layout() {
    let layout_id = CFString::new("com.apple.keylayout.ABC");
    let key = unsafe { CFString::wrap_under_get_rule(kTISPropertyInputSourceID) };
    let dict = CFDictionary::from_CFType_pairs(&[(key, layout_id)]);

    let array_raw = unsafe { TISCreateInputSourceList(dict.as_concrete_TypeRef(), 0) };
    if array_raw.is_null() {
        log::debug!("TISCreateInputSourceList(ABC) 返回空，可能未在系统设置中启用 ABC");
        return;
    }

    let array = unsafe { CFArray::<*const c_void>::wrap_under_create_rule(array_raw) };
    let vals = array.get_all_values();
    let Some(&src) = vals.first() else {
        log::debug!("未找到 com.apple.keylayout.ABC 输入源");
        return;
    };

    let status = unsafe { TISSelectInputSource(src as TISInputSourceRef) };
    if status != 0 {
        log::debug!("TISSelectInputSource(ABC) 返回 {}", status);
    } else {
        log::info!("已尝试切换到英文键盘布局 (com.apple.keylayout.ABC)");
    }
}
