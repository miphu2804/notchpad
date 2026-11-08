//! Pill-shaped text button that dims while pressed.

use objc2::{define_class, msg_send, MainThreadOnly};
use objc2_app_kit::{NSButton, NSControl, NSEvent, NSResponder, NSView};
use objc2_foundation::NSObject;

use crate::widgets as w;

const PRESSED_ALPHA: f64 = 0.6;

define_class!(
    /// The fill is a plain layer colour, so AppKit's own highlight shows nothing; this
    /// gives the press a visible, quick dim and a softer release.
    #[unsafe(super(NSButton, NSControl, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "PillButton"]
    pub struct PillButton;

    impl PillButton {
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            w::fade(self, PRESSED_ALPHA, 0.06);
            // Runs AppKit's tracking loop until the mouse goes up (and sends the action).
            let _: () = unsafe { msg_send![super(self), mouseDown: event] };
            w::fade(self, 1.0, 0.2);
        }
    }
);
