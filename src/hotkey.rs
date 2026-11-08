//! Global Option-N hotkey via Carbon `RegisterEventHotKey` (needs no Accessibility permission).

use std::ffi::c_void;

#[repr(C)]
struct EventTypeSpec {
    class: u32,
    kind: u32,
}

#[repr(C)]
struct EventHotKeyID {
    signature: u32,
    id: u32,
}

type Handler = extern "C" fn(next: *mut c_void, event: *mut c_void, user: *mut c_void) -> i32;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> *mut c_void;
    fn InstallEventHandler(
        target: *mut c_void,
        handler: Handler,
        count: u32,
        types: *const EventTypeSpec,
        user: *mut c_void,
        out: *mut c_void,
    ) -> i32;
    fn RegisterEventHotKey(
        code: u32,
        modifiers: u32,
        id: EventHotKeyID,
        target: *mut c_void,
        options: u32,
        out: *mut *mut c_void,
    ) -> i32;
}

const EVENT_CLASS_KEYBOARD: u32 = u32::from_be_bytes(*b"keyb");
const EVENT_HOTKEY_PRESSED: u32 = 5;
const OPTION_KEY: u32 = 0x800;
const KEY_N: u32 = 45;

/// Calls `on_press` (on the main thread) whenever Option-N is pressed.
pub fn register(on_press: Handler) -> bool {
    let spec = EventTypeSpec { class: EVENT_CLASS_KEYBOARD, kind: EVENT_HOTKEY_PRESSED };
    let id = EventHotKeyID { signature: u32::from_be_bytes(*b"ntch"), id: 1 };
    let mut hotkey = std::ptr::null_mut();
    unsafe {
        let target = GetApplicationEventTarget();
        InstallEventHandler(target, on_press, 1, &spec, std::ptr::null_mut(), std::ptr::null_mut()) == 0
            && RegisterEventHotKey(KEY_N, OPTION_KEY, id, target, 0, &mut hotkey) == 0
    }
}
